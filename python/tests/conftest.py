import pathlib
import shutil

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
