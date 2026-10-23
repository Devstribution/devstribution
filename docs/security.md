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
| A package swapped on disk between the check and the install | The launcher reads the package once, into its own staging folder, and installs those very files. |
| Going back to an older, vulnerable version | The highest installed version per app is remembered; lower versions, and the same version with other content, are refused (also checked again at install). |
| Code running at install time | Install only extracts, verifies, and runs `uv sync --no-build` from the signed lockfile: no `setup.py`, no build backend. Nothing from the package runs before the user confirms. |
| A package making uv fetch or install something else | `uv.lock` must name only wheels from PyPI's file host, over HTTPS, with SHA-256 hashes, checked before install. uv runs with `--no-config` (no `uv.toml` or `[tool.uv]` from the package), a cleaned environment (no `UV_*`, `PIP_*`, `PYTHON*` from the session), its own Python builds (`--managed-python` with an explicit version, so no `.python-version` from the package) and the system certificate store. Release launchers use only the uv they ship, pinned and hash-checked at build time. |
| A look-alike publisher key | Keys are shown by a SHA-256 fingerprint of the whole key, not minisign's 8-byte key number, which anyone can choose. Trust decisions compare the full key. |
| An app's output or result tricking the launcher | Output is read as bytes (invalid UTF-8 cannot stop the reading), lines and logs are capped, events reach the window in batches. Results are re-checked by the launcher: only declared outputs, file outputs only inside the run's output folder or a folder the user chose for that run's `folder` inputs. "Open result" opens only a recorded result, by its resolved path, and only shows programs, scripts and shortcuts in their folder. |
| An app making the launcher end other processes | COM processes (Excel) an app reports are only ended if they started during the run, and only if they are still the same process. |
| An app's processes surviving Cancel or the launcher closing (Windows) | The app starts suspended and joins a Job Object with kill-on-close before it runs; a run that cannot get one does not start. |
| An app tampering with the runner other apps load | Before every run the runner folder is compared with the copy built into the launcher and rewritten if a file differs or was added. Each installed app version has its own bytecode cache. |
| Package or run data running script in the launcher window | Everything reaches the page as text; no `innerHTML` with data; strict CSP (`script-src 'self'`, no objects, frames, forms or base URL); icons only as `data:` images. |
| The launcher window being used to do more than the launcher needs | The window may call only the launcher's own commands, event listening, the file picker and yes/no questions (no `core:default`). The backend checks every argument (ids, run ids, secret names, paths). |
| A DLL planted next to the signed launcher (Windows) | Imported DLLs load from System32 only (`/DEPENDENTLOADFLAG`), others never from the current folder or PATH (`SetDefaultDllDirectories`). The launcher's data is kept out of its install folder. |
| An app whose code does more than it declares, or hides what it does | Before the install button, the code scan lists what the code uses and flags undeclared network or SAP use, programs and compiled files that cannot be reviewed, code built at run time, and what is typical of malware (decode-and-run, start-with-Windows entries, browser password stores, antivirus switches, process injection, hidden text-direction characters). A red banner and "Install anyway" when something looks like malware. The scan reads, never runs, the code; see limit 2. |
| An app doing more than it declared, or what malware does, while it runs | The runtime guard: an audit hook in the runner, installed before app code, blocks network use the app does not declare (and hosts it does not list) and SAP's RFC library without `sap`; it stops the run before the call on start-with-Windows entries, the antivirus's settings, browser password stores, the Windows tools malware uses, process injection, keyboard hooks and hidden compiled code. The launcher checks every process in the run's Job Object against the same rules. A stop ends the whole tree and quarantines the app until the user decides. See limit 2 for what it does not see. |
| A downloaded package without a warning | Mark-of-the-Web is read (also UTF-16); a mark that cannot be understood counts as "from the internet". |
| A tampered CI pipeline (once it signs releases) | Actions pinned to commit SHAs, read-only token, `cargo audit` and `npm audit` on every change, Dependabot updates. |

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
     not read dependencies, and code that builds names or downloads code at run time can get past
     it. It is a prompt to ask questions, not a verdict.
   - The **runtime guard** ([`roadmap.md`](roadmap.md), section 4) sees what goes through
     Python's audit events, inside the app's own process. It does not see network use, files or
     programs reached through native code (`ctypes` calls to WinHTTP, COM objects such as
     `MSXML2.XMLHTTP`, C extensions), SAP driven through SAP GUI's COM interface, or anything
     after code patches the interpreter's memory. Folder permissions are not enforced. The Job
     Object check sees every program the run starts, but only once it has started, so a very
     short program can act before it is ended.

   After 1.0: the IT review kit ([`plan.md`](plan.md)) and a kernel-enforced sandbox
   (AppContainer) where the cost is reasonable.
3. **Trust on first use.** Outside managed mode, whoever first installs a namespace on a PC pins its
   key, and the name is marked "unverified". Catalog and GitHub-source verification (Phase 2)
   replace this with a verified binding.
4. **A local administrator** can change anything, including policy and the launcher itself.
5. **Bugs in signed apps** are the publisher's. The platform makes them fail cleanly and alone
   (timeouts, Job Objects, per-version environments), not correct.
6. **Linux GTK libraries** that Tauri uses on Linux are flagged "unmaintained" by RustSec (`glib`,
   `zbus`, …). There is no known vulnerability, and the Windows launcher does not use them.

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
