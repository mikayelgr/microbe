import { expect, test, type Page } from "@playwright/test";

type W = Window & {
  __calls: { run: { code: string; preludes: { script: string }[] }[]; term: number | null; log: [string, Record<string, unknown>][] };
  __stubs: Record<string, (a: Record<string, unknown>) => unknown>;
  __channel(id: number, m: unknown): void;
  __emit(e: string, p: unknown): void;
  connectBoard(): Promise<unknown>;
  openProject(dir: string): Promise<unknown>;
};

const HEAP = `GC: total: 6144, used: 608, free: 5536
 No. of 1-blocks: 17, 2-blocks: 3, max blk sz: 9, max free sz: 120
GC memory layout; from 20000000:
00000000: ${"hLLDS=h===".padEnd(64, ".")}
       (5 lines all free)
`;

async function setup(page: Page, extras: Record<string, unknown> = {}, capabilities: string[] = []) {
  await page.addInitScript(([heap, extras, capabilities]) => {
    const w = window as unknown as W;
    const calls: W["__calls"] = { run: [], term: null, log: [] };
    w.__calls = calls;
    w.__stubs = {
      device_connect: (a) => {
        calls.term = (a.term as { id: number }).id;
        return {
          session_id: "s1",
          connected: true,
          state: "idle",
          endpoint: { transport: "serial", address: "/dev/fake", label: "Fake Pico", detail: null, usb: null, platform_hint: "rp2", role: null },
          target: { platform: "rp2", platform_name: "RP2", board_id: "RPI_PICO", capabilities, fs_root: "/" },
          info: null,
        };
      },
      project_list: () => [],
      project_read: () => "def test_one():\n    assert True\n\ndef test_two():\n    assert 1 == 2, 'one is not two'\n",
      project_sync: () => ({ uploaded: [], deleted: [], unchanged: 3 }),
      tests_discover: () => [
        { path: "/p/tests/test_math.py", rel: "tests/test_math.py", tests: [{ name: "test_one", line: 1 }, { name: "test_two", line: 4 }] },
      ],
      device_run: (a) => (calls.run.push(a as unknown as W["__calls"]["run"][number]), null),
      device_script: (a) => (a.script === "heap" ? heap : ""),
    };
    for (const [k, v] of Object.entries(extras)) w.__stubs[k] = () => v;
    for (const [k, fn] of Object.entries(w.__stubs)) w.__stubs[k] = (a) => (calls.log.push([k, a]), fn(a));
  }, [HEAP, extras, capabilities] as const);
  await page.goto("/e2e/pages/app.html");
  await page.evaluate(() => (window as unknown as W).connectBoard());
}

const send = (page: Page, message: unknown) =>
  page.evaluate((m) => {
    const w = window as unknown as W;
    w.__channel(w.__calls.term!, m);
  }, message);

test("tests run on the board and report per test", async ({ page }) => {
  await setup(page);
  await page.evaluate(() => (window as unknown as W).openProject("/p"));
  await page.getByRole("button", { name: "Tests", exact: true }).click();
  await expect(page.getByText("tests/test_math.py")).toBeVisible();
  await page.getByRole("button", { name: "Run all tests on the board" }).click();

  await expect.poll(() => page.evaluate(() => (window as unknown as W).__calls.run.length)).toBe(1);
  const run = await page.evaluate(() => (window as unknown as W).__calls.run[0]);
  expect(run.code).toContain('__mb_tests.run(globals(), ["test_one","test_two"])');
  expect(run.preludes[0].script).toBe("testrunner");

  await send(page, { kind: "structured", tag: "TEST", payload: { name: "test_one", status: "pass", ms: 1, message: null, traceback: null } });
  await send(page, { kind: "structured", tag: "TEST", payload: { name: "test_two", status: "fail", ms: 2, message: "one is not two", traceback: null } });
  await page.evaluate(() => (window as unknown as W).__emit("session", { session_id: "s1", event: { kind: "run_finished", error: null } }));

  await expect(page.locator(".failure")).toContainText("one is not two");
  await expect(page.getByText("1 passed", { exact: true })).toBeVisible();
  await expect(page.getByText("1 failed", { exact: true })).toBeVisible();
});

test("memory snapshot and asyncio tasks", async ({ page }) => {
  await setup(page);
  await page.getByRole("button", { name: "Memory", exact: true }).click();
  await page.getByRole("button", { name: "Snapshot" }).click();
  await expect(page.getByText("fragmentation")).toBeVisible();
  await expect(page.getByText("65%")).toBeVisible();
  await expect(page.getByText("list", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Tasks", exact: true }).click();
  await send(page, {
    kind: "structured",
    tag: "TASKS",
    payload: { now: 1, tasks: [{ id: 1, name: "blink", state: "sleeping", wake_ms: 120, on: null, error: null }, { id: 2, name: "reader", state: "waiting", wake_ms: null, on: "TaskQueue", error: null }] },
  });
  await expect(page.getByText("wakes in 120 ms")).toBeVisible();
  await expect(page.getByText("waiting on an event, lock or task")).toBeVisible();
});

test("ESP32 partitions and an over-the-air update", async ({ page }) => {
  await setup(
    page,
    {
      esp32_partitions: {
        partitions: [
          { label: "nvs", kind: "data", type_id: 1, subtype: 2, subtype_name: "nvs", offset: 0x9000, size: 0x6000, encrypted: false },
          { label: "ota_0", kind: "app", type_id: 0, subtype: 16, subtype_name: "ota_0", offset: 0x10000, size: 0x180000, encrypted: false },
          { label: "ota_1", kind: "app", type_id: 0, subtype: 17, subtype_name: "ota_1", offset: 0x190000, size: 0x180000, encrypted: false },
        ],
        running: "ota_0",
        next_update: "ota_1",
        csv: "",
      },
      firmware_flash: { file: "fw.app-bin", version: "1.29.0", method: "esp32_ota", port: null },
    },
    ["esp32_partitions"],
  );
  await page.getByRole("button", { name: "Board", exact: true }).click();
  await page.getByRole("button", { name: "ESP32 partitions" }).click();
  await expect(page.getByText("ota_0 ●")).toBeVisible();
  await expect(page.getByText("ota_1 ○")).toBeVisible();
  await page.getByRole("button", { name: "Update to latest" }).click();
  await page.getByRole("button", { name: "Update", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__calls.log.some(([c]) => c === "firmware_flash"))).toBe(true);
  const req = await page.evaluate(() => (window as unknown as W).__calls.log.find(([c]) => c === "firmware_flash")![1].request);
  expect(req).toMatchObject({ method: "esp32_ota", session_id: "s1", target: { kind: "session", session_id: "s1" } });
  await expect(page.getByText("Updated to MicroPython 1.29.0 over the air")).toBeVisible();
});
