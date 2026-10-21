import sys
import threading
import time
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional

from . import _com


class Cancelled(BaseException):
    """Raised inside the app when the user presses Cancel (or the run times out).

    It derives from BaseException, like KeyboardInterrupt, so that `except Exception:` blocks in
    the app do not swallow it; `finally` blocks and `ctx.on_cleanup()` functions still run.
    """


class Secrets:
    def __init__(self, values: Dict[str, str], allowed: Iterable[str]):
        self._values = dict(values)
        self._allowed = set(allowed)

    def get(self, name: str) -> str:
        if name not in self._allowed:
            raise KeyError(
                f"secret {name!r} is not declared; add it to "
                "[tool.devstribution.permissions] secrets in pyproject.toml"
            )
        if name not in self._values:
            raise KeyError(f"secret {name!r} was not provided")
        return self._values[name]

    def __repr__(self) -> str:  # never show values in logs or tracebacks
        return f"<Secrets {sorted(self._allowed)}>"


class Context:
    """What the app can ask of the platform during a run."""

    def __init__(
        self,
        emit: Callable[[Dict[str, Any]], None],
        output_dir: Path,
        secrets: Optional[Dict[str, str]] = None,
        allowed_secrets: Iterable[str] = (),
        web_port: Optional[int] = None,
        web_token: Optional[str] = None,
    ):
        self._emit = emit
        self.output_dir = Path(output_dir)
        self.secrets = Secrets(secrets or {}, allowed_secrets)
        self.web_port = web_port
        self.web_token = web_token
        self._cancel = threading.Event()
        self._cleanup: List[Callable[[], None]] = []
        self._own_start = _com.own_creation_time()

    # -- feedback ----------------------------------------------------------------------------

    def progress(self, value: Optional[float] = None, message: str = "") -> None:
        """Reports progress between 0 and 1 (None = indeterminate) with a short message."""
        self.check_cancelled()
        if value is not None:
            value = min(1.0, max(0.0, float(value)))
        self._emit({"type": "progress", "value": value, "message": str(message)})

    def log(self, message: str, level: str = "info") -> None:
        """Writes a line to the run log shown to the user (levels: info, warning, error)."""
        self.check_cancelled()
        if level not in ("info", "warning", "error"):
            level = "info"
        self._emit({"type": "log", "level": level, "message": str(message)})

    # -- cancellation ------------------------------------------------------------------------

    @property
    def cancelled(self) -> bool:
        return self._cancel.is_set()

    def check_cancelled(self) -> None:
        """Raises Cancelled if the user asked to stop. Called by progress() and log()."""
        if self._cancel.is_set():
            raise Cancelled()

    def _request_cancel(self) -> None:
        self._cancel.set()

    # -- cleanup -----------------------------------------------------------------------------

    def on_cleanup(self, fn: Callable[[], None]) -> None:
        """Registers a function to run at the end of the run, also on error or cancel."""
        self._cleanup.append(fn)

    def track_com(self, obj: Any) -> Any:
        """Registers a COM server (e.g. Excel) created with `DispatchEx` for cleanup.

        At the end of the run the runner calls `obj.Quit()`; if the run is killed, the launcher
        terminates the server process, but only if it was started after this run began.
        """
        info = _com.server_process(obj)
        if info is None:
            if sys.platform == "win32":
                self._emit_warning(
                    "track_com: could not find the process of this COM object; it will be "
                    "closed with Quit() but not killed if the run is stopped"
                )
        else:
            pid, created = info
            if self._own_start is not None and created is not None and created < self._own_start:
                # The object belongs to an application that was already open: most likely the
                # app used Dispatch() and attached to the user's own Excel. Never close it.
                self._emit_warning(
                    "track_com: this application was already running before the app started "
                    "(use DispatchEx, not Dispatch); it will not be closed"
                )
                return obj
            self._emit({"type": "com_process", "pid": pid, "created": created})
        self._cleanup.append(lambda: _com.quit(obj))
        return obj

    def _emit_warning(self, message: str) -> None:
        self._emit({"type": "log", "level": "warning", "message": message})

    def _run_cleanup(self) -> None:
        while self._cleanup:
            fn = self._cleanup.pop()
            try:
                fn()
            except BaseException as exc:  # cleanup must never stop the next cleanup
                self._emit_warning(f"cleanup function failed: {exc!r}")


def local_context(
    output_dir: Optional[Path] = None,
    secrets: Optional[Dict[str, str]] = None,
) -> Context:
    """A context for running an app's code directly, e.g. in tests: messages go to stderr."""
    if output_dir is None:
        output_dir = Path("output") / time.strftime("%Y%m%d-%H%M%S")
    Path(output_dir).mkdir(parents=True, exist_ok=True)

    def emit(msg: Dict[str, Any]) -> None:
        if msg["type"] == "progress":
            pct = "" if msg["value"] is None else f"{msg['value'] * 100:.0f}% "
            print(f"[progress] {pct}{msg['message']}", file=sys.stderr)
        elif msg["type"] == "log":
            print(f"[{msg['level']}] {msg['message']}", file=sys.stderr)

    return Context(emit, Path(output_dir), secrets, allowed_secrets=(secrets or {}).keys())
