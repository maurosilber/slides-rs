// The steps of a slide, which slides.js steps through, and the notebook
// renderer of the VS Code extension too, through the steps of a figure. The
// deck's slides.js starts with this file.

// The deck numbers the steps of each slide, as src/step/html.rs says: each
// element that steps has the class `step`, and the steps it shows in as its
// `--from` and `--to`, and slides.css shows it as the `--step` of what it is
// in says. A slide says how many steps it has as its `data-count`.

// Whether CSS can hide a step by itself, with `if()`. Where it cannot, each
// step hidden is marked `step-hidden`, as slides.css computes `--shown`.
const HIDES = globalThis.CSS?.supports("display", "if(style(--shown: 0): none; else: revert)") ?? false;

// Shows the steps in `container` as they are at step `step`.
function showSteps(container, step) {
    container.style.setProperty("--step", step);
    if (HIDES) return;
    for (const element of container.querySelectorAll(".step")) {
        element.classList.toggle("step-hidden", hidesItself(element));
    }
}

// How many steps the steps in `container` make, as many as the highest one
// any shows from, or is hidden again at.
function countSteps(container) {
    let count = 1;
    for (const element of container.querySelectorAll(".step")) {
        for (const name of ["--from", "--to"]) {
            const step = Number(element.style.getPropertyValue(name));
            if (Number.isInteger(step)) count = Math.max(count, step);
        }
    }
    return count;
}

// Whether a step is hidden at the step it is at, as slides.css says.
function hidesItself(element) {
    return getComputedStyle(element).getPropertyValue("--shown").trim() == "0";
}

// Whether an element is hidden by a step it is in, or is.
function inHiddenStep(element) {
    for (let step = element.closest(".step"); step; step = step.parentElement?.closest(".step")) {
        if (hidesItself(step)) return true;
    }
    return false;
}

// The SVG animations that begin when asked, as slides_rs.Motion writes them,
// or copies of them, which begin as the deck says, at a time of their own.
const ANIMATIONS = "[begin=indefinite], [data-begin=indefinite]";

// How many seconds an animation still playing takes, sped up, to get where it
// is going, when another begins: as the frontmatter's `figures.rush` says, which
// the page's root has, or else 0.2. Taking none, it takes a moment still, as an
// animation cannot last none.
const RUSH = Math.max(Number(globalThis.document?.documentElement.dataset.rush ?? 0.2), 0.001);

// How each animation the deck plays, a copy of one as written, plays: the one
// as written, to begin when asked, how long it lasts and how long each time it
// repeats does, and whether it plays forward or backward, from when, and for
// how long, sped up.
const played = new WeakMap();

// What each container waits to play, until those sped up get where they are
// going, and its timer.
const waiting = new WeakMap();

// Plays each of the SVG animations in `container` that begin when asked once
// it shows, when every step it is in does, and plays backward, from where it
// is, each that is hidden again, so that stepping back undoes it, and stepping
// on plays it again. Those still playing, when others begin or play backward,
// speed up to get where they were going, their end or, backward, their start,
// and the others wait for them, so that one step's animation does not run on
// into the next one's.
//
// Finished, those that begin are at their end already, as if played, but for
// those that repeat on and on, which have none, as when a slide opens at one
// of its steps rather than being stepped through to it.
function playAnimations(container, finished = false) {
    // What waited to play plays now, to be sped up in turn.
    waited(container)?.();
    const begun = [];
    const before = [];
    const hidden = [];
    const back = [];
    for (const animation of container.querySelectorAll(ANIMATIONS)) {
        const shown = !inHiddenStep(animation);
        const forward = played.get(animation)?.forward;
        if (shown) (forward ? before : begun).push(animation);
        else if (forward) hidden.push(animation);
        else if (forward === false) back.push(animation);
    }
    let wait = 0;
    if (begun.length || hidden.length) {
        for (const animation of before) wait = Math.max(wait, rush(animation, true));
        for (const animation of back) wait = Math.max(wait, rush(animation, false));
    }
    const start = () => {
        hidden.forEach(reverse);
        for (const animation of begun) {
            const end = finished && Number.isFinite(lengthOf(animation));
            play(animation, true, end ? 1 : progressOf(animation));
        }
    };
    if (wait > 0) {
        waiting.set(container, { start, timer: setTimeout(() => waited(container)(), wait * 1000) });
    } else {
        start();
    }
}

// Speeds up every animation still playing in `container` to get where it is
// going, as when stepping on from its last step, or back from its first, before
// leaving it. Returns whether any was playing.
function rushAnimations(container) {
    waited(container)?.();
    let rushed = false;
    for (const animation of container.querySelectorAll(ANIMATIONS)) {
        const state = played.get(animation);
        if (state && rush(animation, state.forward) > 0) rushed = true;
    }
    return rushed;
}

// What `container` waits to play, if anything, no longer waiting.
function waited(container) {
    const { start, timer } = waiting.get(container) ?? {};
    clearTimeout(timer);
    waiting.delete(container);
    return start;
}

// Starts over every animation played in `container`, as when its slide closes.
function stopAnimations(container) {
    waited(container);
    for (const animation of container.querySelectorAll(ANIMATIONS)) {
        if (played.has(animation)) restart(animation);
    }
}

// Puts in place of `animation` a copy of it as written, playing forward or
// backward from `progress`, from 0 at its start to 1 at its end, `speed` times
// as fast. An animation playing already cannot begin again before now, but a
// copy can, as its `begin` says: begun so by beginElementAt, Chrome may not
// play others begun at other times before now in the same moment.
function play(animation, forward, progress, speed = 1) {
    const { written, length, each } = played.get(animation) ?? {
        written: animation.cloneNode(true),
        length: lengthOf(animation),
        each: eachOf(animation),
    };
    const copy = forward ? written.cloneNode(true) : backward(written);
    if (speed != 1) {
        copy.setAttribute("dur", `${each / speed}s`);
        const most = copy.getAttribute("repeatDur");
        if (most != null && most != "indefinite") copy.setAttribute("repeatDur", `${parseFloat(most) / speed}s`);
    }
    const lasts = length / speed;
    // Chrome holds no animation over by the time the SVG's timeline begins, at
    // 0, as one played to its end as the page loads is: it ends just after.
    const now = animation.ownerSVGElement.getCurrentTime();
    // Not begun, it is none of the way along, even as one that repeats on and on
    // lasts forever, which would make it not a number of the way.
    const along = forward ? progress : 1 - progress;
    const start = Math.max(now - (along && along * lasts), 0.001 - lasts);
    copy.setAttribute("begin", `${start}s`);
    copy.dataset.begin = "indefinite";
    animation.replaceWith(copy);
    played.set(copy, { written, length, each, forward, start, lasts });
}

// How far along an animation is, from 0 at its start to 1 at its end, or 0
// if the deck has not played it.
function progressOf(animation) {
    const state = played.get(animation);
    if (!state) return 0;
    const elapsed = (animation.ownerSVGElement.getCurrentTime() - state.start) / state.lasts;
    const done = Math.min(Math.max(elapsed, 0), 1);
    return state.forward ? done : 1 - done;
}

// How long an animation lasts, repeated, or Infinity if it repeats on and on.
function lengthOf(animation) {
    let length = eachOf(animation);
    const count = animation.getAttribute("repeatCount");
    if (count == "indefinite") length = Infinity;
    else if (count != null) length *= Number(count);
    const most = animation.getAttribute("repeatDur");
    if (most != null && most != "indefinite") length = Math.min(length, parseFloat(most));
    return length;
}

// How long an animation lasts each time it repeats, or Infinity if it is not
// said.
function eachOf(animation) {
    try {
        return animation.getSimpleDuration();
    } catch {
        return Infinity;
    }
}

// Speeds an animation up, forward or backward, to get where it is going within
// RUSH seconds, unless it is there already, or it repeats on and on, which has
// no end. Returns how many seconds it takes to get there.
function rush(animation, forward) {
    const { length } = played.get(animation);
    const progress = progressOf(animation);
    const left = (forward ? 1 - progress : progress) * length;
    if (!Number.isFinite(length) || !(left > 0)) return 0;
    const speed = Math.max(1, left / RUSH);
    play(animation, forward, progress, speed);
    return left / speed;
}

// Plays an animation backward from where it is. One that repeats on and on,
// or builds on itself each time, which has no way back, or that is back at its
// start already, starts over instead.
function reverse(animation) {
    const { written, length } = played.get(animation);
    const progress = progressOf(animation);
    const frozen = written.getAttribute("fill") == "freeze";
    if (!reversible(written, length) || !(progress > 0) || (progress >= 1 && !frozen)) {
        return restart(animation);
    }
    play(animation, false, progress);
}

// An animation, once begun, cannot be taken back to before it began, but a
// copy of it as written, to begin when asked, has not.
function restart(animation) {
    animation.replaceWith(played.get(animation).written.cloneNode(true));
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
if (typeof module == "object") module.exports = { showSteps, countSteps, playAnimations };
