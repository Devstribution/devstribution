# Checking devstribution on a stand-in for a company PC

Roadmap N4: B1 (does devstribution install and run where IT has locked the PC down?) and B2
(does managed mode work on Windows?) do not have to wait for a company. A virtual machine set up
like a company PC answers most of it; a real company PC confirms it later.

What a company PC has, and what stands in for it here:

| On a company PC | Here |
| --- | --- |
| No admin rights | a standard local account, `dsuser` |
| AppLocker or WDAC: programs run only from `Program Files` and `Windows` | AppLocker's default rules, enforced (DLL rules too with `-Dll`, which is what WDAC does) |
| A proxy that inspects HTTPS, with the company's certificate trusted | mitmproxy on `127.0.0.1:8080`, its certificate in the machine's trusted roots, `HTTPS_PROXY` set |
| Trust files on a file server | an SMB share of the VM itself, `\\<PC>\ds-trust` |
| Managed mode set by group policy | `HKLM\SOFTWARE\Policies\devstribution` |

Not covered: WDAC itself (its policies are signed and harder to set up; AppLocker with DLL rules
blocks the same files), a proxy that asks for a password, a domain, Intune, real file servers.

## 1. The VM

1. A Windows 11 **Enterprise** VM, so AppLocker is enforced: Hyper-V Quick Create's "Windows 11
   dev environment" or the evaluation ISO. Pro and Home ignore AppLocker rules.
2. Take a checkpoint (snapshot) before anything else.
3. Copy into it: this repository's `scripts\company-pc\setup.ps1`, the devstribution installer
   (`devstribution_<version>_x64-setup.exe`), and the test apps (below) with their `.trust` files.
4. As administrator:

   ```powershell
   powershell -ExecutionPolicy Bypass -File setup.ps1 -All
   ```

   It asks for a password for `dsuser`. `-All` is `-User -AppLocker -Proxy -Share -Policy`; each
   can be run alone, `-Dll` adds the DLL rules, `-AuditOnly` only logs what AppLocker would block
   (Event Viewer → Applications and Services Logs → Microsoft → Windows → AppLocker), `-Undo`
   takes it all back out.
5. Sign out, sign in as `dsuser`.

## 2. Test apps

Pack them on your own PC with `devstribution pack`, signed with a test key (`devstribution keygen`):

- `examples/excel-report`: no native code of its own (openpyxl).
- A pandas app, e.g. the pandas app `crates/devstribution-host/examples/perf.rs` generates: `.pyd`
  files, which DLL rules and WDAC block.
- One app signed with a second key that is not in the trust folder.

## 3. B1: installing and running under AppLocker, behind the proxy

Write down for each step what happened, and the message if it failed. AppLocker logs every
block (Event Viewer, above), with the path.

| # | Step | Expected on a locked-down PC |
| --- | --- | --- |
| 1 | As `dsuser`, run the installer from Downloads | Blocked: AppLocker allows no program in a user's folders. Note the message |
| 2 | As administrator, install it anyway (or, with `-AuditOnly`, as `dsuser`), then start it as `dsuser` | The launcher is in `%LOCALAPPDATA%`: blocked under the default rules. This is the finding that decides the per-machine mode ([`plan.md`](plan.md), Phase 0) |
| 3 | Add a path rule for the launcher's folder only (Local Security Policy → AppLocker), start it | The window opens |
| 4 | Install `excel-report.dsapp` | uv downloads Python and wheels through the proxy (mitmweb shows them); `--system-certs` makes uv trust the proxy's certificate. Python is in `%LOCALAPPDATA%`: does it start? |
| 5 | Run it | The runner and the app start, or AppLocker blocks `python.exe` |
| 6 | With `-Dll`: install and run the pandas app | Do the `.pyd` files load? |
| 7 | Time the cold install of the pandas app (a stopwatch is enough) | Compare with section 9 of the roadmap; the proxy is the difference |
| 8 | Stop the proxy (`Stop-ScheduledTask 'devstribution mitmproxy'`) and install again | A clear message that the network is unreachable, not a hang |
| 9 | An app that drives Excel through COM (`ctx.track_com`), run, then run and Cancel | Excel closed after the run, also after a Cancel |

Each block is one more reason, with its path, for the per-machine mode or for IT's rules.

## 4. B2: managed mode

The script set `TrustFolder` to `\\<PC>\ds-trust`, which only administrators can change. Put a
`.trust` file in it as administrator. Change the folder with
`setup.ps1 -Policy -TrustFolder <path>` and restart devstribution after each change.

| # | Trust folder | Expected |
| --- | --- | --- |
| 1 | `\\<PC>\ds-trust` (administrators only) | Sidebar: "Managed by your organisation". The approved author installs; the other is refused, with a request file to send to IT |
| 2 | `\\<PC>\ds-trust-open` (users can change) | Settings shows a problem with the folder; no one is approved |
| 3 | `C:\ProgramData\ds-trust`, created by an administrator, users read only | Works: users can add folders to `C:\ProgramData` but not change this one |
| 4 | A folder only administrators can change, inside a folder `dsuser` owns (e.g. `C:\Users\dsuser\x\trust`) | A problem: `dsuser` could rename `x` |
| 5 | A junction `dsuser` made in `C:\ProgramData` pointing at the folder of row 3 | A problem: `dsuser` can delete the junction and point it elsewhere |
| 6 | An administrator owns the folder, `dsuser` only reads it | Works |
| 7 | No `TrustFolder`, `Authors` with one line | Only that author installs |
| 8 | Delete the registry key, restart | Free mode again |

Also check the screens: the install screen of a refused package, the request file, Settings'
policy section, and that `dsuser` cannot change any of it.

## 5. Writing it down

Put the results in [`report.md`](report.md) (a dated block, like the others) and tick B1/B2 in
the [roadmap](roadmap.md) for what passed on the stand-in; what failed comes first in the next
release. Restore the checkpoint before the next round.
