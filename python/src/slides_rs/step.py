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

__all__ = ["Step"]


@dataclasses.dataclass(frozen=True)
class Step(str):
    """The steps an artist shows in, written as ``src/step.rs`` reads them.

    It shows from ``start``, or the start, up to ``stop``, excluded, or the
    end, as a Rust range does: ``Step(3, 5)`` is ``step=3..5``, ``Step(3)`` is
    ``step=3..``, and ``Step(stop=3)`` is ``step=..3``. Without either, it
    shows on every step: ``Step()`` is ``step=..``. ``collapse`` makes it take
    no space while hidden, as ``step*`` does.

    It is the string itself, as matplotlib escapes a ``gid`` as one::

        >>> Step(1, 2)
        Step(start=1, stop=2, collapse=False)
        >>> str(Step(1, 2)), f"{Step(3)}", Step(stop=3) + ""
        ('step=1..2', 'step=3..', 'step=..3')
        >>> str(Step()), str(Step(1, 3, collapse=True))
        ('step=..', 'step*=1..3')
    """

    start: int | None = None
    stop: int | None = None
    collapse: bool = dataclasses.field(default=False, kw_only=True)

    def __new__(
        cls,
        start: int | None = None,
        stop: int | None = None,
        *,
        collapse: bool = False,
    ) -> Step:
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
        """
        if self.start is None and self.stop is None:
            raise ValueError("Step() shows on every step, which has no steps to move")
        shift = lambda bound: None if bound is None else bound + n
        return Step(
            start=shift(self.start),
            stop=shift(self.stop),
            collapse=self.collapse,
        )

    def previous(self, n: int = 1) -> Step:
        """The same steps, ``n`` earlier.

        >>> Step(2, 3).previous()
        Step(start=1, stop=2, collapse=False)
        """
        return self.next(-n)
