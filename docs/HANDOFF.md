# Handoff: picking up the next session

How to continue on your own machine, and what the next session should do. Where things stand is
in [`status_quo.md`](status_quo.md); the long-term order of work is in [`roadmap.md`](roadmap.md).

## The next session, in order

> **Update, 23 October:**
> - Steps 1 to 4 are done. Results are in [`report.md`](report.md) and the roadmap is ticked.
> - Two checks remain open: the data folder move with real old data, and uv behind a company
>   proxy.
> - The code scan was added before install (PR "Code scan before install").
> - Since then: the runtime guard, the dependency check, "Uninstall devstribution" in Settings
>   and `.zip` downloads, all checked in the installed launcher (PR #19).
>
> **Update, later on 23 October:** managed mode's first version is in the code (step 1 below):
> the policy from the registry, `.trust` files, the block message with a request for IT, the
> policy in Settings, dry run, tests. How IT sets it up: [`managed-mode.md`](managed-mode.md).
> Also fixed: "Open result" now opens the folder the user chose when the app wrote its results
> there, and CI runs each setup step once.
>
> Next: check managed mode on Windows (registry value set by hand, a trust folder only
> administrators can write, then a network share), then signing and Phase 0.

1. **Managed mode, first version: no server needed.** *Done in code, to check on Windows.* This is what a company's IT will ask
   for before allowing devstribution on its PCs. Today trust is per user, on first use. The
   design is in [`plan.md`](plan.md), "Trust model and policy"; the checklist is
   [`roadmap.md`](roadmap.md), section 10. Everything below works with what IT already has: the
   registry pushed through GPO or Intune, and files on a network share. No server of ours is
   involved.
   - **Read the policy.** If `HKLM\SOFTWARE\Policies\devstribution` exists, the launcher is in
     managed mode; otherwise it stays in free mode exactly as today. A user can't write under
     HKLM, so an app or the user can't switch the mode off. The policy holds the allowed
     authors, either inline or as a path to a TOML policy file. That file must sit somewhere
     only admins can write, and the launcher should refuse it otherwise.
   - **Allowed authors from `.trust` files.** A `.trust` file holds an author's namespace and
     public keys (fingerprints). `devstribution trust export` writes one from a publisher key,
     and IT lists it in the policy. In managed mode, `admit` (`devstribution-host/src/store.rs`)
     accepts only packages whose namespace and key are in the policy. There is no trust on first
     use, and no "Install anyway" for an unknown author.
   - **The block message helps.** A refused package says who to ask, and offers to save a
     ready-to-forward `.trust` file ("ask your IT to trust X").
   - **Policy options, all optional:**
     - what happens to a danger finding in the code scan or dependency check (refuse, or let
       the user decide);
     - the runtime guard's rules per rule, and whether a user may release a quarantine
       (roadmap, section 4, "Modes per rule");
     - the minimum launcher version.
   - **The launcher shows it.** The mode line says "Managed by your organisation · N approved
     authors", Settings lists the policy, and the install screen marks approved authors.
   - **Dry run.** A policy value that only logs what would have been refused, so IT can roll the
     policy out without breaking anyone.
   - **Tests** in `flow.rs` with a policy given through a test hook instead of the real
     registry. Then on Windows: set the registry value by hand and check that an unapproved
     package is refused while an approved one installs.
   - **Later, not in this step:** the per-machine service and installs in Program Files (the
     WDAC part of the plan), an ADMX template, IT countersignatures, and a revocation list.
     Like the policy, the revocation list can live on a network share.
2. **Apply for signing:** SignPath Foundation (free for open source), with Certum's open-source
   certificate as the fallback. Then sign in CI ([`roadmap.md`](roadmap.md), step 3).
3. **Phase 0 of [`plan.md`](plan.md):** install and run on a locked-down company PC
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
runner starts Python with `-I -S`. Run `cargo test` from a shell where `uv` is on PATH.

## Layout

| Path | What |
|---|---|
| `crates/devstribution-core` | Package format: manifest, `FILES.sha256`, signatures and fingerprints, packing, safe extraction, lockfile checks (`lock::check_sources` is the install gate), the code scan (`scan`), `.trust` files (`trust`) |
| `crates/devstribution-host` | Install store (pins, anti-downgrade, `admit`), managed mode's policy (`policy`), uv environments (hardened `uv::sync`), supervised runs (Job Object), runner, history, secrets, data folder |
| `crates/devstribution-cli` | `devstribution init / dev / pack / keygen / verify / scan / trust` |
| `runner/` | Stdlib-only Python runner started by the launcher; protocol in `devstribution_runner/_runner.py` |
| `launcher/src` | TypeScript UI, no framework. `style.css` holds the design tokens, documented in [`design.md`](design.md); `look.ts` handles the theme and colour choices. |
| `launcher/src-tauri` | Rust commands over the host crate, `capabilities/` (what the window may call), `build.rs` (command list, DLL load flag), `windows/hooks.nsh` (uninstaller), icons |
| `examples/` | `excel-report` (form), `hello-gui` (gui, tkinter) |
| `docs/` | `plan.md` (design), `roadmap.md` (to 1.0), `format.md` (package spec), `security.md` (threat model, review), `design.md` (look), `report.md` (Windows findings), `status_quo.md`, `managed-mode.md` (for IT) |

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
