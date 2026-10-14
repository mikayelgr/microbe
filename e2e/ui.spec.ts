import { expect, test } from "@playwright/test";

const SAMPLE = "import machine\nfrom time import sleep\n\nled = machine.Pin(25, machine.Pin.OUT)\nwhile True:\n    led.toggle()\n";

async function openEditor(page: import("@playwright/test").Page) {
  await page.goto("/e2e/pages/app.html");
  await page.evaluate((t) => (window as unknown as { openSample(t: string): void }).openSample(t), SAMPLE);
  await page.locator(".cm-content").click();
}

for (const colorScheme of ["light", "dark"] as const) {
  test.describe(`${colorScheme} theme`, () => {
    test.use({ colorScheme });

    test("select-all paints the selection in the selection colour", async ({ page }) => {
      await openEditor(page);
      await page.keyboard.press("ControlOrMeta+a");
      const sel = page.locator(".cm-selectionBackground").first();
      await expect(sel).toBeVisible();
      const [painted, token] = await sel.evaluate((el) => {
        const probe = document.createElement("div");
        probe.style.background = "var(--sel-bg)";
        document.body.append(probe);
        const expected = getComputedStyle(probe).backgroundColor;
        probe.remove();
        return [getComputedStyle(el).backgroundColor, expected];
      });
      expect(painted).toBe(token);
      // Blue: the blue channel dominates.
      const [r, g, b] = painted.match(/\d+/g)!.map(Number);
      expect(b).toBeGreaterThan(r + 40);
      expect(b).toBeGreaterThan(g);
    });
  });
}

test("buttons show a hover hint with their shortcut", async ({ page }) => {
  await openEditor(page);
  await page.getByRole("button", { name: "Run" }).hover();
  const tip = page.getByRole("tooltip");
  await expect(tip).toContainText("Run the current file on the board");
  await expect(tip).toContainText("⌘↵");
});

test("analysis findings are underlined with their message", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as { __stubs: Record<string, (a: { request: { source: string } }) => unknown> }).__stubs = {
      analysis_run: ({ request }) => {
        const from = request.source.indexOf("25");
        return {
          findings: [{ rule: "pins", severity: "warning", message: "GPIO 25 is wired to the CYW43 wireless chip", from, to: from + 2 }],
          suppressions: [],
        };
      },
    };
  });
  await openEditor(page);
  const mark = page.locator(".cm-lintRange-warning");
  await expect(mark).toHaveText("25");
  await mark.hover();
  await expect(page.locator(".cm-tooltip-lint")).toContainText("GPIO 25 is wired to the CYW43 wireless chip");
});

test("enabling the MCP server shows its address and client setup", async ({ page }) => {
  await page.addInitScript(() => {
    const status = (running: boolean, allow_write = true) => ({
      running,
      url: running ? "http://127.0.0.1:7357/mcp" : null,
      token: "0123456789abcdef",
      allow_write,
      tools: running ? ["list_boards", "exec"] : [],
    });
    (window as unknown as { __stubs: Record<string, (a: { options?: { allow_write: boolean } }) => unknown> }).__stubs = {
      analysis_checks: () => [{ id: "pins", description: "Pin numbers checked against the connected board's GPIO map", needs_board: true }],
      mcp_status: () => status(false),
      mcp_start: ({ options }) => status(true, options!.allow_write),
      mcp_stop: () => status(false),
    };
  });
  await page.goto("/e2e/pages/app.html");
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByText("Pin numbers checked against the connected board's GPIO map")).toBeVisible();
  await page.getByLabel("Let AI agents use the connected boards").check();
  await expect(page.getByText("http://127.0.0.1:7357/mcp")).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy for Claude Code" })).toBeVisible();
  await expect(page.getByText("2 tools: list_boards, exec")).toBeVisible();
  await page.getByLabel("Let AI agents use the connected boards").uncheck();
  await expect(page.getByText("http://127.0.0.1:7357/mcp")).toBeHidden();
});
