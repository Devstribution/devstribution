# devstribution

Code gets written at agent speed; getting it onto other people's computers still means
installers, IT tickets and "works on my machine". **devstribution** distributes Python apps
(tools, automations, proofs of concept) as signed data packages that one signed launcher
verifies, installs and runs. What an app may do is decided by trust in its publisher, and the
platform guarantees that what you run is exactly what that publisher published.

**Status: pre-alpha.** Nothing here is ready for real users yet. The design is in
[docs/plan.md](docs/plan.md).

## What exists today

| Part | Where | State |
|---|---|---|
| Package format: manifest, `FILES.sha256`, minisign signatures, safe extraction, wheels-only lockfile check | `crates/devstribution-core` | Working, tested |
| Developer CLI: `init`, `dev`, `pack`, `keygen`, `verify` | `crates/devstribution-cli` | Working for `form`, `gui` and `cli` apps |
| Python runner (stdlib only) and its JSON protocol | `runner/` | Working, tested |
| Example apps | `examples/` | `excel-report` (form), `hello-gui` (gui) |
| Launcher (Tauri) | — | Next |

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

## Development

```sh
cargo test                                            # Rust: core + CLI end-to-end
python3 -m unittest discover -s runner/tests          # runner protocol and code checks
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
Security issues: see [SECURITY.md](SECURITY.md).
