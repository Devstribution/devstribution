# Roadmap: from proof of concept to a signed 1.0

The proof of concept works end to end (see the [changelog](../CHANGELOG.md) and the Windows
findings in [`report.md`](report.md)), and a first real app has been delivered with it. This is the work
between that and a launcher we can sign and hand to people who will never read this repository.

*Revised 9 November 2026*, after 0.5.0 (earlier: 5 November, after 0.4.0 and a technical
assessment of performance and security).
What blocks a signed 1.0 is no longer code: it is **evidence** (the launcher checked on a locked-down
company PC and in managed mode) and **reputation** (SignPath Foundation wants a project with a
"verifiable reputation", which a two-week-old repository with no outside users does not have yet).
0.5.0 went after both and fixed what the assessment found; 0.6.0 makes the launcher ready
for a company network and builds the reputation:

- **[0.6.0](#060-released-9-november)**: released with Tiers 2 and 3 mostly done and a security
  review's fixes; what is left (P1, P2, P6, P7, P13, Tier 4) goes on in 0.7.0.
- **[0.5.0](#050-released-6-november)**: the assessment's findings, a stand-in for a company PC,
  signing applied for on both routes, and what the first outside users meet.
- **[A. First public release](#a-first-public-release-done)**: done.
- **[B. Signed 1.0](#b-before-a-signed-10)**: what has to hold before our signature goes on it.
- **[C. After 1.0](#c-after-10)**: wanted, but nothing about 1.0 depends on it.

The numbered sections after the stages are the record per area. Earlier revisions (23, 26 and
27 October) and the details of finished items are in the git history and
[`CHANGELOG.md`](../CHANGELOG.md). Status: ☐ to do · ◐ in progress · ☑ done

## 0.6.0 (released 9 November)

Released with P3, P4, P5, P8, P9, P10, P11, P12 and P14 done or nearly (each says what is left),
and the fixes of the security review of 9 November ([`security.md`](security.md)): "Open result"
no longer opens documents that run code, a package's signature is checked before anything is
unpacked, and the Excel preview has the package's size limits. Not done, and carried to 0.7.0:
P1 and P2 (the Windows proxy and the package mirror), P6 (Italian), P7 (several files in one
input), P13 (example apps) and Tier 4.

*Proposed 9 November*, from a scan of the product after 0.5.0: what a company network, the
people who run apps and the people who write them would run into next. Carried over from 0.5.0:
N4 (run B1 and B2 on the stand-in) and N5 (signing), both on the maintainer's side.

**Tier 1: what the first company PC would likely trip on.** B1 cannot pass without these, so they
come first; each is checked on the stand-in (N4).

- ☑ **P1. The proxy Windows is set to.** uv, which downloads Python and the packages, reads the
  proxy from `HTTPS_PROXY` / `HTTP_PROXY`. Company PCs usually set it in Windows instead, often as
  an automatic configuration script (PAC) or WPAD, and set no variable: installs would then
  likely fail with a network error. Done: when no variable is set, the launcher asks
  Windows (WinHTTP, which runs the script or the detection) which proxy it would use for
  files.pythonhosted.org and for Python's hosts (releases.astral.sh, then GitHub; or the Python
  mirror), and gives that to uv as
  `HTTPS_PROXY`, with the hosts that go direct in `NO_PROXY`; a script that cannot be read falls
  back to the fixed proxy, as Windows does. The update check does the same. "Copy diagnostics"
  and a failed download's message say which proxy was used and where it came from. Tested
  against the real WinHTTP with a script served from the test itself, and on the stand-in (N4):
  a standard user, a TLS-inspecting proxy given to Windows only by a PAC file, no variable set,
  Python downloaded through it, the mirror reached directly as the PAC said, and the installed
  launcher's update check sent through it; a PAC that names a proxy that is down fails and
  names it. A proxy that wants a Windows login (NTLM, Kerberos) is still named in the error as
  such, since uv cannot log in to one.
- ☑ **P2. The company's package mirror.** Many companies block pypi.org and serve packages from
  their own mirror (Artifactory, Nexus). A policy value names it, and uv downloads from there.
  The signed lockfile's hashes still decide what installs, so a mirror cannot change a package;
  packages keep naming only PyPI (`lock::check_sources`). Done: `PackageIndex` (`https://`, no
  login). `uv sync --frozen` downloads from the lockfile's own addresses whatever index is set,
  so with a mirror the lockfile is exported with its hashes and installed with
  `uv pip sync --require-hashes --no-build` from the mirror. Tested with real uv against a mirror
  served by the test: the package installs from it, and a file with other bytes is refused. The
  proxy is asked for the mirror's host, and a failed download names the mirror. Checked on the
  stand-in (N4) with PyPI blocked: pandas, openpyxl and numpy installed from a mirror over HTTPS,
  and a mirror that is down gets "Ask your IT department to check the mirror at …".
- ☑ **P3. Where Python comes from.** uv downloads Python from releases.astral.sh, then GitHub,
  which some companies block for downloads. A policy value names a mirror (uv's
  `python-install-mirror`), checked the same way. Done: `PythonMirror` (`https://` or
  `file:///`), shown in "Copy diagnostics". Checked on the stand-in (N4) with GitHub and
  releases.astral.sh blocked: Python installs from the mirror, and a tampered Python on the
  mirror is refused. Its message then blamed the connection; it now points at the mirror
  (checked in a unit test, not yet on the stand-in).
- ◐ **P4. The error for a failed download says what to ask IT for**: the host it could not
  reach, the proxy it used, and whether the certificate was the problem, in "Copy diagnostics"
  too. Done: read from uv's error lines by their error codes (Windows translates the words),
  checked against uv 0.13's real output for a missing name, a proxy that refuses and a 404. Seen
  on the stand-in (N4): a mirror that is down says to ask IT to check it, and a proxy that is
  down is named with the PAC it came from. A damaged download and an AppLocker block got the
  general message there; each now has its own (unit tests, not yet seen on the stand-in). Left:
  those two on the stand-in, and a proxy that wants a login and an untrusted certificate.

**Tier 2: a better product for the people who run apps.**

- ☑ **P5. A notification when a run ends** while the window is not in front (a Windows toast:
  "Report finished", "Report stopped"). Long runs are the ones people leave. Done. On the
  stand-in, an installed build's notification (from a scheduled run) reached the notification
  centre under the launcher's own app ID; Windows held back the banner (Do Not Disturb or a
  window in front), which is Windows' choice, not the launcher's.
- ☐ **P6. Italian**, the language of the first users: the launcher's text in one catalogue
  (`design.md` keeps the wording rules), then Italian, picked from Windows' language. Other
  languages follow from the catalogue (was in C).
- ☐ **P7. Several files in one input** (`multiple = true`), the most asked-for input after the
  ones that exist; and "Run for each file", one run per file with the same other inputs. (Dates
  and choices exist already; C said otherwise.)
- ◐ **P8. Excel results and images shown in the launcher**: the first sheet of an `.xlsx` as a
  table, as N7 did for CSV; PNG and JPEG results as a picture (was in C). Done: the first visible
  sheet, dates as dates and numbers to Excel's 15 digits, read by the launcher's own reader
  (checked on files written by openpyxl, XlsxWriter and pandas); a picture is recognised by its
  bytes, and pressing it opens it. Seen in an installed build on the stand-in, on a workbook
  written by openpyxl. Left: a workbook saved by Excel itself, and a picture result in an
  installed build.
- ☑ **P9. An app in the Start menu**: "Add to Start" in the panel's ⋯ menu makes a shortcut that
  opens the launcher on that app; removed with the app. Done, in "devstribution apps" with the
  app's icon (ICO, or PNG up to 256 px); the uninstaller removes them. Tried in an installed
  build on the stand-in: the shortcut is made under "devstribution apps" and opens the launcher
  on the app (removal by the uninstaller was not tried there).
- ☑ **P10. Runs on a schedule**: a form app with saved inputs runs daily or weekly (the weekly
  report), through a Task Scheduler entry that starts the launcher, with the same guard, the
  results kept as for any run and a notification (P5). Off in managed mode unless the policy
  allows it. Done: every day, Monday to Friday or weekly, a missed time runs when the computer is
  next on, the launcher starts hidden and closes after the run (`ScheduledRuns` in managed mode).
  The task registers as a standard user (checked). An installed build on the stand-in, in
  managed mode with `ScheduledRuns` = 1, ran hidden at the set time, notified and kept the
  result.

**Tier 3: for the people who write apps, and the reputation signing waits on (N5).**

- ◐ **P11. A folder that stays between runs** (`ctx.data_dir`): settings and caches an app keeps,
  outside the output folder and not counted as changed files by the guard; removed on uninstall,
  and never shared with another app. Done: `app-data\<namespace>\<app>` in the data folder,
  kept through updates; the guard keeps other apps out and never runs code from it (so nothing
  written by one run runs in the next). `devstribution dev` has its own, and `local_context()`
  uses `output/app-data`. Left: a way to see or empty it from the launcher.
- ◐ **P12. Pack and sign in CI**: a GitHub Action (or a documented workflow) that packs and signs
  an app on a tag with its key in a repository secret and attaches the `.dsapp` to a release
  (`keygen --no-password` exists for this). Done: `actions/pack`, which runs the released CLI
  after checking its checksum and attestation, then `pack`, `verify` and `scan`; the release
  workflow to copy is in `authors.md`. CI runs it on the examples with a CLI built from source.
  A key with a password and none given now fails at once instead of waiting for someone to type
  it. Left: a first run from an author's repository against a published release (0.6.0).
- ☐ **P13. Example apps people would use**: a few public, signed apps that do real work (merge
  Excel files, a CSV cleaner, a PDF report), linked from the README with a short screen
  recording. This is the "verifiable reputation" SignPath asks about.
- ◐ **P14. Contributors**: fold `HANDOFF.md` and `status_quo.md` into `CONTRIBUTING.md`, and
  "good first issue" labels on a few small items (was in C). Done: one `CONTRIBUTING.md` with
  where things stand, setup, the layout and what to know before changing code; the two pages are
  gone. Left: the issues themselves, on GitHub.

**Tier 4: evidence for IT**, from C, as soon as a company evaluates (B1): Windows Event Log
(runs, findings, quarantines) and an SBOM written by `pack`.

## 0.5.0 (released 6 November)

Roughly in this order; N1 and N2 first, as they finish what 0.4.0 started and close known gaps.

- ☑ **N1. Finish the 0.4.0 rollout**: 0.4.0 is the latest release, and an installed 0.3.0
  launcher offered it and updated itself to it (6 November): the last check of B5 (self-update),
  B13 and B15. The installer's progress window showed during the update; from 0.5.0 on, updates
  install without it.
- ☑ **N2. Guard gaps from the 5 November assessment**: programs that reach the network (`curl`,
  …) count as network use for an app that declares none; the process rules know the commands that
  dump or list saved passwords, add accounts, open files to everyone or clear the event logs;
  Python code a run wrote is not run; and the gaps B10 left (databases opened past the `sqlite3`
  module, this PC's own shares, junctions above a trust folder) are closed
  ([`security.md`](security.md)). What remains is tracked privately, as
  [`SECURITY.md`](../SECURITY.md) asks of reporters.
- ☑ **N3. Faster runs** (section 9): an app's code starts after 0.16 s instead of 0.4 s. The
  standard library's bytecode is cached in the data folder, where the guard stops apps' writes
  (the runner's own modules are still compiled from their checked source every run); the
  launcher notices the end of a run at once instead of on a 100 ms tick; the app is resumed
  through its process handle, not a snapshot of every thread on the system. The app's code and
  its dependencies are compiled at install, so the first run of a pandas app takes 1.9 s, not
  4.1–4.7 s.
- ☑ **N4. A stand-in for a company PC**, so B1 and B2 stop waiting for a company: a script
  that sets up a Windows VM (a standard account, AppLocker's default rules, mitmproxy as a
  TLS-inspecting proxy, trust folders on an SMB share, managed mode) and a checklist
  ([`company-pc.md`](company-pc.md)). Run on 13 November, with the proxy set only in Windows (a
  PAC script, no `HTTPS_PROXY`: P1), package and Python mirrors, and PyPI, GitHub and
  releases.astral.sh blocked: B1 and B2 there, and P1 to P5 and P8 to P10 (what each showed
  is in its entry). An install of a pandas app from the mirrors, with AppLocker's path rules
  on, took about 40 s. A real company still confirms it later.
- ☐ **N5. Signing, both routes at once**: apply to SignPath Foundation (was A9) with 0.4.0 linked,
  and to Certum's open-source certificate, which checks who we are rather than the project's
  reputation. Whichever says yes first → B4.
- ☑ **N6. What the first outside users meet** (section 7): plainer trust wording (no signing keys
  on the install screen or the main window) and a short explanation of publishers and trust
  before the first install; table and text results over 256 KB show their start, with all of it
  saved in the results folder (they used to end "without a result" over 16 KB); one run at a
  time per app.
- ☑ **N7. Results shown in the launcher, first step**: a CSV or TSV result shows its first 50
  rows as a table under the file, as table results do, with the number of rows (section 7). Excel,
  images and charts later.
- ☑ **N8. On GitHub**: secret scanning with push protection, and code scanning (CodeQL's default
  setup), on since 6 November.
- ☑ **N9. Keep measuring**: `crates/devstribution-host/examples/perf.rs` times install, first run
  and later runs; the numbers are in section 9 and [`report.md`](report.md), and timing them is a
  step of a release that changes installs or runs ([`releasing.md`](releasing.md)).

## A. First public release (done)

All done by 28 October 2026 except A9, which is now N5:

1. ☑ A release workflow in CI, from a tag, with checksums ([`releasing.md`](releasing.md)).
2. ☑ Versioning, changelog and release notes; `v0.1.0` published as a pre-release (26 October).
3. ☑ A code signing policy ([`code-signing-policy.md`](code-signing-policy.md)).
4. ☑ An About page in Settings, with the licences of everything shipped.
5. ☑ Error messages reviewed end to end; an error screen instead of a blank window.
6. ☑ `logs\launcher.log` and "Copy diagnostics", without secrets.
7. ☑ Docs for app authors ([`authors.md`](authors.md)).
8. ☑ Private vulnerability reporting and two-factor authentication on GitHub.
9. → N5. Apply to SignPath Foundation.
10. ☑ The install dialog can be cancelled.
11. ☑ No silent behaviour (empty states, cancelled prompts, "Forget" asks first, repair progress).
12. ☑ Keyboard and screen reader basics.
13. ☑ Docs that match the release.

## B. Before a signed 1.0

What has to hold before our signature goes on it. Open items first; the numbers stay as they are,
as other documents refer to them.

- ◐ **B1. Phase 0** of [`plan.md`](plan.md): install and run on a locked-down company PC (AppLocker or
  WDAC, a proxy with TLS inspection, no admin rights), uv behind the company proxy included. If
  uv or the runner fails there, it comes before everything else (section 3). First on the stand-in
  (N4), then on a real one. On the stand-in: AppLocker's default rules block the installer, the
  launcher and Python in the user's folders, as expected; with path rules for the user's
  devstribution and uv folders (and, for the test, its temporary folder) the launcher starts,
  and a pandas app installs from the mirrors and its packages import; with DLL rules on,
  Python exits 0xC0000364, so they need the same paths. Left: a real company PC.
- ◐ **B2. Managed mode checked on Windows** (on the stand-in: the registry read, a trust folder
  on an SMB share approving an author, an author not approved blocked with IT's contact, in the
  machine-policy test and in the launcher; left: the folder permission cases below, and a real company PC): the registry read, the permission check on a local
  folder and on a network share, and the screens (section 10). The B10 review found the folder
  check never worked and fixed it, so this is the first real test of it: a trust folder users can
  change must show a problem and approve no one; one only administrators can change must work,
  including when an administrator owns it and the user does not. One inside a folder users can
  rename must show a problem, while one under `C:\ProgramData`, where users can only add, must
  work. On the stand-in first (N4).
- ☐ **B3. The remaining Windows checks**: the data folder move with real old data; no sideways
  scrolling (section 2).
- ☐ **B4. Signing in CI**, once SignPath or Certum has said yes (N5), then the Windows checks again
  with the signed installer (section 3).
- ☑ **B15. A release with the B10 fixes**: 0.4.0, released 5 November from the hardened workflow (its
  first tag run), and installed launchers update to it (N1).

Done:

- ☑ **B5.** The launcher updates itself (signed update feed; IT can turn it off); seen from 0.3.0 to
  0.4.0 (N1).
- ☑ **B6.** Revocation, first two steps: the list check and owners' withdrawal and rotation statements
  ([`revocation.md`](revocation.md)).
- ☑ **B7.** Fuzzing of everything that reads untrusted bytes, nightly in CI ([`fuzz/README.md`](../fuzz/README.md)).
- ☑ **B8.** The scan's false positives measured (section 5).
- ☑ **B9.** App updates from folders the user or IT chooses. Still to see: a real network share (N4).
- ☑ **B10.** Security review again (30 October): 14 findings fixed ([`security.md`](security.md)); what
  the fixes left is part of N2. Repeat before signing if these parts change.
- ☑ **B11.** `state.json` survives a crash.
- ☑ **B12.** uv can be stopped (timeout, Cancel, Job Object).
- ☑ **B13.** Release workflow hardened: only from `main` after CI, attested checksums.
- ☑ **B14.** CI and tests: minimum Rust version, `cargo deny`, `npm audit`, timeouts, Biome.

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
- **Evidence for IT** (section 8): Windows Event Log; SBOM written by `pack` (Tier 4 of 0.6.0 as
  soon as a company evaluates).
- **Results shown in the launcher, the rest** (section 7): charts drawn by the launcher; other
  sheets of a workbook. Excel, images and several files moved to 0.6.0 (P7, P8).
- **Launcher, smaller items** (section 7): Settings: stop trusting a publisher; languages after
  Italian (P6); an accessible version of the runs chart.
- **CLI**: `verify` shows the scan and can check against a `.trust` file.
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
  the user's files, including through programs the app starts. The strongest guarantee, and the
  most work: environments, Excel COM and the output folder all need access granted. The runtime
  guard (section 4) tells us which apps would fit.
- **The IT review kit** ([plan](plan.md), "IT review before approval"): a review report for each
  package, nothing executed (`devstribution diff` is the start of it); a one-click trial in Windows
  Sandbox whose review mode records what the app did; approval by IT countersignature. The report
  needs nothing from managed mode, so it can come earlier if a company asks for it.
- **Other languages: not planned.** The environments, the runner, `ctx`, the scan and the runtime
  guard are all Python-specific. If needed, a "signed executable" kind (no environment, no scan,
  only the signature and the Job Object) is the cheap option, clearly labelled as reviewed less.

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

☑ All done: `.dsappignore` and a data guard that refuses client data (also files next to it),
`pack --list`, a pinned and hash-checked uv, and package format 1 specified and frozen
([`format.md`](format.md)).

## 2. Windows checks (by hand, on a real PC)

Each gets a result in `report.md`. ☑ Job Object tree kill; Excel closed after a run; Credential
Manager; `gui` apps without a console; suspended start, restricted DLL loading, the uninstaller
removing secrets. Open: no sideways scrolling and the data folder move with real old data → B3;
uv behind a company proxy → B1.

## 3. Installer and signing

☑ Per-user NSIS installer without admin rights, file association, single instance,
Mark-of-the-Web, the release workflow. Open: code signing → N5, B4; Phase 0 → B1.

## 4. Runtime guard: enforce what an app declared

The permissions on the install screen were only what the publisher *declares*
([`security.md`](security.md), limit 2). The guard watches every run from two places and acts on what
it sees. Neither needs admin rights. The process rules live in one file,
`runner/devstribution_runner/_process_rules.json`, read by both.

- ☑ **Audit hook in the runner** (`_guard.py`, `sys.addaudithook`, PEP 578), installed before any
  app code and before `site`, so the code dependencies run from `.pth` files is checked too:
  - **undeclared use is blocked**: without `permissions.network`, a connection or name lookup
    other than this PC fails with a `PermissionError` naming the missing permission; with a list
    of hosts, only those (and the system's proxy). Without any, the programs that reach the
    network (`curl`, …) count too (5 November). SAP's RFC library without `permissions.sap` the
    same;
  - **tripwires stop the run** before the call: start-with-Windows entries, the antivirus's
    settings, browser password and cookie stores, the Windows tools malware uses, deleting
    backups, switching off the firewall, process injection and keyboard hooks, listing saved
    Windows passwords, loading compiled code outside imports;
  - **files**: reading saved keys and passwords, writing to Windows, Program Files, PowerShell
    profiles, devstribution's data folder, other apps or the app's own Python, reading another
    run's results, running a program the run wrote, changing more than 50 existing files outside
    the output folder, the user's choices and the temporary folder. Paths are compared as the
    system names them;
  - **observed**: programs started, hosts contacted and files changed outside the run's folders,
    summed up under the results as "What it did";
  - **other Pythons** the app starts (`subprocess`, `multiprocessing`) start through `child.py`
    with the same guard; a Python started any other way is stopped.
- ☑ **Watching from the launcher** through the run's Job Object, which the app cannot hide from:
  every new process is checked against the same rules; at most 128 processes; no switching
  desktops, changing display or system settings, or logging off.
- ☑ **When a rule fires**, the whole tree is ended, the run is recorded as stopped and the app is
  **quarantined** until the user decides ("Uninstall" or "Allow and run again").
- ◐ **Modes per rule**: managed mode can make every rule report-only and decide whether users may
  allow a stopped rule (section 10). A different mode per rule → C.
- ☑ **Tests** per rule in `runner/tests/test_guard.py` (Windows, Linux, macOS), shared command-line
  cases with the launcher, and the stop → quarantine → allow flow in `flow.rs`; checked in the
  installed launcher on Windows (`report.md`).
- ☑ **Gaps found on 5 November** (N2).

The hook runs inside the app's process, so code that patches memory through `ctypes` can get past
it; the Job Object check does not depend on the app, and the sandbox (C) is the real wall.

## 5. A deeper scan before install

- ☑ **Dependencies** (`devstribution-core/src/deps.rs`), offline from bundled lists refreshed before
  each release by `scripts/update_dep_data.py`: packages reported as malicious on PyPI (a danger
  finding), names one edit away from the 1,000 most downloaded (a warning), and code that runs from
  `.pth` files when Python starts (checked by the guard, reported in the run log).
- ☑ **What changed in an update** (`devstribution diff`): permissions, dependencies, the scan's
  findings and every file; the update screen leads with what deserves a look.
- ☑ **False positives measured** (30 October), nothing declared:

  | Corpus | Apps | Danger | Warning | Neither |
  | --- | --- | --- | --- | --- |
  | Scripts and demos, each file one app | 229 | 0 | 17 (7.4%) | 212 |
  | Standard library, each module or package one app (Python 3.13) | 187 | 0 | 71 (38%) | 116 |
  | Installed libraries, each one app (`site-packages`, 12,325 files) | 172 | 5 (2.9%) | 111 (65%) | 56 |

  All 17 script warnings say what the code does, so a warning there is a fair question; a library
  warns mostly because it is large. `crates/devstribution-core/examples/scan_corpus.rs` repeats the
  measurement on any folder.
- ☐ AMSI, vulnerable versions, unexpected binaries in wheels, reading more cleverly → C.

## 6. Updates and key lifecycle

☑ The launcher updates itself (B5); app updates in place with the diff, from folders the user or IT
chooses (B9). There is no download feed: fetching from a URL would need its own trust design.
◐ Key rotation and revocation: owners' statements and the list check are done (B6); the trust
repository, fetching and the policy values → C.

## 7. User experience

☑ The "Launchpad" design and its written language ([design.md](design.md)); the code scan on the
install screen; results in a folder the user chose; "Uninstall devstribution" in Settings; `.zip`
downloads install; error messages, About page, cancellable install, keyboard and screen reader
basics (stage A); plainer trust wording, large table results, one run per app (N6); CSV results
as a table (N7). ◐ Uninstall easy to find: check with users. Open: Excel and images → P8 (0.6.0); charts → C.

## 8. Release engineering and evidence for IT

☑ Security reviews before signing and again on 30 October (B10; repeat before each release that
changes install, runs or the guard); `SECURITY.md` with private reporting; versioning and release
notes; "Copy diagnostics"; docs for authors; fuzzing (B7); a performance and security assessment
on 5 November (N2, N3); secret scanning with push protection and CodeQL (N8). Open: Windows
Event Log and SBOM → C.

## 9. Performance

Measured with `crates/devstribution-host/examples/perf.rs` (Windows 11, release build, uv
0.13.4, Python 3.15). "Cold" downloads Python and every wheel; "warm" has them cached. Before N3
on 5 November, after it on 6 November:

| App | Install, cold | Install, warm | First run | Later runs |
| --- | --- | --- | --- | --- |
| No dependencies | 2.1 s → 4.5 s | 0.4 s → 1.1 s | 0.5–0.7 s → 0.4 s | 0.40 s → 0.18 s |
| openpyxl (1,000 rows to `.xlsx`) | 2.5 s → 3.6 s | 0.4 s → 1.4 s | 1.4 s → 0.8–0.9 s | 0.71 s → 0.50 s |
| pandas + openpyxl (10,000 rows to `.xlsx`) | 4.8 s → 7.2 s | 0.9 s → 3.2 s | 4.1–4.7 s → 1.9 s | 1.7 s → 1.4 s |

- Installs now compile the app's code and its dependencies (0.7–2.4 s), which the first run did
  before; a company proxy will be slower (timed in N4). The cold installs also vary with the
  download.
- Every run carries ~0.16 s before the app's code starts (Python itself starts in 35 ms); it was
  ~0.4 s before N3. The first run on a Python version not used before also compiles the
  standard library once (~0.2 s).
- The guard adds no cost proportional to the work: the pandas job takes 1.3 s bare and 1.4 s
  under devstribution.
- The CLI's `scan`, `diff`, `verify` and `pack --list` take 50–110 ms on a real app.

## 10. Managed mode, first version

What a company's IT asks for before allowing devstribution on its PCs, with no server: the policy
comes through the registry (GPO or Intune), and the approved authors are files IT can keep on a
network share. The design is in [`plan.md`](plan.md), "Trust model and policy"; how IT sets it up is
in [`managed-mode.md`](managed-mode.md).

- ☑ **Policy from HKLM.** Managed mode when `HKLM\SOFTWARE\Policies\devstribution` exists,
  free mode otherwise. The allowed authors are listed inline, in `.trust` files in a folder, or in a
  TOML policy file. Files and folders users can write are refused, and a policy that cannot be
  read fails closed.
- ☑ **`.trust` files**: `devstribution trust export` writes one, `trust show` prints its
  fingerprints. Only approved authors install and run: no trust on first use, no "Install anyway".
- ☑ **A helpful block message**: the reasons, who to ask, and a ready-to-forward `.trust`.
- ☑ **Policy options**: scan and dependency danger findings (refuse or let the user decide), guard
  rules made report-only, whether users may allow a stopped rule, the minimum launcher version,
  launcher updates off.
- ☑ **Shown in the launcher**, a **dry run** that logs what would have been refused, and **tests**.
- ☐ **Checked on Windows** → B2 (on the stand-in first, N4).
