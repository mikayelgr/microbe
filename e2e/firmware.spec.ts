import { expect, test } from "@playwright/test";

type W = Window & {
  __stubs: Record<string, (a: Record<string, unknown>) => unknown>;
  __channel(id: number, m: unknown): void;
  __buildRequest: Record<string, unknown> | null;
};

test("build firmware in Docker and use it for flashing", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as W;
    w.__buildRequest = null;
    w.__stubs = {
      firmware_index: () => ({ boards: [], ports: ["rp2"], fetched_at: 0 }),
      firmware_detect: () => [
        { platform: "rp2", board_id: "RPI_PICO", confidence: 90, source: "USB id", detail: null, target: { kind: "volume", path: "/Volumes/RPI-RP2", label: "RPI-RP2" }, chip: null },
      ],
      device_discover: () => [],
      firmware_board: () => ({
        id: "RPI_PICO",
        name: "Raspberry Pi Pico",
        vendor: "Raspberry Pi",
        port: "rp2",
        features: [],
        variants: [{ title: "Firmware", variant: null, builds: [{ version: "1.24.1", date: "2024-11-29", preview: false, files: [] }] }],
      }),
      firmware_builders: () => ({ ports: ["esp32", "rp2", "unix"], docker: true, problem: null }),
      firmware_build: (a) => {
        w.__buildRequest = a.request as Record<string, unknown>;
        const id = (a.log as { id: number }).id;
        w.__channel(id, "== mpy-cross");
        w.__channel(id, "== firmware");
        return { output_dir: "/cache/build/out/x", artifacts: ["/cache/build/out/x/firmware.uf2"] };
      },
    };
  });
  await page.goto("/e2e/pages/app.html");
  await page.getByRole("button", { name: "Firmware", exact: true }).click();
  await expect(page.getByText("Raspberry Pi Pico", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Build from source" }).click();
  const build = page.getByRole("button", { name: "Build", exact: true });
  await expect(build).toBeEnabled();
  await build.click();
  await expect(page.locator("pre.log")).toContainText("== firmware");
  const req = await page.evaluate(() => (window as unknown as W).__buildRequest);
  expect(req).toMatchObject({ port: "rp2", board: "RPI_PICO", variant: null, version: "1.24.1", freeze_project: null, c_modules: null });

  await page.getByRole("button", { name: "Use for flashing" }).click();
  await expect(page.getByRole("button", { name: "firmware.uf2" })).toBeVisible();
});
