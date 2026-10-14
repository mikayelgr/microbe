// Parses serial output lines into named numeric samples (Arduino-plotter style):
//   "1.5 2 3"  "1.5,2,3"  "temp:21.5 hum:40"  "temp=21.5, hum=40"
export type Sample = Record<string, number>;

const NUM = /^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/;

export function parseLine(line: string): Sample | null {
  const text = line.trim();
  if (!text || text.startsWith(">>>") || text.startsWith("Traceback")) return null;
  const parts = text.split(/[\s,;\t]+/).filter(Boolean);
  if (!parts.length) return null;
  const out: Sample = {};
  let idx = 1;
  for (const part of parts) {
    const m = part.match(/^([A-Za-z_][\w.\-]*)[:=]([^:=]+)$/);
    if (m) {
      if (!NUM.test(m[2])) return null;
      out[m[1]] = parseFloat(m[2]);
    } else if (NUM.test(part)) {
      out[String(idx++)] = parseFloat(part);
    } else {
      return null;
    }
  }
  return Object.keys(out).length ? out : null;
}

/** Splits a byte stream into complete lines, keeping the partial tail. */
export class LineBuffer {
  #tail = "";
  #dec = new TextDecoder();

  push(bytes: Uint8Array | number[]): string[] {
    const chunk = this.#dec.decode(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes), { stream: true });
    const text = this.#tail + chunk;
    const lines = text.split(/\r?\n/);
    this.#tail = lines.pop() ?? "";
    if (this.#tail.length > 4096) this.#tail = "";
    return lines;
  }
}
