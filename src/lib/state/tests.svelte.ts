// The on-device test runner: discovers tests in the project (host side, with
// tree-sitter) and runs each test file on the active board with the
// `testrunner` prelude, collecting `TEST` records.
import { api, type TestFile } from "../ipc";
import { runTaps } from "../runtaps";
import { boards } from "./boards.svelte";
import { editor } from "./editor.svelte";
import { attempt, notices } from "./notices.svelte";
import { project } from "./project.svelte";
import { settings } from "./settings.svelte";

export type TestStatus = "pending" | "running" | "pass" | "fail" | "error" | "skip";

export interface TestResult {
  status: TestStatus;
  ms?: number;
  message?: string | null;
  traceback?: string | null;
}

interface TestRecord {
  name: string;
  status: "pass" | "fail" | "error" | "skip";
  ms: number;
  message: string | null;
  traceback: string | null;
}

export const resultKey = (file: string, name: string) => `${file}::${name}`;

class Tests {
  files = $state<TestFile[]>([]);
  results = $state<Record<string, TestResult>>({});
  running = $state(false);
  discovering = $state(false);
  #current: { boardId: string; file: string } | null = null;
  #cancelled = false;

  constructor() {
    runTaps.on("TEST", (boardId, p) => {
      const c = this.#current;
      if (!c || c.boardId !== boardId) return;
      const r = p as TestRecord;
      this.results[resultKey(c.file, r.name)] = { status: r.status, ms: r.ms, message: r.message, traceback: r.traceback };
    });
  }

  get summary() {
    const counts = { pass: 0, fail: 0, error: 0, skip: 0, total: 0 };
    for (const f of this.files)
      for (const t of f.tests) {
        counts.total++;
        const s = this.results[resultKey(f.path, t.name)]?.status;
        if (s === "pass" || s === "fail" || s === "error" || s === "skip") counts[s]++;
      }
    return counts;
  }

  async discover() {
    if (!project.root) return void (this.files = []);
    this.discovering = true;
    const files = await attempt(() => api.testsDiscover(project.root!), "Finding tests");
    this.discovering = false;
    if (files) this.files = files;
  }

  /** Run the given files (all by default), optionally only some tests of one file. */
  async run(only?: { file: TestFile; names?: string[] }[]) {
    const b = boards.active;
    if (!b?.connected) return void notices.push("info", "Connect a board to run tests on it");
    if (b.running || this.running) return void notices.push("info", "Wait for the running program to finish");
    if (!only) await this.discover();
    const plan = only ?? this.files.map((file) => ({ file, names: undefined as string[] | undefined }));
    if (!plan.length) return void notices.push("info", "No tests found: add test_*.py files with test_* functions");

    this.running = true;
    this.#cancelled = false;
    for (const { file, names } of plan)
      for (const n of names ?? file.tests.map((t) => t.name)) this.results[resultKey(file.path, n)] = { status: "pending" };

    try {
      if (settings.value.syncBeforeTests && project.root) {
        const synced = await attempt(
          () => api.sync(b.id, { local_dir: project.root!, remote_dir: b.status.target?.fs_root ?? "/", ignore: [], delete_extra: false, dry_run: false }),
          "Syncing the project before tests",
        );
        if (!synced) return;
      }
      for (const { file, names } of plan) {
        if (this.#cancelled) break;
        await this.#runFile(b.id, file, names ?? file.tests.map((t) => t.name));
      }
    } finally {
      this.running = false;
      this.#current = null;
      for (const [k, r] of Object.entries(this.results)) if (r.status === "pending" || r.status === "running") delete this.results[k];
    }
    const s = this.summary;
    notices.push(s.fail || s.error ? "warn" : "success", `Tests: ${s.pass} passed, ${s.fail} failed, ${s.error} errors${s.skip ? `, ${s.skip} skipped` : ""}`);
  }

  async #runFile(boardId: string, file: TestFile, names: string[]) {
    const tab = editor.localTab(file.path);
    const source = tab ? editor.docOf(tab) : await api.projectRead(file.path);
    for (const n of names) this.results[resultKey(file.path, n)] = { status: "running" };
    this.#current = { boardId, file: file.path };
    const done = boards.waitRunFinished(boardId, { quiet: true });
    const code = `${source}\n__mb_tests.run(globals(), ${JSON.stringify(names)})\n`;
    const started = await attempt(() => api.run(boardId, code, [{ script: "testrunner", args: {}, cleanup: "testrunner_end" }]), `Running ${file.rel}`);
    if (started === undefined) return;
    const error = await done;
    // Tests that never reported: the file failed to import, or the run was stopped.
    for (const n of names) {
      const k = resultKey(file.path, n);
      if (this.results[k]?.status === "running") this.results[k] = { status: "error", message: error ?? "did not run" };
    }
  }

  async stop() {
    this.#cancelled = true;
    const b = boards.active;
    if (b?.running) await attempt(() => api.stop(b.id), "Stop");
  }
}

export const tests = new Tests();
