import { describe, expect, it } from "vitest";
import { fragmentation, objectAt, parseHeap } from "./heap";

const ROW1 = "h=hh=hhLhhSh===h========hhhBDh.LhA..h=h==Sh===h===".padEnd(64, ".");
const ROW6 = ".".repeat(54) + "h=" + ".".repeat(8);
const SAMPLE = `stack: 1008 out of 80000
GC: total: 6144, used: 608, free: 5536
 No. of 1-blocks: 17, 2-blocks: 3, max blk sz: 9, max free sz: 120
GC memory layout; from 7886805f00:
00000000: ${ROW1}
       (4 lines all free)
00001400: ${ROW6}
`;

describe("heap map", () => {
  it("parses totals and expands free lines", () => {
    const h = parseHeap(SAMPLE)!;
    expect(h).toMatchObject({ total: 6144, used: 608, free: 5536, maxFreeBlocks: 120 });
    expect(h.blocks.length).toBe(64 * 6);
    expect(h.blockSize).toBe(16);
    expect(h.blocks.slice(64, 64 * 5)).toBe(".".repeat(256));
  });

  it("finds the object under a block", () => {
    const h = parseHeap(SAMPLE)!;
    expect(objectAt(h.blocks, 16)).toEqual({ head: 15, kind: "h", length: 9 });
    expect(objectAt(h.blocks, 7)).toEqual({ head: 7, kind: "L", length: 1 });
    expect(objectAt(h.blocks, 30).kind).toBe(".");
  });

  it("measures fragmentation", () => {
    const h = parseHeap(SAMPLE)!;
    expect(fragmentation(h)).toBeCloseTo(1 - (120 * 16) / 5536);
    expect(parseHeap("nothing")).toBeNull();
  });
});
