// Puts `uv` next to the launcher as a Tauri sidecar: src-tauri/binaries/uv-<target-triple>[.exe].
//
// Default: downloads the uv release pinned in uv-pin.json and refuses it unless its sha256 matches
// the pinned one, so every build ships exactly the uv we tested. A stamp file next to the binary
// skips the download when the pinned version is already in place.
//
// `--from-path`: copies the uv found on PATH instead (offline development only; never for a
// release, the launcher would ship whatever uv the build machine happens to have).
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const triple = process.env.TAURI_ENV_TARGET_TRIPLE
  ?? execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)[1].trim();
const windows = triple.includes("windows");
const exe = windows ? ".exe" : "";
const target = join(here, "..", "src-tauri", "binaries", `uv-${triple}${exe}`);
const stamp = `${target}.pin`;
mkdirSync(dirname(target), { recursive: true });

if (process.argv.includes("--from-path")) {
  const finder = process.platform === "win32" ? "where" : "which";
  const source = execFileSync(finder, ["uv"], { encoding: "utf8" }).split(/\r?\n/)[0].trim();
  copyFileSync(source, target);
  if (!windows) chmodSync(target, 0o755);
  rmSync(stamp, { force: true });
  console.log(`sidecar: copied ${source} -> ${target} (NOT the pinned uv: development only)`);
  process.exit(0);
}

const pin = JSON.parse(readFileSync(join(here, "..", "uv-pin.json"), "utf8"));
const expected = pin.sha256[triple];
if (!expected) {
  console.error(`sidecar: uv-pin.json has no sha256 for ${triple}`);
  process.exit(1);
}
const wanted = `${pin.version} ${expected}`;
if (existsSync(target) && existsSync(stamp) && readFileSync(stamp, "utf8").trim() === wanted) {
  console.log(`sidecar: uv ${pin.version} already in place (${target})`);
  process.exit(0);
}

const asset = `uv-${triple}.${windows ? "zip" : "tar.gz"}`;
const url = `https://github.com/astral-sh/uv/releases/download/${pin.version}/${asset}`;
const response = await fetch(url);
if (!response.ok) {
  console.error(`sidecar: download failed: ${response.status} ${response.statusText} (${url})`);
  process.exit(1);
}
const archive = Buffer.from(await response.arrayBuffer());
const actual = createHash("sha256").update(archive).digest("hex");
if (actual !== expected) {
  console.error(`sidecar: REFUSED ${asset}: sha256 ${actual}, uv-pin.json expects ${expected}`);
  process.exit(1);
}

// tar reads both formats: bsdtar ships with Windows 10 and later and opens .zip files. Named by
// its full path there, since Git Bash and conda shells put a GNU tar first on PATH, which can't.
const tar = process.platform === "win32" ? join(process.env.SystemRoot ?? "C:\\Windows", "System32", "tar.exe") : "tar";
const work = mkdtempSync(join(tmpdir(), "uv-sidecar-"));
try {
  const file = join(work, asset);
  writeFileSync(file, archive);
  execFileSync(tar, ["-xf", file, "-C", work]);
  const extracted = [join(work, `uv${exe}`), join(work, `uv-${triple}`, `uv${exe}`)].find(existsSync);
  if (!extracted) throw new Error(`no uv${exe} in ${asset}`);
  copyFileSync(extracted, target);
  if (!windows) chmodSync(target, 0o755);
  writeFileSync(stamp, `${wanted}\n`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
console.log(`sidecar: uv ${pin.version} (sha256 verified) -> ${target}`);
