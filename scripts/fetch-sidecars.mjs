// Downloads sidecar binaries (currently the `ty` language server) for the host
// target triple into src-tauri/binaries/<name>-<triple>[.exe], as Tauri expects.
// Usage: node scripts/fetch-sidecars.mjs [--target <triple>] [--ty-version <x.y.z>]
import { execFileSync } from "node:child_process";
import { mkdirSync, existsSync, writeFileSync, chmodSync, renameSync, rmSync, readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "binaries");
const args = process.argv.slice(2);
const arg = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};

const triple =
  arg("--target") ??
  execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/host: (\S+)/)[1];
const tyVersion = arg("--ty-version") ?? "0.0.87";

const SIDECARS = [
  {
    name: "ty",
    url: (t) => {
      const ext = t.includes("windows") ? "zip" : "tar.gz";
      return `https://github.com/astral-sh/ty/releases/download/${tyVersion}/ty-${t}.${ext}`;
    },
  },
];

mkdirSync(outDir, { recursive: true });
for (const sc of SIDECARS) {
  const exe = triple.includes("windows") ? ".exe" : "";
  const dest = join(outDir, `${sc.name}-${triple}${exe}`);
  if (existsSync(dest) && !args.includes("--force")) {
    console.log(`✓ ${sc.name} already present (${dest})`);
    continue;
  }
  const url = sc.url(triple);
  console.log(`↓ ${url}`);
  const res = await fetch(url);
  if (!res.ok) throw new Error(`download failed: ${res.status} ${url}`);
  const work = join(tmpdir(), `microbe-sidecar-${sc.name}-${Date.now()}`);
  mkdirSync(work, { recursive: true });
  const archive = join(work, url.endsWith(".zip") ? "a.zip" : "a.tar.gz");
  writeFileSync(archive, Buffer.from(await res.arrayBuffer()));
  if (archive.endsWith(".zip")) execFileSync("tar", ["-xf", archive, "-C", work]);
  else execFileSync("tar", ["-xzf", archive, "-C", work]);
  const found = findFile(work, sc.name + exe);
  if (!found) throw new Error(`${sc.name}${exe} not found in archive`);
  renameSync(found, dest);
  if (!exe) chmodSync(dest, 0o755);
  rmSync(work, { recursive: true, force: true });
  console.log(`✓ ${sc.name} → ${dest}`);
}

function findFile(dir, name) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, e.name);
    if (e.isDirectory()) {
      const f = findFile(p, name);
      if (f) return f;
    } else if (e.name === name) return p;
  }
  return null;
}
