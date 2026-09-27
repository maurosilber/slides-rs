"""Artists that move along the path of another, in the steps of a slides-rs deck.

matplotlib has no animation of its own in an SVG, so a :class:`Motion` draws
its artist with an ``<animateMotion>`` that follows the path of another artist,
such as a line from ``plot``, by an ``<mpath>``. The deck plays it when its
step shows, and starts it over when its step is hidden again::

    from slides_rs import Motion, Step

    (line,) = plt.plot(x, y)
    (dot,) = plt.plot(x[0], y[0], "o", gid=Step(1))
    Motion(dot, along=line).starts(Step(2)).timing(duration=2).repeat()
"""

from __future__ import annotations

import numbers
from collections.abc import Sequence
from typing import Literal

import numpy as np
from matplotlib.artist import Artist
from matplotlib.backends.backend_svg import RendererSVG
from matplotlib.path import Path
from matplotlib.transforms import Affine2D

from .step import Step

__all__ = ["Motion"]

#: The easings CSS names, as the cubic Bézier curves an SVG animation takes.
EASINGS = {
    "linear": (0.0, 0.0, 1.0, 1.0),
    "ease": (0.25, 0.1, 0.25, 1.0),
    "ease-in": (0.42, 0.0, 1.0, 1.0),
    "ease-out": (0.0, 0.0, 0.58, 1.0),
    "ease-in-out": (0.42, 0.0, 0.58, 1.0),
}

#: Accumulated, the artist is taken back as far as the path starts from the origin
#: every time, within the motion, which would turn that too.
ACCUMULATE_ROTATE = "a motion that accumulates does not rotate"

Curve = tuple[float, float, float, float]
Easing = Literal["linear", "ease", "ease-in", "ease-out", "ease-in-out"] | Curve


class Motion:
    """Moves ``artist`` along the path of ``along``, as its methods set.

    Draw ``artist`` where the path of ``along`` starts: it moves as the path
    does from there. ``along`` is any artist drawn as a path, such as the line
    ``plot`` returns or a patch, and is drawn as it is. Its ``gid``, a step or
    none, names its path for the motion to follow, as ``step=2.. #path``,
    which slides-rs reads: the SVG matplotlib saves has no path by that name,
    and does not move it. Hidden, it has no path to follow either.

    The artist shows in the step its own ``gid`` says, as any artist does, and
    moves once it shows, or from the step :meth:`starts` says. It takes a
    second, at an even pace, as :meth:`timing` changes, and moves once, as
    :meth:`repeat` changes, staying at the end, as :meth:`hold` changes. Each
    method returns the motion, for the next one::

        Motion(dot, along=line).starts(Step(2)).timing(t).rotate("auto")

    Only an SVG in a deck moves it: drawn otherwise, as in a PNG, it stays
    where it is. What changes after it is made takes effect the next time it
    is drawn.
    """

    def __init__(self, artist: Artist, along: Artist):
        self.artist = artist
        self.along = along
        self._step: Step | None = None
        self._times = np.array([0.0, 1.0])
        self._fraction: np.ndarray | Literal["vertices"] | None = None
        self._curves: list[Curve] | Literal["discrete"] = [EASINGS["linear"]]
        self._rotate: str | None = None
        self._repeat: dict[str, str] = {}
        self._fill = "freeze"
        # The artist draws itself through the motion, where the axes draw it, and the
        # path it follows with its name.
        self._draws = {artist: artist.__dict__.get("draw") for artist in (artist, along)}
        artist.draw = self._draw_moving
        along.draw = self._draw_named

    def __repr__(self):
        return f"Motion({self.artist!r}, along={self.along!r})"

    def starts(self, step: Step | None) -> Motion:
        """Moves from ``step``, a :class:`~slides_rs.Step`, rather than once the artist
        shows, or, with ``None``, once it shows. Hidden again, as when a range ends, it
        starts over.
        """
        if not (step is None or isinstance(step, Step)):
            raise TypeError(f"starts takes a Step or None, not {step!r}")
        self._step = step
        return self

    def timing(
        self,
        t: Sequence[float] | np.ndarray | None = None,
        *,
        duration: float | None = None,
        fraction: Sequence[float] | np.ndarray | None = None,
        easing: Easing | Sequence[Easing] | Literal["discrete"] = "linear",
    ) -> Motion:
        """When it is where along the path.

        With a ``duration`` alone, it takes that many seconds from the start of
        the path to its end. With the times ``t``, in seconds, it is
        ``fraction[i]`` of the way along the path, from 0 at its start to 1 at
        its end, at time ``t[i]``::

            motion.timing([0.5, 1, 1.5, 2], fraction=[0, 0.5, 0.5, 1])

        waits at the start up to 0.5 s, goes halfway by 1 s, waits there up
        to 1.5 s, and goes on to the end by 2 s. Without ``fraction``, it is
        at the i-th vertex of ``along``, drawn as a line, at time ``t[i]``: a
        line plotted from ``x(t), y(t)`` is followed as it was plotted. It
        takes ``t[-1]`` seconds, and waits where it starts up to ``t[0]``.

        Between them, it goes at the pace ``easing`` says, for every interval
        or for each: ``"linear"``, one of the easings CSS names, or the control
        points of a cubic Bézier curve, as ``cubic-bezier()`` takes them. Or,
        ``"discrete"``, it jumps from each to the next.
        """
        if (t is None) == (duration is None):
            raise ValueError("the timing takes either the times t or a duration")
        if t is None:
            if fraction is not None:
                raise ValueError("the fractions of the path are at the times t, which a duration has none of")
            times = np.array([0.0, duration], dtype=float)
        else:
            times = np.asarray(t, dtype=float)
        if times.ndim != 1 or len(times) < 2 or not np.isfinite(times).all():
            raise ValueError(f"the times must be two or more numbers, not {t!r}")
        if not (times[0] >= 0 and (np.diff(times) >= 0).all() and times[-1] > 0):
            raise ValueError(f"the times must go on from 0 and end after it, not {times!r}")
        if fraction is not None:
            points = np.asarray(fraction, dtype=float)
            if points.shape != times.shape:
                raise ValueError(f"it takes a fraction of the path at each of the {len(times)} times, not {len(points)}")
            if not ((0 <= points) & (points <= 1)).all():
                raise ValueError(f"the fractions of the path go from 0 to 1, not {points!r}")
        elif t is not None:
            points = "vertices"
            self._progress(times)  # Checks the line has as many vertices, before a figure fails to save.
        else:
            points = None
        self._times, self._fraction = times, points
        self._curves = _curves(easing, len(times) - 1)
        return self

    def rotate(self, angle: Literal["auto", "auto-reverse"] | float | None) -> Motion:
        """Turns it along the path, as ``"auto"`` or ``"auto-reverse"``, or by a fixed
        ``angle`` in degrees, or, with ``None``, not."""
        if not (angle is None or angle in ("auto", "auto-reverse") or _real(angle)):
            raise ValueError(f"rotate takes auto, auto-reverse or an angle, not {angle!r}")
        if angle is not None and "accumulate" in self._repeat:
            raise ValueError(ACCUMULATE_ROTATE)
        self._rotate = angle if angle is None or isinstance(angle, str) else _number(angle)
        return self

    def repeat(
        self,
        count: float | Literal["indefinite"] = "indefinite",
        *,
        seconds: float | None = None,
        accumulate: bool = False,
    ) -> Motion:
        """Moves ``count`` times, or on and on, for ``seconds`` at most if given.

        With ``accumulate``, each time moves on from where the last one ended,
        rather than from the start, as along a path that is one of many alike.
        It does not turn as it does, as :meth:`rotate` would.
        """
        if not (count == "indefinite" or (_real(count) and count > 0)):
            raise ValueError(f"repeat takes a positive number of times or indefinite, not {count!r}")
        if not (seconds is None or (_real(seconds) and seconds > 0)):
            raise ValueError(f"repeat takes a positive number of seconds, not {seconds!r}")
        if accumulate and self._rotate is not None:
            raise ValueError(ACCUMULATE_ROTATE)
        self._repeat = {}
        if count != 1:
            self._repeat["repeatCount"] = count if isinstance(count, str) else _number(count)
        if seconds is not None:
            self._repeat["repeatDur"] = f"{_number(seconds)}s"
        if accumulate:
            self._repeat["accumulate"] = "sum"
        return self

    def hold(self, hold: bool = True) -> Motion:
        """Stays at the end, once it has moved, or, if not ``hold``, goes back to the start."""
        self._fill = "freeze" if hold else "remove"
        return self

    def remove(self):
        """Stops the artist moving: it and ``along`` are drawn as they were before
        the motion was made, the next time the figure is drawn."""
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
        moves = isinstance(svg, RendererSVG) and self.artist.get_visible() and self.along.get_visible()
        start = self._start(svg) if moves else None
        attributes = self._attributes(svg) if start is not None else None
        if attributes is None:
            return self._draw_original(self.artist, renderer)
        writer = svg.writer
        moving, anchored = f"motion-{id(self):x}", f"motion-{id(self):x}-anchored"
        # The artist's step is the whole motion's, that it moves in once it shows.
        gid = self.artist.get_gid()
        renderer.open_group("motion", gid=gid)
        # The motion moves and turns the artist, drawn at the start of the path, as from
        # the origin, where it is taken while it moves. It is on a group of its own, as
        # how it adds to a transform differs across browsers.
        writer.start("g", id=moving)
        writer.start("g", id=anchored)
        self.artist.set_gid(None)
        try:
            self._draw_original(self.artist, renderer)
        finally:
            self.artist.set_gid(gid)
        writer.end("g")
        writer.end("g")
        # The animations are apart from what they move, in a group of the step they
        # start in: the deck starts them over by replacing them, and steps through the
        # group.
        if self._step is not None:
            writer.start("g", id=self._step)
        writer.start("animateMotion", attrib={"xlink:href": f"#{moving}", **attributes})
        writer.element("mpath", attrib={"xlink:href": f"#{self._name}"})
        writer.end("animateMotion")
        # Taken to the origin only while it moves, it is where it is drawn otherwise. The
        # path is where it is drawn too, so that the motion ends where the path does,
        # rather than as far on as it goes: accumulated, each time is taken back as much.
        origin = f"{_number(-start[0])} {_number(-start[1])}"
        timing = {key: attributes[key] for key in ("begin", "dur", "fill", *self._repeat)}
        writer.element(
            "animateTransform",
            attrib={
                "xlink:href": f"#{anchored}",
                "attributeName": "transform",
                "type": "translate",
                "from": origin,
                "to": origin,
                **timing,
            },
        )
        if self._step is not None:
            writer.end("g")
        renderer.close_group("motion")

    def _points(self, svg: RendererSVG | None) -> tuple[Path, np.ndarray]:
        """The path of ``along``, and its vertices as the SVG renderer draws them: in
        points, from the top."""
        path = self.along.get_path()
        transform = self.along.get_transform()
        if svg is not None:
            transform = transform + Affine2D().scale(1, -1).translate(0, svg.height)
        return path, transform.transform(path.vertices) if len(path.vertices) else path.vertices

    def _start(self, svg: RendererSVG):
        """Where the path of ``along`` starts in the SVG, if it has a start."""
        _, points = self._points(svg)
        finite = points[np.isfinite(points).all(axis=1)] if len(points) else points
        return finite[0] if len(finite) else None

    def _progress(self, times: np.ndarray) -> np.ndarray | None:
        """How far along the line each of its vertices is, from 0 to 1, if it has a length,
        as the SVG draws it: in proportion to the figure's, as any drawing of it."""
        if getattr(self.along, "get_drawstyle", lambda: "default")() != "default":
            raise ValueError("it takes a time at each vertex of a line drawn straight, not in steps: give the fractions of the path")
        path, points = self._points(None)
        codes = path.codes
        if codes is not None and not np.isin(codes, [Path.MOVETO, Path.LINETO]).all():
            raise ValueError("it takes a time at each vertex of a line, not of curves: give the fractions of the path")
        if len(points) != len(times):
            raise ValueError(f"it takes a time at each of the {len(points)} vertices of the line, not {len(times)}")
        lengths = np.hypot(*np.diff(points, axis=0).T)
        # A gap in the line, where it moves or is not a number, takes it no further.
        lengths[~np.isfinite(lengths)] = 0
        if codes is not None:
            lengths[codes[1:] == Path.MOVETO] = 0
        distance = np.concatenate([[0], np.cumsum(lengths)])
        return distance / distance[-1] if distance[-1] > 0 else None

    def _attributes(self, svg: RendererSVG) -> dict[str, str] | None:
        """The animation's attributes, or none if it has nowhere to move."""
        times, points, curves = self._times, self._fraction, self._curves
        attributes = {
            # The deck begins it when its step shows.
            "begin": "indefinite",
            "dur": f"{_number(times[-1])}s",
            "fill": self._fill,
        }
        if points is None and curves == [EASINGS["linear"]]:
            pass  # At an even pace along the path, as it moves by default.
        else:
            if points is None:
                points = np.array([0.0, 1.0])
            elif isinstance(points, str):
                points = self._progress(times)
                if points is None:
                    return None
            if times[0] > 0:
                # It waits at the start, up to the first time.
                times, points = np.concatenate([[0], times]), np.concatenate([points[:1], points])
                if curves != "discrete":
                    curves = [EASINGS["linear"], *curves]
            attributes |= {
                "keyTimes": ";".join(_number(time) for time in times / times[-1]),
                "keyPoints": ";".join(_number(point) for point in points),
            }
            if curves == "discrete":
                attributes["calcMode"] = "discrete"
            elif all(curve == EASINGS["linear"] for curve in curves):
                attributes["calcMode"] = "linear"
            else:
                attributes["calcMode"] = "spline"
                attributes["keySplines"] = ";".join(" ".join(_number(value) for value in curve) for curve in curves)
        if self._rotate is not None:
            attributes["rotate"] = self._rotate
        return attributes | self._repeat


def _real(value) -> bool:
    return isinstance(value, numbers.Real) and not isinstance(value, bool) and np.isfinite(value)


def _curve(easing) -> Curve | None:
    """The cubic Bézier curve an easing is, if it is one."""
    if isinstance(easing, str):
        return EASINGS.get(easing)
    if isinstance(easing, (tuple, list, np.ndarray)) and len(easing) == 4 and all(_real(value) for value in easing):
        # As keySplines takes them, every control point is within the unit square.
        return tuple(float(value) for value in easing) if all(0 <= value <= 1 for value in easing) else None
    return None


def _curves(easing, intervals: int) -> list[Curve] | Literal["discrete"]:
    """The curve of each interval, as an easing for them all or for each says."""
    if isinstance(easing, str) and easing == "discrete":
        return "discrete"
    curve = _curve(easing)
    if curve is not None:
        return [curve] * intervals
    if isinstance(easing, str) or not isinstance(easing, Sequence) or len(easing) != intervals:
        raise ValueError(
            f"the easing must be discrete, linear, one of {list(EASINGS)[1:]} or a cubic Bézier curve within the "
            f"unit square, or one for each of the {intervals} intervals, not {easing!r}"
        )
    curves = [_curve(each) for each in easing]
    if None in curves:
        raise ValueError(f"each easing must be linear, one of {list(EASINGS)[1:]} or a cubic Bézier curve, not {easing!r}")
    return curves


def _number(value: float) -> str:
    """A number as short as SVG writes it, as matplotlib does."""
    text = f"{float(value):f}".rstrip("0").rstrip(".")
    return "0" if text in ("", "-0") else text
