import { expect, test, type Page } from "@playwright/test";

const SAMPLE = "import machine\nfrom time import sleep\n\nled = machine.Pin(25, machine.Pin.OUT)\nwhile True:\n    led.toggle()\n";

type Calls = { run: { code: string; preludes: { script: string; args: Record<string, string> }[] }[]; write: string[]; term: number | null };

async function setup(page: Page) {
  await page.addInitScript(() => {
    const calls: Calls = { run: [], write: [], term: null };
    (window as unknown as { __calls: Calls }).__calls = calls;
    (window as unknown as { __stubs: Record<string, (a: Record<string, unknown>) => unknown> }).__stubs = {
      device_connect: (a) => {
        calls.term = (a.term as { id: number }).id;
        return {
          session_id: "s1",
          connected: true,
          state: "idle",
          endpoint: { transport: "serial", address: "/dev/fake", label: "Fake Pico", detail: null, usb: null, platform_hint: "rp2", role: null },
          target: { platform: "rp2", platform_name: "RP2", board_id: "RPI_PICO", capabilities: [], fs_root: "/" },
          info: null,
        };
      },
      debug_instrument: (a) => ({ code: `# instrumented\n${a.source}`, functions: ["<module>"], lines: [1, 2, 4, 5, 6] }),
      device_run: (a) => (calls.run.push(a as unknown as Calls["run"][number]), null),
      device_write: (a) => (calls.write.push(a.data as string), null),
      device_stop: () => null,
      analysis_run: () => ({ findings: [], suppressions: [] }),
    };
  });
  await page.goto("/e2e/pages/app.html");
  await page.evaluate(() => (window as unknown as { connectBoard(): Promise<unknown> }).connectBoard());
  await page.evaluate((t) => (window as unknown as { openSample(t: string): void }).openSample(t), SAMPLE);
}

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __calls: Calls }).__calls);

test("debugging: breakpoints, stop location, variables and stepping", async ({ page }) => {
  await setup(page);
  // Set a breakpoint on line 4 from the gutter.
  const gutterLine4 = page.locator(".cm-breakpoint-gutter .cm-gutterElement").nth(4);
  await gutterLine4.click();
  await expect(page.locator(".cm-breakpoint")).toHaveCount(1);

  await page.keyboard.press("F6");
  await expect(page.getByText("main.py:4", { exact: true })).toBeVisible();
  await expect.poll(async () => (await calls(page)).run.length).toBe(1);
  const run = (await calls(page)).run[0];
  expect(run.code.startsWith("# instrumented")).toBe(true);
  expect(run.preludes[0]).toMatchObject({ script: "debugger", args: { breakpoints: "[4]", functions: '["<module>"]' } });

  // The board reports a stop.
  const term = (await calls(page)).term!;
  await page.evaluate((id) => {
    const w = window as unknown as { __channel(id: number, m: unknown): void };
    w.__channel(id, {
      kind: "structured",
      tag: "DBG",
      payload: { event: "stopped", line: 4, reason: "breakpoint", stack: ["<module>"], locals: {}, globals: { led: "Pin(GPIO25, mode=OUT)" } },
    });
  }, term);
  await expect(page.locator(".cm-debug-line")).toContainText("led = machine.Pin(25");
  await expect(page.getByText("Paused at main.py:4")).toBeVisible();
  await expect(page.getByText("Pin(GPIO25, mode=OUT)")).toBeVisible();

  await page.keyboard.press("F10");
  await expect.poll(async () => (await calls(page)).write).toEqual(["n\r"]);
  await expect(page.locator(".cm-debug-line")).toHaveCount(0);
  await expect(page.getByText("Running main.py…")).toBeVisible();

  // The program ends: the session closes.
  await page.evaluate(() => (window as unknown as { __emit(e: string, p: unknown): void }).__emit("session", { session_id: "s1", event: { kind: "run_finished", error: null } }));
  await expect(page.getByRole("button", { name: /Debug main\.py/ })).toBeVisible();
});
