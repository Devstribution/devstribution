# Status quo: 22 October 2026

Where the project stands. What the next session does is in [`HANDOFF.md`](HANDOFF.md), and the
plan to a signed 1.0 is in [`roadmap.md`](roadmap.md).

> **Update, 23 October:** the security PR and the code scan are merged. Since then: the runtime
> guard, the dependency check, "Uninstall devstribution" in Settings, `.zip` downloads, and the
> first version of **managed mode** (the policy from the registry, `.trust` files, a block
> message with a request for IT; see [`managed-mode.md`](managed-mode.md)). Managed mode still
> needs checking on a real Windows PC. The rest of this page is from 22 October.

## In one paragraph

The proof of concept from [`plan.md`](plan.md) became a proper app:
- the package format is specified and frozen;
- packing keeps client data out;
- the launcher ships a pinned uv;
- the launcher has a new design with a written design language and a new logo.

A full security review then ran before code signing. Every finding is fixed, and the fixes are in
the open security PR. A first real app (`ricon.dsapp`) was delivered with the proof of concept.
The launcher is **not signed yet**, so it isn't ready to hand out.

## What happened, PR by PR

| PR | What |
|---|---|
| [#3](https://github.com/Devstribution/devstribution/pull/3) | The proof of concept: core, host, CLI, runner, launcher, examples, CI on Windows/macOS/Linux |
| [#4](https://github.com/Devstribution/devstribution/pull/4) | Fixes from the first Windows run ([`report.md`](report.md)) |
| [#5](https://github.com/Devstribution/devstribution/pull/5) | Roadmap, plus package integrity: `.dsappignore`, the data guard and `pack --list`; the pinned, hash-checked uv sidecar; format 1 specified and frozen with a golden package |
| [#6](https://github.com/Devstribution/devstribution/pull/6) | Launchpad design:<br>- Home: two recent, two often-used apps<br>- Apps: filters<br>- Activity: dashboard<br>- apps open in a side panel; Settings sits behind a round button<br>- design language in `style.css` and [`design.md`](design.md); solid fills instead of tints<br>- Settings: accent colour, colour-blind safe status colours |
| [#7](https://github.com/Devstribution/devstribution/pull/7), [#8](https://github.com/Devstribution/devstribution/pull/8) | New flat logo, and sharp small icons (each size rendered directly) |
| [#9](https://github.com/Devstribution/devstribution/pull/9) | Plan: IT review kit (review report, Windows Sandbox trial, approval by countersignature) |
| Security PR (open) | The pre-signing security review and all its fixes ([`security.md`](security.md)) |

You reported that the Windows checks after PR #5 worked. The details aren't in `report.md` yet.

## The security review (open PR)

Three independent reviews (package format and verification; install and runs; launcher and
installer) plus a dependency and CI audit. The most important fixes:

- **Critical:** the package is read once. A file swapped between two reads could install over
  another publisher's app.
- **High:**
  - uv ignores everything a package or the user's session could configure;
  - `uv.lock` must name only PyPI wheels with hashes, checked at install;
  - the key, version and pin checks run again when an install is confirmed;
  - an app's output is read as bytes, so an accented character can no longer kill a run.
- **Medium:**
  - apps join their Job Object before they run (Windows);
  - the runner is checked before every run, with one bytecode cache per app;
  - environments are built aside and swapped in, and nothing is changed while an app runs;
  - single-instance comes before any data access;
  - run events are batched and Cancel stays clickable;
  - "Open result" opens only recorded results, never programs;
  - the data folder moved out of the install folder, and the uninstaller removes secrets;
  - DLL loading is restricted;
  - keys are shown by SHA-256 fingerprint.
- **Low:**
  - stricter file names and versions (format 1 amended once, before any release);
  - results and COM process IDs re-checked;
  - Mark-of-the-Web fails safe;
  - the window may call only the launcher's own commands;
  - a tighter CSP.
- **Dependencies and CI:**
  - zip 9; Tauri 2.12 on the Rust and JavaScript sides;
  - `cargo audit` and `npm audit` clean and running in CI;
  - actions pinned to commit SHAs;
  - Dependabot.

Stated limits, kept on purpose and written in [`security.md`](security.md):
- Programs running as the same user can change per-user data; per-machine mode is the answer.
- App permissions are declared, not enforced.
- A new namespace is trusted on first use.
- A local administrator can change anything.

## Parts

| Part | State |
|---|---|
| `devstribution-core` | Format 1 with the amendment, install gate for `uv.lock`, fingerprints. 32 tests + frozen-package test |
| `devstribution-host` | Install store with `admit` checks, hardened uv, crash-safe environments, runs with Job Object and result checks. 5 unit + 3 end-to-end tests with real uv |
| `devstribution-cli` | `init`, `dev`, `pack` (same checks as install), `keygen`, `verify`. 2 integration tests |
| `runner/` | Stdlib-only, protocol v1. 16 tests |
| `launcher/` | Launchpad design, Settings with theme and colours, minimal window permissions. Release build verified on Linux: install, app panel, Settings, uninstaller mode |
| CI | Windows, macOS, Linux: tests, launcher build and clippy, audits |

## Not done yet

**Windows only**

Built and type-checked for Windows but never run there:
- suspended start in the Job Object;
- the DLL load restrictions;
- the data folder move;
- the uninstaller hook;
- uv behind a company proxy.

The same goes for these, which only the installer can test:
- the file association;
- Mark-of-the-Web from a real download.

**Installer and signing**

`npx tauri build` on Windows, then SignPath (or Certum).

**Your real app**

Repack it with the new rules and reinstall it.

**Later (roadmap)**

- updates and key rotation;
- an output type for "a folder the user chose";
- error messages reviewed end to end;
- an About page with licences;
- release engineering;
- after 1.0: managed mode, catalog, `devstribution://` links, revocation, the IT review kit, the
  `web` and `background` kinds.
