# Contributing

Thanks for looking. devstribution is early, so the most useful contributions right now are bug
reports from real Windows PCs, packages that the scan gets wrong, and small focused fixes.

**Security problems: do not open an issue.** Report them privately, as [SECURITY.md](SECURITY.md)
describes.

## Before you start

- For anything larger than a fix, open an issue first so we can agree on the shape of it. The
  design is in [docs/plan.md](docs/plan.md), what is planned and in which order in
  [docs/roadmap.md](docs/roadmap.md), and what protects whom in [docs/security.md](docs/security.md).
- The package format is frozen at schema 1 ([docs/format.md](docs/format.md)). A change that needs a
  newer format is a bigger conversation.
- The launcher's look is written down in [docs/design.md](docs/design.md): tokens live in one
  `style.css`, and wording follows its "plain words" rule.

## Setting up

You need [Rust](https://rustup.rs), [uv](https://docs.astral.sh/uv/) and, for the launcher,
Node 22+. The README has the commands to build and run each part. Windows is the platform that
matters most; the Rust and Python parts also run on Linux and macOS, and CI checks all three.

## Checks before a pull request

CI runs these, so running them first saves a round trip:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 -m unittest discover -s runner/tests
```

For a launcher change, also `npm run build` and `npm run lint` in `launcher/` (type check, then
Biome), and `cargo test -p devstribution-launcher`. A dependency change: `cargo deny check`
(advisories, licences, sources; see `deny.toml`). A change to how packages, manifests, lockfiles
or the scan read their input: a run of the fuzz targets ([`fuzz/README.md`](fuzz/README.md)).

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
