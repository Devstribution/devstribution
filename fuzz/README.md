# Fuzzing

Tests that feed the parts reading untrusted bytes with generated input, using
[`cargo-fuzz`](https://rust-fuzz.github.io/book/cargo-fuzz.html) (libFuzzer). This crate has a
workspace of its own, since it needs a nightly toolchain and the normal build must not.

| Target | What it feeds | What must hold, beyond not crashing |
| --- | --- | --- |
| `package` | any bytes as a `.dsapp` | nothing written beside the destination; a refusal leaves nothing behind |
| `signed_package` | correctly signed packages with hostile names, files, manifest and lockfile | the same; an accepted package holds exactly its signed list, with no links |
| `manifest` | `pyproject.toml`, app ids, entries, versions | what prints reads back; versions have a consistent order |
| `lockfile` | `uv.lock`: the source and platform checks, the dependency check, the scan's reading | |
| `scan` | any file an app can hold, through the code scan | a report always serializes; no input takes 10 seconds |
| `documents` | file lists, signatures, public keys, publisher key files, key statements, revocation lists | a file list accepts only its canonical form; what is written reads back |

Not covered: the launcher's and the CLI's own code, and files only the machine's own user or IT
writes (`state.json`, the policy).

## Running

Needs a nightly toolchain (`rustup toolchain install nightly`) and `cargo install cargo-fuzz`.
On Windows, run from a Visual Studio developer prompt whose installation has the C++ AddressSanitizer
component: linking fails without it, also with `-s none`. With several Visual Studio installations
Rust may otherwise pick one that lacks it. Extraction is worth running there too: names like
`..\x`, `C:\x` or `CON` only mean something on Windows.

```bash
fuzz/run.sh 60
```

```bash
fuzz/run.sh 300 signed_package
```

The arguments are the seconds per target and, optionally, which targets. The inputs found so far
stay in `fuzz/corpus/` (not committed). CI runs every target nightly and when a pull request
changes the core crate (`.github/workflows/fuzz.yml`).

## When it finds something

The input is in `fuzz/artifacts/<target>/`, and the run prints the command that repeats it. Fix the
cause and add the smallest input as a normal test in the crate it belongs to, so `cargo test`
guards it from then on. If the input could hurt users of a released version, report it the way
[`SECURITY.md`](../SECURITY.md) asks, and do not attach it to a public issue.
