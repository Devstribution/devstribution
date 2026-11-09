# Contributing

Thanks for looking. devstribution is early, so the most useful contributions right now are bug
reports from real Windows PCs, packages that the scan gets wrong, and small focused fixes. Issues
labelled [good first issue](https://github.com/Devstribution/devstribution/labels/good%20first%20issue)
are small and self-contained: a good place to start.

**Security problems: do not open an issue.** Report them privately, as [SECURITY.md](SECURITY.md)
describes.

## Where things stand

- **What is released and what changed:** [CHANGELOG.md](CHANGELOG.md).
- **What comes next, and in which order:** [docs/roadmap.md](docs/roadmap.md). The "Next" section
  is the current work; ☐ is not started, ◐ built but not yet checked where it matters (an
  installed build, a company PC), ☑ done.
- **What was checked on real Windows PCs, and what was found:** [docs/report.md](docs/report.md).
- **The design:** [docs/plan.md](docs/plan.md); what protects whom in
  [docs/security.md](docs/security.md); the launcher's look in [docs/design.md](docs/design.md).

## Before you start

- For anything larger than a fix, open an issue first so we can agree on the shape of it.
- The package format is frozen at schema 1 ([docs/format.md](docs/format.md)). A change that needs a
  newer format is a bigger conversation.
- The launcher's look is written down in [docs/design.md](docs/design.md): tokens live in one
  `style.css`, and wording follows its "plain words" rule.

## Setting up

You need:

- [Rust](https://rustup.rs) 1.90 or newer (on Windows the MSVC toolchain, with the Visual Studio
  C++ build tools);
- [uv](https://docs.astral.sh/uv/), on PATH: the host's tests build real environments with it;
- for the launcher, Node 20.19+ or 22.12+. WebView2 is part of Windows 10 and 11; on Linux, the
  WebKitGTK packages that CI installs (`.github/workflows/ci.yml`).

Windows is the platform that matters most; the Rust and Python parts also run on Linux and
macOS, and CI checks all three.

```sh
cargo test                                    # core, host (needs uv on PATH), CLI
python -m unittest discover -s runner/tests   # the runner and the runtime guard

cd launcher
npm ci                # after every pull that changes package-lock.json
npm run sidecar       # downloads the pinned uv (uv-pin.json) and checks its sha256
npx tauri dev         # the launcher, with hot reload
npx tauri build       # release build and installer (target/release/bundle/nsis)
```

`npm run dev` in `launcher/` opens the UI alone in a browser, on made-up data (`src/mock.ts`):
the quick way to work on a screen.

Try the whole path, from an app folder to an installed app:

```sh
cargo run -p devstribution-cli -- keygen --namespace devstribution --no-password
cargo run -p devstribution-cli -- pack examples/excel-report -o excel-report.dsapp
# then drop excel-report.dsapp on the launcher window, or use "Install app"
```

Set `DEVSTRIBUTION_HOME` to a scratch folder to keep test installs out of your real data folder.

- **A release launcher** uses only the uv next to its executable, the one `npm run sidecar`
  fetched. **A development build** also accepts `DEVSTRIBUTION_UV`, or `uv` on PATH.
- **A conda or other Python on your PC** doesn't leak into apps: they get uv's own Python
  (`--managed-python`) with a cleared environment, and the runner starts Python with `-I -S`.
- A file such as `launcher/src-tauri/Cargo.toml` changed locally blocks `git pull`: look at
  `git diff <file>`, then `git restore <file>` if `main` already has the change.

## Layout

| Path | What |
|---|---|
| `crates/devstribution-core` | The package format: manifest, `FILES.sha256`, signatures and fingerprints, packing, safe extraction, lockfile checks (`lock::check_sources` is the install gate), the code scan (`scan`), the dependency check (`deps`), `.trust` files, revocation statements |
| `crates/devstribution-host` | Everything the launcher does with packages: the install store (pins, anti-downgrade, `admit`), managed mode's policy (`policy`), uv environments (`uv`), supervised runs (`run`, Job Object), the runtime guard's launcher side (`guard`), history, secrets, the data folder (`paths`) |
| `crates/devstribution-cli` | `devstribution init / dev / pack / keygen / verify / scan / diff / trust / keys / revocations` |
| `runner/` | The stdlib-only Python runner the launcher starts for each run: the protocol (`_runner.py`), `ctx` (`_context.py`), the runtime guard (`_guard.py`) |
| `launcher/src` | TypeScript UI, no framework: `api.ts` (every call to the Rust side, and its mock), `views/` (one file per screen), `style.css` (the design tokens), `look.ts` (theme and colours) |
| `launcher/src-tauri` | The launcher's Rust side: commands over the host crate (`lib.rs`), Start menu and schedules (`shell.rs`), `capabilities/` (what the window may call), `build.rs`, `windows/hooks.nsh` (the uninstaller), icons |
| `actions/pack` | The GitHub Action authors use to pack and sign in their CI |
| `examples/` | `excel-report` (a form app), `hello-gui` (tkinter) |
| `fuzz/` | Fuzz targets for everything that reads untrusted bytes ([`fuzz/README.md`](fuzz/README.md)) |
| `scripts/` | Releases (`release.py`), the dependency check's data, third-party notices, the company-PC stand-in |
| `docs/` | The pages linked above, plus `authors.md` (for app authors), `managed-mode.md` and `company-pc.md` (for IT), `releasing.md` |

## Things to know before changing code

- **Format 1** is specified in [docs/format.md](docs/format.md). A change a format-1 launcher can't
  read needs `schema_version = 2`, and the frozen package in
  `crates/devstribution-core/tests/fixtures/format-1/` must keep verifying.
- **Security-relevant changes** (extraction, verification, install, the uv call, runs, the guard,
  launcher commands, the CSP, capabilities) get a line in [docs/security.md](docs/security.md).
- **A new launcher command** goes in three places, or the window can't call it:
  `generate_handler!` in `lib.rs`, `COMMANDS` in `build.rs`, and an `allow-<name>` line in
  `capabilities/default.json`.
- **The code scan** (`crates/devstribution-core/src/scan.rs`) is one rule table used by `pack`,
  `devstribution scan` and the install screen. A new rule needs a test that an ordinary app stays
  clean (`ordinary_app_is_clean`): every false alarm teaches users to ignore the screen.
- **Icons** are rendered at each exact size from `launcher/src/logo-small.svg`; don't let
  `tauri icon` overwrite them ([docs/design.md](docs/design.md), "Logo").
- **CI actions** are pinned by commit SHA. Dependabot proposes updates; don't move them to a tag
  by hand.

## Checks before a pull request

CI runs these, so running them first saves a round trip:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 -m unittest discover -s runner/tests
```

For a launcher change, also `npm run build` and `npm run lint` in `launcher/` (type check, then
Biome), and `cargo clippy -p devstribution-launcher --all-targets -- -D warnings` and
`cargo test -p devstribution-launcher` (the launcher is outside the default members). A
dependency change: `cargo deny check` (advisories, licences, sources; see `deny.toml`). A change
to how packages, manifests, lockfiles or the scan read their input: a run of the fuzz targets.

## What a good change looks like

- **One change per pull request**, with a description of what changed and why.
- **A test with it.** A new scan rule needs a test that an ordinary app stays clean, as well as one
  that it fires. A new guard rule gets a test app in `runner/tests/test_guard.py`.
- **Docs that stay true.** If behaviour changes, update the page that describes it, and add a line
  under "Unreleased" in [CHANGELOG.md](CHANGELOG.md) when a user or an app author would notice.
- **Errors say what happened and what to do.** No bare "failed", no codes without words.
- **Code that reads like its neighbours**: same naming, same comment density, comments that say
  why rather than what.
- Rust `unsafe` is for the Windows API calls that need it, and nothing else.

## Licence

By contributing you agree that your work is licensed under the project's terms: MIT or
Apache-2.0, at the user's option (see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE)).
