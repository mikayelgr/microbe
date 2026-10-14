// Completion sources and ranking for Python files.
import type { Completion, CompletionContext, CompletionResult, CompletionSource } from "@codemirror/autocomplete";
import { serverCompletionSource } from "@codemirror/lsp-client";
import { globalCompletion, localCompletionSource } from "@codemirror/lang-python";

// Boost is added to the match score; it only reorders comparable matches, so
// typing "_" still brings underscore names up.
const PRIVATE_BOOST = -40;
const MANGLED_BOOST = -60;
const DUNDER_BOOST = -80;

/** How far down a name should sink: dunders last, then mangled, then private. */
export function hiddenPenalty(label: string): number {
  if (/^__\w+__$/.test(label)) return DUNDER_BOOST;
  if (label.startsWith("__")) return MANGLED_BOOST;
  if (label.startsWith("_")) return PRIVATE_BOOST;
  return 0;
}

export function rankHidden(c: Completion): Completion {
  const penalty = hiddenPenalty(c.label);
  if (!penalty) return c;
  return { ...c, boost: Math.max(-99, (c.boost ?? 0) + penalty) };
}

function ranked(result: CompletionResult | null): CompletionResult | null {
  return result && { ...result, options: result.options.map(rankHidden) };
}

/** The first source that produces completions wins (LSP before local fallbacks). */
export function firstOf(...sources: CompletionSource[]): CompletionSource {
  return async (ctx: CompletionContext) => {
    for (const source of sources) {
      const result = await source(ctx);
      if (result && result.options.length) return result;
    }
    return null;
  };
}

/**
 * The language server when it's attached, otherwise lang-python's local-name
 * and keyword/builtin sources; hidden names always sink to the bottom.
 */
export const pythonCompletions: CompletionSource = async (ctx) =>
  ranked(await firstOf(serverCompletionSource, localCompletionSource, globalCompletion)(ctx));
