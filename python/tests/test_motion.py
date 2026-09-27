import io
import os
import pathlib
import re
import subprocess
import xml.etree.ElementTree as ET

import matplotlib
import numpy as np
import pytest

matplotlib.use("svg")
import matplotlib.pyplot as plt  # noqa: E402

from slides_rs import Motion, Step  # noqa: E402

ROOT = pathlib.Path(__file__).parents[2]
SVG = "{http://www.w3.org/2000/svg}"
XLINK = "{http://www.w3.org/1999/xlink}"


@pytest.fixture
def figure():
    figure, axes = plt.subplots()
    x = np.linspace(0, 2 * np.pi, 20)
    (line,) = axes.plot(x, np.sin(x))
    (dot,) = axes.plot(x[:1], np.sin(x[:1]), "o")
    yield figure, line, dot
    plt.close(figure)


def svg(figure, **options) -> ET.Element:
    buffer = io.StringIO()
    figure.savefig(buffer, format="svg", **options)
    return ET.fromstring(buffer.getvalue())


def motion_of(root: ET.Element) -> tuple[ET.Element, ET.Element]:
    """The group that moves, and its animation."""
    for group in root.iter(f"{SVG}g"):
        animation = group.find(f"{SVG}animateMotion")
        if animation is not None:
            return group, animation
    raise AssertionError("no animateMotion in the SVG")


def translate(element: ET.Element) -> tuple[float, float]:
    x, y = re.fullmatch(r"translate\((\S+) (\S+)\)", element.get("transform")).groups()
    return float(x), float(y)


def test_it_writes_an_animation_the_deck_begins(figure):
    figure, line, dot = figure
    Motion(dot, along=line, step=Step(2), duration=1.5)
    group, animation = motion_of(svg(figure))
    assert animation.get("begin") == "indefinite"
    assert animation.get("dur") == "1.5s"
    assert animation.get("fill") == "freeze"
    # Paced along the path, the default, and not turned.
    assert animation.get("calcMode") is None
    assert animation.get("rotate") is None
    assert animation.get("path").startswith("M 0 0 L ")


def test_its_step_is_the_group_that_holds_it(figure):
    figure, line, dot = figure
    Motion(dot, along=line, step=Step(2))
    root = svg(figure)
    (holder,) = [g for g in root.iter(f"{SVG}g") if g.get("id") == "step=2.."]
    assert holder.find(f".//{SVG}animateMotion") is not None


@pytest.mark.parametrize("options", [{}, {"bbox_inches": "tight"}])
def test_it_moves_from_where_it_is_drawn(figure, options):
    figure, line, dot = figure
    Motion(dot, along=line)
    root = svg(figure, **options)
    group, animation = motion_of(root)
    outer = next(g for g in root.iter(f"{SVG}g") if group in list(g))
    inner = group.find(f"{SVG}g")
    start = translate(outer)
    assert translate(inner) == pytest.approx((-start[0], -start[1]))
    # The dot is drawn where the line starts, which the path starts from.
    (marker,) = inner.iter(f"{SVG}use")
    assert (float(marker.get("x")), float(marker.get("y"))) == pytest.approx(start, abs=1e-3)
    # And follows the line as the SVG draws it, from its start to its end.
    lines = [g.find(f"{SVG}path").get("d") for g in root.iter(f"{SVG}g") if (g.get("id") or "").startswith("line2d") and g.find(f"{SVG}path") is not None]
    points = [float(value) for value in re.findall(r"-?[\d.]+", lines[0])]
    moved = [float(value) for value in re.findall(r"-?[\d.]+", animation.get("path"))]
    assert points[:2] == pytest.approx(start, abs=1e-3)
    assert [value + start[i % 2] for i, value in enumerate(moved)] == pytest.approx(points, abs=1e-3)


def test_a_patch_is_a_path_too(figure):
    figure, line, dot = figure
    circle = plt.Circle((1, 0), 0.5, fill=False)
    figure.axes[0].add_patch(circle)
    Motion(dot, along=circle)
    _, animation = motion_of(svg(figure))
    assert set(animation.get("path").split()) >= {"M", "C", "Z"}


@pytest.mark.parametrize(
    "settings, attributes",
    [
        ({"easing": "ease-in-out"}, {"calcMode": "spline", "keyPoints": "0;1", "keyTimes": "0;1", "keySplines": "0.42 0 0.58 1"}),
        ({"easing": (0.1, 0.7, 1.0, 0.1)}, {"calcMode": "spline", "keySplines": "0.1 0.7 1 0.1"}),
        ({"rotate": "auto"}, {"rotate": "auto"}),
        ({"rotate": 45}, {"rotate": "45"}),
        ({"repeat": "indefinite"}, {"repeatCount": "indefinite"}),
        ({"repeat": 2.5}, {"repeatCount": "2.5"}),
        ({"freeze": False}, {"fill": "remove"}),
    ],
)
def test_its_settings_are_the_animation_s(figure, settings, attributes):
    figure, line, dot = figure
    Motion(dot, along=line, **settings)
    _, animation = motion_of(svg(figure))
    assert {key: animation.get(key) for key in attributes} == attributes


@pytest.mark.parametrize(
    "settings",
    [
        {"duration": 0},
        {"easing": "bounce"},
        {"easing": (2, 0, 0, 1)},
        {"rotate": "sideways"},
        {"repeat": 0},
    ],
)
def test_settings_that_are_not_are_refused(figure, settings):
    figure, line, dot = figure
    with pytest.raises(ValueError):
        Motion(dot, along=line, **settings)


def test_settings_changed_are_drawn(figure):
    figure, line, dot = figure
    motion = Motion(dot, along=line)
    motion.duration = 3
    _, animation = motion_of(svg(figure))
    assert animation.get("dur") == "3s"


def test_elsewhere_it_stays_where_it_is(figure):
    figure, line, dot = figure
    Motion(dot, along=line)
    figure.savefig(io.BytesIO(), format="png")


def test_removed_it_is_drawn_where_it_is(figure):
    figure, line, dot = figure
    Motion(dot, along=line).remove()
    assert f"{SVG}animateMotion" not in {element.tag for element in svg(figure).iter()}


def test_a_slide_moves_it_when_its_step_shows(slides_rs, tmp_path):
    slide = tmp_path / "slide.md"
    slide.write_text(
        "~~~python\n"
        "import matplotlib.pyplot as plt\n"
        "from slides_rs import Motion, Step\n"
        "(line,) = plt.plot([0, 1, 2], [0, 1, 0])\n"
        "(dot,) = plt.plot([0], [0], 'o')\n"
        "Motion(dot, along=line, step=Step(2), duration=2)\n"
        "~~~\n"
    )
    env = {**os.environ, "PYTHONPATH": str(ROOT / "python" / "src")}
    subprocess.run([slides_rs, str(slide)], check=True, env=env, capture_output=True)
    html = slide.with_suffix(".html").read_text()
    assert re.search(r'<g step="2">\s*<g transform="translate\([^)]*\)">\s*<g>\s*<animateMotion [^>]*begin="indefinite"', html)
