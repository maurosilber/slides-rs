// The speaker view, which `S` opens on the deck, in a window of its own, as
// `?speaker`: the slide as the deck shows it, the step that comes next, the
// speaker's notes, and a timer, which says how long each slide left has. It is
// the deck's page too, whose slides it keeps hidden, for the notes in them, and
// shows the slides as pages of the deck of their own, as `?preview`, which
// follow it. slides.js comes before this file, with where the deck is.

// The window of the deck, which it follows, and sends the keys pressed in it
// to, or none, if it was opened on its own, when it steps by itself.
let deck = window.opener;

// The slides as the deck shows them, and the step after it.
let current;
let next;

// The notes, and where the slide is.
let notes;
let where;

// The timer: the field it is set in, the countdown and what it says of the pace.
let target;
let toggle;
let countdown;
let elapsed;
let pace;
let budget;
let clock;

const KEYS = ["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp", "Escape", "Enter", "KeyA"];

const SPEAKER_SHORTCUTS = [
    ["→", "Next step, or next slide"],
    ["←", "Previous step, or previous slide"],
    ["↓", "End of this slide, or next slide"],
    ["↑", "Start of this slide, or previous slide"],
    ["A", "Show every step, or step again"],
    ["Esc", "Overview, on the deck: pick a slide with the arrows and Enter"],
    ["T", "Start, pause or resume the timer"],
    ["?", "Show or hide these shortcuts"],
];

const speakerShortcuts = shortcutsDialog(SPEAKER_SHORTCUTS);

function openSpeakerView() {
    document.documentElement.classList.add("speaker");
    document.title = "Speaker view";
    const view = document.createElement("main");
    view.className = "speaker-view";
    view.innerHTML = `
        <div class="speaker-main">
            <div class="speaker-slide speaker-current"><iframe tabindex="-1" title="The slide shown"></iframe></div>
            <div class="speaker-notes" aria-live="polite"></div>
        </div>
        <div class="speaker-side">
            <div class="speaker-slide speaker-next">
                <iframe tabindex="-1" title="What comes next"></iframe>
                <p class="speaker-end">End of the deck</p>
            </div>
            <p class="speaker-where"><span class="speaker-position"></span><time class="speaker-clock"></time></p>
            <form class="speaker-timer">
                <label>Duration or deadline
                    <input name="target" placeholder="20m, 1h30m or 15:30" autocomplete="off" spellcheck="false">
                </label>
                <output class="speaker-countdown"></output>
                <p class="speaker-elapsed"></p>
                <p class="speaker-pace"></p>
                <p class="speaker-budget"></p>
                <div class="speaker-buttons">
                    <button type="button" name="toggle"></button>
                    <button type="button" name="reset">Reset</button>
                </div>
            </form>
        </div>`;
    document.body.append(view, speakerShortcuts);
    const find = (selector) => view.querySelector(selector);
    current = find(".speaker-current iframe");
    next = find(".speaker-next iframe");
    notes = find(".speaker-notes");
    where = find(".speaker-position");
    clock = find(".speaker-clock");
    target = find("input");
    toggle = find("[name=toggle]");
    countdown = find(".speaker-countdown");
    elapsed = find(".speaker-elapsed");
    pace = find(".speaker-pace");
    budget = find(".speaker-budget");

    const form = find("form");
    form.addEventListener("submit", (event) => {
        event.preventDefault();
        target.blur();
    });
    target.addEventListener("change", () => setTarget(target.value));
    toggle.addEventListener("click", toggleTimer);
    find("[name=reset]").addEventListener("click", resetTimer);

    restoreTimer();
    const [slide, step] = positionFromHash();
    const page = new URL(location.href);
    page.search = "?preview";
    for (const frame of [current, next]) {
        frame.addEventListener("load", showSlides);
        page.hash = `${slide + 1}.${step}`;
        frame.src = page.href;
    }
    follow(slide, step, true);
    greetDeck();
    setInterval(greetDeck, 1000);
    setInterval(showTimer, 250);
}

// The deck answers with where it is, and finds the speaker view again, as
// once it reloads.
function greetDeck() {
    if (hasDeck()) deck.postMessage({ slides: "speaker" }, "*");
}

function hasDeck() {
    return deck && !deck.closed;
}

addEventListener("message", (event) => {
    const message = event.data;
    if (speaker && message?.slides == "deck") {
        deck = event.source;
        follow(message.slide, message.step, message.steps);
    } else if (preview && event.source == parent && message?.slides == "preview") {
        goTo(message.slide, message.step, message.steps, message.finished);
    }
});

function speakerKey(event) {
    if (event.key == "?") {
        speakerShortcuts.open ? speakerShortcuts.close() : speakerShortcuts.showModal();
        return;
    }
    if (speakerShortcuts.open) return;
    if (event.code == "KeyT") return toggleTimer();
    if (!KEYS.includes(event.code)) return;
    event.preventDefault();
    if (hasDeck()) {
        deck.postMessage({ slides: "speaker", key: { code: event.code, key: event.key } }, "*");
    } else {
        stepAlone(event.code);
    }
}

// Steps as the deck would, without one to step, as when the speaker view is
// opened on its own.
function stepAlone(code) {
    const count = stepCount(currentSlide);
    const last = slides.length - 1;
    const at = (slide, step) => follow(slide, step, stepsEnabled);
    const previous = () => currentSlide > 0 && at(currentSlide - 1, stepCount(currentSlide - 1));
    if (code == "ArrowRight") {
        if (stepsEnabled && currentStep < count) at(currentSlide, currentStep + 1);
        else if (currentSlide < last) at(currentSlide + 1, 1);
    } else if (code == "ArrowLeft") {
        if (stepsEnabled && currentStep > 1) at(currentSlide, currentStep - 1);
        else previous();
    } else if (code == "ArrowDown") {
        if (stepsEnabled && currentStep < count) at(currentSlide, count);
        else if (currentSlide < last) at(currentSlide + 1, 1);
    } else if (code == "ArrowUp") {
        if (stepsEnabled && currentStep > 1) at(currentSlide, 1);
        else previous();
    } else if (code == "KeyA") {
        follow(currentSlide, currentStep, !stepsEnabled);
    }
}

// Where the speaker view was, to tell a move from the deck saying again
// where it is.
let shown = null;

// Shows the slide and step the deck is at, with its notes, and the step
// after it. The first move of all starts the timer.
function follow(slide, step, steps) {
    const position = `${slide}.${step}.${steps}`;
    if (position == shown) return;
    const moved = shown != null;
    const entered = slide != currentSlide || shown == null;
    shown = position;
    currentSlide = slide;
    currentStep = step;
    stepsEnabled = steps;
    history.replaceState(null, "", "#" + (slide + 1) + "." + step);
    if (entered) showNotes();
    // Reloaded, the slide keeps the time it had.
    if (entered && (moved || timer.slide != slide)) planSlide();
    markNotes();
    showSlides();
    if (moved && timer.started == null) toggleTimer();
    showTimer();
}

// The step after the one shown: the next of its slide, or the next slide's
// first, or none, at the end of the deck.
function stepAfter() {
    if (stepsEnabled && currentStep < stepCount(currentSlide)) {
        return [currentSlide, currentStep + 1];
    }
    return currentSlide + 1 < slides.length ? [currentSlide + 1, 1] : null;
}

function showSlides() {
    const tell = (frame, slide, step, finished) =>
        frame.contentWindow?.postMessage(
            { slides: "preview", slide, step, steps: stepsEnabled, finished },
            "*",
        );
    tell(current, currentSlide, currentStep, false);
    const after = stepAfter();
    next.parentElement.classList.toggle("ended", !after);
    if (after) tell(next, ...after, true);
    const count = stepCount(currentSlide);
    where.textContent = `Slide ${currentSlide + 1} of ${slides.length}` +
        (stepsEnabled && count > 1 ? ` · step ${currentStep} of ${count}` : "");
}

// The notes of the slide shown: its asides, copied out of it, each showing
// from the step its `--from` says, up to its `--to`, as the deck numbered
// them, or all along.
function showNotes() {
    notes.replaceChildren();
    for (const aside of slides[currentSlide].querySelectorAll("aside")) {
        const note = aside.cloneNode(true);
        note.classList.remove("step-hidden");
        const style = aside.style;
        note.dataset.from = style.getPropertyValue("--from").trim() || 1;
        note.dataset.to = style.getPropertyValue("--to").trim() || Infinity;
        notes.append(note);
    }
    notes.classList.toggle("empty", !notes.children.length);
    notes.scrollTop = 0;
}

// Marks the notes of steps to come, and of steps over, apart from those
// of the step shown, and scrolls the last of those into view.
function markNotes() {
    let latest = null;
    for (const note of notes.children) {
        const from = Number(note.dataset.from);
        const to = Number(note.dataset.to);
        const coming = stepsEnabled && from > currentStep;
        const over = stepsEnabled && to <= currentStep;
        note.classList.toggle("coming", coming);
        note.classList.toggle("over", over);
        if (!coming && !over) latest = note;
    }
    latest?.scrollIntoView({ block: "nearest", behavior: "smooth" });
}

// The timer counts down to the end of the talk: the deadline, today, or the
// duration after it started, but for the time it was paused. What is left
// of it is shared equally among the slides left, the one shown too, each
// time one is opened, and the speaker is told to speed up once the slide
// shown takes longer than its share.
const timer = {
    // What the field says, as the frontmatter set it, unless it was changed,
    // and what the frontmatter set.
    target: defaultTarget(),
    written: defaultTarget(),
    // When it started, was paused, and how long it was paused before, in ms.
    started: null,
    paused: null,
    pausedFor: 0,
    // The slide shown, when, in the time of the talk, it was opened, and how
    // long it has, in ms.
    slide: null,
    entered: 0,
    share: null,
};

// The target the frontmatter sets, as `duration` or `deadline`, which the
// page's root has, as a number of seconds and as `15:30`.
function defaultTarget() {
    const { duration, deadline } = document.documentElement.dataset;
    if (deadline) return deadline;
    if (!duration) return "";
    const seconds = Number(duration);
    const [h, m, s] = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60];
    return [h && `${h}h`, m && `${m}m`, s && `${s}s`].filter(Boolean).join("");
}

// A target, as src/talk.rs reads one: a deadline, as `15:30`, or a duration,
// as a number of minutes, or in hours, minutes and seconds, as `1h30m`,
// `45min` or `90s`. Returns the deadline's hour and minute, or the
// duration's seconds, or nothing, if it is neither.
function parseTarget(text) {
    text = text.trim();
    const time = text.match(/^(\d{1,2}):(\d{2})$/);
    if (time) {
        const [hour, minute] = [Number(time[1]), Number(time[2])];
        return hour < 24 && minute < 60 ? { deadline: [hour, minute] } : null;
    }
    let seconds;
    if (/^\d+(\.\d+)?$/.test(text)) {
        seconds = Number(text) * 60;
    } else if (/^(\d+(\.\d+)?\s*(h|min|m|s)\s*)+$/.test(text)) {
        const units = { h: 3600, min: 60, m: 60, s: 1 };
        seconds = 0;
        for (const [, number, , unit] of text.matchAll(/(\d+(\.\d+)?)\s*(h|min|m|s)/g)) {
            seconds += Number(number) * units[unit];
        }
    }
    return seconds >= 1 ? { duration: Math.round(seconds) } : null;
}

// How long the talk has gone on, in ms, but for the time it was paused.
function talkTime(now) {
    if (timer.started == null) return 0;
    return (timer.paused ?? now) - timer.started - timer.pausedFor;
}

// How long is left of the talk, in ms, as the target says, or nothing, if
// it says nothing.
function remaining(now) {
    const target = parseTarget(timer.target);
    if (!target) return null;
    if (target.duration) return target.duration * 1000 - talkTime(now);
    // Today, or tomorrow, if it was more than half a day ago, as a talk after
    // midnight's.
    const [hour, minute] = target.deadline;
    const end = new Date(now);
    end.setHours(hour, minute, 0, 0);
    if (end - now < -12 * 3600 * 1000) end.setDate(end.getDate() + 1);
    return end - now;
}

// Shares what is left equally among the slides left, the one shown too, as
// it opens, or, once it is open, as the target changes, what was left as it
// opened, which keeps the time it has had.
function planSlide(opened = false) {
    const now = Date.now();
    const talk = talkTime(now);
    if (!opened) {
        timer.slide = currentSlide;
        timer.entered = talk;
    }
    const left = remaining(now);
    const atOpen = left + talk - timer.entered;
    timer.share = left == null ? null : Math.max(atOpen, 0) / (slides.length - currentSlide);
    saveTimer();
}

function setTarget(text) {
    timer.target = text.trim();
    target.setCustomValidity(timer.target && !parseTarget(timer.target) ? "As 20, 45m, 1h30m or 15:30" : "");
    target.reportValidity();
    planSlide(true);
    showTimer();
}

function toggleTimer() {
    const now = Date.now();
    if (timer.started == null) {
        timer.started = now;
        planSlide();
    } else if (timer.paused != null) {
        timer.pausedFor += now - timer.paused;
        timer.paused = null;
    } else {
        timer.paused = now;
    }
    saveTimer();
    showTimer();
}

function resetTimer() {
    Object.assign(timer, { started: null, paused: null, pausedFor: 0 });
    planSlide();
    showTimer();
}

// `h:mm:ss`, or `m:ss` under an hour, with a sign if it is negative.
function duration(ms) {
    const seconds = Math.round(Math.abs(ms) / 1000);
    const [h, m, s] = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60];
    const pad = (number) => String(number).padStart(2, "0");
    const time = h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
    return (ms < 0 && seconds ? "−" : "") + time;
}

function showTimer() {
    const now = Date.now();
    clock.textContent = new Date(now).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (document.activeElement != target) target.value = timer.target;
    toggle.textContent = timer.started == null ? "Start" : timer.paused != null ? "Resume" : "Pause";
    const left = remaining(now);
    const talk = talkTime(now);
    countdown.textContent = duration(left ?? talk);
    countdown.title = left == null ? "Elapsed" : "Left";
    elapsed.textContent = left == null ? "elapsed" : `left · ${duration(talk)} elapsed`;
    const spent = talk - timer.entered;
    let state = "";
    if (left == null) {
        pace.textContent = "Set a duration or a deadline to pace the talk.";
        budget.textContent = "";
    } else {
        if (left < 0) state = "over";
        else if (timer.started != null && spent > timer.share) state = "behind";
        pace.textContent = { over: "Over time", behind: "Speed up", "": "On time" }[state];
        budget.textContent = `This slide: ${duration(spent)} of ${duration(timer.share)}`;
    }
    document.documentElement.dataset.pace = state;
    document.documentElement.classList.toggle("paused", timer.paused != null);
}

// The timer is kept for the window, to go on as it was once the page reloads,
// as when the deck is rendered again.
const TIMER_KEY = "slides-rs timer " + location.pathname;

function saveTimer() {
    try {
        sessionStorage.setItem(TIMER_KEY, JSON.stringify(timer));
    } catch {}
}

// A target the frontmatter sets anew wins over the one the field had.
function restoreTimer() {
    try {
        const saved = JSON.parse(sessionStorage.getItem(TIMER_KEY));
        if (saved?.written != timer.written) delete saved?.target;
        Object.assign(timer, saved ?? {}, { written: timer.written });
    } catch {}
}
