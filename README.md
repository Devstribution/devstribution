# devstribution

Code gets written at agent speed; getting it onto other people's computers still means
installers, IT tickets and "works on my machine". **devstribution** distributes Python apps
(tools, automations, proofs of concept) as signed data packages that one signed launcher
verifies, installs and runs. What an app may do is decided by trust in its publisher, and the
platform guarantees that what you run is exactly what that publisher published.

**Status: pre-alpha proof of concept.** Not ready for real users yet. The design is in
[docs/plan.md](docs/plan.md).

![The launcher: home with recent and most used apps](docs/images/home.png)

## What exists today

A working proof of concept (see [docs/HANDOFF.md](docs/HANDOFF.md) for what is verified):

| Part | Where |
|---|---|
| Launcher (Tauri): home, apps with filters, activity dashboard, install confirmation, app panel, settings; light and dark | `launcher/` |
| Install store, uv environments, supervised runs, history, secrets | `crates/devstribution-host` |
| Package format: manifest, `FILES.sha256`, minisign signatures, safe extraction, wheels-only check | `crates/devstribution-core` |
| Developer CLI: `init`, `dev`, `pack`, `keygen`, `verify` | `crates/devstribution-cli` |
| Python runner (stdlib only) and its JSON protocol | `runner/` |
| Example apps: `excel-report` (form), `hello-gui` (gui) | `examples/` |

| Install confirmation | Running an app |
|---|---|
| ![Install](docs/images/install.png) | ![App](docs/images/app.png) |
| **Apps** | **Activity** |
| ![Apps](docs/images/apps.png) | ![Activity](docs/images/activity.png) |

## Try it

You need [Rust](https://rustup.rs) and [uv](https://docs.astral.sh/uv/).

```sh
cargo install --path crates/devstribution-cli

devstribution init my-tool --id yourname/my-tool      # a form app that passes every check
devstribution dev my-tool -i name=Ada                # run it as the launcher would
devstribution keygen --namespace yourname            # once: your publisher key
devstribution pack my-tool                           # check, sign -> my-tool/dist/my-tool-0.1.0.dsapp
devstribution verify my-tool/dist/my-tool-0.1.0.dsapp --key <keys folder>/yourname.pub
```

An app is an ordinary Python folder: `pyproject.toml` with a `[tool.devstribution]` section,
a `uv.lock`, and the code. See `examples/` and the `AGENTS.md` that `init` writes for coding
agents.

Run the launcher (needs Node 22+; on Linux also the WebKitGTK libraries listed in the CI workflow):

```sh
cd launcher
npm ci
npm run sidecar        # puts your uv next to the launcher
npx tauri dev
```

`npm run dev` alone serves the UI in a browser with demo data.

## Development

```sh
cargo test                                            # core, host (install and run with uv), CLI
python3 -m unittest discover -s runner/tests          # runner protocol and code checks
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Security issues: see [SECURITY.md](SECURITY.md).
