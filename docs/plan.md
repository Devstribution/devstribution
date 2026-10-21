# Project plan: devstribution

## Problem
Writing code has become fast: with automated and agentic coding, a working Python POC, automation or small app takes hours, not weeks. Distribution has not kept up. Getting "it works on my PC" into the hands of a colleague or client still hits the traditional bottlenecks:
- Unsigned `.exe` / `.bat` files are blocked by Microsoft (SmartScreen, antivirus).
- End users cannot install Python, dependencies, etc.
- Packaging each app as its own signed installer is slow, expensive and treated by IT as one more thing to approve.

**Goal:** a platform to distribute almost anything Python can do (POCs, near-DIY apps, automations, tools with or without a GUI), as easily as sharing a file. What an app may do is decided by **trust in its publisher**, not by a fixed list of allowed use cases. The aim is to remove the bottlenecks between writing code and using it, and so make code more democratic.

## Principles
These decide everything else in this plan; when a choice is unclear, they are the rule.

**The promise.** Nothing is bulletproof, and like with a bank or a password manager, using it means trusting someone. devstribution guarantees that **what you run is exactly what that publisher published, by a publisher whose identity is verified, and that it cannot damage the platform or other apps**. Whether the publisher deserves trust is the user's (or IT's) choice. So a breach can only come from misplaced trust, never from the platform itself. Like a bank, trust comes with remedies: revocation, audit log, publisher reputation in the catalog.

1. **Built to be used.** Easy to set up, easy to use, no technical knowledge needed (see "User experience").
2. **Safe itself.** The platform never becomes the weak point: no local server, minimal privileges, signatures checked before anything is extracted, an app can never touch the launcher, other apps, the trust settings or the policy.
3. **Contains badly made apps.** Most failures are bugs, not malice; a broken app must fail cleanly and alone (see "Robustness").
4. **Light.** Small to download, small on disk, nothing running when no app is in use (see "Lightness").
5. **Local and serverless.** Data stays on the user's PC and the project runs no server; GitHub is the only outside service, and apps received as files keep working without it.

## Requirements
| Topic | Decision |
|---|---|
| End users | Anyone (colleagues and external clients, non-technical) |
| Use cases | Almost anything Python can do: POCs, automations, data processing, desktop and local web tools, scripts. Examples: querying Excel files, SAP, APIs, browser automation, local ML models |
| Publishers | Anyone, often coding with AI agents; publishing must be as fast as writing the code |
| Data | Must stay **local** on the user's PC |
| Exception | Diagnostic packages for the app's developer, only with the user's consent |
| Model | Free and open source, initially on GitHub |
| Support | Donations (PayPal / Patreon / Ko-fi / GitHub Sponsors) |

## Options evaluated
| Approach | Outcome |
|---|---|
| A. Server-side with generated UI | Rejected: data would leave the user's PC, infrastructure costs |
| B. In the browser (Pyodide/WASM) | Not enough on its own: SAP (COM/RFC) and many APIs (CORS) are unreachable. Useful only for Excel-based tools, possible extra mode |
| C. Signed local launcher | **Chosen**: everything Python can do locally (filesystem, SAP, internal network, GUIs, hardware); data stays local |

Existing alternatives to study: Windmill, Streamlit, Gradio, NiceGUI, marimo / stlite / PyScript, Retool, Appsmith, **PyApp** (exe that downloads Python with `uv` on first run), **Briefcase**.

**Differentiation**: PyApp and Briefcase package **one** app at a time; Streamlit-style tools need a server or a Python install. None of these combines (1) a single signed launcher for any number of apps of any kind, (2) zero-UI apps that get a generated UI from the manifest, (3) a publisher trust model that IT can manage without a server, (4) publishing fast enough for code written by AI agents.

## Main risks (to verify first)
| Risk | Impact | Mitigation |
|---|---|---|
| **AppLocker / WDAC** block executables in user-writable folders (`%LOCALAPPDATA%`). `uv`'s Pythons (python-build-standalone) have no Authenticode signature; a venv's `python.exe` is also a copy inside the venv folder. **WDAC also enforces on DLLs**, so the `.pyd` files of dependencies (e.g. `pandas`) are blocked too, not just `python.exe` | Apps don't start at all on corporate PCs, the main target | **Phase 0**: test on a real locked-down corporate PC. Plan B: per-machine mode with a signed interpreter and dependencies in an admin-only folder (see below) |
| **SignPath Foundation** requires open source projects that are already established | Unsigned launcher for the first months → SmartScreen | Plan B: paid OV certificate, or distribution via winget/Intune accepting the warning for early users |
| **SAP**: `pyrfc` archived by SAP, NW RFC SDK proprietary and not redistributable; SAP GUI Scripting often disabled by SAP admins | One of the key use cases may not work | Declare prerequisites in the manifest (`requires`); the launcher checks them and explains what's missing. Document what is actually supported |

### Python installation modes
- **Per-user** (default where there is no application control): `uv` manages Python and venvs in `%LOCALAPPDATA%`.
- **Per-machine** (for WDAC/AppLocker environments; installer run by an admin / Intune):
  - **Interpreter**: the official **python.org** build (Authenticode-signed by the Python Software Foundation), installed in `Program Files\devstribution\python\<version>\`. Not python-build-standalone, which is unsigned.
  - **Dependencies**: installed into `Program Files\devstribution\envs\<id>\<version-hash>\` (admin-writable only, so WDAC path rules and AppLocker default rules allow the `.pyd` files). No venv: the runner adds the folder to `sys.path`.
  - **Who writes there**: a small **privileged Windows service** (`devstribution-svc`, part of the Rust core, installed by the per-machine installer). The UI asks it over a local named pipe (ACL: interactive users only) to install an app. Rules:
    - **Managed mode only**. Files in `Program Files` are trusted by WDAC, and in free mode anyone can create a signing key, so a service that installed any validly signed package would let any user or user-level malware place arbitrary `.pyd` files in a trusted folder: a WDAC bypass. The service therefore refuses every request unless an HKLM policy exists, and installs only packages signed by authors that policy approves (plus any countersignatures it requires). Without a policy, the launcher installs in per-user mode instead (where WDAC, if present, still applies normally). The per-machine installer warns IT if it is deployed without a policy.
    - **Copy first, then verify**. The service never verifies files in a folder the user can write to, where they could be swapped between the check and the install. It copies the `.dsapp` (and any offline `wheels/`) into its own staging folder `%ProgramData%\devstribution\staging\<random>\` (ACL: SYSTEM and Administrators only), opened with exclusive access, and runs extraction, hash, signature, policy and anti-downgrade checks there. Wheels downloaded from the index are written to the same staging folder and checked against the lockfile hashes before being unpacked.
    - **Never trusts the UI's verdict**: it repeats every check itself.
    - **Never runs package code**: thanks to the wheels-only rule (see "Dependencies"), installing means only verifying hashes and unzipping wheels, so the elevated part does nothing an attacker can steer into running code.
    - **Keeps its state in protected folders**: app code goes in `Program Files\devstribution\apps\<id>\<version-hash>\` and the trust state (pinned keys, highest installed version per app) in `%ProgramData%\devstribution\state\` (ACL: SYSTEM and Administrators only), so a standard user cannot reset anti-downgrade or replace app code.
  - **Alternatives if IT prefers them** (documented, not default): (a) designate the launcher as an Intune/AppLocker **managed installer**, which tags the files it writes as trusted; simpler, but every wheel it downloads becomes trusted, so it relies entirely on the trust policy; (b) the IT team signs dependencies with its own code-signing certificate via an internal mirror.
  - Phase 0 decides which mode is the default.

## Chosen architecture
### 1. A single signed program: Tauri launcher
- Installed once (MSI/NSIS installer generated by Tauri, winget, Intune), no `.bat`.
- Signing: SignPath Foundation (free for open source) or an OV certificate as plan B.
- No PyInstaller (antivirus false positives): the launcher is a native executable.
- UI in WebView2, communicating with the backend over **Tauri IPC**: no local HTTP server, so no CSRF / DNS rebinding attack surface.
- `uv` bundled as a **sidecar**, signed and installed together with the launcher.
- The **Python runner** is shipped inside the launcher's install folder (see "Execution"), not installed into app environments.

**Code split**
| Part | Language | Responsibility |
|---|---|---|
| Core | Rust | Signature verification (`ed25519-dalek`), policy from HKLM (`winreg`), safe extraction, `uv` invocation, subprocesses (Job Object), logs, secrets (`keyring`), per-machine service |
| UI | TypeScript | Catalog, permissions and confirmation screen, app screen (play button, config, result), windows for `web` apps, progress and results |
| Runner | Python (minimal, stdlib only) | Starts the app according to its `kind`; for `form` apps reads input JSON, calls `run()`, writes result JSON; handles cancellation and cleanup |

Rule: **no launcher logic in Python**. Python exists only inside the runner, in the app's process.

### 2. Apps are data, not executables
Each app is a folder (GitHub repo or zip) with a standard `pyproject.toml`; the manifest lives in `[tool.devstribution]`. The launcher reads the manifest, prepares the environment from the lockfile and starts the app according to its **kind**:

| `kind` | What it is | What the launcher does |
|---|---|---|
| `form` (default) | No UI code: inputs, `run()`, outputs | Generates the app screen and form from the manifest. Fastest path for a POC |
| `gui` | Brings its own UI (tkinter, PySide/Qt, Flet, ...) | Starts it with `pythonw` (no console window) |
| `web` | Local web UI (Streamlit, Gradio, NiceGUI, ...) | Picks a free port on `127.0.0.1` and a one-time token, opens the UI in its own launcher window, stops the server when the window closes |
| `cli` | Script or terminal program | Runs it in a terminal window with the arguments from the manifest or the user |
| `background` | Scheduled or long-running automation | With the user's consent, registers it in Windows Task Scheduler (per user, no admin); shows last run, logs, "Run now", "Disable" |

The runner, Job Object, logs, signatures and trust model are the same for every kind. `kind` is in package format v1 from day one, even though Phase 1 implements only `form` and `gui`.

For `web` apps the local server belongs to the app, not to the launcher, so the launcher's own "no local server" rule still holds. The token is passed to the app on stdin and added to the URL the launcher window opens; the runner's helper for Streamlit/Gradio/NiceGUI rejects requests without it, which blocks other local programs and web pages from reaching the app.

### 3. Distribution
- Install with a link: `devstribution://install/<github-user>/<app>`. **It always leads to the confirmation screen**, even in managed mode: any web page can open the link, so it must never install silently.
- Catalog = a GitHub repo listing the apps. No infrastructure to maintain.
- Updates from GitHub releases, versions pinned by hash.

### 4. Privacy
- Everything runs locally; the platform receives no data.
- **Diagnostics**: on error the launcher creates a zip (logs, versions), applies **automatic redaction** (user paths, user name, environment variables, secret values), **shows the user the contents**, and the user decides whether to send it (pre-filled GitHub issue or file). Never sent automatically.

### 5. Security (critical point)
Apps can do anything Python can, so **trust in the publisher is the safety model** (see "The promise"). The platform's job is to make that trust well-founded and to make sure nothing else can be attacked:
- Permissions declared in the manifest (network, folders, SAP). **They are informational**: the UI says so explicitly ("the author declares that..."), to avoid a false sense of security.
- User confirmation on the first run of each app.
- "Verified" badge for apps reviewed in the catalog.
- Pinned hashes/versions to prevent silent malicious updates; **no downgrades** (see trust model).
- **No code runs at install time**: wheels only, no source builds (see "Dependencies").
- **Origin shown to the user**: files downloaded from the internet show where they came from (see "Receiving channels").
- Each run inside a Windows **Job Object**: the whole process tree is killed on timeout/Cancel, optional memory limits. COM servers started outside the tree (Excel) are cleaned up separately (see "Cancellation and cleanup").

### 6. User experience
- **App screen** for apps without their own GUI (`form`, `cli`): the app's icon (Python's icon if it has none), its title and description, a big **▶ play button**, and when the run ends a clear **"Open result"** button (opens the output file or folder) plus a short status ("Done in 12 s" / readable error).
- **Config only when needed**: the form appears only if the app declares inputs, pre-filled with the last values used (secrets excluded). An app with no inputs is just the play button.
- **One confirmation** at the first launch of each app (publisher, origin, declared permissions); nothing to confirm afterwards unless the publisher, key or permissions change.
- **Home screen**: installed apps as tiles, most recently used first; `background` apps show their next and last run.
- Plain-language messages everywhere: no stack traces in the UI (they are in the log and the diagnostic package).

### 7. Robustness (badly made apps fail cleanly and alone)
- **One environment per app**: a broken or conflicting dependency in app A can never break app B or the launcher.
- **One output folder per run**, so a run never overwrites the previous one's results.
- **Kill cleanly**: hung or crashed apps are terminated with their whole process tree (Job Object) and reported in plain language.
- **Repair**: one button rebuilds the app's environment from the lockfile.
- **Clean uninstall**: removes the app's code, environment, schedules and secrets; uninstalling the launcher removes everything it created.
- **Caught at publish time**: `devstribution pack --test` installs the app in a clean environment and runs it once (with sample inputs for `form` apps), so many bugs never reach users.

### 8. Lightness
- Tauri launcher: installer of about 10 MB (Electron-based tools are around 100 MB).
- Python is downloaded **once** per version and shared by all apps.
- `uv`'s shared cache keeps one copy of each package and links it into each environment: ten apps using pandas take the disk space of one pandas.
- **Nothing runs in the background** when no app is in use: no tray process, no resident updater. The per-machine service is demand-started by the launcher and stops when idle. `background` apps run only via Task Scheduler.

### 9. Developer experience (publishing at agent speed)
- `devstribution init <kind>` creates a working template app.
- `devstribution dev <folder>` runs it straight from the folder while developing.
- `devstribution pack` validates and signs in one command, with error messages precise enough that a coding agent can fix them on its own.
- A ready-made instruction file for coding agents (an `AGENTS.md` and a skill) that teaches them the manifest, the kinds and the rules, so an agent can produce a valid `.dsapp` end to end.
- `devstribution keygen` once; publishing a new version = `pack` + upload to a GitHub release.

## Trust model and policy
Principle: **robust for IT, frictionless for everyone else**. One launcher; behaviour depends on the policy.

**Modes**
- **Free** (no policy): strong warnings and confirmation on the first run of each app, at the user's own risk.
- **Managed** (policy in HKLM / GPO / Intune): runs only apps signed by authors approved by IT.

**Roles** (only two, no server to administer)
- **Publisher**: generates a key (one command), signs and publishes apps.
- **Approver**: usually IT; decides which authors to trust.

**Binding author, namespace and key**
- The app `id` is `<author>/<app>`. The `<author>` part is the **namespace**, and it is the only author identity: the manifest has no self-declared `author` or `key` field, because anything in the manifest is written by the publisher.
- On verification, the launcher identifies the signing key from the signature itself (minisign key ID), then looks up which namespace that key owns:
  - **Managed mode**: from the policy / imported `.trust` files (`.trust` = author namespace + public keys).
  - **Free mode**, in order of strength:
    1. **Catalog**: the catalog countersigns that namespace X belongs to the GitHub user/org that owns the source repo.
    2. **Download source**: for `devstribution://install/<github-user>/<app>` links and other launcher downloads from GitHub, the namespace must equal the owner of the GitHub repo the release was downloaded from (`<github-user>`, compared case-insensitively). A mismatch = refused. This uses GitHub's own account ownership as the proof, with no server of ours.
    3. **Trust on first use**, only for files received outside GitHub (email, network share): the first key seen for a namespace is pinned locally and a later package for that namespace signed by a different key is refused unless it is a declared rotation. Because whoever arrives first could claim any name, the confirmation screen marks such a namespace **"unverified name"** with the key fingerprint, and it never gets the verified style of options 1-2. If the same namespace is later verified by the catalog or a GitHub download with a different key, the launcher stops and explains the conflict instead of choosing silently.
- **Rule**: the namespace in `id` must be owned by the key that verified the signature, otherwise the package is refused. A trusted author A can therefore never publish or overwrite `B/app`.
- The signature's **trusted comment** (signed in minisign) contains `id=<id> version=<version> schema=<n>`; the launcher checks that it matches the manifest, so the key facts can be read and checked before extraction.

**Decisions**
| Topic | Decision |
|---|---|
| Keys | Trust keys (Ed25519), not license keys: no server, consistent with open source and local data |
| Signature format | Ed25519 with a **minisign**-compatible format (same format as the Tauri updater). No Sigstore: it requires network access and an OIDC identity, conflicting with "no server / offline" |
| Signing | Optional layers: author (always) → catalog countersignature → IT countersignature. The policy says which are required |
| Policy | A TOML file with all-optional fields (allowed authors, minimum launcher version, dependency mirror, diagnostics on/off, network, pinned/minimum app versions). 2-3 ready-made templates: permissive, standard, restrictive |
| External authors | `.trust` file published by the author (namespace, public keys, name, verifiable domain/repo). IT imports it with one command and approves all apps and versions **in that namespace** |
| Blocking | The block message generates a ready-to-forward `.trust` ("ask your IT to trust X") |
| Revocation | Signed list with cache and long expiry (~30 days); it can also live on a network share. It can revoke a key, a whole app, or **specific versions**. Rotation: the next key is declared and signed by the previous one, and keeps the same namespace |
| Anti-downgrade | The launcher keeps, per app `id`, the highest version installed and the key that signed it. A lower version is refused. Rollback only: (a) in managed mode, via a pin in the policy; (b) in free mode, via an explicit "install older version" action with a warning. A revoked version is never installable, rollback included |
| Enforcement | Policy in HKLM / Intune; no defence against a local admin (stated in the threat model) |
| Audit | Structured local log / Windows Event Log, collectable by IT. The platform sends nothing |
| App updates | Default: a new version from the same trusted author updates if signature and lockfile are valid and the version is higher. Policy options: waiting ring, manual approval, pin |
| Launcher updates | Tauri updater plugin (signed updates), can be disabled by IT (distribution via Intune/winget) |
| Dependencies | The signature covers the manifest + lockfile with hashes; anything outside the lockfile = refused. **Wheels only**: every package in `uv.lock` must have a wheel for the target platform; `devstribution pack` refuses the package otherwise, and installation uses `--no-build`, so no `setup.py` or build backend ever runs. Allowed only in `devstribution dev`. Optional internal mirror from policy |
| Support | IT-oriented docs, policy dry-run mode, no feature that requires a server |
| Threat model | One page: threat → what blocks it → what it does NOT cover. Covered: malicious or compromised author, author impersonating another namespace, downgrade to an old vulnerable version, code running at install time, use of the per-machine service to place untrusted code in a WDAC-trusted folder, files swapped between check and install, `devstribution://` links from malicious sites, tampered packages. Not covered: local admin, pre-existing malware, bugs in the code of signed apps |

**Implementation order**: (1) author signatures + namespace binding + free/managed mode + minimal policy, (2) `.trust` and import, (3) revocation, rotation, anti-downgrade, logs, (4) advanced options (ring, mirror, pin).

## App package standard (v1)
Goal: receive an app as **a data file**, verify it and launch it in a uniform way. Extension: `.dsapp`.

### Package format
A zip (not an executable, so not subject to SmartScreen) with this structure:
```
my-app.dsapp
  pyproject.toml  # standard Python metadata + manifest in [tool.devstribution]
  uv.lock         # dependencies with hashes (required)
  main.py         # code (plus any other modules/assets)
  FILES.sha256    # list of hashes of all files (canonical format, see below)
  FILES.sha256.minisig  # author's Ed25519 signature on FILES.sha256 (trusted comment: id, version, schema)
```
Optional: `wheels/` folder for offline use (install without internet).

**Canonical format of `FILES.sha256`**: UTF-8, LF line endings, one line per file `<sha256 hex>  <path>`, paths with `/`, sorted by bytes. Every file in the zip must appear, except `FILES.sha256` and its signatures; extra or missing files = refused.

**Extraction rules** (package refused if violated): no absolute paths or paths with `..`, no symlinks or special files, no duplicate names (including names differing only by case), no names Windows cannot represent faithfully (`\`, `:`, reserved device names like `CON`, components ending in a dot or space), no encrypted entries, limits on total size and number of files (counted on the bytes actually extracted, not on the zip headers). A failed extraction leaves nothing behind.

**Packing rules**: `pack` leaves out caches and environments (`.git`, `.venv`, `__pycache__`, top-level `dist/`, ...) and `.env` files, refuses symbolic links and files that look like a secret signing key, and writes entries in a fixed order with fixed timestamps.

### Manifest in `pyproject.toml`
```toml
[project]
name = "query-orders"
version = "1.2.0"
requires-python = ">=3.11"
dependencies = ["openpyxl>=3.1"]

[tool.uv]
package = false                  # the app is not built or installed as a package: no build step

[tool.devstribution]
schema_version = 1
id = "author/query-orders"       # <namespace>/<app>; the namespace must belong to the signing key
title = "Query orders"
description = "Reads the orders file and builds a report"
icon = "icon.png"                # optional; Python's icon if missing
kind = "form"                    # form | gui | web | cli | background
entry = "main:run"               # module:function (form) or module:function / module to start (other kinds)
platforms = ["windows"]
requires = []                    # external prerequisites: "sap-gui-scripting", "sap-nwrfc-sdk", "excel"

[[tool.devstribution.resources]] # files downloaded and verified by the launcher, never by install scripts
url = "https://example.com/model.onnx"
sha256 = "<hex>"
path = "models/model.onnx"       # inside the app's resources folder

[[tool.devstribution.inputs]]    # generates the form
name = "excel_file"
type = "file"                    # file | folder | text | number | bool | choice | date | secret
label = "Orders Excel file"
accept = [".xlsx"]

[[tool.devstribution.outputs]]
name = "report"
type = "file"                    # file | table | text

[tool.devstribution.permissions] # declarative and informational, shown to the user
network = ["api.company.com"]
folders = ["user-selected"]
sap = false
secrets = ["sap_user"]           # names required, values in the keyring
```
Dependencies are declared only once, in `[project]`, as in any Python project. `[project].version` must match the version in the signature's trusted comment. In v1 versions are 1 to 4 dot-separated numbers (`1.2.0`), with no pre-release suffixes, so anti-downgrade has one unambiguous ordering; `[project].dynamic` is refused (it needs a build step).

**Resources** cover what pip packages don't: browser binaries for Playwright, ML models, data files. Instead of a post-install script (e.g. `playwright install`), the manifest lists each file's URL and sha256; the launcher downloads and verifies them itself, and since they are in the signed manifest they are covered by the publisher's signature. Archives are unpacked with the same extraction rules as packages. This keeps "no code runs at install time" true. Anything that needs a real system install (drivers, CUDA, SAP GUI) stays a `requires` prerequisite that the launcher checks and explains.

### Execution contract
For `gui`, `web`, `cli` and `background` apps the contract is minimal: the runner calls the `entry` function (or runs the `entry` module as `__main__`), with `ctx` available via `devstribution_runner.ctx` for logs, secrets, output folder and cleanup. For `web` apps `ctx.web_port` and `ctx.web_token` give the address to serve on. Everything below describes `form` apps.

```python
def run(excel_file: Path, ctx) -> dict:
    ctx.progress(0.3, "Reading the file")
    ...
    return {"report": out_path}
```
- The manifest is the **single source** of inputs; `devstribution dev` and `devstribution pack` check that the signature of `run()` matches (same names and compatible types) and refuse the package otherwise.
- `ctx` exposes `progress()`, `log()`, `output_dir`, `secrets.get(name)`, `cancelled`, `on_cleanup(fn)` and `track_com(obj)` (see "Cancellation and cleanup").
- The return value is a dict whose keys are the `outputs`; files must be written to `ctx.output_dir`.
- Errors: an exception = failed run, with the traceback in the local log and an offer to create a diagnostic package.
- For development, the same `ctx` API is published on PyPI as `devstribution-runner` (type hints, local tests). Apps add it as a **dev dependency** only; it is never installed in production environments (`--no-dev`), because the launcher provides the runner.

### Execution
1. The Rust core starts a **subprocess** inside a Job Object: `python -I <launcher-dir>\runner\bootstrap.py`.
   - The runner is **pure Python, stdlib only**, versioned together with the launcher and stored in the launcher's install folder. Because it is not in the app's environment, it never conflicts with the app's lockfile, and "anything outside the lockfile = refused" stays true.
   - `-I` (isolated mode) ignores `PYTHONPATH`, user site-packages and the current folder; `bootstrap.py` builds `sys.path` explicitly: runner, app code (`apps\<id>\<version-hash>\`), app dependencies.
   - The JSON protocol carries a `protocol` version, so launcher and runner can evolve safely.
2. Inputs and secrets are passed as JSON on **stdin** (secrets never on files or environment variables); stdin stays open for control messages (e.g. `cancel`); results come back as JSON; stdout/stderr go to the log. The runner keeps the original stdout for the protocol and points fd 1 at stderr, so the app's own prints (including from C extensions) can't corrupt it; the app gets an empty stdin. The protocol is documented in `runner/devstribution_runner/_runner.py`.
   - Bytecode goes to a cache outside the app folder (`sys.pycache_prefix`), so the verified, hash-addressed code folder is never modified by running it.
3. Temporary working folder per run, with a "Cancel"/"Stop" button. Timeout by kind: default on for `form` and `cli` (overridable in the manifest), off for `gui` and `web` (they run until closed), set per schedule for `background`.
4. Environment created with `uv sync --frozen --no-build --no-dev --no-install-project` from the lockfile, with `UV_PROJECT_ENVIRONMENT` set to `envs\<id>\<version-hash>\` so `uv` does not create a `.venv` inside the app's code folder (in per-machine mode the service installs the same wheels with `uv pip install --target --no-build`); Python managed by `uv` (per-user) or python.org (per-machine); cache shared across apps.

### Cancellation and cleanup
COM servers (Excel, Word) are started by Windows' DCOM service, not by the app's process, so they are **outside the Job Object** and would survive a kill as orphan processes.
1. **Cooperative cancellation first**: on Cancel/timeout the launcher sends `cancel` on stdin; the runner sets `ctx.cancelled` and raises a `Cancelled` exception at the next `ctx.progress()`/`ctx.log()` call; the app's `finally` blocks and `ctx.on_cleanup()` functions run. `Cancelled` derives from `BaseException`, so `except Exception:` in the app doesn't swallow it. If the app reaches no `ctx` call within 3 s, the runner interrupts the main thread (`KeyboardInterrupt`, which still runs `finally` blocks). If stdin closes (the launcher is gone), the run is cancelled the same way, so no app outlives its launcher.
2. **The app creates the COM object, the runner tracks it**. The runner stays stdlib-only, so it does not create COM objects itself (that needs pywin32 or comtypes). The app depends on pywin32 in its own lockfile and registers the object:
   ```python
   excel = win32com.client.DispatchEx("Excel.Application")  # always a NEW instance
   ctx.track_com(excel)
   ```
   - **`DispatchEx`, never `Dispatch`**: `Dispatch` attaches to an Excel the user already has open, so cleanup would close the user's own workbooks. `devstribution dev`/`pack` warn when the code calls `Dispatch("Excel.Application")` (and the same for Word/Outlook), and the docs and example app use `DispatchEx` only.
   - `track_com(obj)` reads `obj.Hwnd` and gets the server's PID with `GetWindowThreadProcessId` via `ctypes` (stdlib), then sends it to the launcher on stdout. On cleanup it calls `obj.Quit()`.
   - Safety check: the PID is recorded only if that process was started **after** the run began (process creation time via `ctypes`), so a PID that points at a pre-existing Excel is never recorded. In that case `Quit()` is not registered either: the object belongs to an Excel the user already had open.
3. **Hard kill** after a grace period (e.g. 10 s): the launcher terminates the Job Object, then terminates **only** the COM server PIDs recorded for this run, after checking that each PID still has the same creation time (PIDs get reused). Never Excel instances the user opened themselves.
4. **SAP GUI**: scripting attaches to the user's existing SAP GUI session, which is never killed; the helper only closes the connections/windows the app opened.

### Install flow
`receive → safe extraction to a temporary area → verify hashes, signature, namespace binding, policy and anti-downgrade → check prerequisites (requires) → show manifest, permissions and origin → confirmation (or approval by policy) → move into a hash-addressed folder → create environment (wheels only) → download and verify resources → ready`

Several versions of the same app can coexist; the old one is removed after a successful update.

### Receiving channels (all equivalent, same flow)
| Channel | Use |
|---|---|
| Double-click on `.dsapp` | File association registered by the Tauri installer; email attachment, network share |
| `devstribution://install/<author>/<app>` | Link from catalog or web page (Tauri deep-link plugin); downloads from the GitHub release. Always with confirmation |
| From the catalog in the launcher | List from a GitHub repo |
| `devstribution dev <folder>` | For developers: runs an unsigned folder, only in free mode |

**Origin (Mark-of-the-Web)**: when a `.dsapp` is opened, the launcher reads its `Zone.Identifier` stream. If it comes from the internet zone (ZoneId 3) or the restricted zone (4), the confirmation screen shows "Downloaded from `<HostUrl>`" with a stronger warning. The policy can refuse internet-zone files whose author is not approved. For downloads made by the launcher itself (deep link, catalog), the origin shown is the release URL.

### Local folders
```
%LOCALAPPDATA%\devstribution\
  apps\<id>\<version-hash>\   # extracted code
  envs\<id>\<version-hash>\   # venv (per-user mode; per-machine mode uses Program Files)
  runs\<timestamp>\           # output and logs per run
  logs\                       # launcher logs
  state\                      # pinned namespace keys (TOFU), highest installed version per app
```
In **per-machine mode** only `runs\` and `logs\` stay here. App code, dependencies and trust state live in the service's protected folders (`Program Files\devstribution\apps\` and `envs\`, `%ProgramData%\devstribution\state\`), see "Python installation modes".

### Still to decide for the format
- v1 proposal: inputs `file, folder, text, number, bool, choice, secret`; `date` and the `table` output in v1.1; `ctx.progress` already in the MVP (cheap and very visible to the user).

## Project setup
Name availability checked on 2026-10-21 (all free; re-check `.io` at the registrar).

**Now**
- [ ] 2FA enabled on the personal GitHub account.
- [ ] GitHub organization `devstribution` (Free plan): 2FA required for members; Dependabot alerts, secret scanning, push protection, private vulnerability reporting enabled.
- [ ] Single repo `devstribution/devstribution` (monorepo: Rust core, UI, Python runner). The `catalog` repo comes later.
- [ ] Protection of the `main` branch: no force-push, no deletion.
- [ ] Rename the local folder from `devstributor` to `devstribution` (once the session is closed).

**This week**
- [ ] Domain `devstribution.dev` (possibly also `.com`) via Cloudflare Registrar or Porkbun.
- [ ] Domain verified on the GitHub organization (needed for the organization page, for `.trust` files and for GitHub Pages).

**Initial repo files**
| File | Why |
|---|---|
| `LICENSE-MIT`, `LICENSE-APACHE` | Dual license MIT OR Apache-2.0 |
| `README.md` | Problem in 3 lines, "pre-alpha" status, link to `docs/plan.md` |
| `SECURITY.md` | Private vulnerability reporting; in practice also required by SignPath |
| `.github/FUNDING.yml` | Sponsor button (Ko-fi / Patreon / PayPal); GitHub Sponsors for the org later (requires Stripe) |
| `.gitignore` | Rust, Node, Python |
| `docs/plan.md` | This plan: move it from the repo root to `docs/` in the same commit that adds `README.md`, so the link works from the start |

**When there is real code**
- [ ] crates.io: log in with GitHub, first publication with a working version (no empty packages to squat the name).
- [ ] PyPI: account with 2FA, Trusted Publisher from GitHub Actions for `devstribution-runner` (development SDK).

**Later**
- [ ] Apply to SignPath Foundation after the first public release with minimal documentation.
- [ ] Publish on winget with a signed installer.

## MVP roadmap
**Scope rule**: Phase 1 serves only to put the app screen and generated form (`form` apps) and `gui` apps in front of real users; the trust model stays on paper until Phase 1 feedback arrives. The only exception is format rules that are cheap now and hard to retrofit, which go into package format v1 from day one: the `kind` field and `resources` list, `<namespace>/<app>` ids, wheels-only lockfiles, `tool.uv.package = false`, runner outside the environment, and **signed packages**: from Phase 1, `devstribution pack` already writes `FILES.sha256` and `FILES.sha256.minisig` (key created with `devstribution keygen`), and the launcher checks hashes but does not enforce the signature yet. Phase 2 then turns enforcement on without breaking any package already distributed.

**Phase 0: feasibility (before everything else)**
On a real locked-down corporate PC (AppLocker/WDAC active), verify with a script:
1. Does `uv` from the signed sidecar in `Program Files` start?
2. Does `uv`'s Python in `%LOCALAPPDATA%` start? And a venv's `python.exe`?
3. Does the signed python.org Python in `Program Files` start?
4. Do dependency `.pyd` files load (e.g. `pandas`): (a) from `%LOCALAPPDATA%` via `sys.path`, (b) from `Program Files`? This tests WDAC DLL enforcement.
5. If IT designates the launcher as a managed installer: are the files it writes allowed to run?
6. Access to Excel/SAP GUI Scripting from the child process, and cleanup of Excel after a Job Object kill.

The outcome decides the default installation mode, and whether the per-machine service is needed in Phase 1 or can wait.

**Phase 1: prototype to hand to 2-3 colleagues**
1. Minimal Tauri launcher: home screen, app screen (icon, ▶ play, "Open result"), form generated from the manifest, IPC to the Rust core.
2. Execution in a subprocess (Job Object) with environment from the lockfile (wheels only), runner in the launcher folder, `ctx.progress`, cooperative cancellation.
3. Example apps: a `form` app that loads an Excel file, queries it and returns the result; a small `gui` app (tkinter, stdlib) to prove the second kind.
4. Install from a `.dsapp` file (file association) with safe extraction.
5. `devstribution keygen` / `dev` / `pack` commands for developers (`pack` checks wheels-only and the `run()` signature, and signs the package).

Status: items 3 and 5 are done, together with the core of item 4 (safe extraction and verification in `devstribution-core`) and the runner part of item 2 (`runner/`, protocol v1, cooperative cancellation). Next: the Tauri launcher (item 1), Job Objects and file association, which need Windows.

**Phase 2: trust**
6. Enforcement of the author signature (minisign format, trusted comment), namespace binding (catalog, download source, "unverified name" for TOFU).
7. Free/managed mode with a minimal policy from HKLM; anti-downgrade.
8. Install from GitHub URL and the `devstribution://` scheme; Mark-of-the-Web origin.

## Later phases
- Launcher signing (SignPath or OV), winget publication, MSI package for Intune.
- Per-machine service `devstribution-svc` (if Phase 0 did not make it necessary earlier).
- Remaining app kinds: `web`, `cli`, `background`; `resources` downloads; `pack --test`; `AGENTS.md` / skill for coding agents.
- `.trust`, revocation, key rotation, Event Log logging.
- Public catalog and verification badge.
- Diagnostic package with redaction.
- Optional in-browser mode (Pyodide) for Excel-only apps.
- Donation links in the launcher and the README.
- macOS/Linux support (Tauri makes it possible; SAP apps remain Windows-only).

## Open questions: recommendations
| Question | Recommendation |
|---|---|
| Name | **Decided: devstribution.** Free as of 2026-10-21: GitHub, crates.io, PyPI (`devstribution`, `devstribution-runner`), domains `.com/.dev/.org/.io`. Registration steps in "Project setup" |
| Platforms | **Windows first**, but keep the door open: `platforms` field in the manifest and a Rust core with no Windows dependencies outside dedicated modules |
| License | **MIT OR Apache-2.0** (dual, like Rust, Tauri and uv): permissive, liked by companies, Apache adds the patent clause. The runner under the same license, so apps can have any license |
| Secrets | Windows Credential Manager via the `keyring` crate, one entry per app and secret name. The app receives only the secrets declared in the manifest, via stdin; the launcher asks the user for them on first use and redacts them from logs |
| "Verified" badge | Catalog countersignature for **each single version**, after a checklist: public source, package reproducible from the tag (build in GitHub Actions), no obfuscated code or code downloaded at runtime, permissions consistent with the code. Without review the app stays installable but without the badge |
| Signing tool | Ed25519 implementation in the Rust core, **minisign-compatible format** (interoperable, verifiable with existing tools, same format as the Tauri updater). No Sigstore |
| Extension and URL scheme | **Decided**: extension `.dsapp`, scheme `devstribution://`. Check that `.dsapp` isn't already associated with widespread software |
| Catalog and revocation | Repo in the project's GitHub org, served via GitHub Pages. Catalog key **offline**, not in CI secrets; the revocation list is signed with the same key. Mirror on a network share supported by the policy |
| Sandbox | v1: informational permissions + Job Object (timeout, tree kill, memory limits). Later, consider AppContainer or per-app network blocking via Windows Firewall, only if the cost is reasonable; state it in the threat model |
| Per-machine Python | Decide after Phase 0 which mode is the default and which Python versions the per-machine installer includes |
