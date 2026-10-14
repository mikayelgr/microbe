import { describe, expect, it } from "vitest";
import { UI_ICONS } from "./ui";

describe("UI icons", () => {
  it("every name maps to a Lucide component", () => {
    for (const [name, icon] of Object.entries(UI_ICONS)) expect(typeof icon, name).toBe("function");
  });
});
