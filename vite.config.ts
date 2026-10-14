import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    sourcemap: false,
    // Ship the 1,200 file-type icons as separate files loaded on demand rather
    // than inlining them into the bundle as data URIs.
    assetsInlineLimit: (file) => (file.includes("material-icon-theme") ? false : undefined),
  },
  test: { include: ["src/**/*.test.ts"] },
});
