# Roadmap: from proof of concept to a signed 1.0

The proof of concept works end to end (see [`status_quo.md`](status_quo.md) and the Windows findings
in [`report.md`](report.md)), and a first real app has been delivered with it. This is the work
between that and a launcher we can sign and hand to people who will never read this repository.

Order matters: each step makes the next one meaningful. Signing an installer that ships an unpinned
uv, or packages that can carry client data, would put our signature on the wrong thing.

Status: ☐ to do · ◐ in progress · ☑ done

## 1. Package integrity

What goes into a `.dsapp`, and what the launcher runs it with, must be exactly what we intend.

- ☑ **`pack` leaves out what it should.** A `.dsappignore` file (`.gitignore` syntax) next to
  `pyproject.toml`, plus the app folder's own `.gitignore`. Files that look like data
  (spreadsheets, databases, archives, logs) are refused unless a `!pattern` lists them on purpose,
  so client data cannot end up signed by accident. `pack --list` shows what would be packed.
  (From the first real packaging: `app_1/output/` nearly went into `ricon.dsapp`.)
- ☑ **Pinned uv.** The launcher ships a fixed uv release, downloaded and checked against its sha256,
  instead of whatever uv is on the build machine's PATH.
- ☑ **Package format 1 is frozen.** A written specification ([`format.md`](format.md)) and a promise:
  every later launcher installs schema 1 packages; a package needing a newer launcher says so
  plainly instead of failing with a parse error.

## 2. Windows checks (by hand, on a real PC)

The parts that compile on Windows but have never run there. Each gets a result in `report.md`.

- ☐ Cancel, timeout and closing the launcher kill the whole process tree (Job Object).
- ☐ Excel started through COM is closed after a run, also after Cancel.
- ☐ Secrets are saved to and read from Credential Manager.
- ☐ `gui` apps start with `pythonw.exe`, without a console window.
- ☐ The new UI fixes (menu, uninstall dialog, no sideways scrolling) in the real launcher.

## 3. Installer and signing

- ☐ `npx tauri build`: per-user NSIS installer, no admin rights needed.
- ☐ File association (double-click a `.dsapp`) and single instance, from the installed launcher.
- ☐ Mark-of-the-Web: a `.dsapp` downloaded with a browser shows "Downloaded from the internet".
- ☐ Code signing in CI (SignPath for open-source projects), so SmartScreen and AppLocker/WDAC see a
  known publisher.
- ☐ Phase 0 of [`plan.md`](plan.md): install and run on a locked-down company PC.

## 4. Updates and key lifecycle

- ☐ The launcher updates itself (Tauri updater, signed update feed).
- ☐ App updates: a newer `.dsapp` already updates in place; add a clear "update available" path.
- ☐ Publisher key rotation: a signed statement from the old key that names the new one, so a
  publisher can move keys without every user reinstalling.
- ☐ Lost or compromised key: a documented way out (forget a pinned publisher in Settings, with a
  strong warning), and revocation in the catalog later.

## 5. User experience

- ☐ Uninstall easy to find (report: Launcher UX 1).
- ☐ An output type for "a folder the user chose", so stateful apps get an "Open" button
  (report: packaging 2).
- ☐ Error messages reviewed end to end: every failure says what happened and what to do.
- ☐ About page with version, licences of bundled software, and where the data folder is.
- ☐ Navigation sidebar polish.

## 6. Release engineering

- ☐ Security review of install and run (extraction, signature checks, process supervision, the
  launcher's IPC surface and CSP).
- ☐ Logs a user can find and send (a "Copy diagnostics" button), without secrets in them.
- ☐ Versioning, changelog, release notes; `docs/HANDOFF.md` and `status_quo.md` folded into a
  contributor guide.
- ☐ Docs for app authors: from `devstribution init` to a delivered `.dsapp`.

## After 1.0

Phase 2 of the plan: managed mode (company policy in HKLM), catalog with GitHub namespace
verification, `devstribution://` links, revocation; the `web` and `background` app kinds; `resources`
downloads (refused at install today).
