import { describe, expect, it } from "vitest";
import type { Diagnostic } from "@codemirror/lint";
import { isSuppressed, merge, nameIn, toDiagnostics, type Region } from "./diagnostics";

const diag = (from: number, message: string): Diagnostic => ({ from, to: from + 1, severity: "error", message });

describe("diagnostic merging", () => {
  it("extracts the name a message is about", () => {
    expect(nameIn("Name `wrap_target` used when not defined")).toBe("wrap_target");
    expect(nameIn('"set" is not defined')).toBe("set");
    expect(nameIn("Could not find name `pins`")).toBe("pins");
    expect(nameIn("Expected 2 arguments")).toBeNull();
  });

  it("suppresses only listed names inside a region", () => {
    const regions: Region[] = [{ from: 10, to: 50, names: new Set(["wrap", "pins"]) }];
    expect(isSuppressed(diag(20, "Name `wrap` used when not defined"), regions)).toBe(true);
    expect(isSuppressed(diag(20, "Name `typo` used when not defined"), regions)).toBe(false);
    expect(isSuppressed(diag(60, "Name `wrap` used when not defined"), regions)).toBe(false);
    // Regions without names (inline assembler) hide any undefined name.
    expect(isSuppressed(diag(20, "Name `r0` used when not defined"), [{ from: 0, to: 30, names: null }])).toBe(true);
  });

  it("keeps analysis findings and filtered server diagnostics", () => {
    const merged = merge({
      server: [diag(5, "Name `set` used when not defined"), diag(80, "Name `x` used when not defined")],
      analysis: [diag(1, "`threading` isn't available in MicroPython")],
      regions: [{ from: 0, to: 40, names: new Set(["set"]) }],
    });
    expect(merged.map((d) => d.from)).toEqual([80, 1]);
  });

  it("converts engine results, clamped to the document", () => {
    const { diagnostics, regions } = toDiagnostics(
      {
        findings: [{ rule: "pins", severity: "warning", message: "GPIO 25 …", from: 3, to: 99 }],
        suppressions: [{ filter: "inline-code", from: 0, to: 10, names: [] }],
      },
      20,
    );
    expect(diagnostics[0]).toMatchObject({ from: 3, to: 20, severity: "warning", source: "pins" });
    expect(regions[0].names).toBeNull();
  });
});
