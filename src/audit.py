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
                # A file opened by its descriptor has an int for a path.
                if (
                    isinstance(path, str)
                    and isinstance(mode, str)
                    and "r" in mode
                    and not path.endswith(".pyc")
                ):
                    self.files.add(path)

    def add_modules(self):
        """Adds filenames from spec.origin of modules in sys.modules."""
        # A copy, as reading an attribute of a lazy module may import another.
        for module in list(sys.modules.values()):
            # Not everything in sys.modules is a module with a spec.
            spec = getattr(module, "__spec__", None)
            if spec is not None and spec.origin is not None:
                self.files.add(spec.origin)

        self.files.difference_update({"built-in", "frozen"})

    def save(self, path: str):
        """Write the recorded files to `path`, sorted, one per line."""
        with open(path, "w") as file:
            file.writelines(f"{name}\n" for name in sorted(self.files))


audit = Audit()
audit.install_audit_hook()
audit.add_modules()
