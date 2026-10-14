// UI conventions enforced across all components.
import { describe, expect, it } from "vitest";
import { hasAttr, openTags } from "./util/markup";

const files = import.meta.glob("/src/**/*.svelte", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("buttons", () => {
  it("every button has a hover hint (title / data-tip)", () => {
    const missing: string[] = [];
    for (const [path, source] of Object.entries(files)) {
      for (const tag of openTags(source, "button")) {
        if (!hasAttr(tag, "title", "data-tip")) missing.push(`${path}:${tag.line}`);
      }
    }
    expect(missing).toEqual([]);
  });
});

