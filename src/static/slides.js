const slides = document.getElementsByTagName("section");

// Elements marked as a step within a column: every list item, those of an
// SVG, by `fragment="n"`, and those of math, by `data-fragment`, which
// \step{...}, \also{...} and \fragment{n}{...} write. Without a number, an
// element comes one step after the last.
const MARKED = "li, [fragment], [data-fragment]";

// A slide is revealed in steps, and its n-th fragment shows the first n.
// Each h2 and h3 starts a column, a step of its own. After each column, and
// before the first one, come the elements it marks, in order of their number,
// those sharing a number together. The first step is what the slide opens
// with: the first column, unless a marked element comes before it.
//
// A heading with `fragments="false"`, as `{ fragments=false }` writes, shows
// everything under it at once, up to the next heading of its level or above:
// the elements it marks, and the columns it holds, which join the step before
// them. `fragments="true"` steps through them again. Outside every such
// heading, the slide's `data-fragments` decides, from its file's frontmatter.
function stepsOf(slide) {
    const outside = slide.dataset.fragments != "false";
    // The headings whose part of the slide the current child is in, each
    // with whether fragments are on there.
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
            const value = child.getAttribute("fragments");
            scopes.push({ level, on: value == null ? on() : value != "false" });
        }
        groups.at(-1).children.push({ child, on: on() });
    }
    const steps = [[]];
    groups.forEach(({ step, children }, i) => {
        const group = children.map(({ child }) => child);
        // What comes before the first heading is always shown.
        if (i > 0) {
            if ((i == 1 && steps.length == 1) || !step) steps.at(-1).push(...group);
            else steps.push(group);
        }
        const numbered = new Map();
        let last = 0;
        for (const { child: element, on } of children) {
            // Unmarked, the element's parts show along with it.
            if (!on) continue;
            for (const part of element.querySelectorAll(MARKED)) {
                const value = part.getAttribute("fragment") ?? part.dataset.fragment;
                const n = /^\d+$/.test(value) ? Number(value) : last + 1;
                last = Math.max(last, n);
                numbered.set(n, [...(numbered.get(n) ?? []), part]);
            }
        }
        for (const n of [...numbered.keys()].sort((a, b) => a - b)) {
            steps.push(numbered.get(n));
        }
    });
    return steps;
}

// Math is typeset by a module script, which runs after this one but before
// DOMContentLoaded, so the steps it marks are only there from then on.
let fragments = [];
addEventListener("DOMContentLoaded", () => {
    fragments = [...slides].map(stepsOf);
    updateSlide(...positionFromHash());
});

// column-count has to fit the widest h2 group, not the whole slide.
for (const slide of slides) {
    let widest = 0, run = 0;
    for (const child of slide.children) {
        if (child.tagName == "H2") run = 0;
        else if (child.tagName == "H3") widest = Math.max(widest, ++run);
    }
    slide.style.setProperty("--cols", widest || 1);
}

// The current slide and fragment are kept in the URL, as `#slide.fragment`,
// so that the reload after a rebuild comes back to them instead of to the
// first slide.
function positionFromHash() {
    const [slide, fragment] = location.hash.slice(1).split(".").map(Number);
    if (!Number.isInteger(slide) || slide < 1) return [0, 1];
    // The deck may have lost the slide we were on.
    return [Math.min(slide, slides.length) - 1, Number.isInteger(fragment) ? fragment : 1];
}

let currentSlide = 0;
let currentFragment = 1;
let fragmentsEnabled = true;

function showPosition() {
    history.replaceState(null, "", "#" + (currentSlide + 1) + "." + currentFragment);
}

function updateSlide(i, fragment = 1) {
    if (i < 0 || i >= slides.length) return;
    slides[currentSlide].style.display = "none";
    slides[i].style.display = "block";
    currentSlide = i;
    currentFragment = Math.max(1, Math.min(fragment, fragments[i].length));
    updateFragment(currentFragment);
    showPosition();
}

// Hidden rather than removed, so the columns keep their place.
// currentFragment is tracked even while disabled, so re-enabling resumes here.
function updateFragment(i) {
    const steps = fragments[currentSlide];
    if (i < 1 || i > steps.length) return false;
    currentFragment = i;
    const shown = fragmentsEnabled ? i : steps.length;
    steps.forEach((step, j) => {
        for (const element of step) {
            element.style.visibility = j < shown ? "visible" : "hidden";
        }
    });
    showPosition();
    return true;
}

// Disabled fragments are all visible already, so stepping through them is a
// no-op: report failure and let the caller move a whole slide instead.
function stepFragment(i) {
    return fragmentsEnabled && updateFragment(i);
}

addEventListener("keydown", (event) => {
    if (event.code == "ArrowRight") {
        if (!stepFragment(currentFragment + 1)) updateSlide(currentSlide + 1);
    } else if (event.code == "ArrowLeft") {
        if (!stepFragment(currentFragment - 1)) updateSlide(currentSlide - 1, Infinity);
    } else if (event.code == "ArrowDown") {
        // Alternate: finish this slide, then open the next one.
        if (fragmentsEnabled && currentFragment < fragments[currentSlide].length) {
            updateFragment(fragments[currentSlide].length);
        } else {
            updateSlide(currentSlide + 1, 1);
        }
    } else if (event.code == "ArrowUp") {
        // Alternate: go back to the start of this slide, then to the previous one.
        if (fragmentsEnabled && currentFragment > 1) {
            updateFragment(1);
        } else {
            updateSlide(currentSlide - 1, Infinity);
        }
    } else if (event.code == "KeyA") {
        // Toggle: off reveals the whole slide, on returns to where we were.
        fragmentsEnabled = !fragmentsEnabled;
        updateFragment(currentFragment);
    }
});

// The URL can also be edited, or gone back through.
addEventListener("hashchange", () => updateSlide(...positionFromHash()));
