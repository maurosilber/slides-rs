const slides = document.getElementsByTagName("section");

// The page is the deck, as the audience sees it, unless the URL asks for the
// speaker view, as `?speaker`, which speaker.js lays out, or for one of the
// slides it shows, as `?preview`, which follows what the speaker view says.
const params = new URLSearchParams(location.search);
const speaker = params.has("speaker");
const preview = params.has("preview");

addEventListener("DOMContentLoaded", () => {
    // Every slide knows where it is in the deck, for slides.css to count them,
    // which CSS counters cannot, as the slides not shown are not displayed.
    [...slides].forEach((slide, i) => {
        slide.dataset.number = i + 1;
        slide.dataset.total = slides.length;
    });
    if (speaker) return openSpeakerView();
    document.documentElement.classList.toggle("preview", preview);
    if (!preview) document.body.append(shortcuts);
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

// It is written once the moves stop for a moment, as a browser limits how
// often a page writes it, as while a key is held down: Chrome lets some go,
// and Safari throws. A slide the speaker view shows keeps none.
let writing = null;

function showPosition() {
    tellSpeaker();
    if (preview) return;
    clearTimeout(writing);
    writing = setTimeout(writePosition, 300);
}

function writePosition() {
    clearTimeout(writing);
    try {
        history.replaceState(null, "", "#" + (currentSlide + 1) + "." + currentStep);
    } catch {}
}

// Written before the page goes, as it reloads after a rebuild.
addEventListener("pagehide", writePosition);

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

// Shows slide `i` at `step`, as the deck the speaker view follows is: stepped
// to, if it is the slide shown, or opened, as if stepped through already if it
// is before it, or `finished`.
function goTo(i, step, steps, finished = false) {
    stepsEnabled = steps;
    if (i == currentSlide && !finished) {
        showStep(Math.max(1, Math.min(step, stepCount(i))));
    } else {
        updateSlide(i, step, finished || i < currentSlide);
    }
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
    ["S", "Speaker view, in a window of its own: the notes, what comes next, and a timer"],
    ["?", "Show or hide these shortcuts"],
];

// The dialog that lists `keys`, each with what it does.
function shortcutsDialog(keys) {
    const dialog = document.createElement("dialog");
    dialog.className = "shortcuts";
    dialog.setAttribute("aria-label", "Keyboard shortcuts");
    dialog.innerHTML = "<h2>Keyboard shortcuts</h2><dl>" +
        keys.map(([key, action]) => `<dt><kbd>${key}</kbd></dt><dd>${action}</dd>`).join("") +
        "</dl>";
    // A click outside it, on its backdrop, closes it, as Escape does.
    dialog.addEventListener("click", (event) => {
        if (event.target == dialog) dialog.close();
    });
    return dialog;
}

const shortcuts = shortcutsDialog(SHORTCUTS);

// The overview, which Escape toggles: every slide in a row, shrunk, as
// slides.css lays them out, each at its end, as if stepped through, but the
// one we were on, as it is. One is chosen, as ← and → move, starting from
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
    // Centred as it is shown, shrunk, which scrollIntoView does not see.
    const slide = slides[chosen].getBoundingClientRect();
    const row = document.body.getBoundingClientRect();
    document.body.scrollLeft += slide.left + slide.width / 2 - (row.left + document.body.clientWidth / 2);
}

// Opens slide `i`, at its start, or as it was if it is the one we were on.
function closeOverview(i) {
    overview = false;
    document.documentElement.classList.remove("overview");
    slides[chosen].classList.remove("chosen");
    // Scrolled, the page would show the slide out of its frame.
    document.body.scrollLeft = 0;
    [...slides].forEach((slide, j) => {
        if (j != currentSlide) stopAnimations(slide);
    });
    if (i != currentSlide) updateSlide(i);
}

function overviewKey(event) {
    const move = { ArrowRight: 1, ArrowLeft: -1 }[event.code];
    if (move) {
        event.preventDefault?.();
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

// Resized, the frame keeps the one chosen in its middle.
addEventListener("resize", () => {
    if (overview) choose(chosen);
});

// The row scrolls sideways, as a wheel turned up and down does too.
addEventListener("wheel", (event) => {
    if (!overview || event.deltaX) return;
    document.body.scrollLeft += event.deltaY;
}, { passive: true });

// Whether a key pressed is the page's to step with: not one held with a
// modifier, as the browser's own shortcuts are, as Alt+Left goes back, nor one
// pressed in what takes text or arrows itself, as a widget's input does.
function forSlides(event) {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey) return false;
    const target = event.composedPath()[0];
    return !(target instanceof Element && target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])"));
}

addEventListener("keydown", (event) => {
    // A slide the speaker view shows follows it, rather than the keys.
    if (preview || !forSlides(event)) return;
    if (speaker) return speakerKey(event);
    // `?` is where the keyboard puts it, which `code` does not say.
    if (event.key == "?") {
        shortcuts.open ? shortcuts.close() : shortcuts.showModal();
        return;
    }
    press(event);
});

// Answers a key pressed on the deck, or in the speaker view, which sends it
// here, as `{ code, key }`, rather than an event.
function press(event) {
    // The slides stay as they are while the shortcuts show.
    if (shortcuts.open) return;
    if (overview) return overviewKey(event);
    if (event.code == "KeyS") {
        openSpeakerWindow();
    } else if (event.code == "ArrowRight") {
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
}

// The speaker view, in a window the deck opens, which the deck tells where
// it is, and which sends the keys pressed in it, for the deck to answer as
// its own. A message is all a window can send another opened from a file, as
// its origin is none, and the speaker view says it is there every second, for
// the deck to find it again once it reloads.
let speakerWindow = null;

function openSpeakerWindow() {
    if (speakerWindow && !speakerWindow.closed) return speakerWindow.focus();
    const url = new URL(location.href);
    url.search = "?speaker";
    // Named after the deck, for `S` to find it again, rather than open another.
    speakerWindow = open(url, "speaker " + location.pathname, "popup,width=1280,height=800");
}

function tellSpeaker() {
    if (!speakerWindow || speakerWindow.closed) return;
    const position = { slide: currentSlide, step: currentStep, steps: stepsEnabled };
    speakerWindow.postMessage({ slides: "deck", ...position }, "*");
}

addEventListener("message", (event) => {
    const message = event.data;
    if (speaker || preview || message?.slides != "speaker") return;
    speakerWindow = event.source;
    if (message.key) press(message.key);
    else tellSpeaker();
});

// The URL can also be edited, or gone back through.
addEventListener("hashchange", () => {
    if (overview) closeOverview(currentSlide);
    updateSlide(...positionFromHash(), true);
});

// Opened by the editor with `?follow=port/id`, the page goes to the slide and
// step the editor's cursor is at, as it moves, which the editor sends as
// `slide.step` from a server of its own on this machine.
const follow = new URLSearchParams(location.search).get("follow");
if (follow && /^\d+\/\w+$/.test(follow)) {
    new EventSource("http://127.0.0.1:" + follow).onmessage = (event) => {
        if (location.hash.slice(1) !== event.data) location.replace("#" + event.data);
    };
}
