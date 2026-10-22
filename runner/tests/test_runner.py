"""Tests of the runner protocol: run `bootstrap.py` exactly as the launcher does."""

import json
import os
import subprocess
import sys
import tempfile
import textwrap
import threading
import time
import unittest
from pathlib import Path

BOOTSTRAP = Path(__file__).resolve().parents[1] / "bootstrap.py"


class RunnerTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.app = self.root / "app"
        self.app.mkdir()
        self.out = self.root / "out"

    def tearDown(self):
        self.tmp.cleanup()

    def write_app(self, code):
        (self.app / "main.py").write_text(textwrap.dedent(code), encoding="utf-8")

    def start(self, **overrides):
        msg = {
            "protocol": 1,
            "kind": "form",
            "app_dir": str(self.app),
            "entry": "main:run",
            "output_dir": str(self.out),
            "inputs": {},
            "input_types": {},
            "outputs": [],
        }
        msg.update(overrides)
        proc = subprocess.Popen(
            [sys.executable, "-I", str(BOOTSTRAP)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=self.root,
        )
        proc.stdin.write((json.dumps(msg) + "\n").encode())
        proc.stdin.flush()
        return proc

    def run_app(self, **overrides):
        """Runs to completion. Like the launcher, keeps stdin open until the result arrives:
        closing it earlier means "launcher gone" and cancels the run."""
        proc = self.start(**overrides)
        messages = []
        for line in proc.stdout:
            messages.append(json.loads(line))
            if messages[-1]["type"] == "result":
                break
        _, stderr = proc.communicate(timeout=30)
        return proc.returncode, messages, stderr.decode()

    def test_form_run_with_typed_inputs_and_file_output(self):
        self.write_app(
            """
            from pathlib import Path

            def run(source: Path, count: int, flag: bool, ctx, label: str = "default"):
                print("this goes to the log, not the protocol")
                ctx.progress(0.5, "halfway")
                ctx.log("hello")
                out = ctx.output_dir / "report.txt"
                out.write_text(f"{source.read_text()} {count + 1} {flag} {label}")
                return {"report": out, "summary": "done"}
            """
        )
        src = self.root / "in.txt"
        src.write_text("data")
        code, messages, stderr = self.run_app(
            inputs={"source": str(src), "count": "41", "flag": "yes", "label": None},
            input_types={"source": "file", "count": "number", "flag": "bool", "label": "text"},
            outputs=[{"name": "report", "type": "file"}, {"name": "summary", "type": "text"}],
        )
        self.assertEqual(code, 0, stderr)
        self.assertEqual(messages[0], {"type": "progress", "value": 0.5, "message": "halfway"})
        self.assertEqual(messages[1]["message"], "hello")
        result = messages[-1]
        self.assertEqual(result["status"], "ok")
        self.assertEqual(Path(result["outputs"]["report"]).read_text(), "data 42 True default")
        self.assertIn("this goes to the log", stderr)
        self.assertFalse((self.app / "__pycache__").exists(), "the app folder must stay untouched")

    def test_exception_is_reported_with_traceback(self):
        self.write_app("def run(ctx):\n    raise ValueError('boom')\n")
        code, messages, _ = self.run_app()
        self.assertEqual(code, 1)
        self.assertEqual(messages[-1]["status"], "error")
        self.assertIn("ValueError: boom", messages[-1]["error"])
        self.assertIn("Traceback", messages[-1]["traceback"])

    def test_output_outside_output_dir_is_refused(self):
        self.write_app(
            """
            def run(ctx):
                p = ctx.output_dir.parent / "escape.txt"
                p.write_text("x")
                return {"report": p}
            """
        )
        code, messages, _ = self.run_app(outputs=[{"name": "report", "type": "file"}])
        self.assertEqual(code, 1)
        self.assertIn("inside ctx.output_dir", messages[-1]["error"])

    def test_undeclared_secret_is_refused(self):
        self.write_app("def run(ctx):\n    return {'v': ctx.secrets.get('other')}\n")
        code, messages, _ = self.run_app(
            secrets={"token": "s3cret"}, allowed_secrets=["token"], outputs=[{"name": "v", "type": "text"}]
        )
        self.assertEqual(code, 1)
        self.assertIn("not declared", messages[-1]["error"])

    def test_cooperative_cancel_runs_cleanup(self):
        marker = self.root / "cleaned"
        self.write_app(
            f"""
            import time
            from pathlib import Path

            def run(ctx):
                ctx.on_cleanup(lambda: Path({str(marker)!r}).write_text("yes"))
                while True:
                    ctx.progress(None, "working")
                    time.sleep(0.05)
            """
        )
        proc = self.start()
        proc.stdout.readline()  # first progress message: the app is running
        proc.stdin.write(b'{"type": "cancel"}\n')
        proc.stdin.flush()
        stdout, _ = proc.communicate(timeout=30)
        result = json.loads(stdout.decode().splitlines()[-1])
        self.assertEqual(result["status"], "cancelled")
        self.assertEqual(proc.returncode, 2)
        self.assertEqual(marker.read_text(), "yes")

    def test_cancel_interrupts_an_app_that_never_calls_ctx(self):
        self.write_app(
            """
            import time

            def run(ctx):
                try:
                    while True:
                        time.sleep(0.05)
                finally:
                    print("finally ran")
            """
        )
        proc = self.start()
        time.sleep(0.5)
        proc.stdin.write(b'{"type": "cancel"}\n')
        proc.stdin.flush()
        stdout, stderr = proc.communicate(timeout=30)
        self.assertEqual(json.loads(stdout.decode().splitlines()[-1])["status"], "cancelled")
        self.assertIn("finally ran", stderr.decode())

    def test_closing_stdin_cancels(self):
        self.write_app("import time\n\ndef run(ctx):\n    while True:\n        ctx.progress(None)\n        time.sleep(0.05)\n")
        proc = self.start()
        proc.stdout.readline()
        proc.stdin.close()
        self.assertEqual(proc.wait(timeout=30), 2)

    def test_app_standard_input_is_not_the_protocol_pipe(self):
        # Regression: on Windows a read pending on the stdin pipe blocks every other call on it,
        # so loading numpy (whose OpenBLAS looks at the standard input) hung the run at start.
        # Only the runner's private copy of the pipe may be read; fd 0 must be NUL.
        self.write_app(
            """
            import os, stat

            def run(ctx):
                ctx.log("pipe" if stat.S_ISFIFO(os.fstat(0).st_mode) else "not a pipe")
                return {}
            """
        )
        proc = self.start()
        watchdog = threading.Timer(15, proc.kill)
        watchdog.start()
        try:
            messages = [json.loads(line) for line in iter(proc.stdout.readline, b"")
                        if json.loads(line)["type"] in ("log", "result")]
        finally:
            watchdog.cancel()
            proc.stdin.close()
            proc.wait(timeout=30)
        self.assertTrue(messages, "the run hung")
        self.assertEqual(messages[0]["message"], "not a pipe")
        self.assertEqual(messages[-1]["status"], "ok")

    def test_gui_kind_runs_module_as_main(self):
        self.write_app(
            """
            import devstribution_runner

            if __name__ == "__main__":
                devstribution_runner.ctx.log("started as main")
            """
        )
        code, messages, stderr = self.run_app(kind="gui", entry="main")
        self.assertEqual(code, 0, stderr)
        self.assertEqual(messages[0]["message"], "started as main")
        self.assertEqual(messages[-1]["status"], "ok")

    def test_bad_protocol_version(self):
        self.write_app("def run(ctx):\n    return {}\n")
        code, messages, _ = self.run_app(protocol=99)
        self.assertEqual(code, 3)
        self.assertIn("protocol", messages[-1]["error"])

    def test_isolated_mode_ignores_pythonpath(self):
        evil = self.root / "evil"
        evil.mkdir()
        (evil / "json.py").write_text("raise SystemExit('shadowed')")
        self.write_app("def run(ctx):\n    return {}\n")
        proc = subprocess.run(
            [sys.executable, "-I", str(BOOTSTRAP)],
            input=json.dumps({"protocol": 1, "kind": "form", "app_dir": str(self.app), "entry": "main:run",
                              "output_dir": str(self.out)}).encode() + b"\n",
            capture_output=True,
            env={**os.environ, "PYTHONPATH": str(evil)},
            cwd=evil,
            timeout=30,
        )
        self.assertEqual(proc.returncode, 0, proc.stderr.decode())


if __name__ == "__main__":
    unittest.main()
