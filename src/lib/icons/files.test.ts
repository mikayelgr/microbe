import { describe, expect, it } from "vitest";
import { iconId, iconUrl } from "./files";

describe("file icons", () => {
  it("resolves by extension and name", () => {
    expect(iconId({ name: "main.py", isDir: false })).toBe("python");
    expect(iconId({ name: "stubs.pyi", isDir: false })).toBe("python");
    expect(iconId({ name: "data.json", isDir: false })).toBe("json");
    expect(iconId({ name: "lib.mpy", isDir: false })).toBe("python-misc");
    expect(iconId({ name: "unknown.zzz", isDir: false })).toBe("file");
  });
  it("resolves folders, open and closed", () => {
    expect(iconId({ name: "lib", isDir: true })).toBe("folder-lib");
    expect(iconId({ name: "lib", isDir: true, open: true })).toBe("folder-lib-open");
    expect(iconId({ name: "whatever", isDir: true })).toBe("folder");
  });
  it("maps every resolved id to an SVG", () => {
    for (const name of ["main.py", "README.md", "package.json", "x.zzz"]) expect(iconUrl(iconId({ name, isDir: false }))).toBeTruthy();
    expect(iconUrl(iconId({ name: "lib", isDir: true }))).toBeTruthy();
  });
});
