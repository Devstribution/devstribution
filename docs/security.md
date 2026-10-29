# Security: threat model and pre-signing review

What devstribution defends against, how, and where it stops. It also records the security review
done before the launcher is code-signed (October 2026). Signing puts the project's name on the
launcher, so this page lists what it vouches for and what it does not.

Report vulnerabilities privately, as [`SECURITY.md`](../SECURITY.md) explains.

## What is protected, and from whom

**The promise** ([`plan.md`](plan.md), "Principles"): what you run is exactly what its publisher
published, a publisher cannot pass for another, and an app cannot tamper with the platform or other
apps, within the limits listed below.

| Threat | What stops it |
|---|---|
| A package changed after signing: a file added, removed, edited, or its key swapped | Every file is hashed into `FILES.sha256`, which is signed; extraction refuses unlisted, missing and modified files and checks the signature against the carried key ([`format.md`](format.md)). |
| A package that writes outside its folder when extracted | Strict path rules (no `..`, absolute paths, `:`, `\`, device names, links, invisible characters, 8.3 aliases, case clashes); files created with "create new", never overwritten. |
| A zip bomb or a huge package | Size and file-count limits counted on the bytes actually extracted; oversized files refused before the zip index is read. |
| Publisher A shipping or overwriting `B/app` | The namespace's key is pinned at first install; later packages for it must carry the same full key. Checked when the package is inspected *and again* when the install is confirmed. |
| A package swapped on disk between the check and the install | The launcher reads the package once, into its own staging folder, and installs those very files. Just before they move into place, the staged files are hashed again against the signed list, and the confirmation screen's record is compared with the copy the launcher kept in memory: anything that wrote the staging folder in between is refused. |
| Going back to an older, vulnerable version | The highest installed version per app is remembered; lower versions, and the same version with other content, are refused (also checked again at install). |
| Code running at install time | Install only extracts, verifies, and runs `uv sync --no-build` from the signed lockfile: no `setup.py`, no build backend. Nothing from the package runs before the user confirms. |
| A package making uv fetch or install something else | `uv.lock` must name only wheels from PyPI's file host, over HTTPS, with SHA-256 hashes, and for every platform the app declares a wheel of each package (nothing built from source): the same checks as `pack`, run again at install. uv runs with `--no-config` (no `uv.toml` or `[tool.uv]` from the package), a cleaned environment (no `UV_*`, `PIP_*`, `PYTHON*` from the session), its own Python builds (`--managed-python` with an explicit version, so no `.python-version` from the package) and the system certificate store. Release launchers use only the uv they ship, pinned and hash-checked at build time. |
| A look-alike publisher key | Keys are shown by a SHA-256 fingerprint of the whole key, not minisign's 8-byte key number, which anyone can choose. Trust decisions compare the full key. |
| An app's output or result tricking the launcher | Output is read as bytes (invalid UTF-8 cannot stop the reading), lines and logs are capped, events reach the window in batches. Results are re-checked by the launcher: only declared outputs, file outputs only inside the run's output folder or a folder the user chose for that run's `folder` inputs. "Open result" opens only a recorded result, the run's output folder or a folder chosen for the run, by its resolved path, and only shows programs, scripts and shortcuts in their folder. |
| An app making the launcher end other processes | COM processes (Excel) an app reports are only ended if they started during the run, are still the same process, and were started by Windows as COM servers (`-Embedding` on their command line): an Excel the user opened is never ended because an app named it. |
| An app's processes surviving Cancel or the launcher closing (Windows) | The app starts suspended and joins a Job Object with kill-on-close before it runs; a run that cannot get one does not start. |
| An app tampering with the runner other apps load | Before every run the runner folder is compared with the copy built into the launcher and rewritten if a file differs or was added (at each run's start, not only when the launcher starts). Python starts with `-B`, so no `__pycache__` is written in the runner's folder; a compiled file planted there is found by the check and removed. Each installed app version has its own bytecode cache. |
| Package or run data running script in the launcher window | Everything reaches the page as text; no `innerHTML` with data; strict CSP (`script-src 'self'`, no objects, frames, forms or base URL); icons only as `data:` images. |
| The launcher window being used to do more than the launcher needs | The window may call only the launcher's own commands, event listening, the file picker and yes/no questions (no `core:default`). The backend checks every argument (ids, run ids, secret names, paths). |
| A DLL planted next to the signed launcher (Windows) | Imported DLLs load from System32 only (`/DEPENDENTLOADFLAG`), others never from the current folder or PATH (`SetDefaultDllDirectories`). The launcher's data is kept out of its install folder. |
| An app whose code does more than it declares, or hides what it does | Before the install button, the code scan lists what the code uses and flags undeclared network or SAP use, programs and compiled files that cannot be reviewed, code built at run time, and what is typical of malware (decode-and-run, start-with-Windows entries, browser password stores, antivirus switches, process injection, hidden text-direction characters). Names are read as Python reads them (NFKC, so `ｅｖａｌ` is `eval`), escapes in strings are decoded (`"\x73ystem"`), and a call passed around as a value (`f = exec`, `map(eval, …)`, `getattr(os, "system")`) counts as the call. `network = ["*"]`, which turns the guard's network rule off, is a warning of its own. A red banner and "Install anyway" when something looks like malware. The scan reads, never runs, the code; see limit 2. |
| A dependency that is known malware, or named like a popular package to be installed by mistake | The dependency check reads `uv.lock` at install, in `pack` and in `devstribution scan`: a package (or the locked version of one) reported as malicious on PyPI is a danger finding, with the red banner and "Install anyway"; a name one edit away from one of the 1,000 most downloaded packages is a warning. The lists are bundled (OSV's `MAL-` entries and the top 15,000 packages), so nothing is sent anywhere; the screen says up to which date the reports are known. |
| A quiet malicious update from a publisher users already trust | The update screen compares the package with the installed version: new permissions (an amber banner and "Update anyway"), new code scan findings, new or older packages, new programs and compiled files, then every changed file. `devstribution diff` prints the same, with the changed lines, for a reviewer. It compares packages by version, not their code. |
| An app doing more than it declared, or what malware does, while it runs | The runtime guard: an audit hook in the runner, installed before app code and before the environment's `site-packages` (so the code a dependency's `.pth` file runs at start-up is checked too, and reported), blocks network use the app does not declare (and hosts it does not list) and SAP's RFC library without `sap`; it stops the run before the call on start-with-Windows entries (PowerShell profiles too), the antivirus's settings, browser password stores and other saved keys and passwords (SSH keys, Windows credentials, password databases, Outlook data files, wallets), the Windows tools malware uses, process injection, keyboard hooks, hidden compiled code, a program the run wrote itself, writes to Windows, Program Files, devstribution's data folder, another app or the app's own Python, reading another run's results, and more than 50 existing files changed outside the run's output folder, the user's choices and the temporary folder (ransomware). Files and folders the user chose through the app's inputs are the app's to read and change. Files changed elsewhere are reported, and each run ends with a line on the files it wrote and deleted, shown under the results ("What it did"). The launcher checks every process in the run's Job Object against the same rules. Another Python the app starts (`subprocess`, `multiprocessing`) starts through the runner's `child.py`, with the same guard installed before its code runs; a Python started any other way (a shell, `os.system`, `ctypes`, the `py` launcher, a second runner) ends the run, from the hook or from the Job Object. Moving or linking a file counts as reading it. A stop ends the whole tree and quarantines the app until the user decides; when `state.json` cannot be written, the stop is kept in memory, the app stays blocked and the user is told, and the stop is saved at the next chance. See limit 2 for what it does not see. |
| Credentials in the user's environment reaching every app | Runs start with a listed set of environment variables (Windows' own, the user's folders, the proxy, the locale); cloud keys, tokens and any other variable of the user's session are left out. Secrets reach an app only as declared. |
| A downloaded package without a warning | Mark-of-the-Web is read (also UTF-16); a mark that cannot be understood counts as "from the internet". |
| Managed mode: an author IT has not approved, or an approved namespace signed with another key | Only namespaces and full keys listed in the HKLM policy install and run; checked at inspect, again at install, and before every run. No trust on first use, no "Install anyway". |
| Managed mode: a user or an app pointing the launcher at its own policy | The policy is only read from `HKLM`, which users cannot write. Policy files, trust folders and `.trust` files the user could write are refused (Windows is asked whether the user could open them for writing). A policy that cannot be read approves no one. |
| A tampered CI pipeline (once it signs releases) | Actions pinned to commit SHAs, read-only token, `cargo deny` and `npm audit` on every change, Dependabot updates. |

## What is not covered (stated limits)

These are deliberate limits of the current design. Each one says why, and what would change it.

1. **Same-user programs.** In per-user mode (the default) the launcher, its data folder
   (`%LOCALAPPDATA%\dev.devstribution.launcher\data`), the installed apps and the credential
   store all belong to the user. Any program the user runs, including an installed app or
   malware, can therefore:
   - edit `state.json`: reset pins, reset the highest version;
   - change another app's files or its environment;
   - replace the runner between the launcher's check and Python reading it;
   - read secrets from Credential Manager, which has no per-app access control.

   The per-user launcher protects against *packages*, not against code already running as the
   user. The **per-machine mode** ([`plan.md`](plan.md), "Python installation modes") moves code
   and trust state into admin-only folders behind a service, and is the answer where this matters.
2. **Apps are not sandboxed.** Apps run with the user's rights. Trust in the publisher is the
   protection, which is why the confirmation screen is explicit about new publishers. Two layers
   narrow what a dishonest app can do without being noticed, and neither is a wall:
   - The **code scan** at install is a static reading of the package's own Python files: it does
     not read the code of dependencies (only their names and versions, against reports of
     malicious packages up to the bundled list's date, and look-alike names), and code that
     builds names or downloads code at run time can get past it. It is a prompt to ask
     questions, not a verdict.
   - The **runtime guard** ([`roadmap.md`](roadmap.md), section 4) sees what goes through
     Python's audit events, inside the app's own process. It does not see network use, files or
     programs reached through native code (`ctypes` calls to WinHTTP, COM objects such as
     `MSXML2.XMLHTTP`, C extensions), files Excel or another COM server writes for the app,
     SAP driven through SAP GUI's COM interface, or anything after code patches the
     interpreter's memory. Another Python the app starts runs under the same guard, with the same limits; on Linux and macOS (development only) a Python started through a shell or `os.posix_spawn` is not caught, as there is no Job Object there. It sees how much changes, not what: an app may still read the files
     it can reach and keep them in its results. The folder rules are checked from inside the
     app; Windows itself enforces nothing until the sandbox below. The Job
     Object check sees every program the run starts, but only once it has started, so a very
     short program can act before it is ended.

   After 1.0: the IT review kit ([`plan.md`](plan.md)) and a kernel-enforced sandbox
   (AppContainer) where the cost is reasonable.
3. **Trust on first use.** Outside managed mode, whoever first installs a namespace on a PC pins its
   key, and the name is marked "unverified". Catalog and GitHub-source verification (Phase 2)
   replace this with a verified binding.
4. **A local administrator** can change anything, including policy and the launcher itself.
   Managed mode assumes standard user accounts: a user who runs devstribution elevated can write
   the policy's files, which the launcher then refuses (it fails closed).
5. **Bugs in signed apps** are the publisher's. The platform makes them fail cleanly and alone
   (timeouts, Job Objects, per-version environments), not correct.
6. **Linux GTK libraries** that Tauri uses on Linux are flagged "unmaintained" by RustSec (`glib`,
   `zbus`, …). There is no known vulnerability, and the Windows launcher does not use them.
7. **Managed mode with the per-user launcher** protects users against packages IT has not
   approved, not against a user who sets out to get round it: the launcher's own files are in
   the user's folder. AppLocker or WDAC rules that allow only the signed launcher close this,
   as will the per-machine mode. Apps installed under the policy keep running while the
   approved authors cannot be read (a share out of reach), so that laptops work away from the
   office; nothing new installs meanwhile.

## Pre-signing review, October 2026

Three independent reviews (package format and verification; install and runs; launcher and
installer) plus a dependency and CI audit. Every finding was fixed except where noted; the
commits are on `main` after PR "Security review before signing".

| # | Severity | Finding | Fix |
|---|---|---|---|
| 1 | Critical | The package was read twice (once to learn its namespace, once to check the pinned key); a file swapped in between installed over another publisher's app. Confirmed with a test program. | Read once; the pin is checked on the files that get installed. |
| 2 | High | A package's `uv.toml` / `[tool.uv]` / `.python-version`, or the user's `UV_*` variables, steered uv (cache folder outside the app, other Python downloads, system Python). Confirmed. | `--no-config`, cleaned environment, `--managed-python` with an explicit version. |
| 3 | High | The lockfile rules were only checked by `pack`: a hand-made `uv.lock` could name wheels from any URL or index. | Install-time gate: PyPI wheels with hashes only. |
| 4 | High | An install confirmed after another one could ignore the key that one had pinned, or downgrade. Confirmed. | Pin, key and version checked again under the state lock at install. |
| 5 | High | One non-UTF-8 byte in an app's output stopped the reader and killed the run (common on Windows code pages). Confirmed. | Byte-level reading, lenient decoding, `-X utf8`. |
| 6 | Medium-high | The runner in the data folder was trusted as found, and the bytecode cache was shared by all apps. | Runner compared with the embedded copy before every run; one cache per app version. Remaining same-user limit: see above. |
| 7 | Medium | The app joined its Job Object only after it had started, so processes it started at once could escape Cancel. | Suspended start, Job Object, then resume; no job means no run. *To verify on Windows.* |
| 8 | Medium | Uninstall, repair or update could delete a running app's folders; a failed repair left no environment; a crash could leave a half-built one that looked ready. | Refused while the app runs; environments built aside and moved in when complete; state changes under one lock. |
| 9 | Medium | A second launch (double-clicking another `.dsapp`) cleared the first instance's staging and running runs before handing over. | Single-instance check first; data set up afterwards. |
| 10 | Medium | A chatty app could flood the window and make Cancel unclickable. | Batched events, one redraw per frame, a stable Cancel button. |
| 11 | Medium | "Open result" opened any path inside the output folder, including programs and shortcuts, by the unresolved path. | Only recorded results, by resolved path; runnable types only shown in their folder. |
| 12 | Medium | The per-user install folder was also the data folder: environments next to the signed executables, and "delete app data" missed both data and secrets. | Data moved to the bundle-id folder (migrated once); uninstall removes secrets too; DLL search restricted. |
| 13 | Medium | Key IDs shown to people (minisign key numbers) can be chosen by whoever makes a key. | SHA-256 fingerprints. |
| 14 | Low | Paths with Windows-forbidden or invisible characters, `COM¹`, `CONIN$`, 8.3 aliases and case-only folder differences passed; versions like `+1.0`; `1.2` signed for `1.2.0`; the entry module not checked; reserved names only at the root; only 64 bits of content hash compared. | All refused or compared in full (format 1 amended, see [`format.md`](format.md)). |
| 15 | Low | The undeclared outputs and paths an app could forge in its result were stored; any PID could be reported as a COM server; Excel stayed open after a crash. | Results re-checked; PIDs checked against the run's start; COM servers ended when a run fails. |
| 16 | Low | Unreadable Mark-of-the-Web counted as "not downloaded"; the IPC surface included all of `core:default`; non-Unicode arguments crashed the launcher; relative paths from a second launch; poisoned locks blocked installs; logs read whole and strictly as UTF-8; no `object-src`/`base-uri`/`form-action`/`frame-ancestors` in the CSP; a per-machine MSI next to the per-user installer. | Each fixed. |
| 17 | Dependencies | `zip` 7.3 had three advisories (not in code we call), plus `time` and `quick-xml`; Tauri's Rust and JavaScript versions did not match. | `zip` 9, updates, Tauri 2.12 on both sides; `cargo audit` and `npm audit` clean and in CI. |
| 18 | CI | Actions pinned by moving tags. | Pinned by commit SHA; audit job; Dependabot. |

## Codebase scan, 27 October 2026

A scan of the whole codebase after v0.1.0 found places where the code was looser than this page
said, or than it should be. All fixed in PR "Tier S hardening", with tests.

| # | Finding | Fix |
|---|---|---|
| 1 | Runs inherited the launcher's whole environment, so a token in the user's session reached every app. | Environment allow-list for runs, as for installs. |
| 2 | `network = ["*"]` switched the guard's network rule off and nothing said so. | A scan warning (`network-any`) on the install screen and in `pack`. |
| 3 | Moving (`os.rename`) or linking a protected file was checked as a write only, so a saved key could be moved into the output folder and read. | The source is checked as a read. |
| 4 | A guard stop whose `state.json` write failed was lost: the next run started the app again. | Retried, kept in memory meanwhile (the app stays blocked), the user told; the run is always recorded. |
| 5 | The runner was checked when the launcher started, not before every run as this page said; and Python wrote `__pycache__` into its folder. | Checked at each run; Python runs with `-B`. |
| 6 | Install checked only the lockfile's sources; the per-platform wheel checks ran only in `pack`. | Both at install. |
| 7 | Install trusted the staged files and `pending.json` as found. | Re-hashed and compared with the launcher's own copy before the move. |
| 8 | Any process of the user's started during a run could be named as a COM server and ended. | Only processes COM started (`-Embedding`). |
| 9 | Fullwidth names, `\x` and `\u` escapes, and calls passed as values or through `getattr` got past the code scan. | Read as Python reads them. |
| 10 | Another Python the app started (`subprocess.run([sys.executable, "-c", ...])`) ran without the guard: none of its rules applied. | Started through `child.py` with the guard installed first; its reports reach the launcher and a stop ends the run. Any other Python in the Job Object ends the run. |

### Verified on a real Windows PC (23 October 2026)

Results in [`report.md`](report.md):
- **Job Object:** Cancel and closing the launcher end the app and every process it started.
- **DLL loading:** the launcher starts normally with the restrictions; WebView2 loads.
- **Installer:** the NSIS installer with the uninstall hook; "delete app data" removes the data
  folder and the apps' secrets.

### Still to verify on a real Windows PC

- **Job Object:** Excel is closed after a crash (closed after Cancel and after a killed run: verified).
- **Data migration:** an existing `%LOCALAPPDATA%\devstribution` data folder moves to the new
  location on first start of the new launcher.
- **uv behind a company proxy:** `--system-certs` with the company's TLS inspection.
- **Managed mode:** the registry read and the "can the user write this?" check, on a local
  folder and on a network share ([`managed-mode.md`](managed-mode.md)).
