# The `.dsapp` package format, version 1

**Status: frozen.** This page is the specification of package format 1 (`schema_version = 1`). Its
reference implementation is `crates/devstribution-core`. Where the two disagree, that is a bug to
fix, and the frozen test package (below) decides which side is wrong.

## Compatibility promise

1. **Every later version of devstribution accepts format 1 packages.** A package that verifies today
   verifies in every later launcher, with the same manifest meaning and the same content hash
   (which names its install folder).
2. **Changes to format 1 are only ever additive on the reading side.** A later launcher may *read*
   more (new optional fields are a new format number, see 3), but it never refuses or reinterprets
   something format 1 allows.
3. **Anything a format-1 launcher can't read gets a new format number.** A package needing a
   newer launcher declares `schema_version = 2` (or higher). Older launchers refuse it with "this
   app needs a newer version of devstribution", never with a parse error.
4. **The frozen package** `crates/devstribution-core/tests/fixtures/format-1/hello.dsapp` was
   packed when format 1 was frozen and its secret key was thrown away. CI checks on every change
   that it still verifies with the same files, manifest and content hash. If that test fails, the
   change is wrong, not the fixture.

What format 1 does *not* promise: that every launcher implements every app kind or field it
declares. Parts marked *reserved* below are valid format 1, but current launchers refuse them
(resources at install, the reserved kinds when run), with a message saying so.

## The container

A `.dsapp` file is a zip archive.

| Rule | Value |
|---|---|
| Entries | Regular files; directory entries are allowed and ignored. No symbolic links, no special files, no encrypted entries. |
| Compression | Deflate or stored. `pack` writes Deflate. |
| Order and metadata | Not significant for verification. `pack` writes `FILES.sha256`, `FILES.sha256.minisig`, `FILES.sha256.pub`, then the files sorted by path, all with timestamp 1980-01-01 00:00 and Unix mode `0644`, so packing the same files twice gives the same bytes apart from the signature. |
| Limits (launcher) | At most 20,000 files, 512 MiB per file, 2 GiB in total, counted on the bytes actually extracted. |

### Paths

Every entry path is relative, uses `/` as separator and is valid UTF-8. Refused: absolute paths,
`\`, `:`, `.` or `..` components, empty components, control characters, components ending with a
dot or a space, Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`,
with or without an extension), and two paths that differ only by upper/lower case. These rules make
a package extract identically on Windows, macOS and Linux.

## Reserved files

At the root of the archive:

| File | Content |
|---|---|
| `FILES.sha256` | The hash of every other file (see below). |
| `FILES.sha256.minisig` | The publisher's [minisign](https://jedisct1.github.io/minisign/) signature over the exact bytes of `FILES.sha256`. |
| `FILES.sha256.pub` | The publisher's minisign public key: a line `untrusted comment: <text>`, then the base64 key. |

No other file may use these names, anywhere.

### `FILES.sha256`

UTF-8, one line per file: the lowercase hex SHA-256 of the file's bytes, two spaces, the path. Lines
sorted by the bytes of the path, each ending with `\n` (no `\r`). It lists every file in the archive
except the three reserved files, each exactly once. A list in any other form is refused, so a given
set of files has exactly one valid list and so one signature.

The **content hash** of a package is the first 8 bytes of the SHA-256 of `FILES.sha256`, in hex. It
names the folder the version is installed in.

### Signature

Ed25519 minisign signature (prehashed, as `minisign -S -H`) with this **trusted comment**, which the
signature covers:

```
devstribution id=<namespace>/<app> version=<version> schema=<n>
```

Single spaces, fields in this order, nothing else. Values are written in their one canonical form
(`version=1.2.0`, not `01.2.0`). The id, version and schema must equal the manifest's.

The signature is always checked against `FILES.sha256.pub`. That key is not trusted by itself: the
first install of a namespace pins it (trust on first use), and later packages of that namespace must
carry the same key.

## Required app files

| File | Rule |
|---|---|
| `pyproject.toml` | The manifest (next section). |
| `uv.lock` | uv lockfile `version = 1`, with the project itself as `source = { virtual = "." }`. Every package installed at run time on a declared platform must be available as a wheel from a package index (installs use `uv sync --frozen --no-build`). |
| The entry module | `module` or `module:function`, resolved as `<module path>.py` or `<module path>/__init__.py` (`pkg.main` → `pkg/main.py`). |
| The icon | If the manifest names one. |

Everything else is app code and assets, all covered by `FILES.sha256`.

## Manifest (`pyproject.toml`)

Standard `[project]` fields, plus `[tool.uv]` and `[tool.devstribution]`. Unknown keys inside
`[tool.devstribution]` and its sub-tables are refused (a new key needs a new format number).

```toml
[project]
name = "query-orders"            # required
version = "1.2.0"                # required: 1 to 4 numbers separated by dots, no suffixes
requires-python = ">=3.11"       # optional
dependencies = ["openpyxl>=3.1"] # installed from uv.lock
# `dynamic` is refused: it needs a build step.

[tool.uv]
package = false                  # required: apps are not built or installed as packages

[tool.devstribution]
schema_version = 1               # required
id = "author/query-orders"       # required: <namespace>/<app>
title = "Query orders"           # required, not empty
description = "..."              # optional
icon = "icon.png"                # optional, a path in the package
kind = "form"                    # form (default) | gui | cli | web | background
entry = "main:run"               # required; form apps need module:function
platforms = ["windows"]          # default ["windows"]; any of windows, macos, linux
requires = []                    # external prerequisites, e.g. "excel", "sap-gui-scripting"
timeout_seconds = 600            # optional, > 0

[[tool.devstribution.inputs]]    # form apps only; the launcher draws the form from these
name = "excel_file"              # Python identifier, unique, not `ctx`
type = "file"                    # file | folder | text | number | bool | choice | date | secret
label = "Orders Excel file"      # optional
help = "..."                     # optional
accept = [".xlsx"]               # file only
choices = ["a", "b"]             # choice only, required there
default = "a"                    # optional, matching the type; never for secret
required = true                  # default true

[[tool.devstribution.outputs]]   # form apps only
name = "report"                  # Python identifier, unique
type = "file"                    # file | table | text
label = "Report"                 # optional

[tool.devstribution.permissions] # shown to the user before install
network = ["api.company.com"]
folders = ["user-selected"]
sap = false
secrets = ["sap_user"]

[[tool.devstribution.resources]] # reserved: refused at install by current launchers
url = "https://..."              # https only
sha256 = "<64 lowercase hex>"
path = "models/model.onnx"       # a valid package path
```

| Rule | |
|---|---|
| `id` | Namespace 1–39 and app 1–64 characters of `a-z`, `0-9`, `-`, not starting or ending with `-`. |
| `version` | Compared number by number, missing numbers count as 0 (`1.2` = `1.2.0`). Launchers refuse installing a lower version than the highest installed, and the same version with different content. |
| `kind` | `form`, `gui` and `cli` run; `web` and `background` are reserved. |
| Inputs and outputs | Only for `kind = "form"`. The entry function takes one parameter per input plus `ctx`. |

## Not part of the format

These are packing conveniences: they never reach the package and launchers never read them.

- `.dsappignore` and the app folder's `.gitignore` decide what `pack` leaves out. `pack` refuses
  data-like files (spreadsheets, databases, archives, logs) unless they are left out or named with
  `!path` in `.dsappignore`.
- Caches and environments (`.git`, `.venv`, `__pycache__`, top-level `dist/`, ...), `.env` files and
  files that look like a secret signing key are never packed.

## Changing the format

A change that a format-1 launcher could not read gets `schema_version = 2`:

1. Keep reading format 1 exactly as specified here (the frozen-package test enforces it).
2. Document format 2 as a new section or page; keep this page unchanged apart from links.
3. Freeze a format-2 package the same way.
