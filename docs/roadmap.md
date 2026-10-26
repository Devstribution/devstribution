# Roadmap: from proof of concept to a signed 1.0

The proof of concept works end to end (see [`status_quo.md`](status_quo.md) and the Windows findings
in [`report.md`](report.md)), and a first real app has been delivered with it. This is the work
between that and a launcher we can sign and hand to people who will never read this repository.

Order matters: each step makes the next one meaningful. Signing an installer that ships an unpinned
uv, or packages that can carry client data, would put our signature on the wrong thing.

*Revised 23 October 2026.* Package integrity, the Windows checks and the installer are done. What
changed: making the platform **safe enough for IT to allow** moves before 1.0. The declared
permissions become enforced while an app runs (section 4), and the scan reads deeper (section 5).
Before, these were planned only for managed mode after 1.0, and a first version of managed mode
itself now comes before 1.0 too (section 10). Signing and Phase 0 wait on people outside the
project, so they run alongside.

Status: ☐ to do · ◐ in progress · ☑ done

## Next, in order

1. ◐ **The open findings from real use** (from [`report.md`](report.md)): the data guard
   missing `.json` results ☑, results in a folder the user chose ☑; uninstall easy to find is
   waiting on what users say.
2. ☑ **Runtime guard** (section 4): declared network and SAP use enforced, malware-like behaviour
   stops the run and quarantines the app. Checked in the installed launcher; rule modes set by
   policy come with managed mode.
3. ◐ **Deeper scan** (section 5): dependencies ☑ (malicious packages, look-alike names, start-up
   code under the guard); AMSI and what changed in an update next.
4. ☐ **Managed mode, first version** (section 10): a company policy in HKLM decides which authors
   may be installed. No server needed. Next to tackle (see [`HANDOFF.md`](HANDOFF.md)).
5. **Results shown in the launcher** (section 7): tables and images without opening Excel.
6. **IT-facing evidence** (section 8): Windows Event Log, SBOM, `SECURITY.md`, fuzzing.

At the same time, waiting on others: code signing (the SignPath application, Certum as the
fallback) and Phase 0 on a locked-down company PC (section 3).

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
- ◐ The new UI fixes in the real launcher: menu and uninstall dialog ☑; no sideways scrolling not
  checked on purpose yet.
- ◐ The pre-signing security fixes that only Windows can run (list in
  [`security.md`](security.md)): suspended start in the Job Object ☑, restricted DLL loading ☑,
  the uninstaller removing secrets ☑. Still open: the data folder move with real old data, and uv
  behind a company proxy (needs the company network).

## 3. Installer and signing

- ☑ `npx tauri build`: per-user NSIS installer, no admin rights needed; the uninstaller removes the
  data folder and the apps' secrets.
- ☑ File association (double-click a `.dsapp`) and single instance, from the installed launcher.
- ☑ Mark-of-the-Web: a `.dsapp` downloaded with a browser shows "Downloaded from the internet".
- ☐ Code signing in CI (SignPath Foundation for open-source projects; Certum's open-source
  certificate as the fallback), so SmartScreen and AppLocker/WDAC see a known publisher. Then
  the Windows checks again with the signed installer.
- ☐ Phase 0 of [`plan.md`](plan.md): install and run on a locked-down company PC (AppLocker or
  WDAC, a proxy with TLS inspection, no admin rights). If uv or the runner fails there, it comes
  before everything else.

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
  - **Observed**: programs started, hosts contacted and files written, deleted or moved outside
    the run's folders, once each, in the run log. Each run ends with one line on the files it
    changed, shown under the results as "What it did" with the hosts and programs. ricon and
    `excel-report` run clean ("wrote 1 file in what you chose" / "in its output folder").
  - The hook runs inside the app's process: malware patching memory through `ctypes` can get
    past it. The next item does not depend on the app.
- ☑ **Watching from the launcher** through the run's Job Object, which the app cannot hide from:
  - every new process (`JOB_OBJECT_MSG_NEW_PROCESS`): its command line is read and checked
    against the same rules, so a program started straight through the Windows API is still
    caught (it has started by then; it is ended at once with the rest of the tree);
  - at most 128 processes at once (no fork bombs), and UI restrictions: no switching desktops,
    changing display or system settings, or logging off. Not the clipboard (apps that paste
    need it) and no memory limit (pandas apps legitimately use a lot): both would break real
    apps for little gain.
- ☑ **What happens when a rule fires.** The whole tree is ended, the run is recorded as
  "Stopped by devstribution: the app …", and the app is **quarantined**: it does not run until
  the user decides, also after an update. The app panel shows a red "Blocked by devstribution"
  banner with what it tried, "Uninstall" and "Allow and run again" (which lets that one rule pass
  for this app and still reports it); the Apps list shows "Blocked".
- ◐ **Modes per rule**: fixed for now. Malware tripwires stop the run, undeclared network and SAP
  block the call, programs and hosts are observed. Still to do: rules set by the policy in
  managed mode, and who may release a quarantine there.
- ☑ **Tests**: one test app per rule in `runner/tests/test_guard.py` (CI, Windows / Linux / macOS),
  the shared command-line cases in `runner/tests/process_rule_cases.json` (runner and launcher),
  and `devstribution-host/tests/flow.rs`: stop → quarantine → allow, and on Windows a program
  started through `WinExec`, behind the hook's back, caught by the Job Object. ricon runs clean
  under the guard (only its `cmd /c ver` is observed).
- ☑ **Checked in the installed launcher on Windows** (`report.md`): a stop through the hook with the
  app panel open, and one through the Job Object; the banner, the run refused while
  blocked, and "Allow and run again".

## 5. A deeper scan before install

The scan reads only the package's own Python files today. Extend it where attackers actually hide.

- ☐ **AMSI**: send the package's scripts to Windows' antimalware interface before the install
  button, so Defender or the company's antivirus reviews them too ("Checked by your antivirus").
  The runtime hook sends `exec` / `compile` strings to AMSI as well.
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
  - Still to do: vulnerable (not malicious) versions, which need a fresher source than a bundled
    list (an opt-in online check for IT); wheels with unexpected binaries.
- ☐ **What changed in an update**: the update screen shows the scan and permissions compared with
  the installed version ("New: network use in `sync.py`, starts programs"). A quiet malicious
  update is the realistic attack, and the diff is what a reviewer reads.
- ☐ **Reading more cleverly**: names built from strings (`getattr(mod, "sy" + "stem")`), download
  then run (`exec(requests.get(…).text)`), imports of computed names.
- ☐ **False positives measured**: run the scan over a corpus of ordinary open-source scripts and
  keep warnings rare, so a warning still means something.

## 6. Updates and key lifecycle

- ☐ The launcher updates itself (Tauri updater, signed update feed).
- ☐ App updates: a newer `.dsapp` already updates in place; add a clear "update available" path,
  with the scan diff from section 5.
- ☐ Publisher key rotation: a signed statement from the old key that names the new one, so a
  publisher can move keys without every user reinstalling.
- ☐ Lost or compromised key: a documented way out (forget a pinned publisher in Settings, with a
  strong warning), and revocation in the catalog later.

## 7. User experience

- ☑ New design ("Launchpad"): apps first, recent and most used on Home, filters on Apps, an
  Activity dashboard, apps open in a side panel, settings behind the round button; light and dark.
- ☑ Design language written down ([design.md](design.md)): tokens in one `style.css`, filled
  status colours instead of tints, accent colour and colour-blind safe status colours in Settings.
- ☑ Code scan before install: the install screen lists what the code does (network, programs,
  native code, code built at run time) and flags what looks like malware (hidden code, starting
  with Windows, reading browser passwords, switching off the antivirus, injecting into
  processes). Undeclared network or SAP use is a warning. The same scan warns in `pack` and runs
  as `devstribution scan` (exit code 1 on danger, `--json` for CI).
- ◐ Uninstall easy to find (report: Launcher UX 1): now in every app panel's ⋯ menu, next to Pin and
  Repair; check with users whether that is enough.
- ☑ Results in "a folder the user chose", so stateful apps get an "Open" button (report:
  packaging 2): a `file` output may be inside a folder picked for one of the run's `folder`
  inputs. No format change: older launchers only drop such a result with a warning.
- ☐ **Results shown in the launcher**: preview Excel and CSV outputs as a table, show images and
  charts, without opening another program. With more input types (dates, choices, several
  files), most `form` apps need no window of their own.
- ☐ Error messages reviewed end to end: every failure says what happened and what to do.
- ☑ "Uninstall devstribution" in Settings (report: Launcher UX 4): starts the installer's own
  uninstaller, with its "delete app data" choice, and closes the launcher. Checked in the
  installed launcher.
- ☑ A `.dsapp` renamed to `.zip` by a download installs too (report: Launcher UX 5): picker, drag and
  drop and "Open with". `devstribution://` links (after 1.0) would avoid the renaming.
- ☐ About page with version, licences of bundled software, and where the data folder is.

## 8. Release engineering and evidence for IT

- ☑ Security review of install and run (extraction, signature checks, process supervision, the
  launcher's IPC surface and CSP, dependencies, CI): done before signing, findings and fixes in
  [`security.md`](security.md). Repeat before each release that changes these parts.
- ☐ **Windows Event Log**: installs, runs, blocked calls, stopped runs and quarantines, as events
  IT's monitoring already collects. The platform still sends nothing anywhere.
- ☐ **SBOM**: `pack` writes a CycloneDX list of the app's dependencies into the package.
- ☐ **`SECURITY.md`**: how to report a vulnerability, and what we answer.
- ☐ **Fuzzing** (`cargo-fuzz`) of extraction, the manifest and lockfile parsers, and the scan's
  tokenizer: the parts that read untrusted bytes.
- ☐ Logs a user can find and send (a "Copy diagnostics" button), without secrets in them.
- ☐ Versioning, changelog, release notes; `docs/HANDOFF.md` and `status_quo.md` folded into a
  contributor guide.
- ☐ Docs for app authors: from `devstribution init` to a delivered `.dsapp`.

## 9. Performance: measure first

No complaint so far: repeat runs are fast, and uv's shared cache keeps environments cheap. The one
slow moment is the first install of an app with heavy dependencies (pandas).

- ☐ Time install, first run and later runs for ricon and the examples, and note them in
  `report.md`. Optimise only what the numbers point at (for example, sharing an environment
  between versions with the same lockfile, or starting the runner sooner).

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
  version. *Not yet:* a different guard mode per rule beyond report-only.
- ☑ **Shown in the launcher**: the mode line, the policy in Settings, approved authors on the
  install screen and in the app panel.
- ☑ **Dry run**: logs what would have been refused (`state/policy.log`), without refusing it.
- ☑ **Tests** in `flow.rs` (policy set through `Store::set_policy`) and for the policy reader.
- ☐ **Checked on Windows**: the registry read, the permission check on a local folder and on a
  network share, and the screens ([`managed-mode.md`](managed-mode.md), "Checking it on a test
  PC").

## After 1.0

Phase 2 of the plan: the rest of managed mode (the per-machine service and installs in Program
Files, an ADMX template, IT countersignatures), catalog with GitHub namespace verification,
`devstribution://` links, revocation (a signed list, which can also live on a network share; see
"Possibilities" below); the
`background` app kind; `resources` downloads (refused at install today).

- **`web` apps** ([plan](plan.md), "kinds"): Streamlit, Gradio or NiceGUI apps shown in their own
  launcher window, with a one-time token. This is how an app gets a full custom UI *inside*
  devstribution. Native windows (tkinter, Qt) stay separate: placing them inside the launcher's
  window would be fragile (focus, scaling, crashes).
- **A real sandbox**: apps that declare no network and no files outside their output folder start
  in an AppContainer (or at Low integrity), so the kernel itself blocks the network and writes to
  the user's files. The strongest guarantee, and the most work: environments, Excel COM and the
  output folder all need access granted. The runtime guard (section 4) comes first and tells us
  which apps would fit.
- **The IT review kit** ([plan](plan.md), "IT review before approval"), part of managed mode and
  what IT will ask for first:
  - a review report for each package, nothing executed: signature, permissions, files,
    dependencies, SBOM, flagged code; for updates, the diff against the approved version;
  - a one-click trial in Windows Sandbox (a throwaway virtual machine) whose review mode records
    what the app actually did, using the runtime guard in observe mode;
  - approval by IT countersignature.

  The report needs nothing from managed mode, so it can come earlier if a company asks for it.
- **Other languages: not planned.** The environments, the runner, `ctx`, the scan and the runtime
  guard are all Python-specific; a second language would need each of them again. If needed, a
  "signed executable" kind (no environment, no scan, only the signature and the Job Object) is
  the cheap option, clearly labelled as reviewed less.

## Possibilities

Ideas worth keeping, not planned yet.

- **A public revocation list on GitHub.** One list of bad publisher keys (stolen or malicious)
  and bad package hashes. Every launcher checks it, so a stolen key stops working everywhere
  without each IT department removing the author by hand. Private keys never go on it.
  GitHub only hosts the file. What makes the list hard to tamper with is checked by the launcher:
  - **Signed offline.** The launcher has the list's public key built in and refuses any list whose
    signature does not match. Editing the file on GitHub, or anywhere between GitHub and the user,
    gets nowhere.
  - **Numbered.** Launchers never accept an older list than the one they have, so a replayed old
    copy cannot bring revoked keys back.
  - **Expiring.** A list that is overdue (deleted, blocked, held back) is noticed: managed mode
    installs nothing new, free mode warns. Offline, the last list that checked out still applies.
  - **Hard to sign alone.** Ideally two of three keys, held by different people, must sign each
    list. An offline root key certifies the key that signs lists, so that key can be replaced
    without a new launcher.
  - **Mirrorable.** The file carries its own signature, so IT can copy it to an internal share
    (many company networks block github.com) and point the policy at the copy, or turn it off.

  The list only blocks. A public list of *trusted* publishers would be worth only as much as the
  vetting behind it. At most, it would show free-mode users a "known publisher" hint, and in
  managed mode IT's approval would still decide.
- **Apps pushed by IT.** An `Apps` policy value naming a folder. At each start, the launcher
  installs or updates every `.dsapp` in it from an approved author, and removes the apps IT took
  out. Today colleagues double-click the packages themselves.
