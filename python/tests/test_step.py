import os
import pathlib
import re
import subprocess

import pytest
from slides_rs import Step

ROOT = pathlib.Path(__file__).parents[2]


@pytest.mark.parametrize(
    "step, text",
    [
        (Step(1, 2), "step=1..2"),
        (Step(3), "step=3.."),
        (Step(stop=3), "step=..3"),
        (Step(), "step=.."),
        (Step(1, 3, collapse=True), "step*=1..3"),
    ],
)
def test_it_is_written_as_the_deck_reads_it(step, text):
    assert str(step) == text
    assert f"{step}" == text


def test_it_is_the_string_matplotlib_takes_as_a_gid():
    assert isinstance(Step(1, 2), str)
    assert Step(1, 2) == "step=1..2"


def test_it_is_a_value():
    assert Step(1, 2) == Step(1, 2)
    assert {Step(1, 2): "a"}[Step(1, 2)] == "a"
    with pytest.raises(AttributeError):
        Step(1, 2).start = 3


@pytest.mark.parametrize(
    "step, n, moved",
    [
        (Step(1, 2), 1, Step(2, 3)),
        (Step(3), 2, Step(5)),
        (Step(stop=3), 1, Step(stop=4)),
        (Step(1, 2, collapse=True), 1, Step(2, 3, collapse=True)),
    ],
)
def test_next_moves_it_later(step, n, moved):
    assert step.next(n) == moved
    assert moved.previous(n) == step


def test_steps_move_along():
    assert Step(1, 2).next().next() == Step(3, 4)
    assert Step(3, 4).previous(2) == Step(1, 2)


def test_the_step_shown_throughout_does_not_move():
    with pytest.raises(ValueError):
        Step().next()
    with pytest.raises(ValueError):
        Step().previous()


def test_a_figure_steps_on_its_slide(slides_rs, tmp_path):
    slide = tmp_path / "slide.md"
    slide.write_text(
        "~~~python\n"
        "import matplotlib.pyplot as plt\n"
        "from slides_rs import Step\n"
        "step = Step(1, 2)\n"
        "for i in range(3):\n"
        "    plt.plot([0, 1], [i, i], gid=step)\n"
        "    step = step.next()\n"
        "~~~\n"
    )
    env = {**os.environ, "PYTHONPATH": str(ROOT / "python" / "src")}
    subprocess.run([slides_rs, str(slide)], check=True, env=env, capture_output=True)
    html = slide.with_suffix(".html").read_text()
    assert re.findall(r'step="([^"]*)"', html) == ["1..2", "2..3", "3..4"]
