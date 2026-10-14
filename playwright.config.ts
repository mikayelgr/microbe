import { defineConfig, devices } from "@playwright/test";

// UI tests run the real frontend in WebKit — the engine of the macOS app — with
// the Tauri backend stubbed (see e2e/pages/app.html).
export default defineConfig({
  testDir: "e2e",
  use: { baseURL: "http://localhost:5199", viewport: { width: 1100, height: 700 } },
  projects: [{ name: "webkit", use: { ...devices["Desktop Safari"], viewport: { width: 1100, height: 700 } } }],
  webServer: { command: "npx vite --port 5199 --strictPort", url: "http://localhost:5199", reuseExistingServer: true },
});
