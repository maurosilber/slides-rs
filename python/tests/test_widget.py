import re
import xml.etree.ElementTree as ET

import matplotlib
import numpy as np
import pytest

matplotlib.use("svg")
import matplotlib.pyplot as plt  # noqa: E402

from slides_rs import Motion, Slider, Step  # noqa: E402

SVG = "{http://www.w3.org/2000/svg}"
XLINK = "{http://www.w3.org/1999/xlink}"


@pytest.fixture
def figure():
    figure, axes = plt.subplots()
    t = np.linspace(0, 2, 20)
    (line,) = axes.plot(t, np.sin(t), gid=Step(1))
    (dot,) = axes.plot(t[:1], np.sin(t[:1]), "o")
    yield figure, Motion(dot, along=line).timing(t)
    plt.close(figure)


def svg_of(html: str) -> ET.Element:
    return ET.fromstring(re.search(r"<svg.*</svg>", html, re.DOTALL)[0])


def test_the_motion_begins_with_the_figure(figure):
    root = svg_of(Slider(*figure)._repr_html_())
    (animation,) = root.iter(f"{SVG}animateMotion")
    assert animation.get("begin") == "0s"
    assert {each.get("begin") for each in root.iter(f"{SVG}animateTransform")} == {"0s"}


def test_the_motion_is_played_by_the_deck_elsewhere(figure):
    slider = Slider(*figure)
    slider._repr_html_()
    assert figure[1]._begin == "indefinite"


def test_every_reference_is_to_an_id_in_the_figure(figure):
    root = svg_of(Slider(*figure)._repr_html_())
    ids = {element.get("id") for element in root.iter()} - {None}
    for element in root.iter():
        for key in ("href", f"{XLINK}href"):
            if element.get(key):
                assert element.get(key).removeprefix("#") in ids
    (mpath,) = root.iter(f"{SVG}mpath")
    named = next(e for e in root.iter() if e.get("id") == mpath.get(f"{XLINK}href")[1:])
    assert named.tag == f"{SVG}path"


def test_a_step_becomes_an_attribute(figure):
    root = svg_of(Slider(*figure)._repr_html_())
    assert [g.get("step") for g in root.iter(f"{SVG}g") if g.get("step") is not None] == ["1.."]
    assert not any("#" in (e.get("id") or "") or "step" in (e.get("id") or "") for e in root.iter())


def test_the_same_figure_is_the_same_html(figure):
    slider = Slider(*figure)
    assert slider._repr_html_() == slider._repr_html_()


def test_the_slider_goes_up_to_the_end_of_the_motion(figure):
    html = Slider(*figure).label("t")._repr_html_()
    assert '<input type="range" min="0" max="100" value="0" aria-label="t"' in html
    assert '"start": 0.0, "stop": 2.0, "step": 0.02' in html
    assert "<label>t</label>" in html


def test_each_method_returns_the_slider(figure):
    slider = Slider(*figure)
    assert slider.range(0, 4, step=0.5).value(1).label("t").play(speed=2).style(width="50%") is slider
    html = slider._repr_html_()
    assert '<input type="range" min="0" max="8" value="2"' in html
    assert '"step": 0.5, "digits": 1, "speed": 2.0' in html


def test_the_range_is_read_from_the_motion_when_shown(figure):
    slider = Slider(*figure)
    figure[1].timing(duration=5)
    assert '"stop": 5.0' in slider._repr_html_()


def test_without_play_it_has_no_button(figure):
    assert "<button" not in Slider(*figure).play(False)._repr_html_()


def test_a_value_is_at_the_closest_tick(figure):
    html = Slider(*figure).range(step=0.5).value(1.2)._repr_html_()
    assert 'max="4" value="2"' in html
    assert ">1.0</output>" in html


def test_a_part_is_styled_with_properties(figure):
    slider = Slider(*figure).style(width="80%", accent="tomato").style("output", font_size="1em")
    slider.style("figure", css={"--anything": "1"}).style("input", flex=2)
    html = slider._repr_html_()
    assert '<div class="slides-rs-slider" style="width: 80%; --slider-accent: tomato">' in html
    assert '<output style="min-width: 4ch; font-size: 1em">' in html
    assert '<input type="range" min="0" max="100" value="0" style="flex: 2">' in html
    assert svg_of(html).get("style") == "--anything: 1"


def test_a_style_is_removed_with_none(figure):
    slider = Slider(*figure).style(width="80%").style(width=None)
    assert '<div class="slides-rs-slider">' in slider._repr_html_()


def test_a_part_or_value_that_is_not_one_is_refused(figure):
    slider = Slider(*figure)
    with pytest.raises(ValueError, match="parts"):
        slider.style("track", color="red")
    with pytest.raises(ValueError, match="value"):
        slider.style(color="red; display: none")
    with pytest.raises(ValueError, match="name"):
        slider.style(css={"a b": "1"})
    with pytest.raises(ValueError, match="later stop"):
        slider.range(2, 1)
    with pytest.raises(ValueError, match="speed"):
        slider.play(speed=0)


def test_a_motion_of_another_figure_is_refused(figure):
    other = plt.figure()
    with pytest.raises(ValueError, match="not in the figure"):
        Slider(other, figure[1])


def test_the_slider_is_no_step_of_the_deck(figure):
    (input,) = re.findall(r"<input[^>]*>", Slider(*figure)._repr_html_())
    assert not re.search(r"\s(step|also|data-step)=", input)
