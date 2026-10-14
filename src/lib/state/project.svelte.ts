// The open project folder.
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../ipc";
import { settings } from "./settings.svelte";
import { TreeModel } from "./tree.svelte";
import { editor } from "./editor.svelte";
import { baseName } from "../util/path";

class Project {
  tree = new TreeModel(
    {
      list: (dir) => api.projectList(dir),
      create: (path, isDir) => api.projectCreate(path, isDir),
      rename: (from, to) => api.projectRename(from, to),
      remove: (path) => api.projectRemove(path),
    },
    {
      onCreated: (path, isDir) => !isDir && editor.openLocal(path),
      onRenamed: (from, to) => editor.retarget(from, to),
      onRemoved: (path) => editor.closeDeleted(path),
    },
  );

  get root() {
    return this.tree.root;
  }

  get name() {
    return this.root ? baseName(this.root) : null;
  }

  async init() {
    const last = settings.value.lastProject;
    if (last) await this.openFolder(last).catch(() => {});
  }

  async pick() {
    const dir = await open({ directory: true, multiple: false, title: "Open project folder" });
    if (typeof dir === "string") await this.openFolder(dir);
  }

  async openFolder(dir: string) {
    await api.projectList(dir); // throws if the folder is gone
    await this.tree.setRoot(dir);
    await settings.set("lastProject", dir);
  }
}

export const project = new Project();
