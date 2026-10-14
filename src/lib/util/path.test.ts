import { describe, expect, it } from "vitest";
import { baseName, joinPath, moduleName, parentPath } from "./path";

describe("path helpers", () => {
  it("joins", () => {
    expect(joinPath("/", "a.py")).toBe("/a.py");
    expect(joinPath("/lib", "a.py")).toBe("/lib/a.py");
    expect(joinPath("C:\\proj", "a.py")).toBe("C:\\proj\\a.py");
  });
  it("finds parents", () => {
    expect(parentPath("/lib/a.py")).toBe("/lib");
    expect(parentPath("/a.py")).toBe("/");
    expect(parentPath("./lib/a.py")).toBe("./lib");
    expect(parentPath("C:\\proj\\a.py")).toBe("C:\\proj");
  });
  it("names", () => {
    expect(baseName("/x/y/z.py")).toBe("z.py");
    expect(moduleName("./lib/foo.py")).toBe("lib.foo");
  });
});
