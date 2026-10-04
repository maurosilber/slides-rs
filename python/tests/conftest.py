import os
import pathlib
import shutil
import subprocess

import pytest

ROOT = pathlib.Path(__file__).parents[2]


@pytest.fixture
def slides_rs():
    """The slides-rs that `cargo build` builds, or else the one on the PATH."""
    built = ROOT / "target" / "debug" / "slides-rs"
    command = str(built) if built.exists() else shutil.which("slides-rs")
    if command is None:
        pytest.skip("no slides-rs to render the deck with")
    return command


@pytest.fixture
def render(slides_rs):
    """Renders a deck with the package as it is here, returning its page."""

    def render(deck: pathlib.Path) -> str:
        page = deck.with_suffix(".html")
        env = {**os.environ, "PYTHONPATH": str(ROOT / "python" / "src")}
        subprocess.run(
            [slides_rs, str(deck), str(page)], check=True, env=env, capture_output=True
        )
        return page.read_text()

    return render
