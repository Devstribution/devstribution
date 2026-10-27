# Releasing

How a version of devstribution is numbered, built and published. The workflow is
[`.github/workflows/release.yml`](../.github/workflows/release.yml); the versions and release notes
are checked by [`scripts/release.py`](../scripts/release.py).

## Versions

One version for everything we ship: the launcher, the installer and the CLI. It is written in
four files, which must agree:

- `Cargo.toml` (`[workspace.package]`);
- `launcher/package.json` and `launcher/package-lock.json`;
- `launcher/src-tauri/tauri.conf.json` (the installer's name and the version Windows shows).

`python scripts/release.py bump 0.2.0` sets all four; `cargo update --workspace` then updates
`Cargo.lock`.

[Semantic Versioning](https://semver.org/): before 1.0, a new minor version (0.2) may change
behaviour, a patch (0.1.1) only fixes. The package format has its own number (format 1,
[`format.md`](format.md)), which a release does not change.

## Making a release

1. **Changelog.** Move what is under `## [Unreleased]` in [`CHANGELOG.md`](../CHANGELOG.md) into
   `## [0.2.0] - <today>`, written for people who use devstribution, not for us. Update the links
   at the bottom.
2. **Version.** `python scripts/release.py bump 0.2.0`, then `cargo update --workspace`.
   Then `python scripts/third_party_notices.py`: it rewrites `THIRD-PARTY-NOTICES.txt`, which the
   installer ships, for the dependency versions of this release (it needs a network connection).
   Commit the file if it changed.
3. **Check.** `python scripts/release.py check v0.2.0` must say the versions and the changelog
   agree. Open a PR with these changes and merge it when CI passes.
4. **Dry run (optional).** Actions → Release → *Run workflow* on `main`: it builds everything
   without creating a release, so a broken build shows before there is a tag.
5. **Tag** the merged commit on `main` and push the tag. Switch to `main` and pull first: `git tag`
   tags whatever is checked out, so a tag made on a PR branch misses what was merged after it
   (v0.1.0 was tagged that way, three documentation commits behind `main`).

   ```sh
   git checkout main
   git pull
   git log --oneline -1          # the merge commit of the release PR
   git tag -a v0.2.0 -m "devstribution 0.2.0"
   git push origin v0.2.0
   ```

6. **Read the draft.** The workflow creates a *draft pre-release* with:
   - the installer `devstribution_<version>_x64-setup.exe`;
   - the CLI for Windows, Linux and macOS;
   - `SHA256SUMS.txt`;
   - the changelog section as notes, under a notice that the build is not signed.

   Install the draft's installer on a Windows PC. Then publish it in GitHub's interface. Untick
   *pre-release* only for a version meant for everyday use.

A tag whose version does not match the four files, or without a dated changelog section, stops
the workflow before anything is built. A wrong tag can be deleted (`git push --delete origin
v0.2.0`) as long as the draft was not published. Once a version is published, it is never
reused: fix forward with the next patch.

## What the workflow does

| Job | Runs on | What |
|---|---|---|
| `check` | Linux | The tag, the four versions and the changelog agree; writes the release notes |
| `installer` | Windows | `npx tauri build --bundles nsis`; uv is downloaded and checked against `launcher/uv-pin.json` first |
| `cli` | Windows, Linux, macOS (Apple silicon) | `cargo build --release --locked`, packed with the README and licences |
| `release` | Linux | `SHA256SUMS.txt` and the draft pre-release; only for a tag |

- **No build caches.** A cache filled by another run could change what goes into a release.
- **`--locked`**: Cargo builds exactly what `Cargo.lock` lists; `npm ci` does the same for
  `package-lock.json`.
- **Provenance.** For a tag, every file gets a GitHub build attestation: a signed record that this
  repository's release workflow built it from that commit. Anyone can check a download with
  `gh attestation verify <file> --repo Devstribution/devstribution`.
- Only the `release` job can write to the repository, and only to create the release.

## Signing (not yet)

Code signing waits on SignPath Foundation (or Certum as the fallback): see
[`roadmap.md`](roadmap.md), stages A and B. SignPath signs builds that come from CI, so signing will
be a step in the `installer` job, between the build and the attestation, with the signing done
on SignPath's side. Until then the notes tell users what SmartScreen will show.
