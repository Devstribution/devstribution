"""Writes THIRD-PARTY-NOTICES.txt: the licences of the software the launcher ships.

- the Rust libraries the launcher is built from (what `cargo metadata` resolves for Windows,
  normal dependencies only: no build tools, no tests), with the licence files of each package;
- the npm packages the window is built from (launcher/package.json "dependencies");
- uv, the program the launcher ships, with its licence files from the pinned release.

Each distinct licence text is written once, followed by the packages it belongs to. Run it before
a release, after `cargo update` or `npm install`, and commit the result:

    python scripts/third_party_notices.py

Needs `cargo`, the launcher's `node_modules` and a network connection (for uv's licence files).
"""

import json
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LAUNCHER = ROOT / "launcher"
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"
LICENCE_NAMES = ("license", "licence", "copying", "unlicense", "notice")
UV_FILES = ("LICENSE-MIT", "LICENSE-APACHE")


def licence_files(folder: Path) -> list[tuple[str, str]]:
    """The licence-like files at the top of a package folder, as (name, text)."""
    found = []
    for f in sorted(folder.iterdir()):
        if f.is_file() and f.name.lower().startswith(LICENCE_NAMES):
            found.append((f.name, f.read_text(encoding="utf-8", errors="replace").strip()))
    return found


def rust_packages() -> list[dict]:
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc",
             "--manifest-path", str(LAUNCHER / "src-tauri" / "Cargo.toml")],
            check=True, capture_output=True, text=True, encoding="utf-8",
        ).stdout
    )
    by_id = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    seen, todo = {root}, [root]
    while todo:
        for dep in nodes[todo.pop()]["deps"]:
            # kind None is a normal dependency; "dev" and "build" are not shipped.
            if any(k["kind"] is None for k in dep["dep_kinds"]) and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                todo.append(dep["pkg"])
    # Our own crates (no registry source) are under the project's licence.
    return sorted((by_id[i] for i in seen if by_id[i]["source"]), key=lambda p: (p["name"], p["version"]))


def npm_packages() -> list[tuple[str, str, str, Path]]:
    deps = json.loads((LAUNCHER / "package.json").read_text(encoding="utf-8"))["dependencies"]
    out = []
    for name in sorted(deps):
        folder = LAUNCHER / "node_modules" / name
        info = json.loads((folder / "package.json").read_text(encoding="utf-8"))
        out.append((name, info["version"], info.get("license", "unknown"), folder))
    return out


def uv_version() -> str:
    pin = next((LAUNCHER / "src-tauri" / "binaries").glob("uv-*.pin"))
    return pin.read_text(encoding="utf-8").split()[0]


def main() -> int:
    groups: dict[str, list[str]] = {}  # licence text -> packages it belongs to
    without_text: list[str] = []

    def add(label: str, files: list[tuple[str, str]]):
        if not files:
            without_text.append(label)
        for _, text in files:
            groups.setdefault(text, []).append(label)

    for p in rust_packages():
        add(f"{p['name']} {p['version']} ({p['license'] or 'see its package'})", licence_files(Path(p["manifest_path"]).parent))
    for name, version, spdx, folder in npm_packages():
        add(f"{name} {version} ({spdx})", licence_files(folder))
    uv = uv_version()
    uv_files = []
    for f in UV_FILES:
        url = f"https://raw.githubusercontent.com/astral-sh/uv/{uv}/{f}"
        with urllib.request.urlopen(url) as r:
            uv_files.append((f, r.read().decode("utf-8").strip()))
    add(f"uv {uv} (MIT or Apache-2.0), shipped as a program", uv_files)

    lines = [
        "Third-party notices for devstribution",
        "=====================================",
        "",
        "devstribution is MIT or Apache-2.0 (LICENSE-MIT, LICENSE-APACHE). It contains the software",
        "below. Each licence text is given once, with the packages it belongs to.",
        "Python itself is not shipped: uv downloads it when an app is first set up.",
        "",
    ]
    for text, labels in groups.items():
        lines += ["-" * 78, "Used by:"] + [f"  {l}" for l in sorted(set(labels))] + ["", text, ""]
    if without_text:
        lines += ["-" * 78, "Packages that carry no licence file; their declared licence is in the name:"]
        lines += [f"  {l}" for l in sorted(without_text)] + [""]
    OUT.write_text("\n".join(lines), encoding="utf-8", newline="\n")
    print(f"wrote {OUT.relative_to(ROOT)}: {len(groups)} licence texts, {OUT.stat().st_size // 1024} KB")
    return 0


if __name__ == "__main__":
    sys.exit(main())
