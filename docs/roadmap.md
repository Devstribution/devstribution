# Roadmap: from proof of concept to a signed 1.0

The proof of concept works end to end (see [`status_quo.md`](status_quo.md) and the Windows findings
in [`report.md`](report.md)), and a first real app has been delivered with it. This is the work
between that and a launcher we can sign and hand to people who will never read this repository.

*Revised 5 November 2026*, after 0.4.0 and a technical assessment of performance and security.
What blocks a signed 1.0 is no longer code: it is **evidence** (the launcher checked on a locked-down
company PC and in managed mode) and **reputation** (SignPath Foundation wants a project with a
"verifiable reputation", which a two-week-old repository with no outside users does not have yet).
So the next release goes after both, and fixes what the assessment found:

- **[Next: 0.5.0](#next-050)**: the assessment's findings, a stand-in for a company PC, signing
  applied for on both routes, and what the first outside users meet.
- **[A. First public release](#a-first-public-release-done)**: done.
- **[B. Signed 1.0](#b-before-a-signed-10)**: what has to hold before our signature goes on it.
- **[C. After 1.0](#c-after-10)**: wanted, but nothing about 1.0 depends on it.

The numbered sections after the stages are the record per area. Earlier revisions (23, 26 and
27 October) and the details of finished items are in the git history and
[`CHANGELOG.md`](../CHANGELOG.md). Status: ☐ to do · ◐ in progress · ☑ done

## Next: 0.5.0

Roughly in this order; N1 and N2 first, as they finish what 0.4.0 started and close known gaps.

- ☐ **N1. Finish the 0.4.0 rollout**: mark the release as the latest (not a pre-release), so
  installed 0.3.0 launchers are offered it, and see that update happen in a launcher. That is the
  last check of B5 (self-update), B13 (the first tag run worked on 5 November) and B15.
- ◐ **N2. Guard gaps from the 5 November assessment**: ☑ programs that reach the network (`curl`, …)
  now count as network use for an app that declares none ([`security.md`](security.md)). Left:
  commands that steal credentials or tamper with the system which the process rules do not know
  yet, and the gaps left by B10. Details are tracked privately,
  as [`SECURITY.md`](../SECURITY.md) asks of reporters; each fix goes into
  [`security.md`](security.md) and the changelog with its release.
- ☐ **N3. Faster runs** (section 9): every run carries ~0.4 s before the app's code starts, ~0.3 s
  of which can go:
  - the standard library and the runner are compiled on every run (the runner starts with `-B`):
    a bytecode cache instead, **in a folder apps cannot write**, or one app could plant code
    the next run loads before the guard;
  - the launcher notices the end of a run on a 100 ms tick;
  - resuming the app takes a snapshot of every thread on the system.

  Also `--compile-bytecode` at install, so the first run of a pandas app is not 2–3 s slower
  than later ones.
- ☐ **N4. A stand-in for a company PC**, so B1 and B2 stop waiting for a company: a standard
  (non-admin) account or a VM with AppLocker rules, a TLS-inspecting proxy (mitmproxy) for uv,
  and an SMB share for the trust folder. A script to set it up and a checklist; then run B1 and B2
  there and time cold installs behind the proxy. A real company still confirms it later.
- ☐ **N5. Signing, both routes at once**: apply to SignPath Foundation (was A9) with 0.4.0 linked,
  and to Certum's open-source certificate, which checks who we are rather than the project's
  reputation. Whichever says yes first → B4.
- ☐ **N6. What the first outside users meet** (section 7):
  - plainer trust wording on the main screens (no "namespace", key IDs or "Free mode ·
     signatures checked"), and a first-run explanation of publishers and trust;
  - table results over 16 KB, which today end "without a result";
  - one run at a time per app.
- ☐ **N7. Results shown in the launcher, first step**: CSV and table results as a table, without
  opening another program (section 7). Excel, images and charts later.
- ☐ **N8. On GitHub**: secret scanning with push protection, and code scanning (CodeQL), both free
  for public repositories.
- ☐ **N9. Keep measuring**: `crates/devstribution-host/examples/perf.rs` times install, first run
  and later runs; its numbers go into section 9 and [`report.md`](report.md) with each release that
  changes them.

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

- ☐ **B1. Phase 0** of [`plan.md`](plan.md): install and run on a locked-down company PC (AppLocker or
  WDAC, a proxy with TLS inspection, no admin rights), uv behind the company proxy included. If
  uv or the runner fails there, it comes before everything else (section 3). First on the stand-in
  (N4), then on a real one.
- ☐ **B2. Managed mode checked on Windows**: the registry read, the permission check on a local
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
- ◐ **B15. A release with the B10 fixes**: 0.4.0, released 5 November from the hardened workflow (its
  first tag run). Left: N1.

Done:

- ☑ **B5.** The launcher updates itself (signed update feed; IT can turn it off). Still to see: N1.
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
- **Evidence for IT** (section 8): Windows Event Log; SBOM written by `pack`.
- **Results shown in the launcher, the rest** (section 7): Excel, images and charts, more input
  types (dates, choices, several files).
- **Launcher, smaller items** (section 7): Settings: stop trusting a publisher; other languages; an
  accessible version of the runs chart.
- **CLI**: `verify` shows the scan and can check against a `.trust` file.
- **Contributors**: fold `HANDOFF.md` and `status_quo.md` into `CONTRIBUTING.md`.
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
- ◐ **Gaps found on 5 November** → N2 (network through other programs ☑).

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
basics (stage A). ◐ Uninstall easy to find: check with users. Open: trust wording, large table
results, one run per app → N6; results shown in the launcher → N7, then C.

## 8. Release engineering and evidence for IT

☑ Security reviews before signing and again on 30 October (B10; repeat before each release that
changes install, runs or the guard); `SECURITY.md` with private reporting; versioning and release
notes; "Copy diagnostics"; docs for authors; fuzzing (B7); a performance and security assessment
on 5 November (N2, N3). Open: secret scanning and CodeQL → N8; Windows Event Log and SBOM → C.

## 9. Performance

Measured 5 November 2026 with `crates/devstribution-host/examples/perf.rs` (Windows 11, release
build, uv 0.13.4, Python 3.15). "Cold" downloads Python and every wheel; "warm" has them cached.

| App | Install, cold | Install, warm | First run | Later runs |
| --- | --- | --- | --- | --- |
| No dependencies | 2.1 s | 0.4 s | 0.5–0.7 s | 0.40 s |
| openpyxl (1,000 rows to `.xlsx`) | 2.5 s | 0.4 s | 1.4 s | 0.71 s |
| pandas + openpyxl (10,000 rows to `.xlsx`) | 4.8 s | 0.9 s | 4.1–4.7 s | 1.7 s |

- Installs are fast; a company proxy will be slower (timed in N4).
- Every run carries ~0.4 s before the app's code starts (Python itself starts in 35 ms), and the
  first run after an install compiles the dependencies. What to do about both → N3.
- The guard adds no cost proportional to the work: the pandas job takes 1.3 s bare and 1.7 s under
  devstribution, the same fixed ~0.4 s.
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
