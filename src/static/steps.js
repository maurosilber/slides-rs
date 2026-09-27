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
            for (const part of element.querySelectorAll(MARKED)) {
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
// each playing, which is not in the document and so is kept aside.
const playing = new WeakSet();

// Plays each of the SVG animations in `container` that begin when asked once
// it shows, when every step it is in does, and starts over, from before it
// began, each that is hidden again, so that stepping back to it plays it again.
function playAnimations(container) {
    for (const animation of container.querySelectorAll("[begin=indefinite]")) {
        const shown = !animation.closest(".step-hidden");
        if (shown && !playing.has(animation)) {
            playing.add(animation);
            animation.beginElement();
        } else if (!shown && playing.has(animation)) {
            restart(animation);
        }
    }
}

// Starts over every animation playing in `container`, as when its slide closes.
function stopAnimations(container) {
    for (const animation of container.querySelectorAll("[begin=indefinite]")) {
        if (playing.has(animation)) restart(animation);
    }
}

// An animation, once begun, cannot be taken back to before it began, but a
// copy of it has not.
function restart(animation) {
    animation.replaceWith(animation.cloneNode(true));
}

// The renderer bundles this file as a module, which a page never loads it as.
if (typeof module == "object") module.exports = { stepsOf, playAnimations };
