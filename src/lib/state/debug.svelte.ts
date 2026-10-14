// The debugger (see src-tauri/src/debug.rs): runs an instrumented copy of the
// current file with the `debugger` prelude, shows where it stopped, and sends
// commands over the program's stdin.
import { api } from "../ipc";
import { runTaps } from "../runtaps";
import { runtime } from "../runtime/runtime.svelte";
import { boards } from "./boards.svelte";
import { editor, type Tab } from "./editor.svelte";
import { attempt, notices } from "./notices.svelte";

export interface StopRecord {
  event: "stopped";
  line: number;
  reason: "breakpoint" | "step" | "pause";
  /** Outermost first. */
  stack: string[];
  locals: Record<string, string>;
  globals: Record<string, string>;
}

export interface EvalRecord {
  event: "eval";
  expr: string;
  result: string | null;
  error: string | null;
}

class Debugger {
  /** Breakpoint lines by tab id (kept in sync by the editor's gutter). */
  breakpoints = $state<Record<string, number[]>>({});
  session = $state<{ boardId: string; tabId: string; title: string; lines: number[] } | null>(null);
  stopped = $state<StopRecord | null>(null);
  evals = $state<EvalRecord[]>([]);
  /** Replaces a tab's breakpoints in its editor state (set by the editor view). */
  applyBreakpoints: ((tabId: string, lines: number[]) => void) | null = null;

  constructor() {
    runTaps.on("DBG", (boardId, p) => {
      if (this.session?.boardId !== boardId) return;
      const rec = p as StopRecord | EvalRecord;
      if (rec.event === "stopped") {
        this.stopped = rec;
        if (editor.activeId !== this.session.tabId) editor.activeId = this.session.tabId;
      } else if (rec.event === "eval") {
        this.evals = [...this.evals.slice(-49), rec];
      }
    });
  }

  get active() {
    return this.session !== null;
  }

  setBreakpoints(tabId: string, lines: number[]) {
    this.breakpoints[tabId] = lines;
    if (this.session?.tabId === tabId) this.#send(`b ${lines.join(",")}`);
  }

  removeBreakpoint(tabId: string, line: number) {
    const lines = (this.breakpoints[tabId] ?? []).filter((l) => l !== line);
    this.applyBreakpoints?.(tabId, lines);
    this.setBreakpoints(tabId, lines);
  }

  clearBreakpoints() {
    for (const tabId of Object.keys(this.breakpoints)) {
      this.applyBreakpoints?.(tabId, []);
      this.setBreakpoints(tabId, []);
    }
  }

  async start(tab: Tab | null, boardId: string | null) {
    if (this.session) return;
    if (!tab) return void notices.push("info", "Open a file to debug it");
    const b = boardId ? boards.get(boardId) : null;
    if (!b?.connected) return void notices.push("info", "Connect a board to debug on it");
    if (b.running) return void notices.push("info", "Stop the running program first");
    const inst = await attempt(() => api.debugInstrument(editor.docOf(tab)), "Preparing to debug");
    if (!inst) return;
    const bps = this.breakpoints[tab.id] ?? [];
    this.session = { boardId: b.id, tabId: tab.id, title: tab.title, lines: inst.lines };
    this.stopped = null;
    this.evals = [];
    runtime.startRun(b.id);
    const done = boards.waitRunFinished(b.id);
    const preludes = [
      { script: "debugger", args: { breakpoints: JSON.stringify(bps), functions: JSON.stringify(inst.functions) } },
      ...runtime.preludes(),
    ];
    const ok = await attempt(() => api.run(b.id, inst.code, preludes), "Debug");
    if (ok === undefined) return void (this.session = null);
    void done.then(() => {
      this.session = null;
      this.stopped = null;
    });
  }

  #send(cmd: string) {
    const s = this.session;
    if (s) void attempt(() => api.write(s.boardId, `${cmd}\r`), "Debugger");
  }

  #resume(cmd: string) {
    if (!this.stopped) return;
    this.stopped = null;
    this.#send(cmd);
  }

  continue() {
    this.#resume("c");
  }
  stepOver() {
    this.#resume("n");
  }
  stepInto() {
    this.#resume("s");
  }
  stepOut() {
    this.#resume("o");
  }
  pause() {
    if (this.session && !this.stopped) this.#send("p");
  }

  evaluate(expr: string) {
    const e = expr.replace(/[\r\n]+/g, " ").trim();
    if (e && this.stopped) this.#send(`e ${e}`);
  }

  async stop() {
    const s = this.session;
    if (s) await attempt(() => api.stop(s.boardId), "Stop");
  }
}

export const debug = new Debugger();
