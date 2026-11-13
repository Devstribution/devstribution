# Changelog

What changed in each release of devstribution: the launcher, the `devstribution` CLI and the
package format. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/). Before 1.0, a minor version (0.2, 0.3)
may change behaviour; the package format stays at 1 whatever the version (see
[`docs/format.md`](docs/format.md)).

How a release is made: [`docs/releasing.md`](docs/releasing.md).

## [Unreleased]

### Added

- **The proxy Windows is set to**: when `HTTPS_PROXY` is not set, preparing an app asks Windows
  which proxy it would use (a setup script (PAC), automatic detection (WPAD), or a fixed proxy
  and its exceptions) and downloads Python and the packages through it. Checking for launcher
  updates does the same. "Copy diagnostics" says which proxy is used and where it came from.
- **The company's package mirror** (managed mode): the policy's `PackageIndex` names a PyPI
  mirror (Artifactory, Nexus, devpi), and apps' packages are downloaded from it instead of PyPI.
  Each file must still have the hash the signed lockfile names, so a mirror cannot change what
  is installed. "Copy diagnostics" names it, and so does a failed download's message.

### Fixed

- **The CLI on a PC without the Visual C++ runtime**: the Windows `devstribution.exe` of a
  release now carries the C runtime it needs, instead of failing to start on a clean PC.
- **Python's download hosts**: uv now downloads Python from `releases.astral.sh` first, then
  GitHub. The proxy is asked about those hosts, a failed download names them, and the managed
  mode guide lists them for the network allowlist.
- **Clearer messages when preparing an app fails**: a download that was not the expected file
  (damaged or changed on the way, or on the mirror) says so and points at the company's mirror
  when one is set. When AppLocker or WDAC stops Python from starting, the message says so and
  names the folders IT has to allow, for programs and DLLs alike.

## [0.6.0] - 2026-11-09

### Added

- **Run on a schedule**: a form app can run on its own every day, Monday to Friday or once a
  week, with the inputs saved with the schedule (the app's ⋯ menu). Windows Task Scheduler starts
  devstribution for the run, even when it is closed; a time missed while the computer was off
  runs when it is next on. The run has the same checks and runtime guard as one started by hand.
  In managed mode it is off unless the policy's `ScheduledRuns` is `1`.
- **A notification when a run ends** while devstribution is not in front: finished, didn't work
  (with the first line of the error), cancelled, or stopped by devstribution.
- **Add to Start menu** (the app's ⋯ menu): a shortcut, with the app's icon, that opens
  devstribution on that app. Removed with the app, and by the uninstaller.
- Managed mode: `PythonMirror`, where Python itself is downloaded from when GitHub is blocked.
- **Excel results and pictures in the launcher**: an `.xlsx` result shows its first visible
  sheet as a table (dates as dates, numbers as Excel shows them), as CSV results already did; a
  PNG or JPEG result shows as a picture, which opens full size when pressed.
- **What's new, after an update**: the first time devstribution starts on a newer version, a
  notice offers what changed since the version you had (every version in between too). It stays
  until you open it or close it with ×. Settings → About shows this version's notes any time.
  The notes are built into devstribution, so this also works with no connection and after an
  update IT installed.
- **`ctx.data_dir`** for app authors: the app's own folder, kept from one run to the next and
  through updates (settings, caches), removed when the app is uninstalled. No other app can read
  it, and code in it is never run.
- **Pack and sign in GitHub Actions** (`actions/pack`): packs, verifies and scans an app with the
  publisher key from a repository secret, using the released CLI after checking it. The workflow
  to copy is in `docs/authors.md`.

### Changed

- `pack` and the other commands that use a key with a password fail at once, saying to set
  `DEVSTRIBUTION_KEY_PASSWORD`, when there is no terminal to ask for it (CI, scripts), instead of
  waiting.
- `CONTRIBUTING.md` now holds what `docs/HANDOFF.md` and `docs/status_quo.md` held: setup, the
  layout and what to know before changing code. The two pages are gone.
- A failed download says what could not be downloaded (Python or the app's packages), from which
  host, why (the name was not found, the proxy refused, the certificate was not trusted, a proxy
  that wants a Windows login, a mirror without the file), the proxy in use, and what to ask IT
  for. "Copy diagnostics" has a Network section: the proxy (without its password) and where
  Python and packages come from.

### Security

- **"Open result" no longer opens documents that run code of their own.** Help files (`.chm`),
  Excel add-ins (`.xll`, `.xlam`), Office files with macros (`.xlsm`, `.docm`, `.pptm` and the
  rest), app installers (`.msix`, `.appinstaller`), troubleshooters, Remote Desktop and theme
  files, and disk images are shown in their folder instead, like programs and scripts already
  were. An app the guard keeps from starting programs could otherwise have left one as a result
  for you to press.
- **A package's signature is checked before anything in it is unpacked.** An unsigned package,
  or one signed with another key than its publisher's, now writes nothing to disk, and only the
  files its signed list names are unpacked. Before, such a package was refused only after it had
  been unpacked, up to the 2 GB limit.
- **The Excel preview has the same limits as packages**: a workbook over 256 MB, with more than
  10,000 entries in its zip, or with more shared strings than a sheet could show is not shown,
  so a file built to fill memory cannot slow the launcher down.

## [0.5.0] - 2026-11-06

### Added

- A CSV result shows its first 50 rows as a table in the launcher, under the file, with how many
  rows it has: no need to open Excel to look.
- `scripts/company-pc/setup.ps1` and [`docs/company-pc.md`](docs/company-pc.md): a Windows VM set
  up like a locked-down company PC (standard account, AppLocker, a TLS-inspecting proxy, trust
  folders on a share), and what to check there.

### Changed

- Runs start faster: an app's code starts ~0.2 s sooner on every run (0.18 s instead of 0.40 s
  for an app without dependencies), and the first run after an install no longer compiles the
  dependencies (pandas: 1.9 s instead of 4.1–4.7 s). Installs take that time instead, 0.7–2.4 s
  more. The standard library's compiled code is kept in the data folder; the runner's own is
  still compiled from its checked source on every run.
- The launcher's own updates install without showing the installer's window: devstribution
  closes and opens again on the new version. (The update to 0.5.0 still shows it, as the
  version doing the update decides.)
- One run at a time per app: Run waits until the app's current run has finished or was stopped.
- Plainer words about trust: no signing keys on the install screen or in the main window, and a
  short explanation of publishers and trust before the first install.

### Fixed

- A table or text result over 16 KB ended the run "without a result". Results up to 256 KB are
  kept whole; a longer one shows its start, and all of it is saved in the results folder (a CSV
  for a table), one click away.

### Security

- An app that declares no network can no longer reach it by starting another program: `curl`,
  `wget`, `bitsadmin`, `ftp`, `tftp`, `ssh`, `scp`, `sftp`, `telnet` and PowerShell's web commands
  count as network use. The call fails like a Python connection would; one started behind the
  runner's back ends the run. From the technical assessment of 5 November
  ([`docs/security.md`](docs/security.md)), like the items below.
- Programs that dump or list saved passwords (an LSASS dump, `reg save HKLM\SAM`, Wi-Fi keys,
  `cmdkey`, `vaultcmd`), add user accounts, open files to everyone or clear the event logs now end
  the run.
- A database opened through `_sqlite3` instead of the `sqlite3` module is refused: it could read
  any database file unseen. Apps that use `sqlite3`, pandas or SQLAlchemy are not affected.
- Python code a run wrote, then imported or ran, ends the run like a program it wrote: the
  install scan never saw it.
- The guard reads a path through one of this PC's own shares (`\\localhost\data\…`) as the
  folder it is.
- Managed mode: a trust folder reached through a junction a user made, or whose real location
  users can rename, now approves no one.

## [0.4.0] - 2026-11-04

A security release: **update if you use 0.1.0, 0.2.0 or 0.3.0**, which all have the weaknesses
listed under *Security*. From 0.3.0 the launcher offers this version itself; from 0.1.0 or 0.2.0,
install it by hand over the old version (apps, results and secrets are kept). On a PC managed
with a policy, a trust folder that users can change, or that sits in a folder they can rename,
now approves no one: **check Settings after updating**, and tighten the folder's permissions if
it shows a problem. Also new: revoked keys, and app updates from folders you choose. Still a
preview, **not code-signed yet**.

### Changed

- The code scan raises fewer false alarms, measured on about 13,000 files of ordinary open-source
  Python: words that only contain a flagged name (`kid_rsa`) and paths in documentation no longer
  count, and "pip install" in a message is only a warning when the app can start programs.
  `[..., "-m", "pip", "install", ...]`, which it missed, now counts.

### Security

From the security review of 30 October ([`docs/security.md`](docs/security.md)).

- Managed mode: the check that users cannot change the trust folder never worked for folders, and
  it asked for several rights at once, so a user holding only one passed. A folder or file users can
  change in any way (add, replace, delete, change permissions) now approves no one. **After
  updating, check Settings on a managed PC**: a trust folder with loose permissions now shows a
  problem, and its authors are no longer approved.
- Managed mode: an approved app keeps running only while the approved authors cannot be reached. A
  broken `.trust` file or policy file no longer keeps a removed author's apps running.
- The runtime guard reads a path as Windows does: 8.3 short names, NTFS streams and `file:` database
  names no longer get past its rules for saved passwords.
- A secret's value is taken out of a run's log, result and messages when the app prints or returns it.
- Uninstalling an app removes all its secrets, also ones a later version stopped declaring.
- What an app was allowed past the guard no longer carries over to an update signed with a new key.
- The code scan says so when it cannot read Python (a file that is not UTF-8, a `# coding:` line that
  can hide code, a very large file, f-strings nested too deep), and flags `.reg`, `.url`, `.inf`,
  `.xll` and Office files with macros. An update adding a second `exec` to a file that had one now
  shows as new.
- A package whose zip index is far too large is refused before the index is read.
- The CLI strips terminal escape codes from what it prints of packages and `.trust` files.
- A revocation list goes on applying after its signing key's end date.
- The runtime guard checks `ATTACH DATABASE` in SQLite (`ATTACH ?` is now refused: name the file in the
  statement) and reads admin shares (`\\localhost\c$\...`) as the disk path.
- Managed mode: a trust folder or policy file is refused when a user can rename or take over a folder
  above it. **Check Settings on a managed PC after updating.**
- The code scan reads f-strings as Python 3.12 does, and is no longer misled by a `}` in a string or a
  quote in a format spec.
- A revocation entry for a single package also applies to apps installed before full content hashes
  were kept.
- The check of update folders looks at packages only, the newest first, for 30 seconds at most, and
  says when it did not get through a folder.

### Added

- Revoked keys. A publisher whose key is lost or stolen can withdraw it
  (`devstribution keys withdraw`), and one who moves to a new key can hand over to it
  (`devstribution keys rotate`); the launcher takes these statements from Settings, *Revoked
  keys*, *Import*. Packages signed with a withdrawn key are refused, apps it signed ask once
  before they run, and an update signed with the new key of a rotation installs as the same
  publisher. Signed revocation lists are checked the same way, once devstribution publishes them.
- App updates in folders you choose. Settings, *App updates*, takes folders (a network share, say)
  that are checked at start; a newer version signed by the app's publisher shows on the app, and
  installing it goes through the usual install screen and its list of changes. IT can set a folder
  with the policy. A package signed with a revoked key is not offered.

### Fixed

- A crash or a full disk in the middle of saving the list of installed apps no longer blocks the
  launcher: the list is written to disk before it replaces the old one, a copy of the previous list
  is kept (`state.json.bak`), and a damaged `state.json` is set aside and replaced by that copy.

## [0.3.0] - 2026-10-28

The first version that updates itself: later versions arrive from inside the launcher. Getting
here from 0.1.0 or 0.2.0 is one last install by hand, over the old version (apps, results and
secrets are kept). Also: Cancel while an app's Python is prepared, error messages that say what
to do, keyboard and screen reader basics, and a guide for app authors. Still a preview, **not
code-signed yet**.

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
- A guide for app authors, from `devstribution init` to a delivered `.dsapp`:
  [`docs/authors.md`](docs/authors.md). The `AGENTS.md` that `init` writes links to it and
  explains `AppError`.
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

[Unreleased]: https://github.com/Devstribution/devstribution/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/Devstribution/devstribution/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Devstribution/devstribution/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Devstribution/devstribution/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Devstribution/devstribution/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Devstribution/devstribution/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Devstribution/devstribution/releases/tag/v0.1.0
