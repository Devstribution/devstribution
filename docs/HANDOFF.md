# Handoff: picking up the next session

How to continue on your own machine, and what the next session should do. Where things stand is
in [`status_quo.md`](status_quo.md); the long-term order of work is in [`roadmap.md`](roadmap.md).

## The next session, in order

1. **Merge the security PR** ("Security review before signing") once CI is green on all three
   platforms.
2. **Rebuild on Windows** from `main` (commands below) and run the checks that only Windows can
   run. Each check is listed in [`security.md`](security.md), "Still to verify on a real Windows
   PC". Write the results in [`report.md`](report.md) and tick them in the roadmap (step 2):
   - **Cancel and closing the launcher** stop the app and everything it started. The app now
     starts suspended and joins its Job Object first. Try an app that starts a child process,
     and Excel through COM.
   - **The launcher starts normally** with the restricted DLL loading
     (`/DEPENDENTLOADFLAG:0x800`, `SetDefaultDllDirectories`).
   - **The old data moves** from `%LOCALAPPDATA%\devstribution` to
     `%LOCALAPPDATA%\dev.devstribution.launcher\data` on first start, and the installed apps
     still run.
   - **App environments still build behind your company network**: uv now runs with
     `--no-config`, a cleaned environment and `--system-certs`.
   - **A real run through the app panel**, Excel file and all, plus the colours and Settings.
3. **Repack your real app** (`ricon.dsapp`) with the new CLI. `pack` now applies the same checks
   as the launcher:
   - wheels from PyPI only, with hashes;
   - stricter file names;
   - version numbers made of digits only.

   Fix what it reports and give it a new version number, then reinstall it. The same version with
   different content is refused, so bump the version or uninstall the old one first.
4. **Build the installer:** `npx tauri build` in `launcher/`. Then check:
   - the per-user NSIS installer, which no longer builds an MSI;
   - double-clicking a `.dsapp`, and a second double-click while the launcher is open;
   - a `.dsapp` downloaded with a browser shows "Downloaded from the internet";
   - uninstalling with "delete app data" removes the data folder and the apps' secrets in
     Credential Manager (`devstribution.exe --forget-secrets`, run by `windows/hooks.nsh`).
5. **Apply for signing:** SignPath Foundation (free for open source), with Certum's open-source
   certificate as the fallback. Then sign in CI ([`roadmap.md`](roadmap.md), step 3).
6. **Phase 0 of [`plan.md`](plan.md):** install and run on a locked-down company PC
   (AppLocker/WDAC).

GitHub settings only you can change: turn on private vulnerability reporting
(`SECURITY.md` points to it), and merge or close Dependabot's weekly PRs.

## Setting up on Windows

Prerequisites:
- Rust (rustup, MSVC toolchain), 1.90 or newer;
- Node 20.19+ or 22.12+;
- uv;
- the Visual Studio C++ build tools.

WebView2 is part of Windows 10/11.

```powershell
git checkout main
git pull

cargo test                                   # core, host (needs uv on PATH), CLI
python -m unittest discover -s runner/tests  # runner protocol and code checks

cd launcher
npm ci                                       # after every pull that changes package-lock.json
npm run sidecar                              # downloads the pinned uv (uv-pin.json), checks its sha256
npx tauri dev                                # launcher with hot reload
npx tauri build                              # release build + installer (target\release\bundle\nsis)
```

A local file such as `launcher/src-tauri/Cargo.toml` changed by hand blocks `git pull`. Run
`git diff <file>`, then `git restore <file>` if `main` already has the change.

Try it end to end:

```powershell
cargo run -p devstribution-cli -- keygen --namespace devstribution --no-password
cargo run -p devstribution-cli -- pack examples/excel-report -o excel-report.dsapp
# then drop excel-report.dsapp on the launcher window, or use "Install app"
```

Set `DEVSTRIBUTION_HOME` to a scratch folder to keep test installs out of the real data folder.

**Development builds and release builds find uv differently:**
- A **release** launcher uses only the uv next to its executable, the one `npm run sidecar`
  fetched.
- A **development** build also accepts `DEVSTRIBUTION_UV`, or `uv` on PATH.

**The conda environment** is fine for tooling. It doesn't leak into apps: installed apps get uv's
own Python (`--managed-python`), with conda's variables removed from the environment, and the
runner starts Python with `-I`. Run `cargo test` from a shell where `uv` is on PATH.

## Layout

| Path | What |
|---|---|
| `crates/devstribution-core` | Package format: manifest, `FILES.sha256`, signatures and fingerprints, packing, safe extraction, lockfile checks (`lock::check_sources` is the install gate), the code scan (`scan`) |
| `crates/devstribution-host` | Install store (pins, anti-downgrade, `admit`), uv environments (hardened `uv::sync`), supervised runs (Job Object), runner, history, secrets, data folder |
| `crates/devstribution-cli` | `devstribution init / dev / pack / keygen / verify / scan` |
| `runner/` | Stdlib-only Python runner started by the launcher; protocol in `devstribution_runner/_runner.py` |
| `launcher/src` | TypeScript UI, no framework. `style.css` holds the design tokens, documented in [`design.md`](design.md); `look.ts` handles the theme and colour choices. |
| `launcher/src-tauri` | Rust commands over the host crate, `capabilities/` (what the window may call), `build.rs` (command list, DLL load flag), `windows/hooks.nsh` (uninstaller), icons |
| `examples/` | `excel-report` (form), `hello-gui` (gui, tkinter) |
| `docs/` | `plan.md` (design), `roadmap.md` (to 1.0), `format.md` (package spec), `security.md` (threat model, review), `design.md` (look), `report.md` (Windows findings), `status_quo.md` |

## Things to know before changing code

- **Format 1** is specified in [`format.md`](format.md). It was amended once before any release.
  From the first signed launcher on, a change a format-1 launcher can't read needs
  `schema_version = 2`. The frozen package in `crates/devstribution-core/tests/fixtures/format-1/`
  must keep verifying.
- **Security-relevant changes** (extraction, verification, install, uv invocation, runs, launcher
  commands, CSP, capabilities) get a line in [`security.md`](security.md). Repeat the review before
  each release that touches them.
- **A new launcher command** goes in three places:
  - `generate_handler!` in `lib.rs`;
  - `COMMANDS` in `build.rs`;
  - an `allow-<name>` line in `capabilities/default.json`.

  The window can't call it otherwise.
- **Icons:** the Windows icons are rendered at each exact size from `src/logo-small.svg`. Don't let
  `tauri icon` overwrite them (see [`design.md`](design.md), "Logo").
- **The code scan** (`crates/devstribution-core/src/scan.rs`) is one rule table used by `pack`,
  `devstribution scan` and the install screen. A new rule needs a test that an ordinary app stays
  clean (`ordinary_app_is_clean`): every false alarm teaches users to ignore the screen.
- **CI:** actions are pinned by commit SHA. Update them through Dependabot, not by hand to a tag.
