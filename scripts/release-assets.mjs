// Copies the installers Tauri bundled for one target into a directory under
// stable, version-free names, so that
//   https://github.com/<owner>/<repo>/releases/latest/download/<name>
// always points at the newest release (D-029).
// Usage: node scripts/release-assets.mjs --target <triple> --out <dir>
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const arg = (name) => {
  const i = args.indexOf(name);
  if (i < 0 || !args[i + 1]) throw new Error(`missing ${name}`);
  return args[i + 1];
};
const target = arg("--target");
const out = arg("--out");
// Tauri puts the version in bundle names; matching it skips stale bundles from
// older versions left in a cached target directory.
const { version } = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8"));

// Target triple → [platform name, [bundle dir, file extension, stable suffix]…].
// Keep these names stable: the website and README link to them.
const TARGETS = {
  "aarch64-apple-darwin": ["macos-arm64", [["dmg", ".dmg", ".dmg"]]],
  "x86_64-apple-darwin": ["macos-x64", [["dmg", ".dmg", ".dmg"]]],
  "x86_64-pc-windows-msvc": [
    "windows-x64",
    [
      ["nsis", "-setup.exe", "-setup.exe"],
      ["msi", ".msi", ".msi"],
    ],
  ],
  "x86_64-unknown-linux-gnu": [
    "linux-x64",
    [
      ["appimage", ".AppImage", ".AppImage"],
      ["deb", ".deb", ".deb"],
      ["rpm", ".rpm", ".rpm"],
    ],
  ],
};

const entry = TARGETS[target];
if (!entry) throw new Error(`no release assets defined for ${target}`);
const [platform, kinds] = entry;
const bundle = join(root, "src-tauri", "target", target, "release", "bundle");

mkdirSync(out, { recursive: true });
for (const [dir, ext, suffix] of kinds) {
  const from = join(bundle, dir);
  const found = existsSync(from) ? readdirSync(from).filter((f) => f.endsWith(ext) && f.includes(version)) : [];
  if (found.length !== 1) {
    throw new Error(`expected one *${ext} for ${version} in ${from}, found ${found.length ? found.join(", ") : "none"}`);
  }
  const name = `microbe-${platform}${suffix}`;
  copyFileSync(join(from, found[0]), join(out, name));
  console.log(`✓ ${found[0]} → ${name}`);
}
