// Tiny Svelte-markup scanner used by the UI lint tests: finds opening tags,
// skipping `>` inside {expressions} and quoted strings.
export interface OpenTag {
  name: string;
  attrs: string;
  line: number;
}

export function openTags(source: string, name: string): OpenTag[] {
  const out: OpenTag[] = [];
  const re = new RegExp(`<${name}\\b`, "g");
  let m: RegExpExecArray | null;
  while ((m = re.exec(source))) {
    let i = m.index + m[0].length;
    let depth = 0;
    let quote: string | null = null;
    for (; i < source.length; i++) {
      const c = source[i];
      if (quote) {
        if (c === quote && source[i - 1] !== "\\") quote = null;
      } else if (c === '"' || c === "'" || c === "`") quote = c;
      else if (c === "{") depth++;
      else if (c === "}") depth--;
      else if (c === ">" && depth === 0) break;
    }
    out.push({ name, attrs: source.slice(m.index + m[0].length, i), line: source.slice(0, m.index).split("\n").length });
  }
  return out;
}

/** Whether a tag's attributes include any of the given attribute names. */
export function hasAttr(tag: OpenTag, ...names: string[]): boolean {
  return names.some((n) => new RegExp(`(^|\\s)${n}(=|\\s|$)`).test(tag.attrs));
}
