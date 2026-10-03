"""Widgets that drive the motions of a figure, in a slides-rs deck or a notebook.

A :class:`Slider` shows a figure along with a slider, as html, which scrubs
through the figure's motions rather than the deck playing them: the slider's
value is the time, in seconds, the motions are at. Its methods set it up,
each returning the slider, for the next one::

    from slides_rs import Motion, Slider

    t = np.linspace(0, 2 * np.pi, 200)
    (line,) = plt.plot(t, np.sin(t))
    (dot,) = plt.plot(0, 0, "o")
    Slider(plt.gcf(), Motion(dot, along=line).timing(t)).label("t").range(step=0.1)

The motions are timed as they are for the deck, so that ``timing(t)`` with
the times a line was plotted at puts the artist where the line was at the
slider's value.
"""

from __future__ import annotations

import contextlib
import hashlib
import html
import io
import json
import numbers
import re
import xml.etree.ElementTree as ET
from collections.abc import Mapping
from typing import Literal

import matplotlib
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.figure import Figure

from .motion import Motion

__all__ = ["Slider"]

SVG = "http://www.w3.org/2000/svg"
XLINK = "http://www.w3.org/1999/xlink"
ET.register_namespace("", SVG)
ET.register_namespace("xlink", XLINK)

#: An id that marks a step, as src/step.rs reads it: ``step=2..4`` or
#: ``step``, starred if it collapses.
STEP = re.compile(r"^step(\*)?(?:=(.*))?$")

#: The parts of a slider :meth:`Slider.style` styles, by the element each is.
PARTS = {
    "slider": "div",
    "figure": "svg",
    "controls": "div",
    "button": "button",
    "label": "label",
    "input": "input",
    "output": "output",
}
Part = Literal["slider", "figure", "controls", "button", "label", "input", "output"]

#: The custom properties the stylesheet reads, which :meth:`Slider.style` sets on
#: the slider by these names too, as ``accent="tomato"``.
VARIABLES = ("accent", "track", "thumb", "text", "track-height", "thumb-size")

#: How a slider looks, the same for every one, as a theme's colors say, or else
#: the light theme's. Within ``:where()``, a deck's stylesheet or a slider's
#: :meth:`~Slider.style` restyles it as simply as ``.slides-rs-slider``, as with
#: its custom properties: ``.slides-rs-slider { --slider-accent: tomato }``. The
#: figure is not, as it is to be sized over the theme's figures, which a deck's
#: stylesheet restyles as ``.slides-rs-slider > svg``.
CSS = """\
:where(.slides-rs-slider) {
  --slider-accent: var(--accent, #2563eb);
  --slider-track: var(--rule, #d0d7de);
  --slider-thumb: var(--bg, #ffffff);
  --slider-text: var(--muted, #59636e);
  --slider-track-height: 0.3em;
  --slider-thumb-size: 1.1em;
  display: flex;
  flex-direction: column;
  width: fit-content;
  max-width: 100%;
  margin: 0.4em auto;
}
.slides-rs-slider > svg {
  margin: 0 auto;
  max-height: 60cqh;
}
:where(.slides-rs-slider-controls) {
  display: flex;
  align-items: center;
  gap: 0.6em;
  padding: 0.3em 0.2em 0;
  font-size: 0.65em;
  color: var(--slider-text);
  font-variant-numeric: tabular-nums;
}
:where(.slides-rs-slider-controls) button {
  flex: none;
  display: grid;
  place-items: center;
  width: 2em;
  height: 2em;
  padding: 0;
  border: 0.12em solid var(--slider-accent);
  border-radius: 50%;
  background: transparent;
  color: var(--slider-accent);
  cursor: pointer;
}
:where(.slides-rs-slider-controls) button:hover {
  background: color-mix(in srgb, var(--slider-accent) 15%, transparent);
}
:where(.slides-rs-slider-controls) button::before {
  content: "";
  width: 0.7em;
  height: 0.8em;
  margin-left: 0.15em;
  background: currentColor;
  clip-path: polygon(0 0, 100% 50%, 0 100%);
}
:where(.slides-rs-slider-controls) button[aria-pressed="true"]::before {
  width: 0.65em;
  margin-left: 0;
  box-sizing: border-box;
  border-inline: 0.22em solid currentColor;
  background: transparent;
  clip-path: none;
}
:where(.slides-rs-slider-controls) label {
  flex: none;
  font-style: italic;
}
:where(.slides-rs-slider-controls) input {
  --progress: 0%;
  flex: 1;
  min-width: 6em;
  height: var(--slider-thumb-size);
  margin: 0;
  background: transparent;
  accent-color: var(--slider-accent);
  cursor: pointer;
  appearance: none;
}
:where(.slides-rs-slider-controls) input::-webkit-slider-runnable-track {
  height: var(--slider-track-height);
  border-radius: 999px;
  background: linear-gradient(to right, var(--slider-accent) var(--progress), var(--slider-track) var(--progress));
}
:where(.slides-rs-slider-controls) input::-moz-range-track {
  height: var(--slider-track-height);
  border-radius: 999px;
  background: var(--slider-track);
}
:where(.slides-rs-slider-controls) input::-moz-range-progress {
  height: var(--slider-track-height);
  border-radius: 999px;
  background: var(--slider-accent);
}
:where(.slides-rs-slider-controls) input::-webkit-slider-thumb {
  width: var(--slider-thumb-size);
  height: var(--slider-thumb-size);
  margin-top: calc((var(--slider-track-height) - var(--slider-thumb-size)) / 2);
  border: 0.2em solid var(--slider-accent);
  border-radius: 50%;
  background: var(--slider-thumb);
  appearance: none;
}
:where(.slides-rs-slider-controls) input::-moz-range-thumb {
  box-sizing: border-box;
  width: var(--slider-thumb-size);
  height: var(--slider-thumb-size);
  border: 0.2em solid var(--slider-accent);
  border-radius: 50%;
  background: var(--slider-thumb);
}
:where(.slides-rs-slider-controls) :focus-visible {
  outline: 0.12em solid var(--slider-accent);
  outline-offset: 0.2em;
}
:where(.slides-rs-slider-controls) input:focus-visible {
  outline: none;
}
:where(.slides-rs-slider-controls) input:focus-visible::-webkit-slider-thumb {
  box-shadow: 0 0 0 0.2em color-mix(in srgb, var(--slider-accent) 35%, transparent);
}
:where(.slides-rs-slider-controls) input:focus-visible::-moz-range-thumb {
  box-shadow: 0 0 0 0.2em color-mix(in srgb, var(--slider-accent) 35%, transparent);
}
:where(.slides-rs-slider-controls) output {
  flex: none;
  text-align: right;
  font-family: var(--font-mono, ui-monospace, monospace);
}
"""

#: Scrubs the figure's timeline as the slider moves, and plays it with the button.
#: The slider goes through ticks, from 0, each ``step`` seconds from ``start``:
#: an input with a ``step`` attribute would be a step of the deck.
SCRIPT = """\
(() => {
  const root = document.currentScript.parentElement;
  const svg = root.querySelector(":scope > svg");
  const input = root.querySelector("input");
  const output = root.querySelector("output");
  const button = root.querySelector("button");
  const { start, stop, step, digits, speed } = %(options)s;
  let time = start + Number(input.value) * step;
  const show = () => {
    svg.setCurrentTime(time);
    output.value = time.toFixed(digits);
    input.style.setProperty("--progress", `${(100 * input.value) / input.max}%%`);
  };
  let playing = null;
  const pause = () => {
    cancelAnimationFrame(playing);
    playing = null;
    button?.setAttribute("aria-pressed", "false");
  };
  button?.addEventListener("click", () => {
    if (playing !== null) return pause();
    if (time >= stop) time = start;
    let last = performance.now();
    const frame = (now) => {
      time = Math.min(time + (speed * (now - last)) / 1000, stop);
      last = now;
      input.value = Math.round((time - start) / step);
      show();
      if (time >= stop) return pause();
      playing = requestAnimationFrame(frame);
    };
    button.setAttribute("aria-pressed", "true");
    playing = requestAnimationFrame(frame);
  });
  input.addEventListener("input", () => {
    pause();
    time = start + Number(input.value) * step;
    show();
  });
  // The keys that move the slider do not step the deck.
  root.addEventListener("keydown", (event) => event.stopPropagation());
  svg.pauseAnimations();
  show();
})();
"""


class Slider:
    """Shows ``figure`` with a slider that sets the time its ``motions`` are at.

    As made, it goes from 0 to the end of the longest motion, in a hundred
    ticks, starting at 0, unlabeled, with a button that plays through it in
    real time, as its methods change. Each method returns the slider, for the
    next one::

        Slider(figure, motion).range(0, 10, step=0.5).value(2).label("t").play(speed=2)

    The figure is shown as html, which ``_repr_html_`` returns, with the
    slider: rather than the deck, the slider moves the motions, which begin
    with the figure, and the steps the deck reveals the figure in are only
    the artists'. It is closed, for pyplot not to show it once more, and
    drawn each time the slider is shown, as it is then, as are the motions:
    what changes after it is made takes effect the next time it is shown.
    """

    def __init__(self, figure: Figure, *motions: Motion):
        if not motions:
            raise ValueError("a slider drives one motion or more")
        for motion in motions:
            if not isinstance(motion, Motion):
                raise TypeError(f"a slider drives motions, not {motion!r}")
            if motion.artist.get_figure(root=True) is not figure:
                raise ValueError(f"{motion!r} is not in the figure")
        self.figure = figure
        self.motions = motions
        self._start, self._stop, self._step = 0.0, None, None
        self._value: float | None = None
        self._label: str | None = None
        self._speed: float | None = 1.0
        self._styles: dict[str, dict[str, str]] = {part: {} for part in PARTS}
        plt.close(figure)

    def __repr__(self):
        return f"Slider({self.figure!r}, {', '.join(map(repr, self.motions))})"

    def range(self, start: float = 0, stop: float | None = None, *, step: float | None = None) -> Slider:
        """Goes from ``start`` to ``stop`` seconds, or the end of the longest motion,
        by ``step``, or in a hundred ticks."""
        if not _real(start):
            raise ValueError(f"the slider starts at a number, not {start!r}")
        if not (stop is None or (_real(stop) and stop > start)):
            raise ValueError(f"the slider goes from {start} up to a later stop, not {stop!r}")
        if not (step is None or (_real(step) and step > 0)):
            raise ValueError(f"the slider goes by a positive step, not {step!r}")
        self._start = float(start)
        self._stop = None if stop is None else float(stop)
        self._step = None if step is None else float(step)
        return self

    def value(self, value: float | None) -> Slider:
        """Starts at ``value`` seconds, the tick closest to it, or, with ``None``,
        at the start."""
        if not (value is None or _real(value)):
            raise ValueError(f"the slider starts at a number, not {value!r}")
        self._value = None if value is None else float(value)
        return self

    def label(self, text: str | None) -> Slider:
        """Names it ``text``, before it, or, with ``None``, not."""
        if not (text is None or isinstance(text, str)):
            raise TypeError(f"the label is text, not {text!r}")
        self._label = text
        return self

    def play(self, enabled: bool = True, *, speed: float = 1) -> Slider:
        """Adds a button that plays through it, ``speed`` of its seconds each
        second, or, if not ``enabled``, none."""
        if not (_real(speed) and speed > 0):
            raise ValueError(f"it plays at a positive speed, not {speed!r}")
        self._speed = float(speed) if enabled else None
        return self

    def style(self, part: Part = "slider", /, css: Mapping[str, object] | None = None, **properties) -> Slider:
        """Styles a ``part`` of it, the whole slider unless another is named, with
        the CSS ``properties``, which add to those it has, or, as ``None``, remove
        one::

            slider.style(width="80%").style("input", css={"--slider-accent": "tomato"})
            slider.style("output", font_size="1.2em", color=None)

        Named in Python, a property's underscores are dashes: ``font_size`` is
        ``font-size``. Any other name, as a custom property, is in ``css``. On the
        slider, its own custom properties are named without ``--slider-`` too,
        as ``accent``, the color of the track played through and the thumb,
        ``track``, of the rest of the track, ``thumb``, within the thumb,
        ``text``, of the label and value, ``track_height`` and ``thumb_size``.

        The parts are the ``slider``, its ``figure``, and the ``controls`` below
        it, which hold the ``button``, the ``label``, the ``input`` and its
        ``output``, the value it is at. A deck's stylesheet can style them all
        the same, as ``.slides-rs-slider`` and ``.slides-rs-slider-controls``.
        """
        if part not in PARTS:
            raise ValueError(f"the parts of a slider are {list(PARTS)}, not {part!r}")
        names = {**(css or {}), **{name.replace("_", "-"): value for name, value in properties.items()}}
        declarations = self._styles[part]
        for name, value in names.items():
            if part == "slider" and name in VARIABLES:
                name = f"--slider-{name}"
            if not re.fullmatch(r"-{0,2}[A-Za-z][\w-]*", name):
                raise ValueError(f"{name!r} is not the name of a CSS property")
            if value is None:
                declarations.pop(name, None)
            elif isinstance(value, str) or _real(value):
                if re.search(r"[;{}]", str(value)):
                    raise ValueError(f"the value of {name} is one value, with no ; {{ or }}, not {value!r}")
                declarations[name] = str(value)
            else:
                raise TypeError(f"the value of {name} is text or a number, not {value!r}")
        return self

    def _css(self, part: str, defaults: Mapping[str, str] | None = None) -> str:
        """The declarations of a part, after those it has by default."""
        declarations = {**(defaults or {}), **self._styles[part]}
        return "; ".join(f"{name}: {value}" for name, value in declarations.items())

    def _style(self, part: str, defaults: Mapping[str, str] | None = None) -> str:
        css = self._css(part, defaults)
        return f' style="{html.escape(css)}"' if css else ""

    def _range(self) -> tuple[float, float, int]:
        """Where it starts and stops, and in how many ticks."""
        start = self._start
        stop = self._stop if self._stop is not None else max(motion._times[-1] for motion in self.motions)
        if not stop > start:
            raise ValueError(f"the slider starts at {start}, after the motions end, at {stop}")
        step = self._step if self._step is not None else (stop - start) / 100
        return start, stop, max(1, round((stop - start) / step))

    def _repr_html_(self) -> str:
        start, stop, ticks = self._range()
        step = (stop - start) / ticks
        value = start if self._value is None else self._value
        tick = min(max(round((value - start) / step), 0), ticks)
        digits = max(0, -int(f"{step:e}".split("e")[1]))
        options = json.dumps(dict(start=start, stop=stop, step=step, digits=digits, speed=self._speed or 0))
        # The value takes as much space as the widest it is, not to move the slider.
        width = max(len(f"{start:.{digits}f}"), len(f"{stop:.{digits}f}"))
        output = (
            f"<output{self._style('output', {'min-width': f'{width}ch'})}>"
            f"{start + tick * step:.{digits}f}</output>"
        )
        label = f"<label{self._style('label')}>{html.escape(self._label)}</label>" if self._label else ""
        button = (
            f'<button type="button" aria-label="Play" aria-pressed="false"{self._style("button")}></button>'
            if self._speed
            else ""
        )
        name = f' aria-label="{html.escape(self._label)}"' if self._label else ""
        input_ = (
            f'<input type="range" min="0" max="{ticks}" value="{tick}"{name}'
            f"{self._style('input')}>"
        )
        svg = _inline(self._svg(), self._css("figure"))
        return (
            f'<div class="slides-rs-slider"{self._style("slider")}>\n'
            f"<style>\n{CSS}</style>\n{svg}\n"
            f'<div class="slides-rs-slider-controls"{self._style("controls")}>'
            f"{button}{label}{input_}{output}</div>\n"
            f"<script>\n{SCRIPT % {'options': options}}</script>\n</div>"
        )

    def _svg(self) -> str:
        """The figure as an SVG, its motions beginning with it, and its ids the same
        each time it is drawn."""
        buffer = io.StringIO()
        with _beginning(self.motions), matplotlib.rc_context({"svg.hashsalt": "slides-rs"}):
            self.figure.savefig(buffer, format="svg")
        return buffer.getvalue()


def _real(value) -> bool:
    return isinstance(value, numbers.Real) and not isinstance(value, bool) and bool(np.isfinite(value))


@contextlib.contextmanager
def _beginning(motions):
    """The motions begin with the SVG, rather than as the deck says, while drawn."""
    begins = [motion._begin for motion in motions]
    for motion in motions:
        motion._begin = "0s"
    try:
        yield
    finally:
        for motion, begin in zip(motions, begins):
            motion._begin = begin


def _inline(svg: str, style: str = "") -> str:
    """The SVG as markup for an html body, as src/notebook/svg.rs prepares one the
    deck inlines, which it does not with html: a step's id turned into the attribute
    the deck reads, the path an id names given the name, and the ids referred to
    renamed after the figure, to be apart from the other figures' on the page. It
    is styled as ``style`` says, after its own style."""
    root = ET.fromstring(svg)
    if style:
        root.set("style", "; ".join(filter(None, [root.get("style"), style])))
    for metadata in root.findall(f"{{{SVG}}}metadata"):
        root.remove(metadata)
    ids = _referenced(root)
    numbered = _rename(_copy(root), ids, "")
    prefix = "w" + hashlib.sha256(ET.tostring(numbered)).hexdigest()[:8] + "-"
    return ET.tostring(_rename(root, ids, prefix), encoding="unicode")


def _copy(root: ET.Element) -> ET.Element:
    return ET.fromstring(ET.tostring(root))


def _references(key: str, value: str) -> list[str]:
    if key in ("href", f"{{{XLINK}}}href"):
        return [value[1:]] if value.startswith("#") else []
    return re.findall(r"url\(#([^)]*)\)", value)


def _referenced(root: ET.Element) -> dict[str, int]:
    """The ids the SVG refers to, numbered in the order they are first referred to."""
    ids: dict[str, int] = {}
    for element in root.iter():
        for key, value in element.attrib.items():
            for id in _references(key, value):
                ids.setdefault(id, len(ids))
    return ids


def _rename(root: ET.Element, ids: dict[str, int], prefix: str) -> ET.Element:
    rename = lambda id: f"{prefix}{ids[id]}" if id in ids else None
    defs = {element for parent in root.iter(f"{{{SVG}}}defs") for element in parent.iter()}
    for element in root.iter():
        for key, value in list(element.attrib.items()):
            if key != "id" and _references(key, value):
                if key in ("href", f"{{{XLINK}}}href"):
                    element.set(key, f"#{rename(value[1:])}")
                else:
                    element.set(key, re.sub(r"url\(#([^)]*)\)", lambda m: f"url(#{rename(m[1])})", value))
    # Each id is read before any is written, as a group's names a path within it.
    marked = [(element, element.attrib.pop("id")) for element in root.iter() if "id" in element.attrib]
    for element, id in marked:
        if element in defs:
            if id in ids:
                element.set("id", rename(id))
            continue
        value, _, name = id.rpartition("#")
        if "#" not in id or not name or any(character.isspace() for character in name):
            value, name = id, ""
        if name in ids:
            # The path the id names is the first one its group draws, not defines.
            paths = [element] if element.tag == f"{{{SVG}}}path" else element.iter(f"{{{SVG}}}path")
            path = next((path for path in paths if path not in defs), None)
            if path is not None:
                path.set("id", rename(name))
        mark = STEP.match(value.strip())
        if mark:
            collapse, steps = mark.groups()
            element.set("step", steps or "")
            if collapse:
                element.set("collapse", "")
        elif value.strip() in ids and "id" not in element.attrib:
            element.set("id", rename(value.strip()))
    return root
