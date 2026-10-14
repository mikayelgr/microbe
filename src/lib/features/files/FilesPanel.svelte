<script lang="ts">
  // The local project folder.
  import Icon from "../../components/Icon.svelte";
  import FileTree from "../../components/FileTree.svelte";
  import { api } from "../../ipc";
  import type { TreeEntry } from "../../state/tree.svelte";
  import { project } from "../../state/project.svelte";
  import { editor } from "../../state/editor.svelte";
  import { boards } from "../../state/boards.svelte";
  import { menu, type MenuItem } from "../../state/menu.svelte";
  import { attempt, notices } from "../../state/notices.svelte";

  const tree = project.tree;
  let syncing = $state(false);
  const activePath = $derived(editor.active?.source.kind === "local" ? editor.active.source.path : null);

  function requireBoard() {
    const b = boards.active;
    if (!b?.connected) notices.push("info", "Connect a board first");
    return b?.connected ? b : null;
  }

  async function upload(e: TreeEntry) {
    const b = requireBoard();
    if (!b) return;
    const done = await attempt(() => api.fsUpload(b.id, [e.path], b.status.target?.fs_root ?? "/"), "Upload failed");
    if (done) notices.push("success", `Uploaded ${done.length} file${done.length === 1 ? "" : "s"} to ${b.label}`);
  }

  async function runOnBoard(e: TreeEntry) {
    const b = requireBoard();
    if (b) await attempt(async () => api.run(b.id, await api.projectRead(e.path)));
  }

  async function sync() {
    const b = requireBoard();
    if (!b || !project.root) return;
    syncing = true;
    const r = await attempt(
      () =>
        api.sync(b.id, {
          local_dir: project.root!,
          remote_dir: b.status.target?.fs_root ?? "/",
          ignore: [],
          delete_extra: false,
          dry_run: false,
        }),
      "Sync failed",
    );
    syncing = false;
    if (r)
      notices.push(
        "success",
        r.uploaded.length ? `Synced ${r.uploaded.length} changed file(s); ${r.unchanged} unchanged` : `Board is up to date (${r.unchanged} files)`,
      );
  }

  function contextMenu(ev: MouseEvent, e: TreeEntry | null) {
    const dir = e ? (e.is_dir ? e.path : null) : project.root;
    const items: MenuItem[] = [];
    if (dir) {
      items.push(
        { label: "New file", icon: "file-plus", run: () => tree.startCreate(dir, false) },
        { label: "New folder", icon: "folder-plus", run: () => tree.startCreate(dir, true) },
      );
    }
    if (e && !e.is_dir) {
      items.push(
        { label: "Open", icon: "file", run: () => editor.openLocal(e.path) },
        { label: "Run on board", icon: "play", disabled: !boards.active, run: () => runOnBoard(e) },
      );
    }
    if (e) {
      items.push(
        { label: "Upload to board", icon: "upload", disabled: !boards.active, run: () => upload(e) },
        { label: "Rename", icon: "edit", run: () => tree.startRename(e) },
        { label: "Delete", icon: "trash", danger: true, run: () => tree.remove(e) },
      );
    }
    if (items.length) menu.show(ev, items);
  }
</script>

<div class="panel">
  <div class="panel-header">
    <span class="ellipsis" title={project.root ?? undefined}>{project.name ?? "Explorer"}</span>
    <span class="spacer"></span>
    <div class="actions">
      {#if project.root}
        {@const root = project.root}
        <button class="ghost" title="New file" onclick={() => tree.startCreate(root, false)}><Icon name="file-plus" size={14} /></button>
        <button class="ghost" title="New folder" onclick={() => tree.startCreate(root, true)}><Icon name="folder-plus" size={14} /></button>
        <button class="ghost" title="Refresh" onclick={() => tree.reload()}><Icon name="refresh" size={14} /></button>
        <button class="ghost" title="Collapse folders" onclick={() => tree.collapseAll()}><Icon name="collapse" size={14} /></button>
        <button
          class="ghost"
          title={boards.active?.connected ? `Sync changed files to ${boards.active.label}` : "Sync to board (connect a board first)"}
          disabled={!boards.active?.connected || syncing}
          onclick={sync}><Icon name="sync" size={14} /></button
        >
      {/if}
      <button class="ghost" title="Open folder…" onclick={() => project.pick()}><Icon name="folder-open" size={14} /></button>
    </div>
  </div>
  {#if project.root}
    <FileTree {tree} {activePath} onopen={(e) => editor.openLocal(e.path)} oncontext={contextMenu} />
  {:else}
    <div class="welcome">
      <p>No folder is open.</p>
      <button class="primary" onclick={() => project.pick()} title="Choose a folder to use as the project">Open Folder…</button>
      <p class="faint">Files you open here can be run on a board, uploaded, or synced as a project.</p>
    </div>
  {/if}
</div>

<style>
  .welcome {
    padding: 10px 14px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    color: var(--text-dim);
  }
  .welcome p {
    margin: 0;
  }
  .welcome .primary {
    width: 100%;
    justify-content: center;
  }
</style>
