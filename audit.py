import sys


class Audit:
    """Record opened files."""

    def __init__(self):
        self.files: set[str] = set()

    def install_audit_hook(self):
        """Record opened files in read mode (excluding .pyc)."""

        @sys.addaudithook
        def audit_hook(event, args):
            if event == "open":
                path, mode, _flags = args
                if isinstance(mode, str) and "r" in mode and not path.endswith(".pyc"):
                    self.files.add(path)

    def add_modules(self):
        """Adds filenames from spec.origin of modules in sys.modules."""
        for module in sys.modules.values():
            if module.__spec__ is not None and module.__spec__.origin is not None:
                self.files.add(module.__spec__.origin)

        self.files.difference_update({"built-in", "frozen"})


audit = Audit()
audit.install_audit_hook()
audit.add_modules()
