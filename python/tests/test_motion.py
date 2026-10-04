import io
import re
import xml.etree.ElementTree as ET

import matplotlib
import numpy as np
import pytest

matplotlib.use("svg")
import matplotlib.pyplot as plt  # noqa: E402

from slides_rs import Motion, Step  # noqa: E402

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


def by_id(root: ET.Element, id: str) -> ET.Element:
    (element,) = [element for element in root.iter() if element.get("id") == id]
    return element


def motion_of(root: ET.Element) -> tuple[ET.Element, ET.Element]:
    """The group that moves, and its animation."""
    (animation,) = root.iter(f"{SVG}animateMotion")
    return by_id(root, animation.get(f"{XLINK}href").removeprefix("#")), animation


def named(root: ET.Element, animation: ET.Element) -> ET.Element:
    """The group whose path the animation follows, by the name its gid gives it."""
    name = animation.find(f"{SVG}mpath").get(f"{XLINK}href").removeprefix("#")
    (group,) = [g for g in root.iter(f"{SVG}g") if (g.get("id") or "").endswith(f"#{name}")]
    return group


def parent_of(root: ET.Element, element: ET.Element) -> ET.Element:
    return next(parent for parent in root.iter() if element in list(parent))


def anchoring(root: ET.Element) -> ET.Element:
    """The animation that takes the artist to the origin while it moves."""
    (animation,) = root.iter(f"{SVG}animateTransform")
    return animation


def translate(text: str) -> tuple[float, float]:
    x, y = text.split()
    return float(x), float(y)


def numbers(text: str) -> list[float]:
    return [float(value) for value in text.split(";")]


def test_it_writes_an_animation_the_deck_begins(figure):
    figure, line, dot = figure
    Motion(dot, along=line)
    root = svg(figure)
    _, animation = motion_of(root)
    assert animation.get("begin") == "indefinite"
    assert animation.get("dur") == "1s"
    assert animation.get("fill") == "freeze"
    # Paced along the path, the default, and not turned.
    assert {"calcMode", "keyTimes", "keyPoints", "rotate", "repeatCount"}.isdisjoint(animation.keys())
    # It follows the line by its name, which slides-rs gives the line's path.
    assert animation.get("path") is None
    assert named(root, animation).get("id").startswith("#motion-path-")


def test_each_method_returns_the_motion(figure):
    figure, line, dot = figure
    motion = Motion(dot, along=line)
    for method, argument in [("starts", Step(2)), ("rotate", "auto"), ("repeat", 2), ("hold", True)]:
        assert getattr(motion, method)(argument) is motion
    assert motion.timing(duration=2) is motion


def test_it_shows_in_the_step_of_its_artist(figure):
    figure, line, dot = figure
    dot.set_gid(Step(1))
    Motion(dot, along=line)
    root = svg(figure)
    moving, animation = motion_of(root)
    # The step holds what moves and the animation, which begins once it shows.
    (holder,) = [g for g in root.iter(f"{SVG}g") if g.get("id") == "step=1.."]
    assert parent_of(root, moving) is holder
    assert parent_of(root, animation) is holder
    # And is the artist's again, once drawn, which draws within it without a step.
    assert dot.get_gid() == "step=1.."
    assert not any((g.get("id") or "").startswith("step") for g in moving.iter(f"{SVG}g"))


def test_it_starts_in_a_step_of_its_own(figure):
    figure, line, dot = figure
    dot.set_gid(Step(1))
    Motion(dot, along=line).starts(Step(2))
    root = svg(figure)
    moving, animation = motion_of(root)
    # The animation is in a group of the step, apart from what it moves, which shows
    # in the artist's step.
    start = parent_of(root, animation)
    assert start.get("id") == "step=2.."
    assert list(start) == [animation, anchoring(root)]
    assert parent_of(root, start) is parent_of(root, moving)
    assert parent_of(root, moving).get("id") == "step=1.."


def test_it_starts_in_a_step_not_a_string(figure):
    figure, line, dot = figure
    with pytest.raises(TypeError):
        Motion(dot, along=line).starts("step=2..")


@pytest.mark.parametrize("options", [{}, {"bbox_inches": "tight"}])
def test_it_moves_from_where_it_is_drawn(figure, options):
    figure, line, dot = figure
    Motion(dot, along=line)
    root = svg(figure, **options)
    moving, animation = motion_of(root)
    (inner,) = list(moving)
    # The dot is drawn where the line starts, as the SVG draws it, and taken to the
    # origin while it moves, which the motion moves along the line from. Otherwise, it
    # is where it is drawn.
    d = named(root, animation).find(f"{SVG}path").get("d")
    start = [float(value) for value in re.findall(r"-?[\d.]+", d)[:2]]
    anchor = anchoring(root)
    assert inner.get("transform") is None
    assert anchor.get(f"{XLINK}href") == f"#{inner.get('id')}"
    assert (anchor.get("attributeName"), anchor.get("type")) == ("transform", "translate")
    assert translate(anchor.get("from")) == translate(anchor.get("to")) == pytest.approx((-start[0], -start[1]), abs=1e-3)
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
    "timing, attributes",
    [
        ({"duration": 2.5}, {"dur": "2.5s"}),
        (
            {"duration": 2, "easing": "ease-in-out"},
            {"dur": "2s", "calcMode": "spline", "keyPoints": "0;1", "keyTimes": "0;1", "keySplines": "0.42 0 0.58 1"},
        ),
        ({"duration": 1, "easing": (0.1, 0.7, 1.0, 0.1)}, {"calcMode": "spline", "keySplines": "0.1 0.7 1 0.1"}),
        (
            {"t": [0, 1, 4], "fraction": [0, 0.8, 1]},
            {"dur": "4s", "calcMode": "linear", "keyTimes": "0;0.25;1", "keyPoints": "0;0.8;1"},
        ),
        (
            {"t": [0, 1, 4], "fraction": [0, 0.8, 1], "easing": ["ease-in", "linear"]},
            {"calcMode": "spline", "keySplines": "0.42 0 1 1;0 0 1 1"},
        ),
        (
            {"t": [0, 1, 2], "fraction": [0, 0.5, 1], "easing": "discrete"},
            {"calcMode": "discrete", "keyTimes": "0;0.5;1", "keyPoints": "0;0.5;1"},
        ),
        # Up to the first time, it waits at the start.
        (
            {"t": [1, 2, 4], "fraction": [0.2, 0.5, 1], "easing": "ease"},
            {
                "dur": "4s",
                "keyTimes": "0;0.25;0.5;1",
                "keyPoints": "0.2;0.2;0.5;1",
                "keySplines": "0 0 1 1;0.25 0.1 0.25 1;0.25 0.1 0.25 1",
            },
        ),
    ],
)
def test_its_timing_is_the_animation_s(figure, timing, attributes):
    figure, line, dot = figure
    Motion(dot, along=line).timing(**timing)
    _, animation = motion_of(svg(figure))
    assert {key: animation.get(key) for key in attributes} == attributes


def test_a_time_at_each_vertex_follows_the_line_as_plotted():
    figure, axes = plt.subplots()
    # Twice as far to the second vertex as to the first, in the SVG as in the data.
    axes.set_aspect("equal")
    (line,) = axes.plot([0, 1, 3], [0, 0, 0])
    (dot,) = axes.plot([0], [0], "o")
    Motion(dot, along=line).timing([0, 2, 3])
    _, animation = motion_of(svg(figure))
    plt.close(figure)
    assert animation.get("dur") == "3s"
    assert numbers(animation.get("keyTimes")) == pytest.approx([0, 2 / 3, 1], abs=1e-6)
    assert numbers(animation.get("keyPoints")) == pytest.approx([0, 1 / 3, 1], abs=1e-6)
    assert animation.get("calcMode") == "linear"


def test_a_gap_in_the_line_takes_it_no_further():
    figure, axes = plt.subplots()
    axes.set_aspect("equal")
    (line,) = axes.plot([0, 1, np.nan, 2, 3], [0, 0, np.nan, 0, 0])
    (dot,) = axes.plot([0], [0], "o")
    Motion(dot, along=line).timing([0, 1, 2, 3, 4])
    _, animation = motion_of(svg(figure))
    plt.close(figure)
    assert numbers(animation.get("keyPoints")) == pytest.approx([0, 0.5, 0.5, 0.5, 1], abs=1e-6)


@pytest.mark.parametrize(
    "settings, attributes",
    [
        (("rotate", "auto"), {"rotate": "auto"}),
        (("rotate", 45), {"rotate": "45"}),
        (("repeat",), {"repeatCount": "indefinite"}),
        (("repeat", 2.5), {"repeatCount": "2.5"}),
        (("repeat", 1), {"repeatCount": None}),
        (("hold", False), {"fill": "remove"}),
    ],
)
def test_its_settings_are_the_animation_s(figure, settings, attributes):
    figure, line, dot = figure
    method, *arguments = settings
    getattr(Motion(dot, along=line), method)(*arguments)
    _, animation = motion_of(svg(figure))
    assert {key: animation.get(key) for key in attributes} == attributes


def test_it_repeats_for_a_while_and_on_from_where_it_ended(figure):
    figure, line, dot = figure
    Motion(dot, along=line).repeat(seconds=5, accumulate=True)
    root = svg(figure)
    _, animation = motion_of(root)
    # Taken back to the origin as it goes on, as long as it moves.
    for animation in (animation, anchoring(root)):
        assert {key: animation.get(key) for key in ("repeatCount", "repeatDur", "accumulate")} == {
            "repeatCount": "indefinite",
            "repeatDur": "5s",
            "accumulate": "sum",
        }


def test_it_is_taken_to_the_origin_as_long_as_it_moves(figure):
    figure, line, dot = figure
    Motion(dot, along=line).timing(duration=3).repeat(2).hold(False)
    root = svg(figure)
    _, animation = motion_of(root)
    for key in ("begin", "dur", "fill", "repeatCount"):
        assert anchoring(root).get(key) == animation.get(key), key


def test_it_does_not_rotate_as_it_accumulates(figure):
    figure, line, dot = figure
    with pytest.raises(ValueError):
        Motion(dot, along=line).rotate("auto").repeat(accumulate=True)
    with pytest.raises(ValueError):
        Motion(dot, along=line).repeat(accumulate=True).rotate(30)


@pytest.mark.parametrize(
    "method, arguments, options",
    [
        ("timing", (), {}),
        ("timing", ([0, 1],), {"duration": 1}),
        ("timing", (), {"duration": 0}),
        ("timing", (), {"duration": 1, "fraction": [0, 1]}),
        ("timing", ([0, 2, 1],), {"fraction": [0, 0.5, 1]}),
        ("timing", ([-1, 1],), {"fraction": [0, 1]}),
        ("timing", ([0, 1],), {"fraction": [0, 0.5, 1]}),
        ("timing", ([0, 1],), {"fraction": [0, 2]}),
        ("timing", ([0, 1, 2],), {}),  # The line has 20 vertices.
        ("timing", (), {"duration": 1, "easing": "bounce"}),
        ("timing", (), {"duration": 1, "easing": (2, 0, 0, 1)}),
        ("timing", (), {"duration": 1, "easing": (0.5, 0, 0.5, 2)}),
        ("timing", ([0, 1, 2],), {"fraction": [0, 0.5, 1], "easing": ["ease"]}),
        ("rotate", ("sideways",), {}),
        ("repeat", (0,), {}),
        ("repeat", (), {"seconds": -1}),
    ],
)
def test_settings_that_are_not_are_refused(figure, method, arguments, options):
    figure, line, dot = figure
    with pytest.raises(ValueError):
        getattr(Motion(dot, along=line), method)(*arguments, **options)


def test_a_time_at_each_vertex_takes_a_line_of_them(figure):
    figure, line, dot = figure
    circle = plt.Circle((1, 0), 0.5)
    figure.axes[0].add_patch(circle)
    with pytest.raises(ValueError, match="curves"):
        Motion(dot, along=circle).timing(np.linspace(0, 1, len(circle.get_path().vertices)))
    line.set_drawstyle("steps-mid")
    with pytest.raises(ValueError, match="steps"):
        Motion(dot, along=line).timing(np.linspace(0, 1, 20))


def test_settings_changed_are_drawn(figure):
    figure, line, dot = figure
    motion = Motion(dot, along=line)
    svg(figure)
    motion.timing(duration=3)
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


def test_a_slide_moves_it_when_its_step_shows(render, tmp_path):
    slide = tmp_path / "slide.md"
    slide.write_text(
        "~~~python\n"
        "import matplotlib.pyplot as plt\n"
        "from slides_rs import Motion, Step\n"
        "(line,) = plt.plot([0, 1, 2], [0, 1, 0], gid=Step(1))\n"
        "(dot,) = plt.plot([0], [0], 'o', gid=Step(2))\n"
        "Motion(dot, along=line).starts(Step(3)).timing([0, 1, 2])\n"
        "~~~\n"
    )
    html = render(slide)
    # The dot shows in its step, and moves in the next, what it moves named for the
    # animation to move it.
    (moving,) = re.findall(r'<g step="2"[^>]*>\s*<g id="([^"]+)">', html)
    (path,) = re.findall(
        rf'<g step="3"[^>]*>\s*<animateMotion xlink:href="#{moving}" [^>]*begin="indefinite"[^>]*>\s*<mpath xlink:href="#([^"]+)"',
        html,
    )
    # The line keeps its step, and its path is named for the motion to follow.
    assert re.search(rf'<g step="1"[^>]*>\s*<path [^>]*id="{path}"', html)
