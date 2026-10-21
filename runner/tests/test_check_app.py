import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "devstribution_tools"))

import check_app  # noqa: E402

INPUTS = [{"name": "excel_file", "type": "file"}, {"name": "limit", "type": "number"}]


class CheckAppTest(unittest.TestCase):
    def check(self, code, kind="form", entry="main:run", inputs=INPUTS):
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "main.py").write_text(textwrap.dedent(code), encoding="utf-8")
            return check_app.check(Path(tmp), {"kind": kind, "entry": entry, "inputs": inputs})

    def test_matching_signature(self):
        r = self.check(
            """
            from pathlib import Path
            from typing import Optional

            def run(excel_file: Path, limit: Optional[int] = None, *, ctx) -> dict:
                return {}
            """
        )
        self.assertEqual(r, {"errors": [], "warnings": []})

    def test_missing_extra_and_mistyped_parameters(self):
        r = self.check("def run(excel_file: int, other, ctx):\n    pass\n")
        text = "\n".join(r["errors"])
        self.assertIn("missing the parameter `limit`", text)
        self.assertIn("`other`, which is not declared", text)
        self.assertIn("`excel_file: int` does not match input type \"file\"", text)

    def test_ctx_required_and_entry_missing(self):
        self.assertIn("must accept a `ctx`", self.check("def run(excel_file, limit):\n    pass\n")["errors"][0])
        self.assertIn("no top-level function `go`", self.check("def run(ctx): pass", entry="main:go")["errors"][0])
        self.assertIn("not found", self.check("", entry="other:run")["errors"][0])

    def test_dispatch_warning(self):
        r = self.check(
            """
            import win32com.client
            def run(excel_file, limit, ctx):
                a = win32com.client.Dispatch("Excel.Application")
                b = win32com.client.DispatchEx("Excel.Application")
            """
        )
        self.assertEqual(len(r["warnings"]), 1)
        self.assertIn("main.py:4: Dispatch('Excel.Application')", r["warnings"][0])

    def test_syntax_error_reported_once(self):
        r = self.check("def run(ctx:\n")
        self.assertEqual(len(r["errors"]), 1)
        self.assertIn("syntax error", r["errors"][0])


if __name__ == "__main__":
    unittest.main()
