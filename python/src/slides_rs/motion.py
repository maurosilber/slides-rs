"""Artists that move along the path of another, as a step of a slides-rs deck.

matplotlib has no animation of its own in an SVG, so a :class:`Motion` draws
its artist inside an ``<animateMotion>`` that follows the path of another
artist, such as a line from ``plot``, by an ``<mpath>``. The deck plays it when
its step shows, and starts it over when its step is hidden again::

    from slides_rs import Motion, Step

    (line,) = plt.plot(x, y)
    (dot,) = plt.plot(x[0], y[0], "o")
    Motion(dot, along=line, step=Step(2), duration=2)
"""

from __future__ import annotations

import dataclasses
from typing import Literal

import numpy as np
from matplotlib.artist import Artist
from matplotlib.backends.backend_svg import RendererSVG
from matplotlib.transforms import Affine2D

__all__ = ["Motion"]

#: The easings CSS names, as the cubic Bézier curves an SVG animation takes.
EASINGS = {
    "ease": (0.25, 0.1, 0.25, 1.0),
    "ease-in": (0.42, 0.0, 1.0, 1.0),
    "ease-out": (0.0, 0.0, 0.58, 1.0),
    "ease-in-out": (0.42, 0.0, 0.58, 1.0),
}

Easing = Literal["linear", "ease", "ease-in", "ease-out", "ease-in-out"] | tuple[float, float, float, float]


@dataclasses.dataclass(eq=False)
class Motion:
    """Moves ``artist`` along the path of ``along`` when ``step`` shows.

    Draw ``artist`` where the path of ``along`` starts: it moves as the path
    does from there. ``along`` is any artist drawn as a path, such as the line
    ``plot`` returns or a patch, and is drawn as it is. Its ``gid``, a step or
    none, names its path for the motion to follow, as ``step=2.. #path``,
    which slides-rs reads: the SVG matplotlib saves has no path by that name,
    and does not move it. Hidden, it has no path to follow either.

    ``step`` is when it shows and moves, as a :class:`~slides_rs.Step` or its
    string; without one, it moves when the slide opens. It takes ``duration``
    seconds, at the pace ``easing`` says, which is ``"linear"``, one of the
    easings CSS names, or the control points of a cubic Bézier curve, as
    ``cubic-bezier()`` takes them. ``rotate`` turns it along the path, as
    ``"auto"``, or ``"auto-reverse"``, or by a fixed angle in degrees. It moves
    ``repeat`` times, or ``"indefinite"``ly, and, if ``freeze``, stays at the
    end.

    Only an SVG in a deck moves it: drawn otherwise, as in a PNG, it stays
    where it is.
    Settings changed after it is made take effect the next time it is drawn.
    """

    artist: Artist = dataclasses.field(repr=False)
    along: Artist = dataclasses.field(repr=False)
    step: str | None = None
    duration: float = 1.0
    easing: Easing = "linear"
    rotate: Literal["auto", "auto-reverse"] | float | None = None
    repeat: int | float | Literal["indefinite"] = 1
    freeze: bool = True

    def __post_init__(self):
        self._attributes()  # Checks the settings, before a figure fails to save.
        # The artist draws itself through the motion, where the axes draw it, and the
        # path it follows with its name.
        self._draws = {artist: artist.__dict__.get("draw") for artist in (self.artist, self.along)}
        self.artist.draw = self._draw_moving
        self.along.draw = self._draw_named

    def remove(self):
        """Draws the artist where it is again."""
        for artist, draw in self._draws.items():
            if draw is None:
                del artist.draw
            else:
                artist.draw = draw

    @property
    def _name(self) -> str:
        """The name of the path the artist follows, the same for every motion along it."""
        return f"motion-path-{id(self.along):x}"

    def _draw_original(self, artist: Artist, renderer):
        draw = self._draws[artist] or type(artist).draw.__get__(artist)
        return draw(renderer)

    def _draw_named(self, renderer):
        # A step's gid, or none, followed by the name, as slides-rs reads them. Another
        # motion along the same path may have named it already.
        gid = self.along.get_gid()
        if gid is not None and gid.endswith(f"#{self._name}"):
            return self._draw_original(self.along, renderer)
        self.along.set_gid(f"{gid} #{self._name}" if gid else f"#{self._name}")
        try:
            return self._draw_original(self.along, renderer)
        finally:
            self.along.set_gid(gid)

    def _draw_moving(self, renderer):
        svg = getattr(renderer, "_renderer", renderer)
        start = self._start(svg) if isinstance(svg, RendererSVG) and self.artist.get_visible() and self.along.get_visible() else None
        if start is None:
            return self._draw_original(self.artist, renderer)
        writer = svg.writer
        # The motion moves and turns the artist, drawn at the start of the path, as
        # from the origin. It is on a group of its own, as how it adds to a transform
        # differs across browsers.
        renderer.open_group("motion", gid=self.step and str(self.step))
        writer.start("g")
        writer.start("animateMotion", attrib=self._attributes())
        writer.element("mpath", attrib={"xlink:href": f"#{self._name}"})
        writer.end("animateMotion")
        writer.start("g", transform=f"translate({_number(-start[0])} {_number(-start[1])})")
        self._draw_original(self.artist, renderer)
        writer.end("g")
        writer.end("g")
        renderer.close_group("motion")

    def _start(self, svg: RendererSVG):
        """Where the path of ``along`` starts in the SVG, if it has a start."""
        vertices = self.along.get_path().vertices
        # As the SVG renderer draws it: in points, from the top.
        transform = self.along.get_transform() + Affine2D().scale(1, -1).translate(0, svg.height)
        points = transform.transform(vertices) if len(vertices) else vertices
        finite = points[np.isfinite(points).all(axis=1)] if len(points) else points
        return finite[0] if len(finite) else None

    def _attributes(self) -> dict[str, str]:
        if not self.duration > 0:
            raise ValueError(f"the duration must be positive, not {self.duration!r}")
        attributes = {
            # The deck begins it when its step shows.
            "begin": "indefinite",
            "dur": f"{_number(self.duration)}s",
            "fill": "freeze" if self.freeze else "remove",
        }
        if self.easing != "linear":
            curve = EASINGS.get(self.easing, self.easing) if isinstance(self.easing, str) else self.easing
            if isinstance(curve, str) or len(curve) != 4 or not all(0 <= curve[i] <= 1 for i in (0, 2)):
                raise ValueError(f"the easing must be linear, one of {list(EASINGS)}, or a cubic Bézier curve, not {self.easing!r}")
            attributes |= {
                "calcMode": "spline",
                "keyPoints": "0;1",
                "keyTimes": "0;1",
                "keySplines": " ".join(_number(value) for value in curve),
            }
        if self.rotate is not None:
            if not (self.rotate in ("auto", "auto-reverse") or isinstance(self.rotate, (int, float))):
                raise ValueError(f"rotate must be auto, auto-reverse or an angle, not {self.rotate!r}")
            attributes["rotate"] = self.rotate if isinstance(self.rotate, str) else _number(self.rotate)
        if self.repeat != 1:
            if not (self.repeat == "indefinite" or (isinstance(self.repeat, (int, float)) and self.repeat > 0)):
                raise ValueError(f"repeat must be a positive number of times or indefinite, not {self.repeat!r}")
            attributes["repeatCount"] = self.repeat if isinstance(self.repeat, str) else _number(self.repeat)
        return attributes


def _number(value: float) -> str:
    """A number as short as SVG writes it, as matplotlib does."""
    text = f"{float(value):f}".rstrip("0").rstrip(".")
    return "0" if text in ("", "-0") else text
