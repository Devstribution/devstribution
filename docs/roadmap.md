# Roadmap: from proof of concept to a signed 1.0

The proof of concept works end to end (see [`status_quo.md`](status_quo.md) and the Windows findings
in [`report.md`](report.md)), and a first real app has been delivered with it. This is the work
between that and a launcher we can sign and hand to people who will never read this repository.

Order matters: each step makes the next one meaningful. Signing an installer that ships an unpinned
uv, or packages that can carry client data, would put our signature on the wrong thing.

*Revised 26 October 2026.* The runtime guard, the dependency check, the update diff and a first
managed mode are built, so the work left is sorted into three stages instead of one list:

- **[A. First public release](#a-before-the-first-public-release-unsigned)**, unsigned: SignPath
  signs only a project that is "already released in the form that should be signed", so a public
  release has to come before the application, not after it.
- **[B. Signed 1.0](#b-before-a-signed-10)**: what has to hold before our signature goes on it.
- **[C. After 1.0](#c-after-10)**: wanted, but nothing about 1.0 depends on it.

*27 October 2026:* a scan of the whole codebase. What broke a promise the docs already made was
fixed in 0.2.0 ([`security.md`](security.md), "Codebase scan"); the rest is sorted into the
stages below (A10–A13, B11–B14, C).

*Earlier revision, 23 October 2026:* making the platform safe enough for IT to allow moved before
1.0: the runtime guard (section 4), the deeper scan (section 5) and a first managed mode
(section 10), all planned for after 1.0 before.

The numbered sections after the stages are the record per area: what is done, and where each open
item went. Status: ☐ to do · ◐ in progress · ☑ done

## A. Before the first public release (unsigned)

What a stranger downloading it meets first, and what the SignPath application needs. Small items.

1. ☑ **A release workflow in CI** (GitHub Actions): builds the NSIS installer and the CLI on
   Windows from a tag and attaches them to a GitHub release, with checksums. Signing is added to
   the same workflow later (SignPath signs CI builds, not ones made on a PC).
2. ☑ **Versioning, changelog and release notes**, then `v0.1.0` published as a pre-release.
   SmartScreen warns about the unsigned installer; the release notes say so. How a release is
   made: [`releasing.md`](releasing.md).
3. ☑ **A code signing policy** (README or its own page), as SignPath projects publish: who may
   approve a signed release, the attribution to SignPath, and what the launcher sends over the
   network (nothing of ours: uv, Python and packages, later the revocation list).
   28 October: [`code-signing-policy.md`](code-signing-policy.md), linked from the README and
   the release notes.
4. ☑ **An About page**: version, licences of bundled software, where the data folder is
   (section 7). 27 October: in Settings, with the licence texts of everything shipped
   (`THIRD-PARTY-NOTICES.txt`, made by `scripts/third_party_notices.py`, shipped with the installer).
5. ☑ **Error messages reviewed end to end**: every failure says what happened and what to do,
   and a launcher that fails to start shows an error screen, not a blank window (section 7).
   27 October: the window shows "devstribution could not start" with the error and "Try again"
   when it cannot set itself up (also when the first page fails to load), uncaught errors become
   a message, and the messages a user is most likely to meet say what to do: a damaged
   `state.json`, a failed save, an install that is no longer pending, uv missing or failing
   (internet or proxy), a Python that will not start, the credential store. 28 October: the rest
   of the host's messages; a failed run says what to try (a file open in Excel, no connection,
   Repair) or that the problem is in the app, with "Copy details" for its author; apps can raise
   `AppError` with a message written for the user.
6. ☑ **"Copy diagnostics"**: logs a user can find and send, without secrets in them (section 8).
   27 October: the launcher writes `logs\launcher.log` (starts, failed commands, panics), Settings
   shows where, and "Copy diagnostics" puts versions, apps, the latest runs and the end of that log
   on the clipboard.
7. ☑ **Docs for app authors, the basics**: from `devstribution init` to a delivered `.dsapp`
   (section 8). 28 October: [`authors.md`](authors.md), linked from the README and from the
   `AGENTS.md` that `init` writes.
8. ☑ **On GitHub (repository owner)**: private vulnerability reporting on (`SECURITY.md` points to
   it), two-factor authentication for everyone with write access. 28 October.
9. ☐ **Apply to SignPath Foundation**, with the release linked. If refused (the project is young),
   Certum's open-source certificate is the fallback.
10. ☑ **The install dialog can be cancelled** (button and Escape) while it inspects or installs.
    27 October: Cancel and Escape work while the package is checked, on the confirmation screen
    and on the finished screen; a package checked after Cancel is discarded. 28 October: also
    while the environment is prepared (B12).
11. ☑ **No silent behaviour** (section 7; 27 October):
    - an empty launcher says "No apps yet", not "No app matches these filters" and "All 0 apps";
    - dropping several packages says only the first is installed;
    - cancelling the secret prompt says the run was cancelled;
    - "Forget" asks before deleting a secret;
    - repair shows its progress (the events exist);
    - a missing log looks different from an empty one.
12. ☑ **Keyboard and screen reader basics** (section 7; 28 October):
    - dialogs (install, app panel, Settings, run details, clean-up, the secret prompt) keep focus
      inside, give it back when they close, and are marked `aria-modal`;
    - the segmented controls and chips keep focus on click, radio groups take arrow keys, and the
      `⋯` menu opens on its first item, takes arrow keys, Home, End and Escape;
    - run progress (each step, every quarter), a run's result and install steps are announced;
    - error toasts stay until closed;
    - motion follows `prefers-reduced-motion`.
13. ☑ **Docs that match the release** (28 October): `README.md` and `SECURITY.md` describe the 0.x
    pre-releases, `status_quo.md` has current test counts and Windows checks, and
    `tauri.conf.json` builds only the Windows installer.

## B. Before a signed 1.0

What has to hold before our signature goes on it, roughly in this order.

1. ☐ **Phase 0** of [`plan.md`](plan.md): install and run on a locked-down company PC (AppLocker or
   WDAC, a proxy with TLS inspection, no admin rights), uv behind the company proxy included. If
   uv or the runner fails there, it comes before everything else (section 3).
2. ☐ **Managed mode checked on Windows**: the registry read, the permission check on a local
   folder and on a network share, and the screens (section 10).
3. ☐ **The remaining Windows checks**: the data folder move with real old data; no sideways
   scrolling (section 2).
4. ☐ **Signing in CI**, once SignPath (or Certum) has said yes, then the Windows checks again with
   the signed installer (section 3).
5. ☑ **The launcher updates itself** (Tauri updater, signed update feed): without it every fix means
   asking users to reinstall by hand (section 6). 28 October: each release carries `latest.json`
   with the installer's updater signature (a key of its own, in the repository's secrets); the
   launcher looks at start and from Settings, shows the notes, and installs only an installer
   signed for the version announced, not while apps run. Rolling out = publishing a release that
   is not a pre-release. Users can turn the check at start off, IT can turn updates off
   (`LauncherUpdates`). Still to see: a real update from one release to the next.
6. ☑ **Revocation, first two steps** ([`revocation.md`](revocation.md), "Building it, in order"):
   the list check with a bundled copy and `revocations import`, then owners' withdrawal and
   rotation statements. A launcher that cannot read a list can never learn of a revoked key, so
   this goes in before many launchers are out there (section 6). 29 October: lists (the chain from
   a root key, serials that only go up, expiry), a list next to the launcher used while it is the
   newest, `devstribution keys withdraw` / `keys rotate` / `revocations import`, Settings'
   *Revoked keys* with *Import*; checked at inspect, install and before every run, as the table in
   `revocation.md` says (free mode asks once per app before a run, managed mode refuses). The root
   keys and the first list are step 3: until then no list verifies, and owners' statements are
   what works.
7. ☑ **Fuzzing** (`cargo-fuzz`) of extraction, the manifest and lockfile parsers and the scan's
   tokenizer: the parts that read untrusted bytes (section 8). 30 October: six targets
   ([`fuzz/README.md`](../fuzz/README.md)), also signed packages with hostile contents, file lists,
   key statements and revocation lists; nightly in CI (`fuzz.yml`), on Linux. The first runs, 5 to
   7 minutes per target, found nothing; with the path check taken out on purpose, the extraction
   target found the escape within a minute (Linux). The two extraction targets also ran by hand on
   Windows and found nothing; CI does not fuzz on Windows.
8. ☐ **False positives measured**: the scan over a corpus of ordinary open-source scripts, so a
   warning still means something (section 5).
9. ☑ **An "update available" path** for apps: a newer `.dsapp` already updates in place with the
   diff (section 6). 29 October: folders the user chooses and one IT sets are checked at start;
   newer versions signed by the app's publisher show on the app, and installing one still goes
   through the install screen and its diff. Still to see: a real network share.
10. ☐ **Security review again** for what changed since the last one (guard file rules, managed
    mode, updates), as section 8 asks before each such release. Weaknesses still open from the
    October scan are tracked privately until fixed, as `SECURITY.md` asks reporters to send them.
11. ☑ **`state.json` survives a crash**: written to disk before the rename, with a backup the
    launcher recovers from. 28 October: `state.json.bak` holds the list from before the latest
    change; a file that cannot be read is moved to `state.json.damaged` and the copy takes its
    place (noted in `launcher.log`). With no copy, the message says what to move away.
12. ☑ **uv can be stopped**: a timeout and Cancel during installs, and uv inside a Job Object
    like runs, so a stalled proxy cannot leave the install screen stuck (with B1). 28 October:
    uv starts suspended in a Job Object; Cancel (button or Escape) or 30 minutes end it and
    everything it started, and the half-made environment is removed. Repair's progress has a
    Cancel too; the old environment stays until a new one is complete.
13. ☑ **Release workflow hardened**: runs only from `main` after CI has passed, attests
    `SHA256SUMS.txt` (GitHub artifact attestations), says plainly what the checksums prove, and
    `releasing.md` lists refreshing the dependency data (section 5) before each release.
    28 October. Still to see: a real tag run (the first release after this one).
14. ☑ **CI and tests**: done 28 October: the minimum Rust version (1.90) checked, `npm audit`
    including dev dependencies, job timeouts in `ci.yml`, Python 3.10 (the runner's minimum), and
    a `SAFETY` comment on each `unsafe` block (clippy refuses one without). 29 October:
    `cargo deny` in place of `cargo audit` (also licences we can ship and crates.io as the only
    source, `deny.toml`; the crates are marked unpublished), timeouts in `release.yml`, the
    examples packed, verified and scanned in CI, tests for the checks the launcher's commands make
    themselves ("open result" opens only a run's results, when it may update, which arguments
    are packages), and Biome linting the UI.

## C. After 1.0

Wanted, not needed for 1.0. **AMSI** and the **Windows Event Log** move up to B as soon as a real
company evaluates devstribution: IT asks for both.

- **Scan** (section 5): AMSI; vulnerable (not malicious) versions, through an opt-in online check;
  unexpected binaries in wheels; reading more cleverly (names built from strings, download then
  run); what changed inside a dependency's own code.
- **Guard and managed mode** (sections 4 and 10): a mode per rule set by policy, beyond
  report-only; who may release a quarantine in managed mode; "compare with the version IT
  approved".
- **Revocation, the rest** ([`revocation.md`](revocation.md)): the `Devstribution/trust` repository
  with its offline keys, fetching the list, the policy values, two-of-three signing.
- **Evidence for IT** (section 8): Windows Event Log; SBOM written by `pack`.
- **Results shown in the launcher** (section 7): Excel and CSV as tables, images and charts, more
  input types.
- **Performance** (section 9): measure first; nobody has complained.
- **Launcher, smaller items** (section 7):
  - plainer trust wording on the main screens (no "namespace", key IDs or "Free mode ·
    signatures checked"), and a first-run explanation of publishers and trust;
  - Settings: stop trusting a publisher (☑ 27 October: "Open folder" for the data folder);
  - run folders pruned after a while (☑ 27 October as a choice, not automatic: Settings → Storage →
    "Clean up…" removes old results after showing the size, and keeps the runs in Activity); one run
    at a time per app; table results over 16 KB (today "ended without a result");
  - the update screen shows a changed title or icon (☑ 27 October: it compares package versions
    like `1.0rc1` the way PEP 440 orders them);
  - other languages; an accessible version of the runs chart.
- **CLI**: `verify` shows the scan and can check against a `.trust` file. (☑ 27 October:
  `scan` and `diff` exit 2 when the package cannot be read, 1 for danger; `trust export` asks
  before overwriting, `--force` skips the question.)
- **Contributors**: the contributor guide that section 8 plans, folded from `HANDOFF.md` and
  `status_quo.md` after the first release. (☑ 27 October: `CONTRIBUTING.md`, issue and PR
  templates.)
- **Phase 2 of the plan**: the rest of managed mode (the per-machine service and installs in
  Program Files, an ADMX template, IT countersignatures), the catalog with GitHub namespace
  verification and a "known publisher" hint, `devstribution://` links, the `background` app kind,
  `resources` downloads (refused at install today).
- **`web` apps** ([plan](plan.md), "kinds"): Streamlit, Gradio or NiceGUI apps shown in their own
  launcher window, with a one-time token. This is how an app gets a full custom UI *inside*
  devstribution. Native windows (tkinter, Qt) stay separate: placing them inside the launcher's
  window would be fragile (focus, scaling, crashes).
- **A real sandbox**: apps that declare no network and no files outside their output folder start
  in an AppContainer (or at Low integrity), so the kernel itself blocks the network and writes to
  the user's files. The strongest guarantee, and the most work: environments, Excel COM and the
  output folder all need access granted. The runtime guard (section 4) tells us which apps would
  fit.
- **The IT review kit** ([plan](plan.md), "IT review before approval"):
  - a review report for each package, nothing executed: signature, permissions, files,
    dependencies, SBOM, flagged code; for updates, the diff against the approved version
    (`devstribution diff` is the start of it);
  - a one-click trial in Windows Sandbox (a throwaway virtual machine) whose review mode records
    what the app actually did, using the runtime guard in observe mode;
  - approval by IT countersignature.

  The report needs nothing from managed mode, so it can come earlier if a company asks for it.
- **Other languages: not planned.** The environments, the runner, `ctx`, the scan and the runtime
  guard are all Python-specific; a second language would need each of them again. If needed, a
  "signed executable" kind (no environment, no scan, only the signature and the Job Object) is
  the cheap option, clearly labelled as reviewed less.

### Possibilities

Ideas worth keeping, not planned yet.

- **A list of trusted publishers**: it would be worth only as much as the vetting behind it, so at
  most a "known publisher" hint in free mode. (The list of *revoked* keys is designed: B6 and
  [`revocation.md`](revocation.md).)
- **Apps pushed by IT.** An `Apps` policy value naming a folder. At each start, the launcher
  installs or updates every `.dsapp` in it from an approved author, and removes the apps IT took
  out. Today colleagues double-click the packages themselves.

## Findings from real use

From [`report.md`](report.md): the data guard missing `.json` results ☑, results in a folder the
user chose ☑, uninstall easy to find ◐ (in every app panel's ⋯ menu; waiting on what users say).

## 1. Package integrity

What goes into a `.dsapp`, and what the launcher runs it with, must be exactly what we intend.

- ☑ **`pack` leaves out what it should.** A `.dsappignore` file (`.gitignore` syntax) next to
  `pyproject.toml`, plus the app folder's own `.gitignore`. Files that look like data
  (spreadsheets, databases, archives, logs) are refused unless a `!pattern` lists them on purpose,
  so client data cannot end up signed by accident. `pack --list` shows what would be packed.
  (From the first real packaging: `app_1/output/` nearly went into `ricon.dsapp`.)
- ☑ **The data guard also catches what sits next to data** (report: packaging 5). Repacking ricon,
  `output/ultimo_esito.json` (client figures) passed because JSON is too common as configuration
  to refuse everywhere. Now a folder that holds refused data files has its other files refused
  too, apart from Python code and files listed on purpose.
- ☑ **Pinned uv.** The launcher ships a fixed uv release, downloaded and checked against its sha256,
  instead of whatever uv is on the build machine's PATH.
- ☑ **Package format 1 is frozen.** A written specification ([`format.md`](format.md)) and a promise:
  every later launcher installs schema 1 packages; a package needing a newer launcher says so
  plainly instead of failing with a parse error.

## 2. Windows checks (by hand, on a real PC)

The parts that compile on Windows but have never run there. Each gets a result in `report.md`.

- ☑ Cancel, timeout and closing the launcher kill the whole process tree (Job Object).
- ☑ Excel started through COM is closed after a run, also after Cancel.
- ☑ Secrets are saved to and read from Credential Manager.
- ☑ `gui` apps start with `pythonw.exe`, without a console window.
- ◐ The new UI fixes in the real launcher: menu and uninstall dialog ☑; no sideways scrolling →
  B3.
- ◐ The pre-signing security fixes that only Windows can run (list in
  [`security.md`](security.md)): suspended start in the Job Object ☑, restricted DLL loading ☑,
  the uninstaller removing secrets ☑. The data folder move with real old data → B3; uv behind a
  company proxy (needs the company network) → B1.

## 3. Installer and signing

- ☑ `npx tauri build`: per-user NSIS installer, no admin rights needed; the uninstaller removes the
  data folder and the apps' secrets.
- ☑ File association (double-click a `.dsapp`) and single instance, from the installed launcher.
- ☑ Mark-of-the-Web: a `.dsapp` downloaded with a browser shows "Downloaded from the internet".
- ☑ A release workflow and a first unsigned release (v0.1.0, 26 October) → A1, A2.
- ☐ Code signing (SignPath Foundation for open-source projects; Certum's open-source certificate
  as the fallback), so SmartScreen and AppLocker/WDAC see a known publisher: apply → A9, sign in
  CI → B4.
- ☐ Phase 0 on a locked-down company PC → B1.

## 4. Runtime guard: enforce what an app declared

The permissions on the install screen were only what the publisher *declares*
([`security.md`](security.md), limit 2). The guard now watches every run from two places and acts
on what it sees. Neither needs admin rights. The process rules live in one file,
`runner/devstribution_runner/_process_rules.json`, read by both.

- ☑ **Audit hook in the runner** (`_guard.py`, `sys.addaudithook`, PEP 578), installed before any
  app code is imported; a hook cannot be removed. It checks what Python reports against the
  manifest:
  - **Undeclared use is blocked**: without `permissions.network`, any connection or name lookup
    other than this PC fails with a `PermissionError` that names the missing permission; with a
    list of hosts, only those (and the system's proxy) resolve, and connections go only to the
    addresses they resolved to. SAP's RFC library (`pyrfc`) without `permissions.sap` the same.
  - **Tripwires stop the run** before the call happens: start-with-Windows entries (Run and
    Winlogon keys, services, the Startup folder, `schtasks /create`), the antivirus's keys and
    settings, browser password and cookie stores, the Windows tools malware uses (`powershell -enc`,
    download-and-run, `certutil -decode`, `mshta`, …), deleting backups, switching off the
    firewall, process injection and keyboard hooks (native functions looked up through `ctypes`),
    listing every saved Windows password, and loading compiled code outside imports (`marshal`).
  - ☑ **Files** (26 October): stops on reading saved keys and passwords (SSH keys, Windows
    credentials, password databases, Outlook `.pst`/`.ost`, wallets), writing to Windows,
    Program Files, PowerShell profiles, devstribution's data folder, other apps or the app's own
    Python, reading another run's results, running or loading a program the run wrote itself,
    and changing more than 50 existing files outside the output folder, the user's choices and
    the temporary folder. What the user chose through inputs is the app's. Cloud credentials
    (`.aws`, `.azure`, …), which cloud libraries read on their own, are reported, not stopped.
    Paths are resolved as the system names them before they are compared, so the rules hold on
    macOS and Linux too (CI caught that they did not at first).
  - **Observed**: programs started, hosts contacted and files written, deleted or moved outside
    the run's folders, once each, in the run log. Each run ends with one line on the files it
    changed, shown under the results as "What it did" with the hosts and programs. ricon and
    `excel-report` run clean ("wrote 1 file in what you chose" / "in its output folder").
  - ☑ **Other Pythons** (27 October): a Python the app starts through `subprocess` or
    `multiprocessing` starts as `python -I -S -B … child.py <settings> …`, so the same guard is
    installed before its code runs. The settings sit in a folder the guard seals, and the
    children's reports reach the launcher through the runner: a stop in a child ends the run.
    A Python started any other way (a shell, `os.system`, a second runner) is stopped.
  - The hook runs inside the app's process: malware patching memory through `ctypes` can get
    past it. The next item does not depend on the app.
- ☑ **Watching from the launcher** through the run's Job Object, which the app cannot hide from:
  - every new process (`JOB_OBJECT_MSG_NEW_PROCESS`): its command line is read and checked
    against the same rules, so a program started straight through the Windows API is still
    caught (it has started by then; it is ended at once with the rest of the tree), and so is
    a Python that is not the runner or a guarded child;
  - at most 128 processes at once (no fork bombs), and UI restrictions: no switching desktops,
    changing display or system settings, or logging off. Not the clipboard (apps that paste
    need it) and no memory limit (pandas apps legitimately use a lot): both would break real
    apps for little gain.
- ☑ **What happens when a rule fires.** The whole tree is ended, the run is recorded as
  "Stopped by devstribution: the app …", and the app is **quarantined**: it does not run until
  the user decides, also after an update. The app panel shows a red "Blocked by devstribution"
  banner with what it tried, "Uninstall" and "Allow and run again" (which lets that one rule pass
  for this app and still reports it); the Apps list shows "Blocked".
- ◐ **Modes per rule**: malware tripwires stop the run, undeclared network and SAP block the call,
  programs and hosts are observed; managed mode can make every rule report-only and decide whether
  users may allow a stopped rule (section 10). A different mode per rule, and who may release a
  quarantine there → C.
- ☑ **Tests**: one test app per rule in `runner/tests/test_guard.py` (CI, Windows / Linux / macOS),
  the shared command-line cases in `runner/tests/process_rule_cases.json` (runner and launcher),
  and `devstribution-host/tests/flow.rs`: stop → quarantine → allow, and on Windows a program
  started through `WinExec`, behind the hook's back, caught by the Job Object. ricon runs clean
  under the guard (only its `cmd /c ver` is observed).
- ☑ **Checked in the installed launcher on Windows** (`report.md`): a stop through the hook with the
  app panel open, and one through the Job Object; the banner, the run refused while
  blocked, and "Allow and run again".

## 5. A deeper scan before install

The scan read only the package's own Python files. Extend it where attackers actually hide.

- ☑ **Dependencies** (`devstribution-core/src/deps.rs`), offline from bundled lists refreshed by
  `scripts/update_dep_data.py` before each release, so `pack` and the launcher send nothing:
  - packages reported as malicious on PyPI (OSV `MAL-` entries, about 11,900): the whole package,
    or only the hijacked versions (`num2words` 0.5.15); a danger finding, so the red banner;
  - names one edit away from the 1,000 most downloaded packages (`requezts`): a warning. The top
    15,000 count as known, and adding or dropping a digit (`elasticsearch8`) does not count.
    Measured: about 1% of the packages ranked 5,000 to 15,000 would have matched;
  - code a dependency runs when Python starts (`.pth` files): the runner now starts with `-S` and
    loads `site` after installing the guard, so that code is checked like the app's, and each
    such file is reported in the run log.
  - Vulnerable (not malicious) versions, which need a fresher source than a bundled list, and
    wheels with unexpected binaries → C.
- ☑ **What changed in an update** (26 October): `devstribution_core::diff` compares two versions:
  permissions, other declarations, packages in `uv.lock` (added, removed, older), the code scan's
  findings (new ones by rule and file, so moved lines do not count) and every file (lines added
  and removed; programs and compiled files flagged as unreadable). The update screen leads with
  what deserves a look, an amber "asks for more than before" banner and "Update anyway" when
  permissions widen, and folds every change below. `devstribution diff <old> <new>` (folders or
  `.dsapp` files, `--code` for the changed lines, `--json`, exit code 1 when the update adds
  something malware-like) is what an IT reviewer reads. What changed inside a dependency's own
  code, and "compare with the version IT approved" in managed mode → C.
- ☐ **False positives measured** → B8.
- ☐ **AMSI**: send the package's scripts to Windows' antimalware interface before the install
  button, so Defender or the company's antivirus reviews them too ("Checked by your antivirus");
  the runtime hook sends `exec` / `compile` strings to AMSI as well → C (B when a company asks).
- ☐ **Reading more cleverly**: names built from strings (`getattr(mod, "sy" + "stem")`), download
  then run (`exec(requests.get(…).text)`), imports of computed names → C.

## 6. Updates and key lifecycle

- ☑ The launcher updates itself (Tauri updater, signed update feed) → B5.
- ☑ App updates: a newer `.dsapp` updates in place, with the diff from section 5; folders the user
  chooses and one IT sets are checked at start, and newer signed versions show on the app → B9.
  There is no download feed: fetching from a URL would need its own trust design.
- ◐ Publisher key rotation and lost or compromised keys: the owner's signed statements, and a
  signed revocation list of bad keys and packages with evidence, advice in free mode and IT's call
  in managed mode ([design](revocation.md), agreed October 2026). The list check and owners'
  statements → B6; the trust repository, fetching and the policy values → C.

## 7. User experience

- ☑ New design ("Launchpad"): apps first, recent and most used on Home, filters on Apps, an
  Activity dashboard, apps open in a side panel, settings behind the round button; light and dark.
- ☑ Design language written down ([design.md](design.md)): tokens in one `style.css`, filled
  status colours instead of tints, accent colour and colour-blind safe status colours in Settings.
- ☑ Code scan before install: the install screen lists what the code does (network, programs,
  native code, code built at run time) and flags what looks like malware (hidden code, starting
  with Windows, reading browser passwords, switching off the antivirus, injecting into
  processes). Undeclared network or SAP use is a warning. The same scan warns in `pack` and runs
  as `devstribution scan` (exit code 1 on danger, 2 when the target cannot be read, `--json` for CI).
- ◐ Uninstall easy to find (report: Launcher UX 1): now in every app panel's ⋯ menu, next to Pin and
  Repair; check with users whether that is enough.
- ☑ Results in "a folder the user chose", so stateful apps get an "Open" button (report:
  packaging 2): a `file` output may be inside a folder picked for one of the run's `folder`
  inputs. No format change: older launchers only drop such a result with a warning.
- ☑ "Uninstall devstribution" in Settings (report: Launcher UX 4): starts the installer's own
  uninstaller, with its "delete app data" choice, and closes the launcher. Checked in the
  installed launcher.
- ☑ A `.dsapp` renamed to `.zip` by a download installs too (report: Launcher UX 5): picker, drag and
  drop and "Open with". `devstribution://` links (after 1.0) would avoid the renaming.
- ☑ Error messages reviewed end to end → A5.
- ☑ About page with version, licences of bundled software, and where the data folder is → A4.
- ☑ Install dialog cancellable, no silent behaviour, keyboard and screen reader basics → A10–A12;
  smaller items → C.
- ☐ **Results shown in the launcher**: preview Excel and CSV outputs as a table, show images and
  charts, without opening another program. With more input types (dates, choices, several
  files), most `form` apps need no window of their own → C.

## 8. Release engineering and evidence for IT

- ☑ Security review of install and run (extraction, signature checks, process supervision, the
  launcher's IPC surface and CSP, dependencies, CI): done before signing, findings and fixes in
  [`security.md`](security.md). Repeat before each release that changes these parts → B10.
- ☑ **`SECURITY.md`**: how to report a vulnerability (privately, through GitHub), what is in
  scope, and what we answer. ☑ Private vulnerability reporting turned on → A8.
- ☑ Versioning, changelog, release notes → A2. ☐ `docs/HANDOFF.md` and `status_quo.md` folded into a
  contributor guide, after the first release.
- ☑ Logs a user can find and send (a "Copy diagnostics" button), without secrets in them → A6.
- ☑ Docs for app authors: from `devstribution init` to a delivered `.dsapp` → A7.
- ☑ **Fuzzing** (`cargo-fuzz`) of extraction, the manifest and lockfile parsers, and the scan's
  tokenizer → B7.
- ☐ **Windows Event Log**: installs, runs, blocked calls, stopped runs and quarantines, as events
  IT's monitoring already collects. The platform still sends nothing anywhere → C (B when a
  company asks).
- ☐ **SBOM**: `pack` writes a CycloneDX list of the app's dependencies into the package → C.

## 9. Performance: measure first

No complaint so far: repeat runs are fast, and uv's shared cache keeps environments cheap. The one
slow moment is the first install of an app with heavy dependencies (pandas).

- ☐ Time install, first run and later runs for ricon and the examples, and note them in
  `report.md`. Optimise only what the numbers point at (for example, sharing an environment
  between versions with the same lockfile, or starting the runner sooner) → C.

## 10. Managed mode, first version

Moved here from "After 1.0" on 23 October 2026. It is what a company's IT asks for before
allowing devstribution on its PCs, and it needs no server: the policy comes through the registry
(GPO or Intune), and the approved authors are files IT can keep on a network share. The design is
in [`plan.md`](plan.md), "Trust model and policy".

The first version is in the code; how IT sets it up is in [`managed-mode.md`](managed-mode.md).

- ☑ **Policy from HKLM.** Managed mode when `HKLM\SOFTWARE\Policies\devstribution` exists,
  free mode as today otherwise. The allowed authors are listed inline, in `.trust` files in a
  folder, or in a TOML policy file. Files and folders users can write are refused, and a policy
  that cannot be read fails closed.
- ☑ **`.trust` files** (an author's namespace and public keys): `devstribution trust export`
  writes one, `trust show` prints its fingerprints. In managed mode, only approved authors
  install and run: no trust on first use and no "Install anyway".
- ☑ **A helpful block message**: the reasons, who to ask, and a ready-to-forward `.trust`
  ("Save a request for IT…").
- ☑ **Policy options**: scan and dependency danger findings (refuse or let the user decide),
  guard rules made report-only, whether users may allow a stopped rule, the minimum launcher
  version. A different guard mode per rule → C.
- ☑ **Shown in the launcher**: the mode line, the policy in Settings, approved authors on the
  install screen and in the app panel.
- ☑ **Dry run**: logs what would have been refused (`state/policy.log`), without refusing it.
- ☑ **Tests** in `flow.rs` (policy set through `Store::set_policy`) and for the policy reader.
- ☐ **Checked on Windows**: the registry read, the permission check on a local folder and on a
  network share, and the screens ([`managed-mode.md`](managed-mode.md), "Checking it on a test
  PC") → B2.
