"""Tests of the runtime guard: apps that do what malware does are stopped before the call
happens, undeclared network and SAP use is blocked, and what is allowed still works.

Every "malicious" call here is stopped before it reaches Windows: nothing is written to the
registry or the Startup folder, and no PowerShell starts.
"""

import json
import os
import subprocess
import sys
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from devstribution_runner import _guard  # noqa: E402
from test_runner import BOOTSTRAP, RunnerBase  # noqa: E402

STOPPED = _guard.STOP_EXIT_CODE


class GuardTest(RunnerBase):
    def run_guarded(self, body, guard=None, **overrides):
        lines = textwrap.dedent(body).strip("\n").splitlines()
        self.write_app("def run(ctx):\n" + "".join(f"    {line}\n" for line in lines))
        if guard is not None:
            overrides["guard"] = guard
        overrides.setdefault("outputs", [{"name": "v", "type": "text"}])
        return self.run_app(**overrides)

    def run_app(self, **overrides):
        # A stopped run ends without a result message: read until the runner exits.
        proc = self.start(**overrides)
        out, err = proc.communicate(timeout=30)
        messages = [json.loads(line) for line in out.decode().splitlines() if line.strip()]
        return proc.returncode, messages, err.decode()

    def guard_messages(self, messages, action):
        return [m for m in messages if m.get("type") == "guard" and m["action"] == action]

    def assert_stopped(self, body, rule, guard=None):
        code, messages, stderr = self.run_guarded(body, guard)
        self.assertEqual(code, STOPPED, (messages, stderr))
        stops = self.guard_messages(messages, "stop")
        self.assertEqual([s["rule"] for s in stops], [rule], messages)
        self.assertFalse([m for m in messages if m.get("type") == "result"], "no result after a stop")
        return stops[0]

    def environment_with(self, pth_files):
        """A real environment (like the app's) whose site-packages holds these `.pth` files."""
        env = self.root / "env"
        subprocess.run([sys.executable, "-m", "venv", "--without-pip", str(env)], check=True)
        python = env / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        purelib = subprocess.run(
            [str(python), "-c", "import sysconfig; print(sysconfig.get_path('purelib'))"],
            check=True, capture_output=True, text=True,
        ).stdout.strip()
        for name, text in pth_files.items():
            Path(purelib, name).write_text(text)
        self.python = str(python)

    def test_dependency_startup_code_runs_under_the_guard(self):
        # A `.pth` line runs when Python starts. Here it loads hidden compiled code: before the
        # guard was installed first, nothing would have seen it.
        self.environment_with({"evil.pth": "import marshal; marshal.loads(marshal.dumps(1))\n"})
        stop = self.assert_stopped("return {'v': 'ran'}", "hidden-code")
        self.assertEqual(stop["action"], "stop")

    def test_dependency_startup_code_is_reported(self):
        self.environment_with({
            "helper.pth": "import sys\n",
            "paths.pth": "some/folder\n",
            "distutils-precedence.pth": "import os\n",
        })
        code, messages, _ = self.run_guarded("return {'v': 'ok'}")
        self.assertEqual(code, 0, messages)
        startup = [m["message"] for m in self.guard_messages(messages, "observe") if m["rule"] == "startup-code"]
        self.assertEqual(startup, ["a dependency runs code when Python starts: helper.pth"])

    def test_ordinary_app_runs(self):
        code, messages, _ = self.run_guarded(
            """
            import json, pathlib, socket, subprocess, sys
            (ctx.output_dir / "x.json").write_text(json.dumps({"host": socket.gethostname()}))
            subprocess.run([sys.executable, "-c", "pass"], check=True)
            return {"v": "ok"}
            """
        )
        self.assertEqual(code, 0, messages)
        self.assertEqual(messages[-1]["outputs"], {"v": "ok"})
        observed = self.guard_messages(messages, "observe")
        self.assertTrue(any(m["rule"] == "program" for m in observed), observed)

    def test_undeclared_network_is_blocked(self):
        code, messages, _ = self.run_guarded(
            """
            import socket
            try:
                socket.create_connection(("example.com", 80), timeout=1)
            except PermissionError as e:
                return {"v": str(e)}
            return {"v": "connected"}
            """
        )
        self.assertEqual(code, 0, messages)
        self.assertIn("does not declare", messages[-1]["outputs"]["v"])
        self.assertEqual(self.guard_messages(messages, "block")[0]["detail"], "example.com")

    def test_connecting_to_an_ip_is_blocked_too(self):
        code, messages, _ = self.run_guarded(
            """
            import socket
            s = socket.socket()
            try:
                s.connect(("93.184.215.14", 80))
            except PermissionError:
                return {"v": "blocked"}
            return {"v": "connected"}
            """
        )
        self.assertEqual(messages[-1]["outputs"], {"v": "blocked"})

    def test_declared_hosts_only(self):
        code, messages, _ = self.run_guarded(
            """
            import socket
            try:
                socket.getaddrinfo("other.example.org", 443)
            except PermissionError as e:
                return {"v": str(e)}
            return {"v": "resolved"}
            """,
            guard={"network": ["https://api.example.com/v1"]},
        )
        self.assertIn("does not declare", messages[-1]["outputs"]["v"])

    def test_loopback_is_always_allowed(self):
        code, messages, _ = self.run_guarded(
            """
            import socket
            server = socket.socket()
            server.bind(("127.0.0.1", 0))
            server.listen(1)
            client = socket.create_connection(server.getsockname(), timeout=5)
            client.close()
            return {"v": "ok"}
            """
        )
        self.assertEqual(code, 0, messages)
        self.assertEqual(messages[-1]["outputs"], {"v": "ok"})

    def test_undeclared_sap_is_blocked(self):
        code, messages, _ = self.run_guarded(
            """
            try:
                import pyrfc
            except PermissionError:
                return {"v": "blocked"}
            except ImportError:
                return {"v": "not installed"}
            """
        )
        self.assertEqual(messages[-1]["outputs"], {"v": "blocked"})

    @unittest.skipUnless(sys.platform == "win32", "registry")
    def test_start_with_windows_registry(self):
        stop = self.assert_stopped(
            """
            import winreg
            key = winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\\Microsoft\\Windows\\CurrentVersion", 0, winreg.KEY_READ)
            winreg.CreateKey(key, "Run")
            """,
            "start-with-windows",
        )
        self.assertIn("currentversion\\run", stop["detail"])

    @unittest.skipUnless(sys.platform == "win32", "registry")
    def test_reading_the_run_key_is_fine(self):
        code, messages, _ = self.run_guarded(
            """
            import winreg
            # HKLM: the HKCU Run key is missing on fresh profiles (CI runners)
            winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, r"Software\\Microsoft\\Windows\\CurrentVersion\\Run").Close()
            return {"v": "ok"}
            """
        )
        self.assertEqual(code, 0, messages)

    def test_start_with_windows_folder(self):
        self.assert_stopped(
            """
            import pathlib
            p = ctx.output_dir / "Start Menu" / "Programs" / "Startup"
            p.mkdir(parents=True)
            (p / "evil.bat").write_text("calc")
            """,
            "start-with-windows",
        )

    def test_browser_passwords(self):
        self.assert_stopped(
            """
            p = ctx.output_dir / "Google" / "Chrome" / "User Data" / "Default"
            p.mkdir(parents=True)
            open(p / "Login Data", "rb")
            """,
            "browser-passwords",
        )

    def test_saved_keys(self):
        stop = self.assert_stopped(
            """
            p = ctx.output_dir / ".ssh"
            p.mkdir(parents=True)
            open(p / "id_ed25519", "rb")
            """,
            "saved-secrets",
        )
        self.assertTrue(stop["detail"].endswith("id_ed25519"), stop)

    def test_password_database(self):
        self.assert_stopped("open(ctx.output_dir / 'Passwords.kdbx', 'rb')", "saved-secrets")

    def test_moving_a_key_out_reads_it(self):
        ssh = self.root / "home" / ".ssh"
        ssh.mkdir(parents=True)
        (ssh / "id_ed25519").write_text("key")
        stop = self.assert_stopped(
            f"""
            import os
            os.rename({str(ssh / 'id_ed25519')!r}, ctx.output_dir / 'k.txt')
            """,
            "saved-secrets",
        )
        self.assertTrue(stop["detail"].endswith("id_ed25519"), stop)
        self.assertTrue((ssh / "id_ed25519").exists(), "stopped before the move")

    def test_linking_another_runs_results_reads_them(self):
        other = self.root / "runs" / "other" / "output"
        other.mkdir(parents=True)
        (other / "clients.xlsx").write_text("data")
        self.assert_stopped(
            f"""
            import os
            os.link({str(other / 'clients.xlsx')!r}, ctx.output_dir / 'c.xlsx')
            """,
            "other-app-data",
            guard={"private": [str(self.root / "runs")]},
        )

    # -- other Pythons the app starts ---------------------------------------------------------

    def child_settings(self):
        """The guard settings with the folder the launcher gives for child Pythons."""
        return {"child_dir": str(self.root / "run" / "guard")}

    def test_child_python_runs_under_the_guard(self):
        key = self.root / "home" / ".ssh" / "id_ed25519"
        key.parent.mkdir(parents=True)
        key.write_text("key")
        code, messages, stderr = self.run_guarded(
            f"""
            import subprocess, sys
            ok = subprocess.run([sys.executable, "-c", "import sys; print(6 * 7, sys.argv[1:])", "a"],
                                capture_output=True, text=True)
            stolen = subprocess.run([sys.executable, "-c", "import sys; open(sys.argv[1], 'rb').read()",
                                     {str(key)!r}])
            return {{"v": ok.stdout.strip() + " " + str(stolen.returncode)}}
            """,
            self.child_settings(),
        )
        self.assertEqual(code, 0, (messages, stderr))
        self.assertEqual(messages[-1]["outputs"]["v"], f"42 ['a'] {STOPPED}", messages)
        stops = self.guard_messages(messages, "stop")
        self.assertEqual([s["rule"] for s in stops], ["saved-secrets"], messages)
        self.assertIn("child", stops[0])
        self.assertTrue(stops[0]["detail"].endswith("id_ed25519"), stops)

    def test_child_scripts_and_modules(self):
        (self.app / "helper.py").write_text("import sys\nprint('helper', *sys.argv[1:])\n")
        code, messages, stderr = self.run_guarded(
            f"""
            import subprocess, sys
            app = {str(self.app)!r}
            script = subprocess.run([sys.executable, "-u", app + "/helper.py", "x"],
                                    capture_output=True, text=True)
            module = subprocess.run([sys.executable, "-m", "helper", "y"], cwd=app,
                                    capture_output=True, text=True)
            return {{"v": script.stdout.strip() + "|" + module.stdout.strip() + "|" + module.stderr}}
            """,
            self.child_settings(),
        )
        self.assertEqual(code, 0, (messages, stderr))
        self.assertEqual(messages[-1]["outputs"]["v"], "helper x|helper y|", messages)

    def test_python_through_a_shell_is_stopped(self):
        self.assert_stopped(
            """
            import os, sys
            os.system(f'"{sys.executable}" -c pass')
            """,
            "unguarded-python",
            guard=self.child_settings(),
        )

    def test_a_second_runner_is_stopped(self):
        self.assert_stopped(
            f"""
            import subprocess, sys
            subprocess.run([sys.executable, "-I", "-S", {str(BOOTSTRAP)!r}])
            """,
            "unguarded-python",
            guard=self.child_settings(),
        )

    def test_child_settings_cannot_be_changed(self):
        settings = self.root / "run" / "guard" / "config.json"
        self.assert_stopped(
            f"open({str(settings)!r}, 'w').write('{{}}')",
            "devstribution-files",
            guard=self.child_settings(),
        )

    def test_public_ssh_files_are_fine(self):
        code, messages, _ = self.run_guarded(
            """
            p = ctx.output_dir / ".ssh"
            p.mkdir(parents=True)
            for name in ("known_hosts", "config", "id_ed25519.pub"):
                (p / name).write_text("x")
                open(p / name).read()
            return {"v": "ok"}
            """
        )
        self.assertEqual(code, 0, messages)

    def test_a_chosen_file_is_the_apps_to_read(self):
        db = self.root / "Passwords.kdbx"
        db.write_text("secret")
        self.write_app("def run(db, ctx):\n    return {'v': open(db).read()}\n")
        code, messages, _ = self.run_app(
            inputs={"db": str(db)}, input_types={"db": "file"}, outputs=[{"name": "v", "type": "text"}]
        )
        self.assertEqual(code, 0, messages)
        self.assertEqual(messages[-1]["outputs"], {"v": "secret"})

    def test_powershell_profile(self):
        self.assert_stopped(
            """
            p = ctx.output_dir / "Documents" / "WindowsPowerShell"
            p.mkdir(parents=True)
            (p / "Microsoft.PowerShell_profile.ps1").write_text("calc")
            """,
            "start-with-windows",
        )

    def test_windows_files(self):
        target = (os.environ.get("SystemRoot", "C:\\Windows") + "\\devstribution-test.txt"
                  if sys.platform == "win32" else "/etc/devstribution-test")
        self.assert_stopped(f"open({target!r}, 'w')", "system-files")

    def test_devstribution_and_other_apps(self):
        # The output folder lives inside the data folder, as in the launcher.
        data = self.root
        apps = data / "apps" / "other"
        apps.mkdir(parents=True)
        stop = self.assert_stopped(
            f"""
            (ctx.output_dir / "fine.txt").write_text("ok")
            open({str(apps / "main.py")!r}, "w")
            """,
            "devstribution-files",
            guard={"protected": [str(data)], "temp": []},
        )
        self.assertIn("main.py", stop["detail"])

    def test_the_apps_own_python(self):
        self.assert_stopped(
            """
            import sys, os
            open(os.path.join(sys.prefix, "devstribution-test.pth"), "w")
            """,
            "devstribution-files",
        )

    def test_other_runs_results(self):
        other = self.root / "runs" / "other" / "output"
        other.mkdir(parents=True)
        (other / "clients.xlsx").write_text("data")
        self.assert_stopped(
            f"open({str(other / 'clients.xlsx')!r}, 'rb')",
            "other-app-data",
            guard={"private": [str(self.root / "runs")]},
        )

    def test_many_existing_files_changed(self):
        docs = self.root / "Documents"
        docs.mkdir()
        for i in range(_guard.MASS_CHANGES + 10):
            (docs / f"{i}.docx").write_text("mine")
        stop = self.assert_stopped(
            f"""
            import pathlib
            for p in sorted(pathlib.Path({str(docs)!r}).iterdir()):
                p.write_bytes(b"encrypted")
            """,
            "mass-changes",
            guard={"temp": []},
        )
        self.assertIn(f"{_guard.MASS_CHANGES + 1} files", stop["detail"])
        # Stopped before the call: the file over the limit and those after it were never written.
        untouched = sum((docs / f"{i}.docx").read_text() == "mine" for i in range(_guard.MASS_CHANGES + 10))
        self.assertEqual(untouched, 10)

    def test_changes_in_a_chosen_folder_are_the_apps(self):
        work = self.root / "work"
        work.mkdir()
        for i in range(_guard.MASS_CHANGES + 10):
            (work / f"{i}.xlsx").write_text("old")
        self.write_app(
            "import pathlib\n"
            "def run(folder, ctx):\n"
            "    for p in pathlib.Path(folder).iterdir():\n"
            "        p.write_text('new')\n"
            "    return {'v': 'ok'}\n"
        )
        code, messages, _ = self.run_app(
            inputs={"folder": str(work)}, input_types={"folder": "folder"},
            outputs=[{"name": "v", "type": "text"}], guard={"temp": []},
        )
        self.assertEqual(code, 0, messages)
        summary = [m["message"] for m in self.guard_messages(messages, "observe") if m["rule"] == "files"]
        self.assertEqual(summary, [f"wrote {_guard.MASS_CHANGES + 10} files in what you chose"])

    def test_files_changed_elsewhere_are_reported(self):
        elsewhere = self.root / "elsewhere"
        elsewhere.mkdir()
        (elsewhere / "old.txt").write_text("x")
        code, messages, _ = self.run_guarded(
            f"""
            import os, pathlib
            (ctx.output_dir / "report.txt").write_text("ok")
            base = pathlib.Path({str(elsewhere)!r})
            for name in ("a.txt", "b.txt", "c.txt"):
                (base / name).write_text("new")
            os.remove(base / "old.txt")
            return {{"v": "ok"}}
            """,
            guard={"temp": []},
        )
        self.assertEqual(code, 0, messages)
        observed = self.guard_messages(messages, "observe")
        self.assertEqual(len([m for m in observed if m["rule"] == "file-written"]), 3, observed)
        self.assertEqual(len([m for m in observed if m["rule"] == "file-deleted"]), 1, observed)
        summary = [m["message"] for m in observed if m["rule"] == "files"]
        self.assertEqual(summary, ["wrote 4 files: 1 in its output folder, 3 elsewhere; deleted or moved 1 file elsewhere"])

    def test_temporary_files_are_not_reported(self):
        code, messages, _ = self.run_guarded(
            """
            import tempfile
            with tempfile.TemporaryDirectory() as d:
                open(d + "/scratch.txt", "w").close()
            return {"v": "ok"}
            """
        )
        self.assertEqual(code, 0, messages)
        self.assertFalse([m for m in self.guard_messages(messages, "observe") if m["rule"].startswith("file")])

    def test_running_a_program_it_wrote(self):
        self.assert_stopped(
            """
            import subprocess
            tool = ctx.output_dir / "update.bat"
            tool.write_text("echo hi")
            subprocess.run([str(tool)])
            """,
            "downloaded-program",
        )

    def test_encoded_powershell_never_starts(self):
        stop = self.assert_stopped(
            """
            import subprocess
            subprocess.run(["C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe", "-NoP", "/Enc", "ZQBjAGgAbwA="])
            """,
            "encoded-powershell",
        )
        self.assertIn("powershell", stop["detail"].lower())

    def test_programs_behind_cmd(self):
        self.assert_stopped(
            """
            import os
            os.system('cmd /c "vssadmin delete shadows /all /quiet"')
            """,
            "delete-backups",
        )

    @unittest.skipUnless(sys.platform == "win32", "Windows DLLs")
    def test_process_injection(self):
        self.assert_stopped(
            """
            import ctypes
            ctypes.windll.kernel32.WriteProcessMemory
            """,
            "process-injection",
        )

    def test_hidden_compiled_code(self):
        self.assert_stopped(
            """
            import marshal
            exec(marshal.loads(marshal.dumps(compile("x = 1", "hidden", "exec"))))
            """,
            "hidden-code",
        )

    def test_allowed_rule_runs_and_is_reported(self):
        code, messages, _ = self.run_guarded(
            """
            import marshal
            exec(marshal.loads(marshal.dumps(compile("x = 1", "hidden", "exec"))))
            return {"v": "ran"}
            """,
            guard={"allow": ["hidden-code"]},
        )
        self.assertEqual(code, 0, messages)
        self.assertTrue(any(m["rule"] == "allowed" for m in self.guard_messages(messages, "observe")))


class ProcessRuleTest(unittest.TestCase):
    """The matcher on its own, with the cases the launcher's copy is tested on too."""

    def setUp(self):
        self.rules = _guard.load_process_rules()

    def rule(self, command):
        matched = _guard.match_process(command, self.rules)
        return matched["id"] if matched else None

    def test_cases(self):
        cases = json.loads((Path(__file__).with_name("process_rule_cases.json")).read_text(encoding="utf-8"))
        for command, expected in cases:
            with self.subTest(command=command):
                self.assertEqual(self.rule(command), expected)


if __name__ == "__main__":
    unittest.main()
