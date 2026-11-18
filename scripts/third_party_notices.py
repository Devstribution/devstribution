"""Writes THIRD-PARTY-NOTICES.txt, the licences of the software the launcher ships, and
SBOM.cdx.json, the same software as a CycloneDX 1.6 bill of materials:

- the Rust libraries the launcher is built from (what `cargo metadata` resolves for Windows,
  normal dependencies only: no build tools, no tests), with the licence files of each package;
- the npm packages the window is built from (launcher/package.json "dependencies");
- uv, the program the launcher ships, with its licence files from the pinned release.

Each distinct licence text is written once, followed by the packages it belongs to. The bill of
materials names each package by its package URL, with its licence and the hash its lockfile
records (Cargo.lock, package-lock.json, launcher/uv-pin.json); the installer ships it, and the
launcher's Security page shows it. Run it before a release, after `cargo update` or
`npm install`, and commit both files:

    python scripts/third_party_notices.py

Needs `cargo`, the launcher's `node_modules` and a network connection (for uv's licence files).
"""

import base64
import hashlib
import json
import subprocess
import tomllib
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LAUNCHER = ROOT / "launcher"
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"
SBOM = ROOT / "SBOM.cdx.json"
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


def version() -> str:
    return tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]


def licence(expr: str | None) -> list[dict]:
    return [{"expression": expr}] if expr else []


def sbom(rust: list[dict], npm: list[tuple[str, str, str, Path]], uv: str) -> dict:
    """The CycloneDX 1.6 document for what the launcher ships. No timestamp, so the file only
    changes when what ships changes; the serial number is derived from the components."""
    checksums = {
        (p["name"], p["version"]): p.get("checksum")
        for p in tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    }
    npm_lock = json.loads((LAUNCHER / "package-lock.json").read_text(encoding="utf-8"))["packages"]
    components = []
    for p in rust:
        c = {"type": "library", "bom-ref": f"pkg:cargo/{p['name']}@{p['version']}", "name": p["name"],
             "version": p["version"], "purl": f"pkg:cargo/{p['name']}@{p['version']}",
             "licenses": licence(p["license"])}
        if checksum := checksums.get((p["name"], p["version"])):
            c["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        components.append(c)
    for name, ver, spdx, _ in npm:
        ref = f"pkg:npm/{name.replace('@', '%40')}@{ver}"
        c = {"type": "library", "bom-ref": ref, "name": name, "version": ver, "purl": ref,
             "licenses": licence(spdx if spdx != "unknown" else None)}
        integrity = npm_lock.get(f"node_modules/{name}", {}).get("integrity", "")
        if integrity.startswith("sha512-"):
            c["hashes"] = [{"alg": "SHA-512", "content": base64.b64decode(integrity[7:]).hex()}]
        components.append(c)
    pin = json.loads((LAUNCHER / "uv-pin.json").read_text(encoding="utf-8"))
    components.append({
        "type": "application", "bom-ref": f"pkg:github/astral-sh/uv@{uv}", "name": "uv",
        "version": uv, "purl": f"pkg:github/astral-sh/uv@{uv}", "supplier": {"name": "Astral"},
        "licenses": licence("MIT OR Apache-2.0"),
        "hashes": [{"alg": "SHA-256", "content": pin["sha256"]["x86_64-pc-windows-msvc"]}],
        "description": "Shipped as Astral released it; the hash is of uv-x86_64-pc-windows-msvc.zip",
    })
    components.sort(key=lambda c: c["bom-ref"])
    digest = hashlib.sha256(json.dumps(components, sort_keys=True).encode()).digest()[:16]
    b = bytearray(digest)
    b[6] = (b[6] & 0x0F) | 0x80
    b[8] = (b[8] & 0x3F) | 0x80
    x = b.hex()
    v = version()
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": f"urn:uuid:{x[:8]}-{x[8:12]}-{x[12:16]}-{x[16:20]}-{x[20:]}",
        "version": 1,
        "metadata": {
            "tools": {"components": [{"type": "application", "name": "third_party_notices.py"}]},
            "component": {"type": "application", "bom-ref": f"pkg:github/Devstribution/devstribution@v{v}",
                          "name": "devstribution", "version": v,
                          "purl": f"pkg:github/Devstribution/devstribution@v{v}",
                          "licenses": licence("MIT OR Apache-2.0")},
            "properties": [{"name": "devstribution:scope",
                            "value": "Windows launcher: Rust libraries (normal dependencies), the window's npm packages, uv"}],
        },
        "components": components,
    }


def main() -> int:
    groups: dict[str, list[str]] = {}  # licence text -> packages it belongs to
    without_text: list[str] = []

    def add(label: str, files: list[tuple[str, str]]):
        if not files:
            without_text.append(label)
        for _, text in files:
            groups.setdefault(text, []).append(label)

    rust = rust_packages()
    npm = npm_packages()
    for p in rust:
        add(f"{p['name']} {p['version']} ({p['license'] or 'see its package'})", licence_files(Path(p["manifest_path"]).parent))
    for name, ver, spdx, folder in npm:
        add(f"{name} {ver} ({spdx})", licence_files(folder))
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
    bom = sbom(rust, npm, uv)
    SBOM.write_text(json.dumps(bom, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {SBOM.relative_to(ROOT)}: {len(bom['components'])} components")
    return 0


if __name__ == "__main__":
    sys.exit(main())
