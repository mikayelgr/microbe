// A file tree over any backend (local project folder, a board's filesystem).
// Holds listings, expansion and inline create/rename state; FileTree renders it.
import { dialogs, validateFileName } from "./dialogs.svelte";
import { attempt } from "./notices.svelte";
import { baseName, joinPath, parentPath } from "../util/path";

export interface TreeEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size?: number;
}

export interface TreeBackend {
  list(dir: string): Promise<TreeEntry[]>;
  create(path: string, isDir: boolean): Promise<void>;
  rename(from: string, to: string): Promise<void>;
  remove(path: string): Promise<void>;
  /** Wording for confirmations, e.g. "from the board". */
  where?: string;
}

export interface TreeHooks {
  onCreated?(path: string, isDir: boolean): void;
  onRenamed?(from: string, to: string): void;
  onRemoved?(path: string): void;
}

export type TreeEdit =
  | { mode: "create"; dir: string; isDir: boolean }
  | { mode: "rename"; entry: TreeEntry };

export class TreeModel {
  root = $state<string | null>(null);
  dirs = $state<Record<string, TreeEntry[]>>({});
  expanded = $state<Record<string, boolean>>({});
  loading = $state(false);
  edit = $state<TreeEdit | null>(null);

  constructor(
    public backend: TreeBackend,
    public hooks: TreeHooks = {},
  ) {}

  async setRoot(root: string | null) {
    this.root = root;
    this.dirs = {};
    this.expanded = root ? { [root]: true } : {};
    this.edit = null;
    if (root) await this.load(root);
  }

  async load(dir: string) {
    this.loading = true;
    try {
      const list = await attempt(() => this.backend.list(dir), "Listing files");
      if (list) this.dirs[dir] = list;
    } finally {
      this.loading = false;
    }
  }

  async toggle(dir: string) {
    this.expanded[dir] = !this.expanded[dir];
    if (this.expanded[dir] && !this.dirs[dir]) await this.load(dir);
  }

  collapseAll() {
    this.expanded = this.root ? { [this.root]: true } : {};
  }

  async reload() {
    for (const dir of Object.keys(this.dirs)) await this.load(dir);
  }

  /** Show an inline name field for a new file/folder inside `dir`. */
  async startCreate(dir: string, isDir: boolean) {
    this.expanded[dir] = true;
    if (!this.dirs[dir]) await this.load(dir);
    this.edit = { mode: "create", dir, isDir };
  }

  startRename(entry: TreeEntry) {
    this.edit = { mode: "rename", entry };
  }

  cancelEdit() {
    this.edit = null;
  }

  validate(name: string): string | null {
    const err = validateFileName(name);
    if (err) return err;
    const e = this.edit;
    if (!e) return null;
    const dir = e.mode === "create" ? e.dir : parentPath(e.entry.path);
    const clash = (this.dirs[dir] ?? []).some((x) => x.name === name && !(e.mode === "rename" && x.path === e.entry.path));
    return clash ? `"${name}" already exists here` : null;
  }

  /** Apply the inline edit. Returns false (keeping the field open) on error. */
  async commitEdit(name: string): Promise<boolean> {
    const e = this.edit;
    if (!e) return true;
    name = name.trim();
    if (e.mode === "rename" && name === e.entry.name) return (this.edit = null), true;
    if (this.validate(name)) return false;
    this.edit = null;
    if (e.mode === "create") {
      const path = joinPath(e.dir, name);
      const ok = await attempt(() => this.backend.create(path, e.isDir), `Creating ${name}`);
      await this.load(e.dir);
      if (ok !== undefined) this.hooks.onCreated?.(path, e.isDir);
    } else {
      const dir = parentPath(e.entry.path);
      const to = joinPath(dir, name);
      const ok = await attempt(() => this.backend.rename(e.entry.path, to), `Renaming ${e.entry.name}`);
      await this.load(dir);
      if (ok !== undefined) this.hooks.onRenamed?.(e.entry.path, to);
    }
    return true;
  }

  async remove(entry: TreeEntry) {
    const ok = await dialogs.confirm({
      title: `Delete ${entry.is_dir ? "folder" : "file"} "${entry.name}"?`,
      message: `${entry.path}${this.backend.where ? ` will be deleted ${this.backend.where}` : " will be deleted"}${entry.is_dir ? " with everything in it" : ""}. This can't be undone.`,
      confirmLabel: "Delete",
      danger: true,
    });
    if (!ok) return;
    const done = await attempt(() => this.backend.remove(entry.path), `Deleting ${entry.name}`);
    await this.load(parentPath(entry.path));
    if (done !== undefined) this.hooks.onRemoved?.(entry.path);
  }
}

export { baseName };
