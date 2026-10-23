"""The runner: starts one app run inside the app's process.

Protocol (version 1), JSON Lines in UTF-8:

stdin, launcher → runner
  first line, the start message:
    {"protocol": 1, "kind": "form", "app_dir": "...", "entry": "main:run",
     "output_dir": "...", "inputs": {"name": value}, "input_types": {"name": "file"},
     "outputs": [{"name": "report", "type": "file"}],
     "secrets": {"name": "value"}, "allowed_secrets": ["name"], "args": [],
     "pycache_dir": "...",
     "guard": {"network": ["api.company.com"], "sap": false, "allow": ["rule-id"]}}
  `pycache_dir` keeps bytecode out of the app folder, which must stay as verified.
  `guard` is what the runtime guard checks the run against (see `_guard.py`); a start message
  without it gets the strictest guard: no network, no SAP, nothing allowed.
  then control messages: {"type": "cancel"}. Closing stdin also cancels the run.

runner → launcher, on the original stdout (the app's own stdout is redirected to stderr, which
the launcher writes to the run log):
    {"type": "progress", "value": 0.3, "message": "..."}
    {"type": "log", "level": "info", "message": "..."}
    {"type": "com_process", "pid": 1234, "created": 133000000000000000}
    {"type": "guard", "action": "stop" | "block" | "observe", "rule": "...", "message": "...",
     "detail": "..."}
    {"type": "result", "status": "ok", "outputs": {...}}
    {"type": "result", "status": "error", "error": "...", "traceback": "..."}
    {"type": "result", "status": "cancelled"}

Exit code: 0 ok, 1 error, 2 cancelled, 3 protocol error, 4 stopped by the guard.
"""

import datetime
import importlib
import inspect
import json
import os
import runpy
import sys
import threading
import traceback
import _thread
from pathlib import Path
from typing import Any, Callable, Dict, List

from . import _guard
from ._context import Cancelled, Context

PROTOCOL = 1

#: After a cancel request, how long the app has to reach a ctx call before the runner
#: interrupts the main thread (KeyboardInterrupt, which still runs `finally` blocks).
INTERRUPT_AFTER_SECONDS = 3.0


class AppError(Exception):
    """A mistake in the app or its inputs, reported without a runner traceback."""


class Channel:
    """The protocol stream. Taken over from fd 1 before any app code runs."""

    def __init__(self) -> None:
        sys.stdout.flush()
        fd = os.dup(1)
        os.dup2(2, 1)  # the app's prints (including from C extensions) go to stderr
        self._out = os.fdopen(fd, "w", encoding="utf-8", newline="\n", buffering=1)
        self._lock = threading.Lock()

    def emit(self, message: Dict[str, Any]) -> None:
        line = json.dumps(message, ensure_ascii=False, default=str)
        with self._lock:
            self._out.write(line + "\n")
            self._out.flush()


class ControlReader:
    """Reads stdin lines with os.read on the raw fd: unlike sys.stdin, it holds no lock while
    blocked, so a reader thread still waiting at exit does not crash interpreter shutdown."""

    def __init__(self) -> None:
        self._buffer = b""
        # On Windows a read pending on a synchronous pipe blocks every other call on that pipe.
        # Loading a DLL whose C runtime looks at the standard input (numpy's OpenBLAS does) then
        # waits under the loader lock and the whole process hangs until the launcher writes a
        # line. So the pipe moves to a private fd and the standard input becomes NUL.
        self._fd = os.dup(0)
        null = os.open(os.devnull, os.O_RDONLY)
        os.dup2(null, 0)
        os.close(null)

    def readline(self) -> bytes:
        while b"\n" not in self._buffer:
            chunk = os.read(self._fd, 65536)
            if not chunk:
                line, self._buffer = self._buffer, b""
                return line
            self._buffer += chunk
        line, _, self._buffer = self._buffer.partition(b"\n")
        return line + b"\n"


def main() -> int:
    channel = Channel()
    control = ControlReader()
    try:
        start = json.loads(control.readline().decode("utf-8"))
        if not isinstance(start, dict) or start.get("protocol") != PROTOCOL:
            raise ValueError(f"unsupported protocol (expected {PROTOCOL})")
    except Exception as exc:
        channel.emit({"type": "result", "status": "error", "error": f"bad start message: {exc}"})
        return 3

    if start.get("pycache_dir"):
        sys.pycache_prefix = str(start["pycache_dir"])
    else:
        sys.dont_write_bytecode = True

    # fd 0 belongs to the protocol; the app gets an empty stdin.
    sys.stdin = open(os.devnull, encoding="utf-8")

    ctx = Context(
        channel.emit,
        output_dir=Path(start["output_dir"]),
        secrets=start.get("secrets") or {},
        allowed_secrets=start.get("allowed_secrets") or [],
        web_port=start.get("web_port"),
        web_token=start.get("web_token"),
    )
    ctx.output_dir.mkdir(parents=True, exist_ok=True)
    import devstribution_runner

    devstribution_runner.ctx = ctx

    # Last thing before app code: from here on, what the app does is checked.
    _guard.install(channel.emit, start.get("guard") or {})

    finished = threading.Event()
    threading.Thread(target=_control_loop, args=(control, ctx, finished), daemon=True).start()

    try:
        try:
            outputs = _run(start, ctx)
        finally:
            ctx._run_cleanup()
        result: Dict[str, Any] = {"type": "result", "status": "ok", "outputs": outputs}
        code = 0
    except (Cancelled, KeyboardInterrupt):
        result, code = {"type": "result", "status": "cancelled"}, 2
    except AppError as exc:
        result, code = {"type": "result", "status": "error", "error": str(exc)}, 1
    except SystemExit as exc:
        if exc.code in (None, 0):
            result, code = {"type": "result", "status": "ok", "outputs": {}}, 0
        else:
            msg = f"the app exited with code {exc.code}"
            result, code = {"type": "result", "status": "error", "error": msg}, 1
    except BaseException as exc:
        tb = traceback.format_exc()
        sys.stderr.write(tb)
        error = f"{type(exc).__name__}: {exc}"
        result, code = {"type": "result", "status": "error", "error": error, "traceback": tb}, 1
    finally:
        finished.set()
    channel.emit(result)
    return code


def _control_loop(control: ControlReader, ctx: Context, finished: threading.Event) -> None:
    for raw in iter(control.readline, b""):
        try:
            message = json.loads(raw.decode("utf-8"))
        except ValueError:
            continue
        if isinstance(message, dict) and message.get("type") == "cancel":
            break
    else:
        if finished.is_set():
            return
        # stdin closed: the launcher is gone, nobody will read the result. Stop.
    ctx._request_cancel()
    if not finished.wait(INTERRUPT_AFTER_SECONDS):
        _thread.interrupt_main()


def _run(start: Dict[str, Any], ctx: Context) -> Dict[str, Any]:
    app_dir = Path(start["app_dir"]).resolve()
    sys.path.insert(1, str(app_dir))
    kind = start.get("kind", "form")
    module_name, _, function_name = start["entry"].partition(":")

    if kind == "form":
        func = _load_function(module_name, function_name)
        kwargs = _convert_inputs(func, start, ctx)
        returned = func(**kwargs, ctx=ctx)
        return _convert_outputs(returned, start.get("outputs") or [], _result_folders(start, ctx))

    if kind in ("gui", "cli"):
        sys.argv = [module_name] + [str(a) for a in start.get("args") or []]
        if function_name:
            _load_function(module_name, function_name)()
        else:
            runpy.run_module(module_name, run_name="__main__", alter_sys=True)
        return {}

    raise AppError(f"kind = {kind!r} is not supported by this version of the launcher yet")


def _load_function(module_name: str, function_name: str) -> Callable[..., Any]:
    try:
        module = importlib.import_module(module_name)
    except ModuleNotFoundError as exc:
        if exc.name == module_name.split(".")[0]:
            raise AppError(f"entry module {module_name!r} not found in the app folder") from None
        raise
    func = getattr(module, function_name, None)
    if not callable(func):
        raise AppError(f"{module_name}.py has no function {function_name!r} (see `entry`)")
    return func


def _convert_inputs(func: Callable[..., Any], start: Dict[str, Any], ctx: Context) -> Dict[str, Any]:
    params = inspect.signature(func).parameters
    if "ctx" not in params:
        raise AppError("the entry function must accept a `ctx` argument")
    values = start.get("inputs") or {}
    secrets = start.get("secrets") or {}
    kwargs: Dict[str, Any] = {}
    for name, kind in (start.get("input_types") or {}).items():
        raw = secrets.get(name) if kind == "secret" else values.get(name)
        if raw is None:
            param = params.get(name)
            if param is not None and param.default is not inspect.Parameter.empty:
                continue  # let the function's own default apply
            kwargs[name] = None
            continue
        kwargs[name] = _convert(name, kind, raw)
    return kwargs


def _convert(name: str, kind: str, raw: Any) -> Any:
    try:
        if kind in ("file", "folder"):
            path = Path(raw)
            if kind == "file" and not path.is_file():
                raise AppError(f"input {name!r}: file not found: {path}")
            if kind == "folder" and not path.is_dir():
                raise AppError(f"input {name!r}: folder not found: {path}")
            return path
        if kind == "number":
            if isinstance(raw, bool):
                raise ValueError("not a number")
            if isinstance(raw, (int, float)):
                return raw
            text = str(raw).strip()
            return int(text) if text.lstrip("-").isdigit() else float(text)
        if kind == "bool":
            if isinstance(raw, bool):
                return raw
            lowered = str(raw).strip().lower()
            if lowered in ("true", "yes", "1", "on"):
                return True
            if lowered in ("false", "no", "0", "off"):
                return False
            raise ValueError("expected true or false")
        if kind == "date":
            return datetime.date.fromisoformat(str(raw))
        return str(raw)
    except AppError:
        raise
    except ValueError as exc:
        raise AppError(f"input {name!r}: invalid {kind} {raw!r} ({exc})") from None


def _result_folders(start: Dict[str, Any], ctx: Context) -> List[Path]:
    """Where file outputs may be: the run's output folder, and the folders the user chose for
    `folder` inputs (apps that keep their work between runs write there)."""
    values = start.get("inputs") or {}
    folders = [ctx.output_dir]
    for name, kind in (start.get("input_types") or {}).items():
        if kind == "folder" and values.get(name):
            folders.append(Path(values[name]))
    return folders


def _convert_outputs(returned: Any, outputs: list, folders: List[Path]) -> Dict[str, Any]:
    declared = {o["name"]: o["type"] for o in outputs}
    if returned is None:
        returned = {}
    if not isinstance(returned, dict):
        raise AppError(f"run() must return a dict of outputs, not {type(returned).__name__}")
    unknown = set(returned) - set(declared)
    if unknown:
        raise AppError(f"run() returned undeclared outputs {sorted(unknown)}; declare them in pyproject.toml")
    missing = set(declared) - set(returned)
    if missing:
        raise AppError(f"run() did not return the outputs {sorted(missing)}")
    roots = [folder.resolve() for folder in folders]
    result: Dict[str, Any] = {}
    for name, kind in declared.items():
        value = returned[name]
        if kind == "file":
            path = Path(value).resolve()
            if not path.is_file():
                raise AppError(f"output {name!r}: file not found: {path}")
            if not any(root == path or root in path.parents for root in roots):
                raise AppError(
                    f"output {name!r}: files must be written inside ctx.output_dir or a folder chosen for the run"
                )
            result[name] = str(path)
        elif kind == "table":
            result[name] = json.loads(json.dumps(value, default=str))
        else:
            result[name] = str(value)
    return result
