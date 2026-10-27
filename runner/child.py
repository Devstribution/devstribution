"""Started by the runtime guard in place of a Python an app starts:
`python -I -S -B [options] <runner-dir>/child.py <settings> <what runs>`.

The guard is installed with the run's settings before any of the child's code runs; see
`devstribution_runner/_child.py`.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from devstribution_runner._child import main  # noqa: E402

sys.exit(main(sys.argv[1:]))
