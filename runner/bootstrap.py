"""Started by the launcher as `python -I -S -X pycache_prefix=<cache> <launcher-dir>/runner/bootstrap.py`.

`-I` ignores PYTHONPATH, user site-packages and the current folder, so the only code on
`sys.path` is the Python standard library, the app's environment, this runner and the app.
`-S` leaves the environment out until the runner has installed the runtime guard (`_load_site`).
`-X pycache_prefix` caches the standard library's bytecode in devstribution's data folder (or
`-B`, no cache at all, when that folder cannot be made).
The protocol is described in `devstribution_runner/_runner.py`.
"""

import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))


def _runner_from_source() -> None:
    """The runner's own modules are compiled from their source on every run, never read from a
    bytecode cache: the launcher checks the runner's files before each run, not a cache."""
    from importlib.machinery import FileFinder, SourceFileLoader

    class SourceOnly(SourceFileLoader):
        def get_code(self, fullname):
            path = self.get_filename(fullname)
            return self.source_to_code(self.get_data(path), path)

    for folder in (_HERE, os.path.join(_HERE, "devstribution_runner")):
        sys.path_importer_cache[folder] = FileFinder(folder, (SourceOnly, [".py"]))


_runner_from_source()
sys.path.insert(0, _HERE)

from devstribution_runner._runner import main  # noqa: E402

sys.exit(main())
