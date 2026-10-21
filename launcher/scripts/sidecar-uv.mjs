// Puts `uv` next to the launcher as a Tauri sidecar: src-tauri/binaries/uv-<target-triple>[.exe].
// Development and CI copy the uv found on PATH. Release builds must instead download a pinned uv
// release and check its published sha256 (see docs/HANDOFF.md).
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, chmodSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const triple = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1].trim();
const exe = process.platform === "win32" ? ".exe" : "";
const finder = process.platform === "win32" ? "where" : "which";
const source = execFileSync(finder, ["uv"], { encoding: "utf8" }).split(/\r?\n/)[0].trim();
const target = join(here, "..", "src-tauri", "binaries", `uv-${triple}${exe}`);
mkdirSync(dirname(target), { recursive: true });
copyFileSync(source, target);
if (!exe) chmodSync(target, 0o755);
console.log(`sidecar: ${source} -> ${target}`);
