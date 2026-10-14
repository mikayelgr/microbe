// Routes structured run records (`\x1e<TAG> <json>`) to the features that
// handle them: debugger, asyncio inspector, test runner, memory sampler...
export type RunTap = (boardId: string, payload: unknown) => void;

class RunTaps {
  #taps = new Map<string, Set<RunTap>>();

  on(tag: string, tap: RunTap): () => void {
    if (!this.#taps.has(tag)) this.#taps.set(tag, new Set());
    this.#taps.get(tag)!.add(tap);
    return () => this.#taps.get(tag)?.delete(tap);
  }

  dispatch(boardId: string, tag: string, payload: unknown) {
    const taps = this.#taps.get(tag);
    if (!taps?.size) {
      console.debug("unhandled run record", tag, payload);
      return;
    }
    for (const t of taps) t(boardId, payload);
  }
}

export const runTaps = new RunTaps();
