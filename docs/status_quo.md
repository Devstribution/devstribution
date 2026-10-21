# Status quo: 21 October 2026

## Where we are

The proof of concept from [`plan.md`](plan.md) is built and open for review as
[PR #3](https://github.com/Devstribution/devstribution/pull/3), branch `claude/youthful-ritchie-ursrrw`.

| Part | State |
|---|---|
| `devstribution-core` (package format, signatures, `FILES.sha256.pub`, safe extraction) | Done, 25 tests |
| `devstribution-host` (install store, key pinning, uv environments, supervised runs, history, secrets) | Done, unit tests + end-to-end flow test with real uv |
| `devstribution-cli` (`init`, `dev`, `pack`, `keygen`, `verify`) | Done, 2 integration tests |
| `runner/` (stdlib-only Python runner, protocol v1) | Done, 15 tests |
| `launcher/` (Tauri app, Metricool-inspired UI, browser demo mode) | Done, verified by hand on Linux |
| `examples/` (`excel-report`, `hello-gui`) | Done |
| CI on Windows, macOS, Linux | Running on PR #3 (see below) |

Verified end to end in the real launcher on Linux: open a `.dsapp` → confirmation screen with the
signature result and permissions → install (real uv environment) → run with the native file picker →
result, history, error messages.

### CI on PR #3 (first ever Windows/macOS build)

- macOS and Linux: tests and launcher build green.
- Windows `test`: failed in `init_dev_pack_verify` because the example app writes its output in text
  mode, which gives `\r\n` on Windows. The test now normalizes line endings (fixed today).
  Cargo stopped at that test binary, so **the host flow test has not run on Windows yet**.
- Windows `launcher`: still running when this was written.

### Never run on Windows yet

Compiles and passes clippy for `x86_64-pc-windows-msvc`, nothing more:

- Job Object: Cancel / timeout / launcher exit must kill the whole process tree.
- COM cleanup (`track_com`) with Excel.
- Mark-of-the-Web origin on the confirmation screen.
- `.dsapp` file association and single instance (needs the installer).
- `pythonw.exe` for `gui` apps.
- Secrets in Windows Credential Manager.

## TODO for tomorrow

Local setup is in [`HANDOFF.md`](HANDOFF.md).

1. **Get PR #3 green.** Check the CI after the CRLF fix. The Windows host flow test runs for the
   first time, so fix whatever it shows.
2. **Run it on Windows by hand:** `cargo test`, the runner tests, `npm run sidecar`, `npx tauri dev`,
   then pack `examples/excel-report` and install it in the launcher.
3. **Check the Windows-only parts** in the list above, one by one. Start with Cancel killing the
   process tree, and Excel cleanup via COM.
4. **Build the installer:** run `npx tauri build`. Then test the file association by double-clicking
   a `.dsapp`, and the Mark-of-the-Web origin by downloading one with a browser.
5. **Merge PR #3** once green and checked.
6. **GitHub settings** (only you can do these):
   - turn on private vulnerability reporting;
   - optionally add a `.github/FUNDING.yml` with your sponsor accounts (none exists yet).

### Next after that

- Release sidecar: download a pinned uv and check its sha256, instead of copying the one on PATH
  (`launcher/scripts/sidecar-uv.mjs`).
- Signing of the installer (SignPath).
- Phase 0 of the plan: try it on a locked-down PC (AppLocker/WDAC).
- Phase 2: managed mode (HKLM policy), catalog, GitHub namespace verification, `devstribution://`
  links, revocation.
- Remaining app kinds (`web`, `background`) and `resources` downloads (refused at install today).
