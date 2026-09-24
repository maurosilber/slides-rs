const slides = document.getElementsByTagName("section");

// Each h2 and h3 starts a fragment, owning the elements that follow it.
const fragments = [...slides].map((slide) => {
    const columns = [];
    let column = null;
    for (const child of slide.children) {
        if (child.tagName == "H2" || child.tagName == "H3") {
            column = [];
            columns.push(column);
        }
        column?.push(child);
    }
    return columns;
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
    // A slide without headings has no fragments, and is shown whole.
    currentFragment = Math.max(1, Math.min(fragment, fragments[i].length));
    updateFragment(currentFragment);
    showPosition();
}

// Hidden rather than removed, so the columns keep their place.
// currentFragment is tracked even while disabled, so re-enabling resumes here.
function updateFragment(i) {
    const columns = fragments[currentSlide];
    if (i < 1 || i > columns.length) return false;
    currentFragment = i;
    const shown = fragmentsEnabled ? i : columns.length;
    columns.forEach((column, j) => {
        for (const element of column) {
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

updateSlide(...positionFromHash());

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
