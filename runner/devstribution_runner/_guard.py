"""The runtime guard: what an app does while it runs, checked against what it declared.

Python reports sensitive calls to audit hooks (PEP 578): connections and name lookups,
programs started, registry writes, files opened, native functions looked up, compiled code
loaded. The runner installs one hook before any app code runs (a hook cannot be removed) and
acts on what it sees:

- block: a call the app did not declare (network without `permissions.network`, SAP's RFC
  library without `permissions.sap`) fails with PermissionError and is reported;
- stop: what malware does (start with Windows, read browser passwords, switch off the
  antivirus, inject into other programs, watch the keyboard, run hidden code or the Windows
  tools malware uses) ends the run at once; the launcher ends the whole process tree and
  quarantines the app;
- observe: programs started and hosts contacted are reported once each, for the run log.

The hook runs inside the app's own process, so code that patches the interpreter's memory
(through ctypes) can get past it. The launcher therefore also checks every process that appears
in the run's Job Object, which the app cannot hide; see `_process_rules.json`.
"""

import ipaddress
import json
import os
import sys
import threading
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Optional, Set

#: Exit code of a run the guard stopped.
STOP_EXIT_CODE = 4

#: Reports kept per kind of observation: enough to see what an app does, not a flood.
MAX_OBSERVED = 30

Emit = Callable[[Dict[str, Any]], None]


def load_process_rules() -> List[Dict[str, Any]]:
    path = Path(__file__).with_name("_process_rules.json")
    rules = json.loads(path.read_text(encoding="utf-8"))["rules"]
    for rule in rules:
        rule["programs"] = frozenset(rule.get("programs") or ())
        rule["flags"] = frozenset(rule.get("flags") or ())
        rule["contains"] = tuple(rule.get("contains") or ())
    return rules


def normalize_command(command: str) -> str:
    """Lower case, without the quotes and carets cmd.exe ignores, with typographic dashes as -."""
    text = command.lower()
    for char in '"^':
        text = text.replace(char, "")
    for dash in "–—―":
        text = text.replace(dash, "-")
    return text


def match_process(command: str, rules: Iterable[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    """The first rule a command line matches (same algorithm as the launcher's)."""
    words = normalize_command(command).split()
    for i, word in enumerate(words):
        name = word.replace("/", "\\").rsplit("\\", 1)[-1]
        if name.endswith(".exe"):
            name = name[:-4]
        for rule in rules:
            if name not in rule["programs"]:
                continue
            rest = words[i + 1 :]
            if not rule["flags"] and not rule["contains"]:
                return rule
            if any(("-" + w[1:] if w[:1] in "-/" else w) in rule["flags"] for w in rest):
                return rule
            text = " ".join(rest)
            if any(c in text for c in rule["contains"]):
                return rule
    return None


# Registry keys where a write means starting with Windows or weakening the antivirus, relative
# to the hive (HKCU / HKLM: `software\...`).
_AUTOSTART_KEYS = (
    "software\\microsoft\\windows\\currentversion\\run",  # also RunOnce, RunServices, RunOnceEx
    "software\\microsoft\\windows\\currentversion\\policies\\explorer\\run",
    "software\\wow6432node\\microsoft\\windows\\currentversion\\run",
    "software\\microsoft\\windows\\currentversion\\explorer\\shell folders",
    "software\\microsoft\\windows\\currentversion\\explorer\\user shell folders",
    "software\\microsoft\\windows nt\\currentversion\\winlogon",
    "software\\microsoft\\windows nt\\currentversion\\image file execution options",
    "system\\currentcontrolset\\services",
)
_ANTIVIRUS_KEYS = (
    "software\\policies\\microsoft\\windows defender",
    "software\\microsoft\\windows defender",
)
_HIVES = {
    0x80000000: "hkcr",
    0x80000001: "hkcu",
    0x80000002: "hklm",
    0x80000003: "hku",
    0x80000005: "hkcc",
}
# KEY_SET_VALUE | KEY_CREATE_SUB_KEY, and DELETE.
_KEY_WRITE_BITS = 0x0002 | 0x0004 | 0x10000

_BROWSER_FOLDERS = (
    "\\google\\chrome\\user data\\",
    "\\microsoft\\edge\\user data\\",
    "\\bravesoftware\\brave-browser\\user data\\",
    "\\chromium\\user data\\",
    "\\vivaldi\\user data\\",
    "\\yandex\\yandexbrowser\\user data\\",
    "\\opera software\\",
    "\\mozilla\\firefox\\profiles\\",
)
_BROWSER_FILES = frozenset(
    {
        "login data",
        "login data for account",
        "cookies",
        "web data",
        "local state",
        "logins.json",
        "key3.db",
        "key4.db",
        "cookies.sqlite",
        "signons.sqlite",
        "formhistory.sqlite",
    }
)
_STARTUP_FOLDER = "\\start menu\\programs\\startup\\"

_INJECTION_FUNCTIONS = frozenset(
    {
        "writeprocessmemory",
        "virtualallocex",
        "virtualallocexnuma",
        "createremotethread",
        "createremotethreadex",
        "ntwritevirtualmemory",
        "ntcreatethreadex",
        "rtlcreateuserthread",
        "queueuserapc",
        "ntqueueapcthread",
        "setthreadcontext",
        "ntunmapviewofsection",
    }
)
_KEYBOARD_FUNCTIONS = frozenset({"setwindowshookexa", "setwindowshookexw", "getasynckeystate"})
_CREDENTIAL_FUNCTIONS = frozenset({"credenumeratea", "credenumeratew"})

_LOOPBACK_NAMES = frozenset({"", "localhost", "localhost.localdomain"})


class Guard:
    def __init__(self, emit: Emit, config: Dict[str, Any]) -> None:
        self._emit = emit
        self._allow: Set[str] = set(config.get("allow") or ())
        self._sap = bool(config.get("sap"))
        self._process_rules = load_process_rules()
        self._local = threading.local()
        self._lock = threading.Lock()
        self._reported: Set[Any] = set()
        self._observed: Dict[str, int] = {}
        self._key_paths: Dict[int, str] = {}
        self._own_names = {n.lower() for n in (os.environ.get("COMPUTERNAME"),) if n}

        hosts = [_host_entry(h) for h in config.get("network") or ()]
        self._network_any = "*" in hosts
        self._network_names = {h for h in hosts if h and h != "*"}
        self._network_ips: Set[str] = set()
        for host in list(self._network_names):
            ip = _ip(host)
            if ip is not None:
                self._network_ips.add(str(ip))
        if self._network_names and not self._network_any:
            self._network_names |= _proxy_hosts()

        self._handlers: Dict[str, Callable[[tuple], None]] = {
            "socket.getaddrinfo": self._on_lookup,
            "socket.gethostbyname": self._on_lookup,
            "socket.gethostbyaddr": self._on_lookup,
            "socket.connect": self._on_connect,
            "socket.sendto": self._on_connect,
            "import": self._on_import,
            "winreg.OpenKey": self._on_open_key,
            "winreg.CreateKey": self._on_create_key,
            "winreg.OpenKey/result": self._on_key_result,
            "winreg.SetValue": self._on_set_value,
            "winreg.DeleteKey": self._on_set_value,
            "winreg.DeleteValue": self._on_set_value,
            "open": self._on_open,
            "sqlite3.connect": self._on_read_path,
            "shutil.copyfile": self._on_copy,
            "os.rename": self._on_copy,
            "os.link": self._on_copy,
            "os.symlink": self._on_copy,
            "subprocess.Popen": self._on_popen,
            "os.system": self._on_command,
            "os.startfile": self._on_startfile,
            "os.startfile/2": self._on_startfile,
            "os.exec": self._on_exec,
            "os.spawn": self._on_spawn,
            "os.posix_spawn": self._on_exec,
            "ctypes.dlsym": self._on_dlsym,
            "marshal.loads": self._on_marshal,
            "marshal.load": self._on_marshal,
        }

    def install(self) -> None:
        sys.addaudithook(self._hook)

    # The hook sees every audited event of the process: return at once for the others, and
    # never act on events the guard itself causes (its own name lookups).
    def _hook(self, event: str, args: tuple) -> None:
        handler = self._handlers.get(event)
        if handler is None or getattr(self._local, "busy", False):
            return
        self._local.busy = True
        try:
            handler(args)
        finally:
            self._local.busy = False

    # -- what happens on a match -------------------------------------------------------------

    def _stop(self, rule: str, message: str, detail: str) -> None:
        if rule in self._allow:
            self._observe("allowed", f"{message}: {detail}")
            return
        self._emit({"type": "guard", "action": "stop", "rule": rule, "message": message, "detail": detail[:500]})
        # Nothing the app does can catch this, and the call that tripped never happens.
        os._exit(STOP_EXIT_CODE)

    def _block(self, rule: str, message: str, detail: str) -> None:
        if rule in self._allow:
            return
        with self._lock:
            first = (rule, detail) not in self._reported
            self._reported.add((rule, detail))
        if first:
            self._emit({"type": "guard", "action": "block", "rule": rule, "message": message, "detail": detail[:500]})
        raise PermissionError(f"devstribution: {message} ({detail})")

    def _observe(self, what: str, detail: str) -> None:
        with self._lock:
            key = (what, detail)
            if key in self._reported or self._observed.get(what, 0) >= MAX_OBSERVED:
                return
            self._reported.add(key)
            self._observed[what] = self._observed.get(what, 0) + 1
        self._emit({"type": "guard", "action": "observe", "rule": what, "message": detail[:500]})

    # -- network ---------------------------------------------------------------------------

    def _local_host(self, host: str) -> bool:
        if host in _LOOPBACK_NAMES or host in self._own_names:
            return True
        ip = _ip(host)
        return ip is not None and (ip.is_loopback or ip.is_unspecified)

    def _name_allowed(self, host: str) -> bool:
        if self._network_any or host in self._network_names:
            return True
        return any(e.startswith("*.") and host.endswith(e[1:]) for e in self._network_names)

    def _on_lookup(self, args: tuple) -> None:
        host = _host_text(args[0])
        if host is None or self._local_host(host) or self._network_any:
            return
        if _ip(host) is not None:
            if host in self._network_ips:
                return
        elif self._name_allowed(host):
            self._observe("network", host)
            self._remember_addresses(host)
            return
        self._block_network(host)

    def _on_connect(self, args: tuple) -> None:
        address = args[1] if len(args) > 1 else None
        if not isinstance(address, tuple) or not address:
            return  # a named pipe or a Unix socket: local
        host = _host_text(address[0])
        if host is None or self._local_host(host) or self._network_any:
            return
        if _ip(host) is None:
            if self._name_allowed(host):
                self._observe("network", host)
                return
        elif host in self._network_ips:
            return
        self._block_network(host)

    def _remember_addresses(self, host: str) -> None:
        import socket

        try:
            for info in socket.getaddrinfo(host, None):
                self._network_ips.add(str(_ip(info[4][0]) or info[4][0]))
        except OSError:
            pass

    def _block_network(self, host: str) -> None:
        if self._network_names:
            self._block("network", "connects to a host it does not declare", host)
        self._block("network", "uses the network, which it does not declare", host)

    def _on_import(self, args: tuple) -> None:
        name = args[0] if args else None
        if not self._sap and isinstance(name, str) and (name == "pyrfc" or name.startswith("pyrfc.")):
            self._block("sap", "uses SAP, which it does not declare", name)

    # -- registry --------------------------------------------------------------------------

    def _key_path(self, key: Any, sub_key: Any) -> str:
        hive = _HIVES.get(key) if isinstance(key, int) else None
        base = ""
        if hive is None and isinstance(key, int):
            base = self._key_paths.get(key, "")
        sub = sub_key if isinstance(sub_key, str) else ""
        path = "\\".join(p for p in (base, sub.replace("/", "\\").strip("\\")) if p).lower()
        if hive == "hku" and "\\" in path:
            path = path.split("\\", 1)[1]  # drop the user's SID
        elif hive == "hkcr":
            path = "software\\classes\\" + path
        return path

    def _check_key(self, path: str, action: str) -> None:
        if path.startswith(_ANTIVIRUS_KEYS):
            self._stop("antivirus-off", "tried to switch off or weaken the antivirus", f"{action} {path}")
        if path.startswith(_AUTOSTART_KEYS):
            self._stop("start-with-windows", "tried to make a program start with Windows", f"{action} {path}")

    def _on_open_key(self, args: tuple) -> None:
        path = self._key_path(args[0], args[1])
        self._local.pending_key = path
        access = args[2] if len(args) > 2 and isinstance(args[2], int) else 0
        if access & _KEY_WRITE_BITS:
            self._check_key(path, "opened for writing")

    def _on_create_key(self, args: tuple) -> None:
        path = self._key_path(args[0], args[1])
        self._local.pending_key = path
        self._check_key(path, "created")

    def _on_key_result(self, args: tuple) -> None:
        path = getattr(self._local, "pending_key", None)
        if path is not None and args and isinstance(args[0], int):
            self._key_paths[args[0]] = path
        self._local.pending_key = None

    def _on_set_value(self, args: tuple) -> None:
        self._check_key(self._key_path(args[0], args[1] if len(args) > 1 else None), "changed")

    # -- files -----------------------------------------------------------------------------

    def _check_read(self, path: Optional[str]) -> None:
        if path and any(f in path for f in _BROWSER_FOLDERS):
            if path.rsplit("\\", 1)[-1] in _BROWSER_FILES:
                self._stop("browser-passwords", "tried to read the browser's saved passwords or cookies", path)

    def _check_write(self, path: Optional[str]) -> None:
        if path and _STARTUP_FOLDER in path:
            self._stop("start-with-windows", "tried to make a program start with Windows", path)

    def _on_open(self, args: tuple) -> None:
        path = _path_text(args[0])
        if path is None:
            return
        self._check_read(path)
        mode = args[1] if len(args) > 1 else None
        flags = args[2] if len(args) > 2 else -1
        writing = (isinstance(mode, str) and any(c in mode for c in "wax+")) or (
            isinstance(flags, int) and flags > 0 and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_APPEND)
        )
        if writing:
            self._check_write(path)

    def _on_read_path(self, args: tuple) -> None:
        self._check_read(_path_text(args[0]) if args else None)

    def _on_copy(self, args: tuple) -> None:
        self._check_read(_path_text(args[0]) if args else None)
        self._check_write(_path_text(args[1]) if len(args) > 1 else None)

    # -- programs --------------------------------------------------------------------------

    def _check_command(self, command: str) -> None:
        rule = match_process(command, self._process_rules)
        if rule is not None:
            self._stop(rule["id"], rule["message"], command)
        self._observe("program", command)

    def _on_popen(self, args: tuple) -> None:
        executable, argv = args[0], args[1]
        if isinstance(argv, (str, bytes, os.PathLike)):
            parts = [argv]
        else:
            parts = list(argv or ())
        if executable is not None:
            parts.insert(0, executable)
        self._check_command(" ".join(_text(p) for p in parts))

    def _on_command(self, args: tuple) -> None:
        self._check_command(_text(args[0]))

    def _on_startfile(self, args: tuple) -> None:
        extra = args[2] if len(args) > 2 and isinstance(args[2], str) else ""
        self._check_command(f"{_text(args[0])} {extra}".strip())

    def _on_exec(self, args: tuple) -> None:
        self._check_command(" ".join(_text(p) for p in (args[1] or [args[0]])))

    def _on_spawn(self, args: tuple) -> None:
        self._check_command(" ".join(_text(p) for p in (args[2] or [args[1]])))

    # -- native and hidden code ------------------------------------------------------------

    def _on_dlsym(self, args: tuple) -> None:
        name = args[1] if len(args) > 1 else None
        if not isinstance(name, str):
            return
        lower = name.lower()
        if lower in _INJECTION_FUNCTIONS:
            self._stop("process-injection", "tried to put code into another program", name)
        if lower in _KEYBOARD_FUNCTIONS:
            self._stop("keyboard-watch", "tried to watch what is typed on the keyboard", name)
        if lower in _CREDENTIAL_FUNCTIONS:
            self._stop("credential-theft", "tried to list every password saved in Windows", name)

    def _on_marshal(self, args: tuple) -> None:
        # Imports load cached bytecode with marshal; anything else loading compiled code is how
        # malware hides what it runs.
        try:
            # 0 is this method, 1 the hook, 2 the code that called marshal.
            caller = sys._getframe(2).f_code.co_filename
        except ValueError:
            caller = ""
        if caller.startswith(("<frozen importlib", "<frozen zipimport")):
            return
        self._stop("hidden-code", "loaded hidden compiled code", caller or "marshal")


def _text(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")
    if isinstance(value, os.PathLike):
        return _text(os.fspath(value))
    return str(value)


def _path_text(value: Any) -> Optional[str]:
    if isinstance(value, int) or value is None:
        return None
    try:
        return _text(value).lower().replace("/", "\\")
    except Exception:
        return None


def _host_text(value: Any) -> Optional[str]:
    if value is None:
        return None
    try:
        return _text(value).strip().strip("[]").rstrip(".").lower().split("%", 1)[0]
    except Exception:
        return None


def _host_entry(entry: Any) -> str:
    """A declared host as the guard compares it: `https://API.x.com:443/v1` → `api.x.com`."""
    text = str(entry).strip().lower()
    if "://" in text:
        text = text.split("://", 1)[1]
    text = text.split("/", 1)[0]
    if text.startswith("["):
        return text[1:].split("]", 1)[0]
    if text.count(":") == 1:
        text = text.split(":", 1)[0]
    return text.rstrip(".")


def _ip(host: str) -> Optional[Any]:
    try:
        return ipaddress.ip_address(host)
    except ValueError:
        return None


def _proxy_hosts() -> Set[str]:
    """The proxies Windows or the environment configure: an app declaring a host reaches it
    through them."""
    try:
        import urllib.request

        hosts = set()
        for url in urllib.request.getproxies().values():
            host = _host_entry(url)
            if host:
                hosts.add(host)
        return hosts
    except Exception:
        return set()


def install(emit: Emit, config: Dict[str, Any]) -> None:
    if config.get("enabled", True):
        Guard(emit, config).install()
