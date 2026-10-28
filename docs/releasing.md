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

   - `latest.json`, the update feed (below).

   Install the draft's installer on a Windows PC. Then publish it in GitHub's interface. Untick
   *pre-release* only for a version meant for everyday use: **that is what sends it to installed
   launchers** (next section).

A tag whose version does not match the four files, or without a dated changelog section, stops
the workflow before anything is built. A wrong tag can be deleted (`git push --delete origin
v0.2.0`) as long as the draft was not published. Once a version is published, it is never
reused: fix forward with the next patch.

## Launcher updates

Installed launchers look for a newer version when they start (unless the user or IT turned it
off), and from Settings → "Look for updates". They read
`https://github.com/Devstribution/devstribution/releases/latest/download/latest.json`: GitHub's
*latest* release is the newest one that is **published and not a pre-release**. So:

- a draft, or a published pre-release, reaches no installed launcher;
- unticking *pre-release* (or publishing without it) rolls the version out to everyone;
- a bad release is stopped by marking it as a pre-release again (launchers that already updated
  stay on it: fix forward with the next patch).

`latest.json` holds the version, the changelog section as notes, and the installer's URL with its
**updater signature**: a minisign signature made in the `installer` job with the updater key (the
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository secrets). The
public half is in `tauri.conf.json` (`plugins.updater.pubkey`), so a launcher installs only an
installer signed with that key, and only the version the signature names (`requireSignedVersion`:
a feed edited to pair a new version number with an older installer is refused). The launcher
then runs the installer in passive mode: a progress window, no questions, no administrator
rights, and it opens the new version.

The key cannot be replaced without a manual reinstall everywhere: launchers trust only the
public key they were built with. Keep the private key file and its password outside the
repository, with a backup; generate it with
`npx tauri signer generate -w ~/.tauri/devstribution-updater.key` (in `launcher/`). Without the
secrets the `installer` job fails, and `release.py check` refuses a `tauri.conf.json` without a
public key.

Launchers up to 0.2.0 have no updater: their users install the next version by hand once.

## What the workflow does

| Job | Runs on | What |
|---|---|---|
| `check` | Linux | The tag, the four versions and the changelog agree; writes the release notes |
| `installer` | Windows | `npx tauri build --bundles nsis` with the updater signature (`tauri.updater.conf.json`); uv is downloaded and checked against `launcher/uv-pin.json` first |
| `cli` | Windows, Linux, macOS (Apple silicon) | `cargo build --release --locked`, packed with the README and licences |
| `release` | Linux | `latest.json`, `SHA256SUMS.txt` and the draft pre-release; only for a tag |

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

The updater signature covers the installer's exact bytes, so it must be made **after** the
Authenticode signature: once SignPath signs in CI, `createUpdaterArtifacts` comes off the build
and the job signs the signed installer with `npx tauri signer sign --app-version <version>`.
