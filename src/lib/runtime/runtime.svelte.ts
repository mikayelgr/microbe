// Live data from running programs, fed by structured records (`MEM`,
// `TASKS`), plus the preludes that produce them. Which preludes run is up to
// the user (Memory and Tasks tabs).
import { runTaps } from "../runtaps";
import type { Prelude } from "../ipc";
import { settings } from "../state/settings.svelte";

export interface MemSample {
  /** Seconds since the run started. */
  t: number;
  free: number;
  alloc: number;
}

export interface TaskInfo {
  id: number;
  name: string;
  state: "running" | "ready" | "sleeping" | "waiting" | "done" | "cancelled" | "failed";
  wake_ms: number | null;
  /** What a waiting task waits on (a TaskQueue: an event, lock or other task). */
  on: string | null;
  error: string | null;
}

const MAX_SAMPLES = 2000;
const PERIOD_MS = 250;

class Runtime {
  mem = $state<Record<string, MemSample[]>>({});
  tasks = $state<Record<string, TaskInfo[]>>({});
  #started: Record<string, number> = {};

  constructor() {
    runTaps.on("MEM", (id, p) => {
      const { free, alloc } = p as { free: number; alloc: number };
      const list = (this.mem[id] ??= []);
      list.push({ t: (performance.now() - (this.#started[id] ?? performance.now())) / 1000, free, alloc });
      if (list.length > MAX_SAMPLES) list.splice(0, list.length - MAX_SAMPLES);
    });
    runTaps.on("TASKS", (id, p) => {
      this.tasks[id] = (p as { tasks: TaskInfo[] }).tasks;
    });
  }

  /** A program is starting on `boardId`: reset its live data. */
  startRun(boardId: string) {
    this.#started[boardId] = performance.now();
    this.mem[boardId] = [];
    this.tasks[boardId] = [];
  }

  /** Helpers to run before a program, as chosen in the Memory and Tasks tabs. */
  preludes(): Prelude[] {
    const out: Prelude[] = [];
    const args = { period_ms: PERIOD_MS };
    if (settings.value.sampleMemory) out.push({ script: "mem_watch", args, cleanup: "mem_watch_end" });
    if (settings.value.inspectAsyncio) out.push({ script: "aio_watch", args, cleanup: "aio_watch_end" });
    return out;
  }
}

export const runtime = new Runtime();
