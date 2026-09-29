"""Widgets that drive the motions of a figure, in a slides-rs deck or a notebook.

A :class:`Slider` shows a figure along with a slider, as html, which scrubs
through the figure's motions rather than the deck playing them: the slider's
value is the time, in seconds, the motions are at::

    from slides_rs import Motion, Slider

    t = np.linspace(0, 2 * np.pi, 200)
    (line,) = plt.plot(t, np.sin(t))
    (dot,) = plt.plot(0, 0, "o")
    motion = Motion(dot, along=line).timing(t)
    Slider(plt.gcf(), motion, label="t")

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
import re
import xml.etree.ElementTree as ET

import matplotlib
import matplotlib.pyplot as plt
from matplotlib.figure import Figure

from .motion import Motion

__all__ = ["Slider"]

SVG = "http://www.w3.org/2000/svg"
XLINK = "http://www.w3.org/1999/xlink"
ET.register_namespace("", SVG)
ET.register_namespace("xlink", XLINK)

#: An id that marks a step, as src/step.rs reads it: ``step=2..4``, ``step``,
#: ``also``, starred if it collapses.
STEP = re.compile(r"^(step|also)(\*)?(?:=(.*))?$")

#: Scrubs the figure's timeline as the slider moves, and plays it with the button.
#: The slider goes through ticks, from 0, each ``step`` seconds from ``start``:
#: an input with a ``step`` attribute would be a step of the deck.
SCRIPT = """\
(() => {
  const root = document.currentScript.parentElement;
  const svg = root.querySelector("svg");
  const input = root.querySelector("input");
  const output = root.querySelector("output");
  const button = root.querySelector("button");
  const { start, stop, step, digits } = %(options)s;
  let time = start + Number(input.value) * step;
  const show = () => {
    svg.setCurrentTime(time);
    output.value = time.toFixed(digits);
  };
  let playing = null;
  const pause = () => {
    cancelAnimationFrame(playing);
    playing = null;
    button.textContent = "\\u25B6";
  };
  button.addEventListener("click", () => {
    if (playing !== null) return pause();
    if (time >= stop) time = start;
    let last = performance.now();
    const frame = (now) => {
      time = Math.min(time + (now - last) / 1000, stop);
      last = now;
      input.value = Math.round((time - start) / step);
      show();
      if (time >= stop) return pause();
      playing = requestAnimationFrame(frame);
    };
    button.textContent = "\\u275A\\u275A";
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

STYLE = (
    "display: flex; align-items: center; gap: 0.5em; font-size: 0.6em; "
    "font-variant-numeric: tabular-nums"
)


class Slider:
    """Shows ``figure`` with a slider that sets the time its ``motions`` are at.

    It goes from ``start`` to ``stop`` seconds, by ``step``, starting at
    ``value``: by default, from 0 to the end of the longest motion, by a
    hundredth of it. ``label`` names it, and ``play`` adds a button that
    plays through it in real time.

    The figure is shown as html, which ``_repr_html_`` returns, with the
    slider: rather than the deck, the slider moves the motions, which begin
    with the figure, and the steps the deck reveals the figure in are only
    the artists'. It is closed, for pyplot not to show it once more, and
    drawn each time the slider is shown, as it is then.
    """

    def __init__(
        self,
        figure: Figure,
        *motions: Motion,
        start: float = 0,
        stop: float | None = None,
        step: float | None = None,
        value: float | None = None,
        label: str | None = None,
        play: bool = True,
    ):
        if not motions:
            raise ValueError("a slider drives one motion or more")
        for motion in motions:
            if motion.artist.get_figure(root=True) is not figure:
                raise ValueError(f"{motion!r} is not in the figure")
        if stop is None:
            stop = max(motion._times[-1] for motion in motions)
        if not stop > start:
            raise ValueError(f"the slider goes from {start} up to a later stop, not {stop}")
        self.figure = figure
        self.motions = motions
        self.start, self.stop = float(start), float(stop)
        self.step = float(step) if step is not None else (self.stop - self.start) / 100
        self.value = self.start if value is None else float(value)
        self.label = label
        self.play = play
        plt.close(figure)

    def __repr__(self):
        return f"Slider({self.figure!r}, {', '.join(map(repr, self.motions))})"

    def _repr_html_(self) -> str:
        svg = _inline(self._svg())
        label = f"<label>{html.escape(self.label)}</label>" if self.label else ""
        button = "<button type='button'>&#x25B6;</button>" if self.play else "<button hidden></button>"
        ticks = max(1, round((self.stop - self.start) / self.step))
        step = (self.stop - self.start) / ticks
        tick = min(max(round((self.value - self.start) / step), 0), ticks)
        digits = max(0, -int(f"{step:e}".split("e")[1]))
        options = json.dumps(dict(start=self.start, stop=self.stop, step=step, digits=digits))
        return (
            f'<div class="slides-rs-slider" style="display: inline-flex; flex-direction: column">\n{svg}\n'
            f'<div style="{STYLE}">{button}{label}'
            f'<input type="range" min="0" max="{ticks}" value="{tick}" style="flex: 1">'
            f"<output>{self.start + tick * step:.{digits}f}</output></div>\n"
            f"<script>\n{SCRIPT % {'options': options}}</script>\n</div>"
        )

    def _svg(self) -> str:
        """The figure as an SVG, its motions beginning with it, and its ids the same
        each time it is drawn."""
        buffer = io.StringIO()
        with _beginning(self.motions), matplotlib.rc_context({"svg.hashsalt": "slides-rs"}):
            self.figure.savefig(buffer, format="svg")
        return buffer.getvalue()


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


def _inline(svg: str) -> str:
    """The SVG as markup for an html body, as src/notebook/svg.rs prepares one the
    deck inlines, which it does not with html: a step's id turned into the attribute
    the deck reads, the path an id names given the name, and the ids referred to
    renamed after the figure, to be apart from the other figures' on the page."""
    root = ET.fromstring(svg)
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
            kind, collapse, steps = mark.groups()
            element.set(kind, steps or "")
            if collapse:
                element.set("collapse", "")
        elif value.strip() in ids and "id" not in element.attrib:
            element.set("id", rename(value.strip()))
    return root
