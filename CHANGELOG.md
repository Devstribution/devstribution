# Changelog

What changed in each release of devstribution: the launcher, the `devstribution` CLI and the
package format. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/). Before 1.0, a minor version (0.2, 0.3)
may change behaviour; the package format stays at 1 whatever the version (see
[`docs/format.md`](docs/format.md)).

How a release is made: [`docs/releasing.md`](docs/releasing.md).

## [Unreleased]

### Changed

- `devstribution scan` and `devstribution diff` exit with 2 when a package could not be read, so
  CI can tell it from 1, "danger found". Their other behaviour is unchanged.
- The install window can be cancelled with its button or Escape at every step: while the
  package is checked, on the confirmation screen, and while Python and the packages are
  prepared. Cancelling then stops uv and everything it started (it runs in a Job Object, like
  apps), keeps nothing half-made, and leaves an installed older version as it was. Repair's
  progress has a Cancel button too; the app keeps its old environment.
- Preparing an app's Python stops after 30 minutes, with a message about the connection or a
  proxy, instead of waiting forever.
- A launcher that cannot set itself up shows "devstribution could not start", with the reason and
  "Try again", instead of a blank window. Several messages now say what to do: a damaged
  `state.json`, an install that is no longer pending, uv failing (usually the internet connection or
  a proxy), a Python that will not start, the credential store.
- When an app fails, the launcher says what to try. For errors that usually come from the PC (a
  file open in Excel, a moved file, no connection, a missing part of the app's Python, no memory)
  it says what to do. For anything else, it says the problem is most likely in the app, and offers
  "Copy details" (the error and traceback) to send to the author. The host's remaining messages
  (data folder, output folders, the runner, moving an app or environment into place, the Windows
  calls that watch a run) also say what to do now.
- The launcher works from the keyboard and with a screen reader: dialogs keep the focus inside
  and give it back when they close, the `⋯` menu and the segmented controls take arrow keys,
  clicking a choice no longer drops the focus, and run progress, results and install steps are
  announced. Error messages stay until you close them. With Windows' animation effects turned
  off, the launcher does not slide or fade either.
- `devstribution trust export` asks before replacing an existing file. Without a terminal to ask
  on (CI, a pipe) it refuses; `--force` replaces it.

### Fixed

- The launcher says what happened instead of staying quiet: an empty launcher says "No apps yet";
  opening or dropping several packages says only the first is installed; cancelling the secret
  prompt says the app was not started; "Forget" asks before removing a secret; Repair shows each
  step; a run with no log file says so, instead of looking like one the app left empty.
- The update comparison orders package versions like `1.0rc1`, `1.0.post1` and `1.0.dev1` the way
  PEP 440 does, so a downgrade to a pre-release is marked "older" (it was only for plain numbers).

### Added

- devstribution updates itself. When it starts it looks for a newer version on GitHub and offers
  it with what is new; "Update and restart" downloads it, checks it against devstribution's own
  update key, and installs it without administrator rights. Not while an app is running. Settings
  has "Look for updates" and a switch for the check at start; IT can turn updates off with the
  `LauncherUpdates` policy value. This version is the first that can update: earlier ones are
  updated by hand once.
- Apps can raise `devstribution_runner.AppError("…")` with a message for the user: the launcher
  shows it as written, with no traceback and no "send it to the author".
- Settings has an About block (version, licence, where the log is, and the software inside
  devstribution with its licences; "Open all licence texts" opens `THIRD-PARTY-NOTICES.txt`, which
  the installer ships) and a "Copy diagnostics" button: versions, installed apps, the latest runs,
  the state of the run journal and the end of the launcher's log, as text for a bug report or IT.
  It holds no secrets, inputs, results or keys, and writes your home and data folders as `~` and
  `<data>`.
- The launcher keeps a log, `logs\launcher.log` in the data folder (about 512 KB, then one older
  file): when it starts, commands that failed, panics, and runs that could not be recorded.
- Settings → Storage → "Clean up…" removes the results and logs of old runs to free room. It shows
  how much it frees first (by age and app) and asks before deleting. The runs stay in Activity,
  marked as cleaned, unless you also choose to remove them; folders you chose for an app are never
  touched. Every finished run and every cleanup is also written to `state\journal.jsonl`, a chain of
  hashed lines, so a missing or edited run can be told from what a cleanup did ("Cleanup log").
- Settings → Storage has an "Open folder" button for the data folder, for support and for IT.
- `CONTRIBUTING.md`, and issue and pull request templates.

## [0.2.0] - 2026-10-27

A hardening release from a scan of the whole codebase. Still a preview, **not code-signed yet**.

### Changed

These can change what an existing app does: try yours with this version before updating other
PCs to it.

- Apps get only an allowed list of environment variables (see Security below). An app that read
  a token or setting from the user's environment must get it another way, for example from a
  secret saved in the launcher.
- Python runs isolated (`-I -S`): the user's own site-packages and `PYTHON*` variables are
  ignored.
- An app that starts Python through a shell, `os.system`, `os.spawn*` or the `py` launcher is
  stopped. Start it with `subprocess` and `sys.executable` instead, which runs under the guard.

### Security

- Apps no longer see the launcher's whole environment: only Windows' own variables, the user's
  folders, the proxy and the locale. Tokens and keys in the user's session stay out.
- An app declaring `network = ["*"]` gets a warning before install: it turns off the runtime
  guard's network rule.
- The runtime guard treats moving or linking a protected file as reading it.
- A guard stop that cannot be saved (disk full) keeps the app blocked, and the launcher says so.
- The runner is checked before every run, not only when the launcher starts.
- Install checks the whole lockfile, as `pack` does, and checks the staged files again just
  before they are moved into place.
- The launcher ends only processes Windows started as COM servers, never a program the user
  opened.
- Another Python an app starts (`subprocess`, `multiprocessing`) runs under the runtime guard
  too, and a stop there ends the run. A Python started any other way ends the run.
- The code scan reads names and strings as Python does (fullwidth letters, `\x` escapes) and
  catches calls passed around as values or reached through `getattr`.

## [0.1.0] - 2026-10-26

The first public release: a preview for trying devstribution out, **not code-signed yet**. Windows
warns before the installer runs, and it is not meant for company PCs yet.

### The launcher (Windows)

- A per-user installer, no administrator rights needed. Double-clicking a `.dsapp` opens it in the
  launcher, also when a download renamed it to `.zip`.
- Before an install: the publisher's key, the permissions the app declares, and a scan of its code
  and dependencies (packages reported as malicious, look-alike names, code that runs when Python
  starts), with a red banner for what looks like malware.
- Updates show what changed since the installed version, and say so when the new version asks for
  more than before.
- Every run is watched: undeclared network or SAP use is blocked, and what malware does (starting
  with Windows, reading saved passwords, switching off the antivirus, changing many of your files,
  and more) stops the run and blocks the app until you decide. After each run, "What it did" lists
  the files it changed, the hosts it contacted and the programs it started.
- Each app gets its own Python environment, built by a pinned, hash-checked uv; runs are supervised
  (cancel, timeout, the whole process tree ended), and secrets are kept in Windows Credential
  Manager.
- Home, Apps and Activity, light and dark themes, a choice of accent colours and colour-blind safe
  status colours.
- **Managed mode** for IT: a policy in the registry (GPO or Intune) decides which authors may be
  installed, with a dry run. See [`docs/managed-mode.md`](docs/managed-mode.md). Not yet checked on
  a real company PC.

### The CLI (Windows, Linux, macOS)

- `init`, `dev`, `pack`, `keygen`, `verify`: from a Python folder to a signed `.dsapp`. `pack`
  refuses files that look like client data unless listed on purpose (`pack --list` shows what
  goes in).
- `scan`: the launcher's code and dependency scan, with exit code 1 on danger and `--json` for CI.
- `diff`: what changed between two versions of an app, for a review before an update.
- `trust export` / `trust show`: the `.trust` files IT lists in managed mode.

### Known limits

- Not code-signed: SmartScreen warns, and AppLocker or WDAC may refuse the launcher.
- Not yet tried on a locked-down company PC or behind a company proxy.
- The launcher does not update itself: install a newer version over it.

[Unreleased]: https://github.com/Devstribution/devstribution/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/Devstribution/devstribution/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Devstribution/devstribution/releases/tag/v0.1.0
