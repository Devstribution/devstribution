"""Versions and release notes, for `.github/workflows/release.yml` and by hand.

    python scripts/release.py bump 0.2.0           # set the version everywhere it is written
    python scripts/release.py check v0.2.0         # the tag, every version and the changelog agree
    python scripts/release.py check v0.2.0 --notes notes.md   # and write the release notes
    python scripts/release.py version              # the workspace version
    python scripts/release.py latest-json v0.2.0 dist/devstribution_0.2.0_x64-setup.exe --out dist/latest.json
                                                   # the update feed installed launchers read

The version lives in four files: the Cargo workspace, the launcher's `package.json` and
`package-lock.json`, and `tauri.conf.json` (the installer's name and the version Windows shows).
`Cargo.lock` follows with `cargo update --workspace`.
"""

import argparse
import datetime
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CARGO = ROOT / "Cargo.toml"
PACKAGE = ROOT / "launcher" / "package.json"
LOCK = ROOT / "launcher" / "package-lock.json"
TAURI = ROOT / "launcher" / "src-tauri" / "tauri.conf.json"
CHANGELOG = ROOT / "CHANGELOG.md"

# Installed launchers read `latest.json` from the latest published release (not a pre-release);
# the installer it names must be in the same release.
DOWNLOADS = "https://github.com/Devstribution/devstribution/releases/download"

SEMVER = re.compile(r"\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?")

UNSIGNED = """\
> **Not code-signed yet.** Windows SmartScreen will say "Windows protected your PC": choose
> *More info*, then *Run anyway*. Before that, check the file against `SHA256SUMS.txt`, or that
> it was built by this repository's release workflow:
> `gh attestation verify <file> --repo Devstribution/devstribution`.
>
> [Code signing policy](https://github.com/Devstribution/devstribution/blob/main/docs/code-signing-policy.md)
"""


def versions() -> dict:
    lock = json.loads(LOCK.read_text(encoding="utf-8"))
    return {
        "Cargo.toml": tomllib.loads(CARGO.read_text(encoding="utf-8"))["workspace"]["package"]["version"],
        "launcher/package.json": json.loads(PACKAGE.read_text(encoding="utf-8"))["version"],
        "launcher/package-lock.json": lock["version"],
        "launcher/package-lock.json (root package)": lock["packages"][""]["version"],
        "launcher/src-tauri/tauri.conf.json": json.loads(TAURI.read_text(encoding="utf-8"))["version"],
    }


def changelog_section(version: str) -> tuple:
    """The heading's date and the text of the section for `version` (None if missing)."""
    text = CHANGELOG.read_text(encoding="utf-8").replace("\r\n", "\n")
    m = re.search(
        rf"^## \[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})[ \t]*\n(.*?)(?=^## |^\[[^\]]+\]: |\Z)",
        text,
        re.M | re.S,
    )
    return (m.group(1), m.group(2).strip()) if m else (None, None)


def check(tag: str, notes: Path = None) -> int:
    if not tag.startswith("v") or not SEMVER.fullmatch(tag[1:]):
        print(f"Not releasing: the tag {tag!r} is not v<major>.<minor>.<patch>", file=sys.stderr)
        return 1
    problems = []
    version = tag[1:]
    for where, found in versions().items():
        if found != version:
            problems.append(f"{where} says {found}, the tag says {version}")
    date, section = changelog_section(version)
    if not section:
        problems.append(f"CHANGELOG.md has no dated section '## [{version}] - YYYY-MM-DD' with text")
    updater = json.loads(TAURI.read_text(encoding="utf-8")).get("plugins", {}).get("updater", {})
    if not updater.get("pubkey", "").strip():
        problems.append("tauri.conf.json has no updater public key (plugins.updater.pubkey)")
    if problems:
        print("Not releasing:", *problems, sep="\n  ", file=sys.stderr)
        return 1
    if notes:
        notes.write_text(f"{UNSIGNED}\n{section}\n", encoding="utf-8")
    print(f"{tag}: versions and changelog agree ({date})")
    return 0


def latest_json(tag: str, installer: Path, out: Path) -> int:
    """The update feed: version, notes, and the installer with its updater signature (the
    `.sig` file `tauri build` writes next to it when `createUpdaterArtifacts` is on)."""
    version = tag[1:]
    sig = installer.with_name(installer.name + ".sig")
    if not installer.is_file() or not sig.is_file():
        print(f"need {installer} and {sig}: was the installer built with tauri.updater.conf.json?", file=sys.stderr)
        return 1
    if version not in installer.name:
        print(f"{installer.name} is not the installer of {version}", file=sys.stderr)
        return 1
    _, section = changelog_section(version)
    feed = {
        "version": version,
        "notes": section or "",
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "windows-x86_64": {
                "signature": sig.read_text(encoding="utf-8").strip(),
                "url": f"{DOWNLOADS}/{tag}/{installer.name}",
            }
        },
    }
    out.write_text(json.dumps(feed, indent=2) + "\n", encoding="utf-8")
    print(f"{out}: {version} -> {feed['platforms']['windows-x86_64']['url']}")
    return 0


def bump(version: str) -> int:
    if not SEMVER.fullmatch(version):
        print(f"{version!r} is not <major>.<minor>.<patch>", file=sys.stderr)
        return 1
    text = CARGO.read_text(encoding="utf-8")
    text, n = re.subn(
        r'(\[workspace\.package\][^\[]*?\nversion = ")[^"]+(")', rf"\g<1>{version}\g<2>", text, count=1
    )
    if n != 1:
        print("no version under [workspace.package] in Cargo.toml", file=sys.stderr)
        return 1
    CARGO.write_text(text, encoding="utf-8", newline="")
    for path in (PACKAGE, TAURI, LOCK):
        text = path.read_text(encoding="utf-8")
        # The first "version" of each file is its own; in the lockfile, the root package's comes next.
        count = 2 if path == LOCK else 1
        text = re.sub(r'("version":\s*")[^"]+(")', rf"\g<1>{version}\g<2>", text, count=count)
        path.write_text(text, encoding="utf-8", newline="")
    left = {k: v for k, v in versions().items() if v != version}
    if left:
        print("not updated:", left, file=sys.stderr)
        return 1
    print(f"version {version} set; now run `cargo update --workspace` and add a CHANGELOG.md section")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    c = sub.add_parser("check", help="the tag, every version and the changelog agree")
    c.add_argument("tag")
    c.add_argument("--notes", type=Path, help="write the release notes here")
    b = sub.add_parser("bump", help="set the version everywhere it is written")
    b.add_argument("version")
    sub.add_parser("version", help="print the workspace version")
    j = sub.add_parser("latest-json", help="write the update feed for a release")
    j.add_argument("tag")
    j.add_argument("installer", type=Path)
    j.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "check":
        return check(args.tag, args.notes)
    if args.command == "latest-json":
        return latest_json(args.tag, args.installer, args.out)
    if args.command == "version":
        print(versions()["Cargo.toml"])
        return 0
    return bump(args.version)


if __name__ == "__main__":
    sys.exit(main())
