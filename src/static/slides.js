const slides = document.getElementsByTagName("section");

// Elements marked as a step within a column: every list item, and the
// elements marked `step` or `also`, as an SVG's are, or with a `data-step`,
// which math's \step{...}, \step[...]{...} and \also{...} write.
const MARKED = "li, [step], [also], [data-step]";

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
// ever hidden shows in, from `from` up to `to`, excluded.
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
            for (const { child } of children) elements.push({ element: child, from: column, to: Infinity });
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
            });
        }
    });
    return { count, elements };
}

// Puts each run of h3 columns in a box of its own, as many columns wide as it
// has h3s, from its first h3 up to the next h1 or h2, which stay outside, as
// slides.css lays out.
function wrapColumns(slide) {
    let columns = null;
    for (const child of [...slide.children]) {
        if (child.tagName == "H1" || child.tagName == "H2") {
            columns = null;
        } else if (child.tagName == "H3" && !columns) {
            columns = document.createElement("div");
            columns.className = "columns";
            child.before(columns);
        }
        if (!columns) continue;
        columns.append(child);
        columns.style.setProperty("--cols", columns.querySelectorAll(":scope > h3").length);
    }
}

// Math is typeset by a module script, which runs after this one but before
// DOMContentLoaded, so the steps it marks are only there from then on. The
// columns are boxed after, as the steps are read off the slide's children.
let slideSteps = [];
addEventListener("DOMContentLoaded", () => {
    slideSteps = [...slides].map(stepsOf);
    for (const slide of slides) wrapColumns(slide);
    updateSlide(...positionFromHash());
});

// The current slide and step are kept in the URL, as `#slide.step`,
// so that the reload after a rebuild comes back to them instead of to the
// first slide.
function positionFromHash() {
    const [slide, step] = location.hash.slice(1).split(".").map(Number);
    if (!Number.isInteger(slide) || slide < 1) return [0, 1];
    // The deck may have lost the slide we were on.
    return [Math.min(slide, slides.length) - 1, Number.isInteger(step) ? step : 1];
}

let currentSlide = 0;
let currentStep = 1;
let stepsEnabled = true;

function showPosition() {
    history.replaceState(null, "", "#" + (currentSlide + 1) + "." + currentStep);
}

function updateSlide(i, step = 1) {
    if (i < 0 || i >= slides.length) return;
    slides[currentSlide].style.display = "none";
    slides[i].style.display = "block";
    currentSlide = i;
    currentStep = Math.max(1, Math.min(step, slideSteps[i].count));
    showStep(currentStep);
    showPosition();
}

// Hidden rather than removed, so the columns keep their place. Disabled,
// every element shows, even those whose steps are over.
// currentStep is tracked even while disabled, so re-enabling resumes here.
function showStep(i) {
    const { count, elements } = slideSteps[currentSlide];
    if (i < 1 || i > count) return false;
    currentStep = i;
    for (const { element, from, to } of elements) {
        const shown = !stepsEnabled || (from <= i && i < to);
        element.style.visibility = shown ? "visible" : "hidden";
    }
    showPosition();
    return true;
}

// Disabled steps are all visible already, so stepping through them is a
// no-op: report failure and let the caller move a whole slide instead.
function moveToStep(i) {
    return stepsEnabled && showStep(i);
}

addEventListener("keydown", (event) => {
    if (event.code == "ArrowRight") {
        if (!moveToStep(currentStep + 1)) updateSlide(currentSlide + 1);
    } else if (event.code == "ArrowLeft") {
        if (!moveToStep(currentStep - 1)) updateSlide(currentSlide - 1, Infinity);
    } else if (event.code == "ArrowDown") {
        // Alternate: finish this slide, then open the next one.
        if (stepsEnabled && currentStep < slideSteps[currentSlide].count) {
            showStep(slideSteps[currentSlide].count);
        } else {
            updateSlide(currentSlide + 1, 1);
        }
    } else if (event.code == "ArrowUp") {
        // Alternate: go back to the start of this slide, then to the previous one.
        if (stepsEnabled && currentStep > 1) {
            showStep(1);
        } else {
            updateSlide(currentSlide - 1, Infinity);
        }
    } else if (event.code == "KeyA") {
        // Toggle: off reveals the whole slide, on returns to where we were.
        stepsEnabled = !stepsEnabled;
        showStep(currentStep);
    }
});

// The URL can also be edited, or gone back through.
addEventListener("hashchange", () => updateSlide(...positionFromHash()));
