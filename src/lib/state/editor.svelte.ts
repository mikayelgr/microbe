// Open editor tabs. A tab is either a local file or a file on a board.
import type { EditorState } from "@codemirror/state";
import { api } from "../ipc";
import { attempt, notices } from "./notices.svelte";
import { dialogs } from "./dialogs.svelte";
import { baseName } from "../util/path";

export type TabSource = { kind: "local"; path: string } | { kind: "device"; boardId: string; path: string } | { kind: "scratch" };

export interface Tab {
  id: string;
  title: string;
  source: TabSource;
  /** Content as last loaded/saved. */
  saved: string;
  /** Current editor state (kept per tab so undo history survives switching). */
  state: EditorState | null;
  dirty: boolean;
}

let scratchCount = 1;

class Editor {
  tabs = $state<Tab[]>([]);
  activeId = $state<string | null>(null);
  active = $derived(this.tabs.find((t) => t.id === this.activeId) ?? null);
  /** A line to reveal once the tab is shown (handled by the editor view). */
  pendingLine = $state<{ tabId: string; line: number } | null>(null);
  /** Read the live document of a tab (set by the editor view). */
  docOf: (tab: Tab) => string = (t) => t.state?.doc.toString() ?? t.saved;

  key(source: TabSource): string {
    if (source.kind === "local") return `local:${source.path}`;
    if (source.kind === "device") return `device:${source.boardId}:${source.path}`;
    return `scratch:${scratchCount++}`;
  }

  async openLocal(path: string) {
    const id = this.key({ kind: "local", path });
    if (this.tabs.some((t) => t.id === id)) return void (this.activeId = id);
    const text = await attempt(() => api.projectRead(path), `Opening ${baseName(path)}`);
    if (text !== undefined) this.add({ id, title: baseName(path), source: { kind: "local", path }, saved: text, state: null, dirty: false });
  }

  /** Open a local file and put the cursor on `line` (1-based). */
  async openLocalAt(path: string, line: number) {
    await this.openLocal(path);
    const id = this.key({ kind: "local", path });
    if (this.tabs.some((t) => t.id === id)) this.pendingLine = { tabId: id, line };
  }

  /** The tab of a local file, if open. */
  localTab(path: string): Tab | undefined {
    return this.tabs.find((t) => t.source.kind === "local" && t.source.path === path);
  }

  async openDevice(boardId: string, path: string) {
    const id = this.key({ kind: "device", boardId, path });
    if (this.tabs.some((t) => t.id === id)) return void (this.activeId = id);
    const text = await attempt(() => api.fsReadText(boardId, path), `Opening ${baseName(path)}`);
    if (text !== undefined) this.add({ id, title: baseName(path), source: { kind: "device", boardId, path }, saved: text, state: null, dirty: false });
  }

  newScratch(content = "") {
    const id = this.key({ kind: "scratch" });
    this.add({ id, title: `untitled-${scratchCount - 1}.py`, source: { kind: "scratch" }, saved: content, state: null, dirty: false });
  }

  add(tab: Tab) {
    this.tabs.push(tab);
    this.activeId = tab.id;
  }

  async close(id: string) {
    const t = this.tabs.find((t) => t.id === id);
    if (!t) return;
    if (t.dirty) {
      const ok = await dialogs.confirm({
        title: `Close ${t.title} without saving?`,
        message: "Your changes will be lost.",
        confirmLabel: "Discard changes",
        danger: true,
      });
      if (!ok) return;
    }
    const i = this.tabs.indexOf(t);
    this.tabs.splice(i, 1);
    if (this.activeId === id) this.activeId = this.tabs[Math.min(i, this.tabs.length - 1)]?.id ?? null;
  }

  async save(tab = this.active) {
    if (!tab) return;
    const text = this.docOf(tab);
    const src = tab.source;
    if (src.kind === "scratch") return void notices.push("info", "Scratch files can't be saved yet — create a file in the project instead");
    const ok = await attempt(
      () => (src.kind === "local" ? api.projectWrite(src.path, text) : api.fsWriteText(src.boardId, src.path, text)),
      `Saving ${tab.title}`,
    );
    if (ok === undefined) return;
    tab.saved = text;
    tab.dirty = this.docOf(tab) !== text;
  }

  /** Tabs showing `path` or anything inside it (local, or on `boardId`). */
  #under(path: string, boardId: string | null): Tab[] {
    return this.tabs.filter((t) => {
      const s = t.source;
      if (s.kind === "scratch" || (boardId ? !(s.kind === "device" && s.boardId === boardId) : s.kind !== "local")) return false;
      return s.path === path || s.path.startsWith(path + "/") || s.path.startsWith(path + "\\");
    });
  }

  /** Follow a rename of a file or folder containing open files. */
  retarget(oldPath: string, newPath: string, boardId: string | null = null) {
    for (const t of this.#under(oldPath, boardId)) {
      const s = t.source as Exclude<TabSource, { kind: "scratch" }>;
      s.path = newPath + s.path.slice(oldPath.length);
      const wasActive = this.activeId === t.id;
      t.id = this.key(s);
      if (wasActive) this.activeId = t.id;
      t.title = baseName(s.path);
    }
  }

  /** Close tabs of a deleted file, or of files inside a deleted folder. */
  closeDeleted(path: string, boardId: string | null = null) {
    const gone = new Set(this.#under(path, boardId));
    if (!gone.size) return;
    const i = this.tabs.findIndex((t) => t.id === this.activeId);
    this.tabs = this.tabs.filter((t) => !gone.has(t));
    if (!this.tabs.some((t) => t.id === this.activeId)) {
      this.activeId = this.tabs[Math.min(Math.max(i, 0), this.tabs.length - 1)]?.id ?? null;
    }
  }
}

export const editor = new Editor();
