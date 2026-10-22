# Windows test report

Findings from running the proof of concept on Windows by hand (Windows 11, toolchain from the
`cadenv` conda environment: Rust 1.94, Node 20.20, uv 0.13.4). Newest first within each section.

## Runner

| # | Finding | Where | State |
|---|---|---|---|
| 1 | **Apps using numpy/pandas hung forever at "Starting".** The runner's control thread blocks in a read on the stdin pipe; on Windows that blocks every other call on the same (synchronous) pipe. Importing numpy loads OpenBLAS, whose C runtime looks at the standard input during DLL load, under the loader lock: the whole process froze (even `faulthandler` couldn't exit) until the launcher wrote a line. Reproduced outside the launcher with a stack dump (main thread in `create_module` for `_multiarray_umath`). Fixed: the reader moves the pipe to a private fd and points fd 0 / the standard input at NUL. The same run then finishes in 3 s. | `runner/devstribution_runner/_runner.py` (`ControlReader`) | Fixed; regression test `test_app_standard_input_is_not_the_protocol_pipe` (fails on the old runner) |

## Launcher UX

| # | Finding | Where | State |
|---|---|---|---|
| 3 | **Uninstall's confirmation failed**: `window.confirm` is routed to the dialog plugin, which wasn't allowed (`dialog.confirm not allowed` in the log). Fixed: `api.ask()` uses the plugin's `ask()`, and the capability now grants `dialog:allow-ask`. | `launcher/src/views/app.ts`, `api.ts`, `capabilities/default.json` | Fixed |
| 2 | **The "⋯" menu was cut off**: only "Repair" showed, "Uninstall" was hidden. The app page's top card had `overflow: hidden` (to clip its decorative glow), which also clipped the menu opening below it. Fixed: the glow is now a background layer and the card no longer clips. Checked in the browser demo; check in the real launcher. | `launcher/src/styles.css` (`.hero`) | Fixed |
| 1 | **Uninstall is hard to find.** It exists, but only in the "⋯" menu on the app page, next to Repair; nothing on the home screen, the app tiles or the Apps list offers it. A user looking for "delete this app" doesn't find it. | `launcher/src/views/app.ts:275` | Open |

Ideas for 1: a visible "Uninstall" on the app page (e.g. in the "What it can do" card), a
right-click / "⋯" menu on the tiles in the Apps list, and check that `confirm()` shows a dialog in
WebView2 (if it doesn't, the click silently does nothing; the dialog plugin's `ask()` would be the
safe choice).

## Packaging a real app (`app_1` → `ricon.dsapp`)

| # | Finding | Where | State |
|---|---|---|---|
| 1 | **`pack` has no way to leave out a folder.** `app_1/output/` holds client data (Excel, SQLite, diagnostic zips) and would have been packed and signed. Worked around by packing a clean copy; a `[tool.devstribution] exclude = [...]` list (or honouring `.gitignore`) is needed. | `crates/devstribution-core/src/package.rs` (`EXCLUDED_DIRS`) | Open |
| 2 | **Stateful apps don't fit the per-run output folder.** The app keeps its Excel di lavoro, archive and database between runs, so it writes to a folder the user picks; `file` outputs must be inside `ctx.output_dir`, so the result can only be shown as text, without an "Open result" button. An output type for "a folder the user chose" would fix it. | `runner/devstribution_runner/_runner.py:277` | Open |
| 3 | No cooperative cancellation: the app never calls `ctx.progress()`, so Cancel relies on the 3 s `KeyboardInterrupt` fallback. Fine to test the fallback with. | `app_1/main.py` | Note |

## Setup and docs

| # | Finding | Where | State |
|---|---|---|---|
| 1 | `HANDOFF.md` says uv "is not a conda package": it is, on conda-forge (`conda install -c conda-forge uv`). | `docs/HANDOFF.md` | Open |
| 2 | `HANDOFF.md` asks for Node 22+, but the lockfile needs `^20.19.0 \|\| >=22.12.0` (Vite 8, Rolldown); Node 20.20 works. | `docs/HANDOFF.md` | Open |
| 3 | `status_quo.md` still says PR #3 is open for review; it is merged (`74c4f90`). | `docs/status_quo.md` | Open |
| 4 | `npx tauri dev` rewrites `tauri = "2"` as `{ version = "2", features = [] }` (same meaning) in the launcher's Cargo.toml, leaving the tree dirty after every run. Commit the rewritten form so it stops. | `launcher/src-tauri/Cargo.toml` | Open |

## Environment notes (not project bugs)

- **Running from the Claude desktop app**: the app is an MSIX package, so writes under `%APPDATA%`
  from its shell are redirected into the package's private folder, while the junctions uv creates
  stay at the real path. uv then fails with "Missing expected target directory for Python minor
  version link". Workaround: `UV_PYTHON_INSTALL_DIR` and `DEVSTRIBUTION_HOME` outside AppData
  (used here: `C:\Users\edomo\.devstribution-dev\`). Doesn't happen from a normal terminal.

## Results so far

| Check | Result |
|---|---|
| `cargo test` | Pass: core 25, host 2 + flow 1 (real uv), CLI 2 |
| Runner tests, Python 3.11 and 3.13 | Pass: 15 each |
| `npm ci`, `npm run sidecar`, `npx tauri dev` | Pass: launcher builds without warnings and opens |
| `keygen`, `pack examples/excel-report`, `verify` | Pass |
| Install `excel-report.dsapp` in the launcher | Pass |
| Install and run `ricon.dsapp` (pandas, pdfplumber) | Pass, after runner fix 1 |

## Still to check on Windows

From `status_quo.md`: Cancel kills the whole process tree (Job Object), Excel cleanup via COM,
Mark-of-the-Web origin, file association and single instance (installer), `pythonw.exe` for `gui`
apps, secrets in Credential Manager.
