"""Finding and closing COM servers (Excel, Word, ...) with ctypes only (no pywin32 here).

COM servers are started by Windows' DCOM service, outside the app's Job Object, so the launcher
needs their process id to terminate them if a run is killed. Process creation times (FILETIME,
100 ns units) let the launcher check that a PID still names the same process before killing it.
"""

import sys
from typing import Any, Optional, Tuple

_PROCESS_QUERY_LIMITED_INFORMATION = 0x1000


def _filetime(ft) -> int:
    return (ft.dwHighDateTime << 32) | ft.dwLowDateTime


def _process_times(handle) -> Optional[int]:
    import ctypes
    from ctypes import wintypes

    created, exited, kernel, user = (wintypes.FILETIME() for _ in range(4))
    ok = ctypes.windll.kernel32.GetProcessTimes(
        handle,
        ctypes.byref(created),
        ctypes.byref(exited),
        ctypes.byref(kernel),
        ctypes.byref(user),
    )
    return _filetime(created) if ok else None


def process_creation_time(pid: int) -> Optional[int]:
    if sys.platform != "win32":
        return None
    import ctypes

    kernel32 = ctypes.windll.kernel32
    kernel32.OpenProcess.restype = ctypes.c_void_p
    handle = kernel32.OpenProcess(_PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not handle:
        return None
    try:
        return _process_times(ctypes.c_void_p(handle))
    finally:
        kernel32.CloseHandle(ctypes.c_void_p(handle))


def own_creation_time() -> Optional[int]:
    if sys.platform != "win32":
        return None
    import ctypes

    kernel32 = ctypes.windll.kernel32
    kernel32.GetCurrentProcess.restype = ctypes.c_void_p
    return _process_times(ctypes.c_void_p(kernel32.GetCurrentProcess()))


def _hwnd(obj: Any) -> Optional[int]:
    for path in (("Hwnd",), ("ActiveWindow", "Hwnd")):
        try:
            value = obj
            for attr in path:
                value = getattr(value, attr)
            if value:
                return int(value)
        except Exception:
            continue
    return None


def server_process(obj: Any) -> Optional[Tuple[int, Optional[int]]]:
    """(pid, creation time) of the process serving a COM object, from its window handle."""
    if sys.platform != "win32":
        return None
    import ctypes
    from ctypes import wintypes

    hwnd = _hwnd(obj)
    if hwnd is None:
        return None
    pid = wintypes.DWORD()
    ctypes.windll.user32.GetWindowThreadProcessId(wintypes.HWND(hwnd), ctypes.byref(pid))
    if not pid.value:
        return None
    return pid.value, process_creation_time(pid.value)


def quit(obj: Any) -> None:
    try:
        obj.DisplayAlerts = False  # no "save changes?" dialog blocking the cleanup
    except Exception:
        pass
    obj.Quit()
