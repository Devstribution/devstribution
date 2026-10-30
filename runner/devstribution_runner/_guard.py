"""The runtime guard: what an app does while it runs, checked against what it declared.

Python reports sensitive calls to audit hooks (PEP 578): connections and name lookups,
programs started, registry writes, files opened, native functions looked up, compiled code
loaded. The runner installs one hook before any app code runs (a hook cannot be removed) and
acts on what it sees:

- block: a call the app did not declare (network without `permissions.network`, SAP's RFC
  library without `permissions.sap`) fails with PermissionError and is reported;
- stop: what malware does (start with Windows, read browser passwords or other saved keys
  and passwords, switch off the antivirus, inject into other programs, watch the keyboard, run
  hidden code, a program it wrote itself or the Windows tools malware uses, change Windows,
  devstribution or another app, read another run's results, change many of the user's files
  it was not given) ends the run at once; the launcher ends the whole process tree and
  quarantines the app;
- observe: programs started, hosts contacted and files changed outside the run's own folders
  are reported once each, and the run ends with one line on the files it wrote and deleted.

Files the user chose through the app's inputs are the app's to read and change.

Another Python the app starts (`subprocess.run([sys.executable, ...])`, `multiprocessing`) is
started through `child.py` instead, which installs this guard with the run's settings before any
of its code runs; its reports reach the runner through a file the app cannot change
(`_child.py`). A Python started any other way (`os.system`, a shell, a second runner) is stopped.

The hook runs inside the app's own process, so code that patches the interpreter's memory
(through ctypes) can get past it. The launcher therefore also checks every process that appears
in the run's Job Object, which the app cannot hide; see `_process_rules.json`.
"""

import contextlib
import ipaddress
import json
import os
import re
import subprocess
import sys
import tempfile
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
# A PowerShell profile runs every time PowerShell starts: another way to start with Windows.
_PROFILE_FOLDERS = ("\\windowspowershell\\", "\\powershell\\")

# Saved keys and passwords. An app reads them only when the user chose them as an input.
_SECRET_FOLDERS = (
    "\\appdata\\roaming\\microsoft\\credentials\\",
    "\\appdata\\local\\microsoft\\credentials\\",
    "\\appdata\\roaming\\microsoft\\protect\\",
    "\\appdata\\local\\microsoft\\vault\\",
    "\\appdata\\roaming\\microsoft\\crypto\\",
    "\\appdata\\local\\microsoft\\outlook\\",
    "\\appdata\\roaming\\filezilla\\",
    "\\appdata\\roaming\\telegram desktop\\tdata\\",
    "\\appdata\\roaming\\bitcoin\\",
    "\\appdata\\roaming\\electrum\\",
    "\\appdata\\roaming\\exodus\\",
    "\\appdata\\roaming\\ethereum\\keystore\\",
    "\\appdata\\roaming\\bitwarden\\",
    "\\appdata\\local\\1password\\",
)
_SECRET_NAMES = frozenset({".git-credentials", ".netrc", "_netrc", ".pgpass", "wallet.dat"})
_SECRET_EXTENSIONS = (".kdbx", ".pst", ".ost", ".ppk")
_SSH_FOLDER = "\\.ssh\\"
_SSH_PUBLIC = frozenset({"known_hosts", "known_hosts.old", "config"})
# Read on their own by cloud libraries (boto3, the Azure SDK): reported, not stopped.
_CLOUD_CREDENTIALS = ("\\.aws\\", "\\.azure\\", "\\.kube\\", "\\.docker\\config.json", "\\gcloud\\")

# Files an app writes and then runs or loads: how a download-and-run hides from the command line.
_RUNNABLE_EXTENSIONS = (
    ".exe", ".dll", ".pyd", ".scr", ".com", ".bat", ".cmd", ".ps1", ".vbs", ".js", ".jse",
    ".wsf", ".hta", ".msi", ".py", ".pyw",
)

# `os.devnull` and other devices (`\\.\nul`, `/dev/null`): not files.
_DEVICES = ("\\\\.\\", "\\dev\\")

#: Existing files changed (overwritten, deleted or moved) outside the output folder, the user's
#: choices and the temporary folder before the run is stopped: what ransomware does.
MASS_CHANGES = 50

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
        # Other Pythons the app starts run `child.py` with the settings file `config`.
        child = config.get("child") or {}
        self._child_script: Optional[str] = child.get("script")
        self._child_config: Optional[str] = child.get("config")
        # The runner's own files the app may read but never change: the child settings.
        self._sealed = _folders(config.get("sealed"))
        self._runner_start = _normal(str(Path(__file__).resolve().parent.parent / "bootstrap.py"))

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

        # Where files are. `own`: the run's folders, `output` the one for its results. `chosen`: files and folders the user gave
        # through inputs. `protected`: devstribution, its data and the app's Python. `private`:
        # other runs' results. Resolved now, before the app can change the environment.
        runner_root = Path(__file__).resolve().parent.parent
        self._own = _folders(config.get("own"))
        self._output = _folders(config.get("output"))
        self._chosen = _folders(config.get("chosen"))
        self._protected = _folders(
            list(config.get("protected") or ()) + [str(runner_root), sys.prefix, sys.base_prefix]
        )
        self._private = _folders(config.get("private"))
        self._system = _folders(_system_folders())
        temp = config.get("temp")
        self._temp = _folders([tempfile.gettempdir()] if temp is None else temp)
        self._written: Dict[str, Set[str]] = {"output": set(), "chosen": set(), "elsewhere": set()}
        self._removed: Dict[str, Set[str]] = {"output": set(), "chosen": set(), "elsewhere": set()}
        self._changed: Set[str] = set()
        self._written_programs: Set[str] = set()

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
            "sqlite3.connect": self._on_sqlite,
            "shutil.copyfile": self._on_copy,
            "os.rename": self._on_rename,
            "os.link": self._on_link,
            "os.symlink": self._on_link,
            "os.remove": self._on_remove,
            "os.rmdir": self._on_remove,
            "shutil.rmtree": self._on_remove,
            "os.truncate": self._on_truncate,
            "ctypes.dlopen": self._on_dlopen,
            "subprocess.Popen": self._on_popen,
            "_winapi.CreateProcess": self._on_create_process,
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
        if self._child_script and self._child_config:
            self._start_pythons_guarded()

    @contextlib.contextmanager
    def unchecked(self):
        """For the runner's own work while the app runs (reports written to a file)."""
        busy = getattr(self._local, "busy", False)
        self._local.busy = True
        try:
            yield
        finally:
            self._local.busy = busy

    # -- other Pythons ---------------------------------------------------------------------

    def guarded_command(self, argv: List[str]) -> List[str]:
        """`python [options] <what runs>` as `python -I -S -B [options] child.py <settings>
        <what runs>`: the same Python, with this guard installed before what runs."""
        options, rest = split_python_options(list(argv[1:]))
        return [argv[0], "-I", "-S", "-B", *options, self._child_script, self._child_config, *rest]

    def _is_guarded(self, command: str) -> bool:
        text = normalize_command(command).replace("/", "\\")
        return _normal(self._child_script) in text and _normal(self._child_config) in text

    def _start_pythons_guarded(self) -> None:
        """Every way the standard library starts a process goes through one function: wrapped,
        so that a Python interpreter starts guarded. On Windows that is `_winapi.CreateProcess`
        (`subprocess` and `multiprocessing`), elsewhere `subprocess`'s `_execute_child`."""
        guard = self
        if sys.platform == "win32":
            import _winapi

            create = _winapi.CreateProcess

            def create_process(application, command, *rest):
                argv = split_windows_command(command or "")
                program = application or (argv[0] if argv else "")
                if argv and is_python(program) and not guard._is_guarded(command):
                    command = subprocess.list2cmdline(guard.guarded_command(argv))
                return create(application, command, *rest)

            _winapi.CreateProcess = create_process
        else:
            execute = subprocess.Popen._execute_child

            def execute_child(popen, args, executable, *rest, **kwargs):
                # (preexec_fn, close_fds, pass_fds, cwd, env, startupinfo, creationflags, shell, ...)
                shell = rest[7] if len(rest) > 7 else kwargs.get("shell", False)
                argv = [args] if isinstance(args, (str, bytes, os.PathLike)) else list(args)
                program = executable or (argv[0] if argv else "")
                if argv and not shell and is_python(program):
                    argv = guard.guarded_command([os.fsdecode(a) for a in argv])
                    args = argv
                return execute(popen, args, executable, *rest, **kwargs)

            subprocess.Popen._execute_child = execute_child

    def _check_runner(self, command: str) -> None:
        """A second runner, which the app would feed settings of its own, is never started."""
        if self._runner_start in normalize_command(command).replace("/", "\\"):
            self._stop("unguarded-python", "tried to start devstribution's runner itself", command)

    def _check_python(self, command: str, program: Any) -> None:
        """A Python that will not run under this guard is stopped: started through a shell,
        `os.system` or `os.spawn*` (which cannot be rewritten), or a second runner."""
        self._check_runner(command)
        if self._child_script and is_python(program) and not self._is_guarded(command):
            self._stop("unguarded-python", "started Python in a way devstribution cannot check", command)

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

    def _is_chosen(self, path: str) -> bool:
        return _inside_any(path, self._chosen)

    def _check_read(self, value: Any) -> None:
        """A file read, as the app named it and as the system names it (8.3 short names, links
        and junctions lead elsewhere)."""
        path, real = _abs_path(value), _real(value)
        for spelled in {path, real} - {None}:
            self._check_read_path(spelled)

    def _check_read_path(self, path: str) -> None:
        if not path or self._is_chosen(path):
            return
        name = path.rsplit("\\", 1)[-1]
        if name in _BROWSER_FILES and any(f in path for f in _BROWSER_FOLDERS):
            self._stop("browser-passwords", "tried to read the browser's saved passwords or cookies", path)
        if (
            name in _SECRET_NAMES
            or name.endswith(_SECRET_EXTENSIONS)
            or any(f in path for f in _SECRET_FOLDERS)
            or (_SSH_FOLDER in path and name not in _SSH_PUBLIC and not name.endswith(".pub"))
        ):
            self._stop("saved-secrets", "tried to read saved keys or passwords", path)
        if any(f in path for f in _CLOUD_CREDENTIALS):
            self._observe("cloud-credentials", path)
        if _inside_any(path, self._private) and not _inside_any(path, self._own):
            self._stop("other-app-data", "tried to read another run's results", path)

    def _check_change(self, value: Any, change: str) -> None:
        """A file written (`write`), deleted (`delete`) or moved away (`move`), as the app named it."""
        path = _abs_path(value)
        if not path:
            return
        name = path.rsplit("\\", 1)[-1]
        if _STARTUP_FOLDER in path or (name == "profile.ps1" or name.endswith("_profile.ps1")) and any(
            f in path for f in _PROFILE_FOLDERS
        ):
            self._stop("start-with-windows", "tried to make a program start with Windows", path)
        native = _resolved(value)
        real = _normal(native) if native else path
        if real.startswith(_DEVICES):
            return
        if _inside_any(real, self._sealed) or _inside_any(path, self._sealed):
            self._stop("devstribution-files", "tried to change devstribution or an installed app", real)
            return
        if _inside_any(real, self._output):
            where = "output"
        elif _inside_any(real, self._own):
            where = ""  # the bytecode cache, the run's log: not the app's work
        elif _inside_any(real, self._protected):
            self._stop("devstribution-files", "tried to change devstribution or an installed app", real)
            return
        elif _inside_any(real, self._system):
            self._stop("system-files", "tried to change Windows or installed programs", real)
            return
        elif self._is_chosen(real):
            where = "chosen"
        elif _inside_any(real, self._temp):
            where = ""
        else:
            where = "elsewhere"
            existed = change != "write" or bool(native and os.path.lexists(native))
            with self._lock:
                if existed:
                    self._changed.add(real)
                changed = len(self._changed)
            if changed > MASS_CHANGES:
                self._stop(
                    "mass-changes",
                    "changed many of your files that it was not given",
                    f"{changed} files, the last {real}",
                )
            self._observe({"write": "file-written", "delete": "file-deleted", "move": "file-moved"}[change], real)
        with self._lock:
            if where:
                (self._written if change == "write" else self._removed)[where].add(real)
            if change == "write" and name.endswith(_RUNNABLE_EXTENSIONS):
                self._written_programs.update((path, real))

    def _on_open(self, args: tuple) -> None:
        path = _abs_path(args[0])
        if path is None:
            return
        self._check_read(args[0])
        mode = args[1] if len(args) > 1 else None
        flags = args[2] if len(args) > 2 else -1
        writing = (isinstance(mode, str) and any(c in mode for c in "wax+")) or (
            isinstance(flags, int) and flags > 0 and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_APPEND)
        )
        if writing:
            self._check_change(args[0], "write")

    def _on_sqlite(self, args: tuple) -> None:
        self._check_read(_sqlite_path(args[0]) if args else None)

    def _on_copy(self, args: tuple) -> None:
        self._check_read(args[0] if args else None)
        self._check_change(args[1] if len(args) > 1 else None, "write")

    def _on_rename(self, args: tuple) -> None:
        if _relative_to_fd(args, 2) or _relative_to_fd(args, 3):
            return
        # Moving a file somewhere the app can read gives it the content: the source is read too.
        self._check_read(args[0] if args else None)
        self._check_change(args[0] if args else None, "move")
        self._check_change(args[1] if len(args) > 1 else None, "write")

    def _on_link(self, args: tuple) -> None:
        # A hard or symbolic link to a file reads it as well as an open would.
        if not _relative_to_fd(args, 2):
            self._check_read(args[0] if args else None)
        self._check_change(args[1] if len(args) > 1 else None, "write")

    def _on_remove(self, args: tuple) -> None:
        # With a folder handle (`dir_fd`, not on Windows) the path cannot be placed: the folder
        # was checked when `shutil.rmtree` started.
        if args and not _relative_to_fd(args, 1):
            self._check_change(args[0], "delete")

    def _on_truncate(self, args: tuple) -> None:
        if args:
            self._check_change(args[0], "write")

    def _on_dlopen(self, args: tuple) -> None:
        path = _abs_path(args[0]) if args else None
        if path and (path in self._written_programs or _real(args[0]) in self._written_programs):
            self._stop("downloaded-program", "ran a program it had just written", path)

    def report_files(self) -> None:
        """One line for the run log: which files the app wrote, deleted or moved, and where."""
        with self._lock:
            written = {k: len(v) for k, v in self._written.items()}
            removed = {k: len(v) for k, v in self._removed.items()}
        parts = []
        for verb, counts in (("wrote", written), ("deleted or moved", removed)):
            places = [
                f"{n} {where}"
                for n, where in (
                    (counts["output"], "in its output folder"),
                    (counts["chosen"], "in what you chose"),
                    (counts["elsewhere"], "elsewhere"),
                )
                if n
            ]
            if places:
                total = sum(counts.values())
                if len(places) == 1:
                    parts.append(f"{verb} {total} file{'s' if total != 1 else ''} {places[0].split(' ', 1)[1]}")
                else:
                    parts.append(f"{verb} {total} files: " + ", ".join(places))
        if parts:
            self._emit({"type": "guard", "action": "observe", "rule": "files", "message": "; ".join(parts), "detail": ""})

    # -- programs --------------------------------------------------------------------------

    def _check_command(self, command: str) -> None:
        rule = match_process(command, self._process_rules)
        if rule is not None:
            self._stop(rule["id"], rule["message"], command)
        if self._written_programs:
            text = normalize_command(command).replace("/", "\\")
            if any(p in text for p in self._written_programs):
                self._stop("downloaded-program", "ran a program it had just written", command)
        self._observe("program", command)

    def _on_popen(self, args: tuple) -> None:
        executable, argv = args[0], args[1]
        if isinstance(argv, (str, bytes, os.PathLike)):
            parts = [argv]
        else:
            parts = list(argv or ())
        if executable is not None:
            parts.insert(0, executable)
        command = " ".join(_text(p) for p in parts)
        # A Python started here is rewritten to start guarded (`_start_pythons_guarded`).
        self._check_runner(command)
        self._check_command(command)

    def _on_create_process(self, args: tuple) -> None:
        # Windows, after `subprocess` and the wrapper above: the command line as it will run.
        application, command = args[0], _text(args[1] or "")
        argv = split_windows_command(command)
        self._check_python(command, application or (argv[0] if argv else ""))

    def _on_command(self, args: tuple) -> None:
        command = _text(args[0])
        words = split_windows_command(command) if os.name == "nt" else command.split()
        self._check_python(command, words[0] if words else "")
        self._check_command(command)

    def _on_startfile(self, args: tuple) -> None:
        extra = args[2] if len(args) > 2 and isinstance(args[2], str) else ""
        self._check_command(f"{_text(args[0])} {extra}".strip())

    def _on_exec(self, args: tuple) -> None:
        command = " ".join(_text(p) for p in (args[1] or [args[0]]))
        self._check_python(command, args[0])
        self._check_command(command)

    def _on_spawn(self, args: tuple) -> None:
        command = " ".join(_text(p) for p in (args[2] or [args[1]]))
        self._check_python(command, args[1])
        self._check_command(command)

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


_PYTHON_PROGRAM = re.compile(r"(python|pythonw|py|pyw)(\d+(\.\d+)*)?(\.exe)?")


def is_python(program: Any) -> bool:
    """Whether a program is a Python interpreter (or the `py` launcher), by its file name."""
    name = _text(program).strip().strip('"').replace("/", "\\").rsplit("\\", 1)[-1].lower()
    return bool(_PYTHON_PROGRAM.fullmatch(name))


def split_windows_command(command: str) -> List[str]:
    """A command line split as Python and the C runtime split it on Windows."""
    args: List[str] = []
    i, n = 0, len(command)
    while True:
        while i < n and command[i] in " \t":
            i += 1
        if i >= n:
            return args
        arg: List[str] = []
        quoted = False
        while i < n:
            c = command[i]
            if c == "\\":
                j = i
                while j < n and command[j] == "\\":
                    j += 1
                if j < n and command[j] == '"':
                    arg.append("\\" * ((j - i) // 2))
                    if (j - i) % 2:
                        arg.append('"')
                        j += 1
                else:
                    arg.append("\\" * (j - i))
                i = j
            elif c == '"':
                if quoted and i + 1 < n and command[i + 1] == '"':
                    arg.append('"')
                    i += 2
                else:
                    quoted = not quoted
                    i += 1
            elif c in " \t" and not quoted:
                break
            else:
                arg.append(c)
                i += 1
        args.append("".join(arg))


#: Python's options that take a value, as a separate word or attached (`-Werror`).
_VALUE_OPTIONS = "WX"


def split_python_options(args: List[str]) -> "tuple[List[str], List[str]]":
    """The interpreter's options, and what runs: `-c code ...`, `-m module ...`, a script and its
    arguments, `-` or nothing (standard input)."""
    options: List[str] = []
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "--":
            return options, args[i + 1 :]
        if arg == "-" or not arg.startswith("-"):
            return options, args[i:]
        if arg.startswith("--"):
            options.append(arg)
            if arg == "--check-hash-based-pycs" and i + 1 < len(args):
                options.append(args[i + 1])
                i += 1
            i += 1
            continue
        for j in range(1, len(arg)):
            letter = arg[j]
            if letter in "cm":
                if j > 1:
                    options.append(arg[:j])
                attached = arg[j + 1 :]
                rest = args[i + 1 :]
                return options, ["-" + letter] + ([attached] if attached else []) + rest
            if letter in _VALUE_OPTIONS:
                options.append(arg)
                if j == len(arg) - 1 and i + 1 < len(args):
                    options.append(args[i + 1])
                    i += 1
                break
        else:
            options.append(arg)
        i += 1
    return options, []


def _text(value: Any) -> str:
    if isinstance(value, bytes):
        return value.decode("utf-8", "replace")
    if isinstance(value, os.PathLike):
        return _text(os.fspath(value))
    return str(value)


def _normal(path: str) -> str:
    """How the guard compares paths: lower case, backslashes, no `\\\\?\\` or `\\\\.\\` prefix on a
    disk path, no trailing slash, and for the last name no NTFS stream (`file::$DATA`) and no
    trailing dots or spaces, which Windows ignores: all of them spell the same file."""
    text = path.replace("/", "\\").lower()
    if text.startswith("\\\\?\\unc\\"):
        text = "\\\\" + text[8:]
    elif text.startswith("\\\\?\\") or re.match(r"\\\\\.\\[a-z]:", text):
        text = text[4:]
    text = text.rstrip("\\") if len(text) > 3 else text
    if os.name == "nt" and len(text) > 3 and not text.startswith("\\\\.\\"):
        folder, sep, name = text.rpartition("\\")
        name = name.split(":", 1)[0].rstrip(". ") or name
        text = folder + sep + name
    return text


def _abs_path(value: Any) -> Optional[str]:
    if isinstance(value, int) or value is None:
        return None
    try:
        return _normal(os.path.abspath(_text(value)))
    except Exception:
        return None


def _sqlite_path(value: Any) -> Any:
    """The file behind a database name: `sqlite3.connect("file:C:/x?mode=ro", uri=True)` is `C:/x`."""
    text = _text(value) if isinstance(value, (str, bytes, os.PathLike)) else ""
    if not text.lower().startswith("file:"):
        return value
    from urllib.parse import unquote

    path = text[5:].split("?", 1)[0].split("#", 1)[0]
    if path.startswith("//"):  # `file://host/path`
        path = "/" + path[2:].split("/", 1)[-1]
    path = unquote(path)
    # `/C:/x` is `C:/x`.
    return path[1:] if re.match(r"/[A-Za-z]:", path) else path


def _resolved(value: Any) -> Optional[str]:
    """The path as the system names it, with links, junctions and 8.3 short names (`PROGRA~1`)
    resolved: they lead elsewhere. Resolved before `_normal`, which only Windows could read back."""
    if isinstance(value, int) or value is None:
        return None
    try:
        return os.path.realpath(_text(value))
    except Exception:
        return None


def _real(value: Any) -> Optional[str]:
    native = _resolved(value)
    return _normal(native) if native else None


def _folders(paths: Any) -> List[str]:
    out = []
    for p in paths or ():
        if p:
            normal = _abs_path(p)
            if normal:
                out.extend({normal, _real(p) or normal})
    return out


def _inside_any(path: str, folders: List[str]) -> bool:
    return any(path == f or path.startswith(f if f.endswith("\\") else f + "\\") for f in folders)


def _relative_to_fd(args: tuple, index: int) -> bool:
    return len(args) > index and isinstance(args[index], int) and args[index] >= 0


def _system_folders() -> List[str]:
    if os.name != "nt":
        return ["/etc", "/usr", "/bin", "/sbin", "/boot", "/System", "/Library"]
    names = ("SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432")
    return [os.environ[n] for n in names if os.environ.get(n)]


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


def install(emit: Emit, config: Dict[str, Any]) -> Optional[Guard]:
    if not config.get("enabled", True):
        return None
    guard = Guard(emit, config)
    guard.install()
    return guard
