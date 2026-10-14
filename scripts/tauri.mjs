// Runs the Tauri CLI. On macOS it first checks that the active SDK links with
// the installed linker: some Command Line Tools releases ship an SDK whose .tbd
// files their own `ld` can't read ("unknown architecture arm64e.x1"). If so, it
// switches to the newest installed SDK that does link.
import { spawnSync, spawn } from "node:child_process";
import { mkdtempSync, writeFileSync, readdirSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const env = { ...process.env };

function linksWith(sdk) {
  const dir = mkdtempSync(join(tmpdir(), "microbe-sdk-"));
  writeFileSync(join(dir, "t.rs"), "fn main() {}");
  const e = { ...process.env };
  if (sdk) {
    e.SDKROOT = sdk;
    e.RUSTFLAGS = `-C link-arg=-isysroot -C link-arg=${sdk}`;
  }
  const r = spawnSync("rustc", ["t.rs", "-o", "t"], { cwd: dir, env: e, stdio: "ignore" });
  rmSync(dir, { recursive: true, force: true });
  return r.status === 0;
}

if (process.platform === "darwin" && !env.SDKROOT && !linksWith(null)) {
  const roots = ["/Library/Developer/CommandLineTools/SDKs", "/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs"];
  const sdks = roots
    .filter(existsSync)
    .flatMap((r) => readdirSync(r).filter((n) => /^MacOSX\d+(\.\d+)?\.sdk$/.test(n)).map((n) => join(r, n)))
    .sort((a, b) => parseFloat(b.match(/MacOSX([\d.]+)\.sdk/)[1]) - parseFloat(a.match(/MacOSX([\d.]+)\.sdk/)[1]));
  const ok = sdks.find((s) => linksWith(s));
  if (ok) {
    console.warn(`[microbe] default macOS SDK doesn't link with this linker; using ${ok}`);
    console.warn("[microbe] (updating Xcode / Command Line Tools fixes this permanently)");
    env.SDKROOT = ok;
    const flag = `-C link-arg=-isysroot -C link-arg=${ok}`;
    const key = `CARGO_TARGET_${process.arch === "arm64" ? "AARCH64" : "X86_64"}_APPLE_DARWIN_RUSTFLAGS`;
    env[key] = env[key] ? `${env[key]} ${flag}` : flag;
  } else {
    console.warn("[microbe] the macOS SDK doesn't link and no working SDK was found; update Xcode / Command Line Tools");
  }
}

const bin = join(import.meta.dirname, "..", "node_modules", ".bin", process.platform === "win32" ? "tauri.cmd" : "tauri");
const child = spawn(bin, process.argv.slice(2), { stdio: "inherit", env, shell: process.platform === "win32" });
child.on("exit", (code) => process.exit(code ?? 1));
