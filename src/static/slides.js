const slides = document.getElementsByTagName("section");

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
    // Every slide knows where it is in the deck, for slides.css to count them,
    // which CSS counters cannot, as the slides not shown are not displayed.
    [...slides].forEach((slide, i) => {
        slide.dataset.number = i + 1;
        slide.dataset.total = slides.length;
    });
    slideSteps = [...slides].map(stepsOf);
    // slides.css shows and hides them, as `step-hidden` says.
    for (const { elements } of slideSteps) {
        for (const { element, collapse } of elements) {
            element.classList.add("step");
            if (collapse) element.classList.add("step-collapse");
        }
    }
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

// A slide opens as its step is, without the transitions of stepping to it,
// which would bring in at once all it shows.
function updateSlide(i, step = 1) {
    if (i < 0 || i >= slides.length) return;
    const slide = slides[i];
    slide.classList.add("steps-instant");
    slides[currentSlide].style.display = "none";
    stopAnimations(slides[currentSlide]);
    slide.style.display = "block";
    currentSlide = i;
    currentStep = Math.max(1, Math.min(step, slideSteps[i].count));
    showStep(currentStep);
    // Laid out before the transitions come back, so that none of them runs.
    slide.getBoundingClientRect();
    slide.classList.remove("steps-instant");
    showPosition();
}

// Marks each element hidden or shown for the step, which slides.css lays out:
// hidden, an element keeps its space, so the columns keep their place, unless
// it collapses, when it takes none, so another can show in its place.
// Disabled, every element shows, even those whose steps are over.
// currentStep is tracked even while disabled, so re-enabling resumes here.
function showStep(i) {
    const { count, elements } = slideSteps[currentSlide];
    if (i < 1 || i > count) return false;
    currentStep = i;
    for (const { element, from, to } of elements) {
        const shown = !stepsEnabled || (from <= i && i < to);
        element.classList.toggle("step-hidden", !shown);
    }
    playAnimations(slides[currentSlide]);
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
