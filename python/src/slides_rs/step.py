"""Steps for the figures of a slides-rs deck.

matplotlib writes an artist's ``gid`` as the ``id`` of its group in the SVG,
which the deck reads as the step the artist shows in::

    from slides_rs import Step

    step = Step(1, 2)
    for i, phase in enumerate(phases):
        plt.plot(x, np.sin(x + phase), gid=step.next(i))
"""

from __future__ import annotations

import dataclasses
import re

__all__ = ["Step"]


@dataclasses.dataclass(frozen=True)
class Step(str):
    """The steps an artist shows in, written as ``src/step.rs`` reads them.

    It shows from ``start``, or the start, up to ``stop``, excluded, or the
    end, as a Rust range does: ``Step(3, 5)`` is ``step=3..5``, ``Step(3)`` is
    ``step=3..``, and ``Step(stop=3)`` is ``step=..3``. Without either, it
    shows on every step: ``Step()`` is ``step=..``. ``collapse`` makes it take
    no space while hidden, as ``step*`` does.

    A bound written as a signed string, as ``"+1"``, ``"+0"`` or ``"-1"``,
    is a number of steps from the step of the artist before it that steps,
    or else of the group it is in: ``Step("+0", "+2")`` is ``step=+0..+2``.

    A bound can also be a number of steps from a named one, wherever in the
    slide it is, as ``"a+0"``, ``"a+1"`` or ``"a-1"``: ``Step("a+0", "a+2")``
    is ``step=a+0..a+2``. A step is named by ``gid="step=a"``, or by
    ``step="a"`` on the element it is in, which is a step of its own.

    It is the string itself, as matplotlib escapes a ``gid`` as one::

        >>> Step(1, 2)
        Step(start=1, stop=2, collapse=False)
        >>> str(Step(1, 2)), f"{Step(3)}", Step(stop=3) + ""
        ('step=1..2', 'step=3..', 'step=..3')
        >>> str(Step()), str(Step(1, 3, collapse=True))
        ('step=..', 'step*=1..3')
    """

    start: int | str | None = None
    stop: int | str | None = None
    collapse: bool = dataclasses.field(default=False, kw_only=True)

    def __new__(
        cls,
        start: int | str | None = None,
        stop: int | str | None = None,
        *,
        collapse: bool = False,
    ) -> Step:
        for bound in (start, stop):
            if isinstance(bound, str) and _BOUND.fullmatch(bound) is None:
                raise ValueError(
                    f"{bound!r} is not a step from the one before, as '+1' is,"
                    " nor from a name, as 'a+1' is"
                )
        name = "step*" if collapse else "step"
        start_ = "" if start is None else start
        stop_ = "" if stop is None else stop
        return super().__new__(cls, f"{name}={start_}..{stop_}")

    def next(self, n: int = 1) -> Step:
        """The same steps, ``n`` later.

        >>> Step(1, 2).next()
        Step(start=2, stop=3, collapse=False)
        >>> Step(3).next(2)
        Step(start=5, stop=None, collapse=False)
        >>> Step(stop=3).next()
        Step(start=None, stop=4, collapse=False)
        >>> Step("a+0", "a+1").next()
        Step(start='a+1', stop='a+2', collapse=False)
        >>> Step("+0").next()
        Step(start='+1', stop=None, collapse=False)
        """
        if self.start is None and self.stop is None:
            raise ValueError("Step() shows on every step, which has no steps to move")
        return Step(
            start=_shift(self.start, n),
            stop=_shift(self.stop, n),
            collapse=self.collapse,
        )

    def previous(self, n: int = 1) -> Step:
        """The same steps, ``n`` earlier.

        >>> Step(2, 3).previous()
        Step(start=1, stop=2, collapse=False)
        """
        return self.next(-n)


# A number of steps from the one before, or from a named one, as
# src/step.rs reads it.
_BOUND = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)?([+-])(\d+)")


def _shift(bound: int | str | None, n: int) -> int | str | None:
    """The bound, ``n`` steps later."""
    if bound is None or isinstance(bound, int):
        return None if bound is None else bound + n
    name, sign, offset = _BOUND.fullmatch(bound).groups()
    offset = (-1 if sign == "-" else 1) * int(offset) + n
    return f"{name or ''}{offset:+d}"
