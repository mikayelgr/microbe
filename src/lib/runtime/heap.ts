// Parse `micropython.mem_info(1)`: GC totals plus the block map, where each
// character is one GC block ("." free, "h"/letters an object head, "=" the
// rest of an object).
export interface HeapMap {
  total: number;
  used: number;
  free: number;
  /** Largest run of free blocks. */
  maxFreeBlocks: number;
  /** Bytes per GC block. */
  blockSize: number;
  /** One character per block, free runs expanded. */
  blocks: string;
}

export interface BlockKind {
  label: string;
  /** Index into the categorical palette; -1 for free. */
  color: number;
}

export const BLOCK_KINDS: Record<string, BlockKind> = {
  ".": { label: "free", color: -1 },
  h: { label: "object", color: 0 },
  T: { label: "tuple", color: 1 },
  L: { label: "list", color: 2 },
  D: { label: "dict", color: 3 },
  S: { label: "str / bytes", color: 4 },
  A: { label: "array / bytearray", color: 5 },
  F: { label: "float", color: 6 },
  B: { label: "bytecode", color: 7 },
  M: { label: "module", color: 8 },
};

export function kindOf(c: string): BlockKind {
  return BLOCK_KINDS[c] ?? { label: `other (${c})`, color: 9 };
}

export function parseHeap(text: string): HeapMap | null {
  const gc = /GC: total: (\d+), used: (\d+), free: (\d+)/.exec(text);
  if (!gc) return null;
  const maxFree = /max free sz: (\d+)/.exec(text);
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((l) => l.startsWith("GC memory layout"));
  let blocks = "";
  let perLine = 64;
  if (start >= 0) {
    for (const line of lines.slice(start + 1)) {
      const row = /^\s*[0-9a-f]+: (\S*)\s*$/.exec(line);
      const free = /^\s*\((\d+) lines all free\)/.exec(line);
      if (row) {
        if (row[1].length > perLine || blocks.length === 0) perLine = Math.max(perLine, row[1].length);
        blocks += row[1];
      } else if (free) {
        blocks += ".".repeat(Number(free[1]) * perLine);
      }
    }
  }
  const total = Number(gc[1]);
  const free = Number(gc[3]);
  return {
    total,
    used: Number(gc[2]),
    free,
    maxFreeBlocks: maxFree ? Number(maxFree[1]) : 0,
    blockSize: blocks.length ? Math.max(1, Math.round(total / blocks.length)) : 16,
    blocks,
  };
}

/** Share of free memory that isn't in the largest free run (0 = none). */
export function fragmentation(h: HeapMap): number {
  if (!h.free) return 0;
  return Math.max(0, 1 - (h.maxFreeBlocks * h.blockSize) / h.free);
}

/** The object containing block `i`: its head index, kind and length in blocks. */
export function objectAt(blocks: string, i: number): { head: number; kind: string; length: number } {
  if (blocks[i] === ".") {
    let a = i, b = i;
    while (a > 0 && blocks[a - 1] === ".") a--;
    while (b + 1 < blocks.length && blocks[b + 1] === ".") b++;
    return { head: a, kind: ".", length: b - a + 1 };
  }
  let head = i;
  while (head > 0 && blocks[head] === "=") head--;
  let end = head + 1;
  while (end < blocks.length && blocks[end] === "=") end++;
  return { head, kind: blocks[head], length: end - head };
}
