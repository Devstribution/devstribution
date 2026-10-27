"""Another Python started by the app, under the runtime guard.

The guard rewrites the command that starts a Python interpreter (`python -c ...`, a script,
`python -m ...`, `multiprocessing`'s workers) into `python -I -S -B [options] child.py <settings>
<what runs>`. Here the guard is installed from the settings file the runner wrote for the run
(the app cannot change it: it is in the guard's sealed folder), then the environment's
`site-packages` is loaded and what the app asked for runs, as Python would have run it.

What the guard reports goes to the run's report file, which the runner passes on to the
launcher: a stop here ends this process at once, and the launcher then ends the whole run.
Without readable settings nothing runs.
"""

import json
import os
import runpy
import sys
import traceback
import types
from typing import Any, Dict, List

from . import _guard


def main(argv: List[str]) -> int:
    if not argv:
        sys.stderr.write("devstribution: child.py needs the run's guard settings\n")
        return 2
    settings, rest = argv[0], argv[1:]
    try:
        with open(settings, encoding="utf-8") as f:
            config = json.load(f)
        report = config["child"]["report"]
    except (OSError, ValueError, KeyError, TypeError) as exc:
        sys.stderr.write(f"devstribution: cannot start Python under the runtime guard: {exc}\n")
        return _guard.STOP_EXIT_CODE

    guard: List[_guard.Guard] = []

    def emit(message: Dict[str, Any]) -> None:
        line = json.dumps(dict(message, child=os.getpid()), ensure_ascii=False, default=str)
        with guard[0].unchecked():
            with open(report, "a", encoding="utf-8") as f:
                f.write(line + "\n")

    installed = _guard.install(emit, config)
    if installed is None:
        sys.stderr.write("devstribution: the runtime guard is off for this run\n")
        return _guard.STOP_EXIT_CODE
    guard.append(installed)
    # The guard starts Python with `-S`: the environment's `site-packages` (and the code its
    # `.pth` files run) is loaded here, under the guard, as the runner does.
    import site

    site.main()
    try:
        return _run(rest)
    except SystemExit:
        raise
    except BaseException:
        traceback.print_exc()
        return 1
    finally:
        installed.report_files()


def _run(rest: List[str]) -> int:
    main_module = types.ModuleType("__main__")
    main_module.__dict__["__builtins__"] = __builtins__
    if rest[:1] == ["-c"]:
        code = rest[1] if len(rest) > 1 else ""
        sys.argv = ["-c"] + rest[2:]
        sys.path.insert(0, "")
        _exec(code, "<string>", main_module)
    elif rest[:1] == ["-m"]:
        if len(rest) < 2:
            sys.stderr.write("Argument expected for the -m option\n")
            return 2
        sys.argv = [rest[1]] + rest[2:]
        sys.path.insert(0, os.getcwd())
        runpy.run_module(rest[1], run_name="__main__", alter_sys=True)
    elif not rest or rest[0] == "-":
        sys.argv = list(rest) or [""]
        sys.path.insert(0, "")
        _exec(sys.stdin.read(), "<stdin>", main_module)
    else:
        script = rest[0]
        sys.argv = list(rest)
        sys.path.insert(0, os.path.dirname(os.path.abspath(script)))
        runpy.run_path(script, run_name="__main__")
    return 0


def _exec(code: str, name: str, module: types.ModuleType) -> None:
    sys.modules["__main__"] = module
    exec(compile(code, name, "exec"), module.__dict__)
