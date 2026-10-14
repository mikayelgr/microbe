import { describe, expect, it } from "vitest";
import { compareVersions } from "./updates";

describe("compareVersions", () => {
  it("orders releases", () => {
    expect(compareVersions("1.29.0", "1.28.0")).toBeGreaterThan(0);
    expect(compareVersions("1.29.0", "1.29.0")).toBe(0);
    expect(compareVersions("1.9.4", "1.19.1")).toBeLessThan(0);
    expect(compareVersions("1.30.0-preview.1", "1.29.0")).toBeGreaterThan(0);
  });
});
