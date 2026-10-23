"""Started by the launcher as `python -I -S <launcher-dir>/runner/bootstrap.py`.

`-I` ignores PYTHONPATH, user site-packages and the current folder, so the only code on
`sys.path` is the Python standard library, the app's environment, this runner and the app.
`-S` leaves the environment out until the runner has installed the runtime guard (`_load_site`).
The protocol is described in `devstribution_runner/_runner.py`.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from devstribution_runner._runner import main  # noqa: E402

sys.exit(main())
