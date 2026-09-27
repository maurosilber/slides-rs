// The steps of a slide, which slides.js steps through, and the notebook
// renderer of the VS Code extension too, through the steps of a figure. The
// deck's slides.js starts with this file.

// Elements marked as a step within a column: every list item, and the
// elements marked `step` or `also`, as an SVG's are, or with a `data-step`,
// which math's \step{...}, \step[...]{...} and \also{...} write.
const MARKED = "li, [step], [also], [data-step]";

// Whether an element takes no space while hidden, rather than keeping it, as
// a starred \step*{...} or \also*{...} writes, or `collapse` marks.
function collapses(element) {
    return element.hasAttribute("collapse") || element.dataset.collapse !== undefined;
}

// A range of steps, written as in Rust: `3..5` shows from step 3 and hides
// again at step 5, `..3` shows from the start and hides at step 3, and `3..`,
// or a bare `3`, shows from step 3 to the end. src/step.rs reads them the
// same way.
function parseRange(value) {
    const match = /^\s*(\d*)(\.\.(\d*))?\s*$/.exec(value ?? "");
    if (!match || (!match[1] && !match[2])) return null;
    const bound = (text) => (text ? Number(text) : undefined);
    return { start: bound(match[1]), end: match[2] ? bound(match[3]) : undefined };
}

// The range a marked element shows in: the one its `step` or `data-step`
// says, or else, as for a bare `step` or a list item, one step after the
// latest, the highest any range so far starts at, or, for `also`, along with
// it. An `also` before any step is the first step.
function rangeOf(element, latest) {
    if (element.hasAttribute("also")) return { start: Math.max(latest, 1) };
    return parseRange(element.getAttribute("step") ?? element.dataset.step) ?? { start: latest + 1 };
}

// A slide is revealed in steps, numbered from 1, the step it opens with.
// Each h2 and h3 starts a column, a step of its own. After each column, and
// before the first one, come the steps of the elements it marks: every number
// the column's ranges start or end at is a step, in order, and an element
// shows from the step its range starts at, or its column's, up to the one it
// ends at. The first step is what the slide opens with: the first column,
// unless a marked element comes before it.
//
// A heading with `steps="false"`, as `{ steps=false }` writes, shows
// everything under it at once, up to the next heading of its level or above:
// the elements it marks, and the columns it holds, which join the step before
// them. `steps="true"` steps through them again. Outside every such heading,
// the slide's `data-steps` decides, from its file's frontmatter.
//
// Returns how many steps the slide has, and the steps each element that is
// ever hidden shows in, from `from` up to `to`, excluded, and whether it
// collapses while hidden.
function stepsOf(slide) {
    const outside = slide.dataset.steps != "false";
    // The headings whose part of the slide the current child is in, each
    // with whether steps are on there.
    const scopes = [];
    const on = () => scopes.at(-1)?.on ?? outside;
    const groups = [{ step: true, children: [] }];
    for (const child of slide.children) {
        const level = Number(/^H([1-6])$/.exec(child.tagName)?.[1]);
        if (level) {
            while (scopes.length && scopes.at(-1).level >= level) scopes.pop();
            // Whether a column is a step of its own is up to the part of
            // the slide it is in, and what it holds is up to its heading.
            if (level == 2 || level == 3) groups.push({ step: on(), children: [] });
            const value = child.getAttribute("steps");
            scopes.push({ level, on: value == null ? on() : value != "false" });
        }
        groups.at(-1).children.push({ child, on: on() });
    }
    let count = 1;
    const elements = [];
    groups.forEach(({ step, children }, i) => {
        // What comes before the first heading is always shown.
        let column = 1;
        if (i > 0) {
            column = (i == 1 && count == 1) || !step ? count : ++count;
            for (const { child } of children) {
                elements.push({ element: child, from: column, to: Infinity, collapse: false });
            }
        }
        const parts = [];
        let latest = 0;
        for (const { child: element, on } of children) {
            // Unmarked, the element's parts show along with it.
            if (!on) continue;
            // The element itself steps if it is marked, as raw html can be.
            const marked = element.matches(MARKED) ? [element] : [];
            for (const part of [...marked, ...element.querySelectorAll(MARKED)]) {
                const range = rangeOf(part, latest);
                if (range.start !== undefined) latest = Math.max(latest, range.start);
                parts.push({ part, ...range });
            }
        }
        const bounds = new Set(parts.flatMap(({ start, end }) => [start, end]).filter((n) => n !== undefined));
        const steps = new Map([...bounds].sort((a, b) => a - b).map((n) => [n, ++count]));
        for (const { part, start, end } of parts) {
            elements.push({
                element: part,
                from: start === undefined ? column : steps.get(start),
                to: end === undefined ? Infinity : steps.get(end),
                collapse: collapses(part),
            });
        }
    });
    return { count, elements };
}

// The SVG animations that begin when asked, as slides_rs.Motion writes them,
// or copies of them, which begin as the deck says, at a time of their own.
const ANIMATIONS = "[begin=indefinite], [data-begin=indefinite]";

// Each animation playing forward, which is not in the document and so is kept
// aside, with when each began, on its SVG's clock, and, for each played
// backward, the animation it is played backward from.
const playing = new WeakSet();
const began = new WeakMap();
const forwardOf = new WeakMap();

// Plays each of the SVG animations in `container` that begin when asked once
// it shows, when every step it is in does, and plays backward, from where it
// is, each that is hidden again, so that stepping back undoes it, and stepping
// on plays it again. Those that were playing already, when others begin or
// play backward, are taken to where they were going, their end or, backward,
// their start, so that one step's animation does not run on into the next one's.
function playAnimations(container) {
    const begun = [];
    const before = [];
    const hidden = [];
    const back = [];
    for (const animation of container.querySelectorAll(ANIMATIONS)) {
        const shown = !animation.closest(".step-hidden");
        if (shown && !playing.has(animation)) {
            begun.push(animation);
        } else if (shown) {
            before.push(animation);
        } else if (playing.has(animation)) {
            hidden.push(animation);
        } else if (forwardOf.has(animation)) {
            back.push(animation);
        }
    }
    if (begun.length || hidden.length) {
        before.forEach(finish);
        back.forEach(restart);
    }
    hidden.forEach(reverse);
    for (const animation of begun) {
        if (forwardOf.has(animation)) {
            resume(animation);
        } else {
            playing.add(animation);
            animation.beginElement();
            began.set(animation, animation.ownerSVGElement.getCurrentTime());
        }
    }
}

// Starts over every animation played in `container`, as when its slide closes.
function stopAnimations(container) {
    for (const animation of container.querySelectorAll(ANIMATIONS)) {
        if (playing.has(animation) || forwardOf.has(animation)) restart(animation);
    }
}

// Puts `copy` in place of `animation`, begun `offset` seconds from now, before
// it if negative. An animation playing already cannot begin again before now,
// but a copy can, as its `begin` says: begun so by beginElementAt, Chrome may
// not play others begun at other times before now in the same moment.
function replace(animation, copy, offset) {
    const start = animation.ownerSVGElement.getCurrentTime() + offset;
    copy.setAttribute("begin", `${start}s`);
    copy.dataset.begin = "indefinite";
    animation.replaceWith(copy);
    began.set(copy, start);
}

// How long an animation lasts, repeated, or Infinity if it repeats on and on.
function lengthOf(animation) {
    let length;
    try {
        length = animation.getSimpleDuration();
    } catch {
        return Infinity; // Its duration is indefinite.
    }
    const count = animation.getAttribute("repeatCount");
    if (count == "indefinite") length = Infinity;
    else if (count != null) length *= Number(count);
    const most = animation.getAttribute("repeatDur");
    if (most != null && most != "indefinite") length = Math.min(length, parseFloat(most));
    return length;
}

// How far into its length an animation is, if it began.
function elapsedOf(animation, length) {
    const start = began.get(animation);
    if (start === undefined) return undefined;
    return Math.min(animation.ownerSVGElement.getCurrentTime() - start, length);
}

// Takes an animation to its end, by a copy begun as long ago as it lasts,
// unless it repeats on and on, which has no end.
function finish(animation) {
    const length = lengthOf(animation);
    if (!Number.isFinite(length)) return;
    const copy = animation.cloneNode(true);
    playing.add(copy);
    replace(animation, copy, -length);
}

// Plays an animation backward from where it is, by a copy of it played
// backward, begun as long ago as the animation has left to play. One that
// repeats on and on, or builds on itself each time, which has no way back, or
// that is back at its start already, starts over instead.
function reverse(animation) {
    const length = lengthOf(animation);
    const elapsed = elapsedOf(animation, length);
    const frozen = animation.getAttribute("fill") == "freeze";
    if (!reversible(animation, length) || !(elapsed > 0) || (elapsed >= length && !frozen)) {
        return restart(animation);
    }
    const copy = backward(animation);
    forwardOf.set(copy, animation);
    replace(animation, copy, elapsed - length);
}

// Plays forward again, from where it is, an animation played backward.
function resume(copy) {
    const length = lengthOf(copy);
    const elapsed = elapsedOf(copy, length) ?? length;
    const animation = forwardOf.get(copy).cloneNode(true);
    playing.add(animation);
    replace(copy, animation, elapsed - length);
}

// An animation, once begun, cannot be taken back to before it began, but a
// copy of it, forward, and to begin when asked, has not.
function restart(animation) {
    const copy = (forwardOf.get(animation) ?? animation).cloneNode(true);
    copy.setAttribute("begin", "indefinite");
    delete copy.dataset.begin;
    animation.replaceWith(copy);
}

// Whether an animation, played backward, is the same as played forward, in
// reverse: every time it repeats is whole, and the same.
function reversible(animation, length) {
    const count = animation.getAttribute("repeatCount");
    return (
        Number.isFinite(length) &&
        (count == null || Number.isInteger(Number(count))) &&
        animation.getAttribute("repeatDur") == null &&
        animation.getAttribute("accumulate") != "sum"
    );
}

// A copy of an animation that plays it backward: what it goes through, in
// reverse, at the times mirrored, eased as mirrored, and turned the other way
// along a path.
function backward(animation) {
    const copy = animation.cloneNode(true);
    const list = (name) => copy.getAttribute(name)?.split(";").map((item) => item.trim());
    if (copy.localName == "animateMotion" && !copy.hasAttribute("keyPoints")) {
        // Along its path at an even pace, as it is by default.
        copy.setAttribute("keyPoints", "0;1");
        copy.setAttribute("keyTimes", "0;1");
        copy.setAttribute("calcMode", "linear");
    }
    for (const name of ["keyPoints", "values"]) {
        if (copy.hasAttribute(name)) copy.setAttribute(name, list(name).reverse().join(";"));
    }
    const [from, to] = [copy.getAttribute("from"), copy.getAttribute("to")];
    if (from != null && to != null) {
        copy.setAttribute("from", to);
        copy.setAttribute("to", from);
    }
    const times = list("keyTimes")?.map(Number);
    if (times) {
        // Discrete, each value holds from its time up to the next one's, which
        // mirrored is where it begins to hold: the first, at 0, holds last.
        const mirror = (list) => list.reverse().map((time) => 1 - time);
        const discrete = copy.getAttribute("calcMode") == "discrete";
        const mirrored = discrete ? [0, ...mirror(times.slice(1))] : mirror(times);
        copy.setAttribute("keyTimes", mirrored.join(";"));
    }
    const splines = list("keySplines");
    if (splines) {
        const mirror = (spline) => {
            const [x1, y1, x2, y2] = spline.split(/[\s,]+/).map(Number);
            return [1 - x2, 1 - y2, 1 - x1, 1 - y1].join(" ");
        };
        copy.setAttribute("keySplines", splines.reverse().map(mirror).join(";"));
    }
    const rotate = { auto: "auto-reverse", "auto-reverse": "auto" }[copy.getAttribute("rotate")];
    if (rotate) copy.setAttribute("rotate", rotate);
    return copy;
}

// The renderer bundles this file as a module, which a page never loads it as.
if (typeof module == "object") module.exports = { stepsOf, playAnimations };
