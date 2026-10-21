# Handoff: continuing locally

State at the end of the cloud session, and how to pick it up on your own machine.

## What works

The full proof of concept, tested end to end in the real Tauri launcher (on Linux, under a
virtual display):

1. Double-click a `.dsapp` (the launcher receives it as an argument). The confirmation screen
   shows the verified publisher key, the origin and the permissions.
2. Install: files verified and moved into place, the Python environment created by uv from the
   lockfile, the publisher key pinned (trust on first use).
3. App page: the form is generated from the manifest and prefilled from the last run; the
   native file picker works.
4. ▶ Run: live progress, Cancel, then "Open result" / "Show in folder". Errors are shown in plain
   words. Every run is recorded (Runs page, dashboard).
5. Repair, uninstall, secrets in the system credential store, refusals (another key for the same
   namespace, downgrade, same version with different content).

Screenshots: `docs/images/`. The UI also runs in a plain browser with demo data
(`npm run dev` in `launcher/`, then open http://localhost:5173), which is handy for design work.

## Layout

| Path | What |
|---|---|
| `crates/devstribution-core` | Package format: manifest, `FILES.sha256`, signatures, packing, safe extraction, lockfile checks |
| `crates/devstribution-host` | Install store, uv environments, supervised runs (Job Object on Windows), history, secrets |
| `crates/devstribution-cli` | `devstribution init / dev / pack / keygen / verify` |
| `runner/` | Stdlib-only Python runner started by the launcher; protocol in `devstribution_runner/_runner.py` |
| `launcher/` | Tauri app: `src/` (TypeScript UI, no framework), `src-tauri/` (Rust commands over the host crate) |
| `examples/` | `excel-report` (form), `hello-gui` (gui, tkinter) |

## Setting up on Windows

Prerequisites: Rust (rustup, MSVC toolchain), Node 22+, uv, and the Visual Studio C++ build
tools. WebView2 is already part of Windows 10/11.

```powershell
cargo test                                   # core, host (needs uv on PATH), CLI
python -m unittest discover -s runner/tests  # runner protocol and code checks

cd launcher
npm ci
npm run sidecar                              # copies your uv to src-tauri/binaries/uv-<triple>.exe
npx tauri dev                                # launcher with hot reload
```

Try it end to end:

```powershell
cargo run -p devstribution-cli -- keygen --namespace devstribution --no-password
cargo run -p devstribution-cli -- pack examples/excel-report -o excel-report.dsapp
# then drop excel-report.dsapp on the launcher window, or use "Install app"
```

Set `DEVSTRIBUTION_HOME` to a scratch folder to keep test installs out of
`%LOCALAPPDATA%\devstribution`.

### About the conda environment

Fine for tooling (running the runner tests, Python scripts). It does not leak into apps: the
host runs uv with `UV_PYTHON_PREFERENCE=only-managed`, so app environments are always built on
uv's own Python builds, and the runner starts Python with `-I` (ignores `PYTHONPATH` and user
site-packages). Two things to watch:

- Run `cargo test` from a shell where `uv` is on PATH (it is not a conda package).
- `devstribution dev`'s code check runs `uv run --no-project python`, which may pick conda's
  Python; harmless (it only parses the app's code with `ast`).

## Verified here vs. still to verify on Windows

Compiled and linted for `x86_64-pc-windows-msvc` from Linux, but **never run on Windows**:

- Job Object (kill the whole process tree on Cancel/timeout/launcher exit), `CREATE_NO_WINDOW`.
- `track_com` and COM server cleanup (needs Excel; use `DispatchEx`).
- Mark-of-the-Web origin on the confirmation screen (download a `.dsapp` with a browser).
- File association and single instance (needs the installer: `npx tauri build`).
- `pythonw.exe` for `gui` apps; secrets in Credential Manager.

Phase 0 of `docs/plan.md` (the locked-down PC with AppLocker/WDAC) is still the first real-world
test to run.

## Next steps, in order

1. Run the steps above on Windows and fix what breaks; CI (`.github/workflows/ci.yml`) already
   builds and lints everything on Windows, macOS and Linux once a PR is open.
2. Release build of the sidecar: download a pinned uv release and verify its sha256 instead of
   copying the one on PATH (`launcher/scripts/sidecar-uv.mjs`).
3. Installer: `npx tauri build` (NSIS, per-user), then signing (SignPath).
4. Phase 2 of the plan: managed mode (HKLM policy), catalog and GitHub namespace verification,
   `devstribution://` links, revocation.
5. Remaining app kinds (`web`, `background`) and `resources` downloads (refused at install today).

## Known gaps (POC)

- Runs are not sandboxed beyond the Job Object; trust is in the publisher (see the plan).
- No signature enforcement policy yet beyond pinning: a new namespace is accepted after the user
  confirms, as the plan's free mode describes.
- The runner is written to the data folder on first start rather than shipped in the install
  folder (fine per-user; per-machine mode needs it in Program Files).
- No automated UI tests; the UI was checked with screenshots against the mock backend and the
  real launcher.
