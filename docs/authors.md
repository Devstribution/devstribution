# Writing an app for devstribution

From an empty folder to a `.dsapp` your colleagues install with a double-click. The full manifest
reference is [`format.md`](format.md); what the launcher protects users from is
[`security.md`](security.md).

devstribution is a preview: the launcher is not code-signed yet, so Windows warns when your users
install it (see the release notes).

## 1. What you need

- **The `devstribution` CLI.** Download it from the
  [latest release](https://github.com/Devstribution/devstribution/releases), or build it with
  `cargo install --path crates/devstribution-cli` from a clone.
- **[uv](https://docs.astral.sh/uv/)**, which the CLI uses to set up Python. You don't need to
  install Python yourself.
- Your users need the devstribution launcher, from the same release.

## 2. Create the app

```sh
devstribution init my-tool --id yourname/my-tool
```

This creates `my-tool/` with:
- `pyproject.toml`, the manifest;
- `main.py`;
- `uv.lock`;
- `.gitignore` and `.dsappignore`;
- `AGENTS.md`, rules for coding agents (point yours at it).

The new app already passes every check.

- **The id:** `yourname/my-tool` is your **namespace** (the publisher name, before `/`) and the
  app's name. Pick the namespace once and keep it: every app you sign with your key uses it.
- **The kind:**
  - `--kind form` (the default) is a function with inputs and outputs. The launcher draws the
    form and shows the results.
  - `--kind gui` is your own window (tkinter, Qt).
  - `cli` apps also exist: they get the arguments from the launcher.

## 3. Write the code

### A form app

The manifest lists the inputs and outputs:

```toml
[[tool.devstribution.inputs]]
name = "orders"
type = "file"
label = "Orders export"
accept = [".xlsx"]

[[tool.devstribution.outputs]]
name = "report"
type = "file"
label = "Report"
```

The entry function (`entry = "main:run"`) gets one argument per input, plus `ctx`, and returns a
dict with one value per output:

```python
from pathlib import Path

from devstribution_runner import AppError


def run(orders: Path, ctx) -> dict:
    ctx.progress(0.1, "Reading the orders")
    rows = read_orders(orders)
    if "Total" not in rows.columns:
        raise AppError("The file has no 'Total' column: export it again from the orders system")
    out = ctx.output_dir / "report.xlsx"
    write_report(rows, out)
    return {"report": out}
```

**Inputs** arrive as Python values:

| `type` | Your function gets |
|---|---|
| `file`, `folder` | a `pathlib.Path` that exists |
| `text`, `choice` | `str` |
| `number` | `int` or `float` |
| `bool` | `bool` |
| `date` | `datetime.date` |
| `secret` | `str`, from Windows Credential Manager (the launcher asks once) |

An input with `required = false` that the user left empty is `None`, or your function's own
default.

**Outputs:**

| `type` | Return |
|---|---|
| `file` | a path inside `ctx.output_dir`, or inside a folder the user chose as an input |
| `text` | anything; it is shown as text |
| `table` | a list of dicts (one per row) or a list of lists; keep it under 16 KB |

### What `ctx` gives you

| | |
|---|---|
| `ctx.output_dir` | The run's own folder for results. The user opens it from the launcher. |
| `ctx.progress(fraction, message)` | Progress from 0 to 1 (`None` when you can't tell). Call it in long loops: it is also where Cancel takes effect. |
| `ctx.log(message, level="info")` | A line in the run's log (`info`, `warning`, `error`). `print()` goes to the log too. |
| `ctx.secrets.get(name)` | A secret declared in `permissions.secrets`. Never hard-code credentials. |
| `ctx.on_cleanup(fn)` | Runs `fn` at the end of the run, also after an error or a Cancel. |
| `ctx.track_com(obj)` | Closes an Office instance at the end of the run (see below). |

A GUI or CLI app reads the same object from `devstribution_runner.ctx`, which is `None` outside
devstribution.

### When something goes wrong

- **A problem the user can fix** (wrong file, missing column, a server that refused a login):
  `raise AppError("…")` with a message written for them. The launcher shows it as it is, with no
  traceback.
- **Any other exception** is shown as "It didn't work". For common causes (a file open in Excel,
  no connection), the launcher says what to try. Otherwise it says the problem is most likely in
  the app, and offers "Copy details" (the message and traceback), which the user can send you.

### Excel and other Office programs

Use `win32com.client.DispatchEx("Excel.Application")`, never `Dispatch`, so you get your own
instance and not the one the user has open. Pass it to `ctx.track_com(excel)`, which closes it even
when the run is cancelled. Put `requires = ["excel"]` in the manifest, so users see that the app
needs Excel.

## 4. Dependencies

```sh
uv add openpyxl          # updates pyproject.toml and uv.lock
```

- **PyPI only:** packages must come from PyPI (`https://pypi.org/simple`), with a **wheel** for
  every platform in `platforms`. Git, path and source-only packages are refused, because the user's
  PC builds nothing.
- **Lock after every change:** after editing dependencies by hand, run `uv lock`. The launcher
  installs exactly what `uv.lock` says.
- **Python version:** `requires-python` picks it (the default is `>=3.11`). The launcher downloads
  uv's own Python builds; the user's Python is never used.

## 5. Permissions: say what the app does

```toml
[tool.devstribution.permissions]
network = ["api.company.com"]   # hosts the app connects to
folders = ["user-selected"]
sap = false
secrets = ["api_token"]
```

Users see these before they install. They are also enforced while the app runs:
- the runtime guard blocks network use you did not declare, and hosts you did not list;
- it blocks SAP's RFC library without `sap = true`;
- it stops what malware does: start-with-Windows entries, browser passwords, other apps' files,
  programs the app downloaded or wrote, and more (see [`security.md`](security.md)).

`network = ["*"]` is allowed, but it is a warning of its own on the install screen.

The app can read and change the files and folders the user picks through its inputs. Writing a
few files elsewhere is reported to the user. Changing many existing files outside the output
folder stops the run.

## 6. Try it as the launcher runs it

```sh
devstribution dev my-tool -i orders=C:/data/orders.xlsx
```

This runs the app with the same runner, guard and clean environment as the launcher, unsigned. Its
error messages say what to change.

```sh
devstribution scan my-tool
```

This shows what users will see on the install screen about your code. A warning you can explain
is fine. A red "looks like malware" finding means users have to click "Install anyway", so find
another way.

## 7. Sign and pack

Once, create your key:

```sh
devstribution keygen --namespace yourname
```

- **Protect the key.** It asks for a password and saves `yourname.key` (secret) and `yourname.pub`
  in your devstribution keys folder (`%APPDATA%\devstribution\keys` on Windows).
- **Back up the `.key` file and its password.** Users accept updates only when they are signed
  with the same key. A lost key means your users must uninstall and install again, and a leaked
  key lets someone else publish "your" updates.
- **Moving to a new key** (the old one still works, you only want to change):
  `devstribution keys rotate --namespace yourname --new-key new.key` writes a `.dskey` file signed
  with both keys. Give it to your users with the first release signed with the new key: a launcher
  that has it accepts the new key instead of refusing the update.
- **A lost or stolen key:** `devstribution keys withdraw --namespace yourname --reason stolen`
  (or `lost`, `retired`) writes a `.dskey` file signed with that key. A launcher that has it
  (Settings, *Revoked keys*, *Import*) refuses everything signed with the key and warns before
  running apps it signed. Send it to your users and their IT, report it as `SECURITY.md` says, and
  sign your releases again with a new key. Anyone holding the key can withdraw it, a thief
  included, but nobody can switch it back on: keep the withdrawal for the day you need it only if
  you can keep it safe.

Then, for each release:

```sh
devstribution pack my-tool            # checks, signs -> my-tool/dist/my-tool-1.0.0.dsapp
devstribution pack my-tool --list     # only shows which files would go in
```

`pack` runs every check again. It refuses data-like files (spreadsheets, databases, archives,
logs) unless `.dsappignore` leaves them out, or names them with `!path` because the app really
ships them (a template, say). It never packs `.env` files, keys, `.venv` or `__pycache__`.

## 8. Deliver it

Send the `.dsapp` file any way you like: email, Teams, a network share. The user double-clicks it
(or drops it on the launcher) and sees the install screen: your namespace, your key ID, the
permissions and the scan.

- **The first app from your namespace:** that user's launcher pins your key. Tell your users your
  key ID through another channel (it is printed by `keygen` and shown on the install screen), so
  they can check the first one.
- **Managed mode:** in companies where IT allows only approved authors, give IT your `.trust`
  file:

  ```sh
  devstribution trust export --namespace yourname --name "Finance tools team" --contact you@company.com
  ```

  See [`managed-mode.md`](managed-mode.md).
- **Check a file before sending it:**
  `devstribution verify dist/my-tool-1.0.0.dsapp --key <keys folder>/yourname.pub`.

## 9. Updates

- **New version number:** raise `[project].version` (1 to 4 numbers, `1.2.0`) and pack again. The
  launcher refuses a lower version than the one installed, and the same version with different
  content.
- **Same key:** sign with the same key.
- **What users see:** a comparison with the version they have (new permissions, new scan
  findings, changed packages and files) before they update. `devstribution diff old.dsapp
  new.dsapp` shows you the same comparison first.
