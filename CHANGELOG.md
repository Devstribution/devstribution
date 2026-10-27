# Changelog

What changed in each release of devstribution: the launcher, the `devstribution` CLI and the
package format. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
versions follow [Semantic Versioning](https://semver.org/). Before 1.0, a minor version (0.2, 0.3)
may change behaviour; the package format stays at 1 whatever the version (see
[`docs/format.md`](docs/format.md)).

How a release is made: [`docs/releasing.md`](docs/releasing.md).

## [Unreleased]

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
