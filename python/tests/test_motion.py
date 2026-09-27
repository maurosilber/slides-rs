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


def named(root: ET.Element, animation: ET.Element) -> ET.Element:
    """The group whose path the animation follows, by the name its gid gives it."""
    name = animation.find(f"{SVG}mpath").get(f"{XLINK}href").removeprefix("#")
    (group,) = [g for g in root.iter(f"{SVG}g") if (g.get("id") or "").endswith(f"#{name}")]
    return group


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
    # It follows the line by its name, which slides-rs gives the line's path.
    assert animation.get("path") is None
    assert named(svg(figure), animation).get("id").startswith("#motion-path-")


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
    inner = group.find(f"{SVG}g")
    # The dot is drawn where the line starts, as the SVG draws it, and taken to the
    # origin, which the motion moves along the line from.
    d = named(root, animation).find(f"{SVG}path").get("d")
    start = [float(value) for value in re.findall(r"-?[\d.]+", d)[:2]]
    assert translate(inner) == pytest.approx((-start[0], -start[1]), abs=1e-3)
    (marker,) = inner.iter(f"{SVG}use")
    assert (float(marker.get("x")), float(marker.get("y"))) == pytest.approx(start, abs=1e-3)


def test_a_patch_is_a_path_too(figure):
    figure, line, dot = figure
    circle = plt.Circle((1, 0), 0.5, fill=False)
    figure.axes[0].add_patch(circle)
    Motion(dot, along=circle)
    root = svg(figure)
    _, animation = motion_of(root)
    assert set(named(root, animation).find(f"{SVG}path").get("d").split()) >= {"M", "C", "z"}


def test_the_path_keeps_its_step(figure):
    figure, line, dot = figure
    line.set_gid(Step(2))
    Motion(dot, along=line)
    root = svg(figure)
    _, animation = motion_of(root)
    name = animation.find(f"{SVG}mpath").get(f"{XLINK}href")
    assert named(root, animation).get("id") == f"step=2.. {name}"
    # Its gid is its own again, once drawn.
    assert line.get_gid() == "step=2.."


def test_motions_along_one_path_follow_one_name(figure):
    figure, line, dot = figure
    (other,) = figure.axes[0].plot([0], [0], "s")
    Motion(dot, along=line)
    Motion(other, along=line)
    root = svg(figure)
    hrefs = {mpath.get(f"{XLINK}href") for mpath in root.iter(f"{SVG}mpath")}
    assert len(hrefs) == 1
    assert [g.get("id") for g in root.iter(f"{SVG}g") if "#" in (g.get("id") or "")] == [f"{hrefs.pop()}"]


def test_along_a_hidden_path_it_stays_where_it_is(figure):
    figure, line, dot = figure
    line.set_visible(False)
    Motion(dot, along=line)
    assert f"{SVG}animateMotion" not in {element.tag for element in svg(figure).iter()}


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
    root = svg(figure)
    assert f"{SVG}animateMotion" not in {element.tag for element in root.iter()}
    assert not any("#" in (g.get("id") or "") for g in root.iter(f"{SVG}g"))


def test_a_slide_moves_it_when_its_step_shows(slides_rs, tmp_path):
    slide = tmp_path / "slide.md"
    slide.write_text(
        "~~~python\n"
        "import matplotlib.pyplot as plt\n"
        "from slides_rs import Motion, Step\n"
        "(line,) = plt.plot([0, 1, 2], [0, 1, 0], gid=Step(1))\n"
        "(dot,) = plt.plot([0], [0], 'o')\n"
        "Motion(dot, along=line, step=Step(2), duration=2)\n"
        "~~~\n"
    )
    env = {**os.environ, "PYTHONPATH": str(ROOT / "python" / "src")}
    subprocess.run([slides_rs, str(slide)], check=True, env=env, capture_output=True)
    html = slide.with_suffix(".html").read_text()
    (name,) = re.findall(r'<g step="2">\s*<g>\s*<animateMotion [^>]*begin="indefinite"[^>]*>\s*<mpath xlink:href="#([^"]+)"', html)
    # The line keeps its step, and its path is named for the motion to follow.
    assert re.search(rf'<g step="1">\s*<path [^>]*id="{name}"', html)
