import os
import sys

import matplotlib.axes


class Audit:
    """Record opened files."""

    def __init__(self):
        self.files: set[str] = set()
        # The absolute paths already added, as given, as the same ones come
        # up again. A relative one may stand for another file after a chdir.
        self.seen: set[str] = set()
        # The environment's files are pinned by its lock file. Each prefix is
        # kept both as given and as its real path, followed by a separator,
        # for `str.startswith`.
        prefixes = (sys.prefix, sys.exec_prefix, sys.base_prefix, sys.base_exec_prefix)
        self.environment = tuple(
            {
                os.path.join(path, "")
                for prefix in prefixes
                for path in (os.path.normpath(prefix), os.path.realpath(prefix))
            }
        )

    def add(self, path: str):
        """Record a file by its real path, unless it is in the environment."""
        if os.path.isabs(path):
            if path in self.seen:
                return
            self.seen.add(path)
            # Most files opened are the environment's, which a comparison of
            # strings settles without the lookups of `realpath`.
            if os.path.normpath(path).startswith(self.environment):
                return
        path = os.path.realpath(path)
        if not path.startswith(self.environment):
            self.files.add(path)

    def install_audit_hook(self):
        """Record opened files in read mode (excluding .pyc)."""

        @sys.addaudithook
        def audit_hook(event, args):
            if event == "open":
                path, mode, _flags = args
                # A file opened by its descriptor has an int for a path.
                if isinstance(path, int) or not isinstance(mode, str) or "r" not in mode:
                    return
                path = os.fsdecode(os.fspath(path))
                if not path.endswith(".pyc"):
                    self.add(path)

    def add_modules(self):
        """Adds filenames from spec.origin of modules in sys.modules."""
        # A copy, as reading an attribute of a lazy module may import another.
        for module in list(sys.modules.values()):
            # Not everything in sys.modules is a module with a spec, and a
            # built-in or frozen module has no file.
            spec = getattr(module, "__spec__", None)
            if spec is not None and spec.has_location and spec.origin is not None:
                self.add(spec.origin)

    def save(self, path: str):
        """Write the recorded files to `path`, sorted, one per line."""
        with open(path, "w") as file:
            file.writelines(f"{name}\n" for name in sorted(self.files))


audit = Audit()
audit.install_audit_hook()
audit.add_modules()
