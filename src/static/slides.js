const slides = document.getElementsByTagName("section");

addEventListener("DOMContentLoaded", () => {
    // Every slide knows where it is in the deck, for slides.css to count them,
    // which CSS counters cannot, as the slides not shown are not displayed.
    [...slides].forEach((slide, i) => {
        slide.dataset.number = i + 1;
        slide.dataset.total = slides.length;
    });
    document.body.append(shortcuts);
    updateSlide(...positionFromHash(), true);
});

// How many steps a slide has, as the deck counted them.
function stepCount(i) {
    return Number(slides[i].dataset.count) || 1;
}

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
// which would bring in at once all it shows. Its animations play as it opens,
// unless it opens `finished`, as if stepped through already, as when coming
// back to where the URL says, or to a slide's end, where they are over.
function updateSlide(i, step = 1, finished = false) {
    if (i < 0 || i >= slides.length) return;
    const slide = slides[i];
    slide.classList.add("steps-instant");
    slides[currentSlide].style.display = "none";
    stopAnimations(slides[currentSlide]);
    slide.style.display = "block";
    currentSlide = i;
    currentStep = Math.max(1, Math.min(step, stepCount(i)));
    showStep(currentStep, finished);
    // Laid out before the transitions come back, so that none of them runs.
    slide.getBoundingClientRect();
    slide.classList.remove("steps-instant");
    showPosition();
}

// Shows the slide at the step, as slides.css lays it out: hidden, an element
// keeps its space, so the columns keep their place, unless it collapses, when
// it takes none, so another can show in its place. Disabled, every element
// shows, even those whose steps are over. currentStep is tracked even while
// disabled, so re-enabling resumes here.
function showStep(i, finished = false) {
    if (i < 1 || i > stepCount(currentSlide)) return false;
    currentStep = i;
    document.documentElement.classList.toggle("steps-off", !stepsEnabled);
    showSteps(slides[currentSlide], i);
    playAnimations(slides[currentSlide], finished);
    showPosition();
    return true;
}

// Disabled steps are all visible already, so stepping through them is a
// no-op: report failure and let the caller move a whole slide instead.
function moveToStep(i) {
    return stepsEnabled && showStep(i);
}

// The keys the page answers to, which `?` lists, in a dialog of its own,
// rather than a slide, which is a section.
const SHORTCUTS = [
    ["→", "Next step, or next slide"],
    ["←", "Previous step, or previous slide"],
    ["↓", "End of this slide, or next slide"],
    ["↑", "Start of this slide, or previous slide"],
    ["A", "Show every step, or step again"],
    ["Esc", "Overview: click a slide, or pick it with the arrows and Enter"],
    ["?", "Show or hide these shortcuts"],
];

const shortcuts = document.createElement("dialog");
shortcuts.className = "shortcuts";
shortcuts.setAttribute("aria-label", "Keyboard shortcuts");
shortcuts.innerHTML = "<h2>Keyboard shortcuts</h2><dl>" +
    SHORTCUTS.map(([key, action]) => `<dt><kbd>${key}</kbd></dt><dd>${action}</dd>`).join("") +
    "</dl>";
// A click outside it, on its backdrop, closes it, as Escape does.
shortcuts.addEventListener("click", (event) => {
    if (event.target == shortcuts) shortcuts.close();
});

// The overview, which Escape toggles: every slide at once, shrunk, as
// slides.css lays them out, each at its end, as if stepped through, but the
// one we were on, as it is. One is chosen, as the arrows move, starting from
// the one we were on, to open with Enter, or Escape again, or with a click.
let overview = false;
let chosen = 0;

function openOverview() {
    overview = true;
    document.documentElement.classList.add("overview");
    [...slides].forEach((slide, i) => {
        if (i == currentSlide) return;
        showSteps(slide, stepCount(i));
        playAnimations(slide, true);
    });
    choose(currentSlide);
}

function choose(i) {
    slides[chosen].classList.remove("chosen");
    chosen = Math.max(0, Math.min(i, slides.length - 1));
    slides[chosen].classList.add("chosen");
    slides[chosen].scrollIntoView({ block: "nearest" });
}

// Opens slide `i`, at its start, or as it was if it is the one we were on.
function closeOverview(i) {
    overview = false;
    document.documentElement.classList.remove("overview");
    slides[chosen].classList.remove("chosen");
    // Scrolled, the page would show the slide out of its frame.
    document.body.scrollTop = 0;
    [...slides].forEach((slide, j) => {
        if (j != currentSlide) stopAnimations(slide);
    });
    if (i != currentSlide) updateSlide(i);
}

// How many slides a row of the overview has, as slides.css says.
function overviewColumns() {
    return Number(getComputedStyle(document.documentElement).getPropertyValue("--overview-columns")) || 1;
}

function overviewKey(event) {
    const columns = overviewColumns();
    const move = { ArrowRight: 1, ArrowLeft: -1, ArrowDown: columns, ArrowUp: -columns }[event.code];
    if (move) {
        event.preventDefault();
        choose(chosen + move);
    } else if (event.code == "Enter" || event.code == "Escape") {
        closeOverview(chosen);
    }
}

// What is on a slide in the overview takes no clicks, which go to the slide.
addEventListener("click", (event) => {
    if (!overview) return;
    const slide = event.target.closest?.("section");
    if (slide) closeOverview([...slides].indexOf(slide));
});

// Whether a key pressed is the page's to step with: not one held with a
// modifier, as the browser's own shortcuts are, as Alt+Left goes back, nor one
// pressed in what takes text or arrows itself, as a widget's input does.
function forSlides(event) {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return false;
    const target = event.composedPath()[0];
    return !(target instanceof Element && target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])"));
}

addEventListener("keydown", (event) => {
    if (!forSlides(event)) return;
    // `?` is where the keyboard puts it, which `code` does not say.
    if (event.key == "?") {
        shortcuts.open ? shortcuts.close() : shortcuts.showModal();
        return;
    }
    // The slides stay as they are while the shortcuts show.
    if (shortcuts.open) return;
    if (overview) return overviewKey(event);
    if (event.code == "ArrowRight") {
        if (!moveToStep(currentStep + 1) && !rushAnimations(slides[currentSlide])) {
            updateSlide(currentSlide + 1);
        }
    } else if (event.code == "ArrowLeft") {
        if (!moveToStep(currentStep - 1) && !rushAnimations(slides[currentSlide])) {
            updateSlide(currentSlide - 1, Infinity, true);
        }
    } else if (event.code == "ArrowDown") {
        // Alternate: finish this slide, then open the next one.
        if (stepsEnabled && currentStep < stepCount(currentSlide)) {
            showStep(stepCount(currentSlide));
        } else if (!rushAnimations(slides[currentSlide])) {
            updateSlide(currentSlide + 1, 1);
        }
    } else if (event.code == "ArrowUp") {
        // Alternate: go back to the start of this slide, then to the previous one.
        if (stepsEnabled && currentStep > 1) {
            showStep(1);
        } else if (!rushAnimations(slides[currentSlide])) {
            updateSlide(currentSlide - 1, Infinity, true);
        }
    } else if (event.code == "Escape") {
        openOverview();
    } else if (event.code == "KeyA") {
        // Toggle: off reveals the whole slide, on returns to where we were.
        stepsEnabled = !stepsEnabled;
        showStep(currentStep);
    }
});

// The URL can also be edited, or gone back through.
addEventListener("hashchange", () => {
    if (overview) closeOverview(currentSlide);
    updateSlide(...positionFromHash(), true);
});
