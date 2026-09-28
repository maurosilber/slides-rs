"""Install a .vsix in the VS Code profile associated with a folder.

`code --install-extension` targets the default profile unless told otherwise,
and no environment variable names the profile of the window a terminal runs in.
VS Code records which profile each folder opens with in its global storage,
so the profile is looked up there.

Usage: python install.py <code> <vsix> <folder>
"""

import json
import subprocess
import sys
from pathlib import Path

STORAGE = (
    Path.home()
    / "Library/Application Support/Code/User/globalStorage/storage.json"
)


def profile_name(folder: Path) -> str | None:
    """The name of the profile the folder opens with, or None for the default."""
    try:
        storage = json.loads(STORAGE.read_text())
    except FileNotFoundError:
        return None
    workspaces = storage.get("profileAssociations", {}).get("workspaces", {})
    location = workspaces.get(folder.resolve().as_uri())
    for profile in storage.get("userDataProfiles", []):
        if profile["location"] == location:
            return profile["name"]
    return None


def main() -> None:
    code, vsix, folder = sys.argv[1:]
    cmd = [code, "--install-extension", vsix, "--force"]
    if (name := profile_name(Path(folder))) is not None:
        print(f"Installing in profile {name!r}")
        cmd += ["--profile", name]
    subprocess.run(cmd, check=True)


if __name__ == "__main__":
    main()
