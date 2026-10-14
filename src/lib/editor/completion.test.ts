import { describe, expect, it } from "vitest";
import type { CompletionContext } from "@codemirror/autocomplete";
import { firstOf, hiddenPenalty, rankHidden } from "./completion";

describe("hidden-name ranking", () => {
  it("sinks dunders below mangled below private below public", () => {
    const order = ["value", "_cache", "__secret", "__init__"].map(hiddenPenalty);
    expect(order).toEqual([...order].sort((a, b) => b - a));
    expect(hiddenPenalty("value")).toBe(0);
    expect(hiddenPenalty("__init__")).toBeLessThan(hiddenPenalty("__secret"));
    expect(hiddenPenalty("__secret")).toBeLessThan(hiddenPenalty("_cache"));
  });

  it("keeps an existing boost and clamps to CodeMirror's range", () => {
    expect(rankHidden({ label: "_x", boost: 10 }).boost).toBe(-30);
    expect(rankHidden({ label: "__x__", boost: -50 }).boost).toBe(-99);
    expect(rankHidden({ label: "x", boost: 5 }).boost).toBe(5);
  });
});

describe("firstOf", () => {
  const ctx = {} as CompletionContext;
  it("falls through empty sources", async () => {
    const r = await firstOf(
      () => null,
      () => ({ from: 0, options: [] }),
      () => ({ from: 3, options: [{ label: "x" }] }),
    )(ctx);
    expect(r?.from).toBe(3);
  });
});
