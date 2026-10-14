<script lang="ts">
  // The connected board: info, controls and its filesystem.
  import { open, save } from "@tauri-apps/plugin-dialog";
  import Icon from "../../components/Icon.svelte";
  import FileTree from "../../components/FileTree.svelte";
  import { api } from "../../ipc";
  import { boards, type Board } from "../../state/boards.svelte";
  import { editor } from "../../state/editor.svelte";
  import { menu, type MenuItem } from "../../state/menu.svelte";
  import { dialogs } from "../../state/dialogs.svelte";
  import { attempt, notices } from "../../state/notices.svelte";
  import { TreeModel, type TreeEntry } from "../../state/tree.svelte";
  import { formatBytes } from "../../util/format";
  import { moduleName } from "../../util/path";
  import { rescueBoard } from "./rescue";
  import { mcp } from "../../state/mcp.svelte";
  import Esp32Section from "./Esp32Section.svelte";

  let { onconnect }: { onconnect?: () => void } = $props();
  const board = $derived(boards.active);
  const root = $derived(board?.status.target?.fs_root ?? "/");
  const activePath = $derived(
    editor.active?.source.kind === "device" && editor.active.source.boardId === board?.id ? editor.active.source.path : null,
  );

  function boardTree(b: Board) {
    const id = b.id;
    return new TreeModel(
      {
        list: (dir) => api.fsList(id, dir),
        create: (path, isDir) => (isDir ? api.fsMkdir(id, path) : api.fsWriteText(id, path, "")),
        rename: (from, to) => api.fsRename(id, from, to),
        remove: (path) => api.fsRemove(id, path),
        where: "from the board",
      },
      {
        onCreated: (path, isDir) => !isDir && editor.openDevice(id, path),
        onRenamed: (from, to) => editor.retarget(from, to, id),
        onRemoved: (path) => editor.closeDeleted(path, id),
      },
    );
  }

  let tree = $state<TreeModel | null>(null);
  let treeFor = "";
  $effect(() => {
    const b = board;
    if (!b?.connected || b.running) return;
    if (b.id !== treeFor || !tree) {
      treeFor = b.id;
      tree = boardTree(b);
      tree.setRoot(root);
    }
  });

  // An AI agent changed this board's files over MCP: show the new state.
  $effect(() =>
    mcp.onActivity((a) => {
      if (a.path && a.session_id === treeFor && tree && !board?.running) void tree.reload();
    }),
  );

  async function uploadTo(dir: string) {
    if (!board || !tree) return;
    const files = await open({ multiple: true, title: "Upload files to the board" });
    if (!files) return;
    const id = board.id;
    await attempt(() => api.fsUpload(id, Array.isArray(files) ? files : [files], dir), "Upload failed");
    await tree.load(dir);
  }

  async function download(e: TreeEntry) {
    if (!board) return;
    const dest = await save({ defaultPath: e.name, title: "Save from board" });
    if (!dest) return;
    const id = board.id;
    if ((await attempt(() => api.fsDownload(id, e.path, dest), "Download failed")) !== undefined)
      notices.push("success", `Saved ${e.name}`);
  }

  async function runFile(e: TreeEntry) {
    if (!board) return;
    const id = board.id;
    const code = `import sys\nsys.modules.pop(${JSON.stringify(moduleName(e.path))}, None)\nexec(open(${JSON.stringify(e.path)}).read())\n`;
    await attempt(() => api.run(id, code));
  }

  function contextMenu(ev: MouseEvent, e: TreeEntry | null) {
    if (!board || !tree) return;
    const t = tree;
    const id = board.id;
    const dir = e ? (e.is_dir ? e.path : null) : root;
    const items: MenuItem[] = [];
    if (dir) {
      items.push(
        { label: "New file", icon: "file-plus", run: () => t.startCreate(dir, false) },
        { label: "New folder", icon: "folder-plus", run: () => t.startCreate(dir, true) },
        { label: "Upload here…", icon: "upload", run: () => uploadTo(dir) },
      );
    }
    if (e && !e.is_dir) {
      items.push(
        { label: "Open", icon: "file", run: () => editor.openDevice(id, e.path) },
        { label: "Run", icon: "play", run: () => runFile(e) },
      );
    }
    if (e) {
      items.push(
        { label: e.is_dir ? "Download folder…" : "Download…", icon: "download", run: () => download(e) },
        { label: "Rename", icon: "edit", run: () => t.startRename(e) },
        { label: "Delete", icon: "trash", danger: true, run: () => t.remove(e) },
      );
    }
    menu.show(ev, items);
  }

  async function installBle() {
    if (!board) return;
    const name = await dialogs.prompt({
      title: "Install BLE REPL",
      message: "The board will advertise the REPL over Bluetooth LE under this name after its next reset.",
      value: "mpy-repl",
      confirmLabel: "Install",
      validate: (v) => (v.trim() && v.length <= 20 ? null : "Use 1–20 characters"),
    });
    if (!name) return;
    const id = board.id;
    if ((await attempt(() => api.bleHelperInstall(id, name.trim()), "BLE REPL")) !== undefined)
      notices.push("success", "BLE REPL installed. Reset the board, then connect via Bluetooth LE.");
  }
</script>

<div class="panel">
  {#if !board}
    <div class="panel-header">Board</div>
    <div class="empty col" style="align-items:center">
      <p>No board connected.</p>
      <button class="primary" onclick={() => onconnect?.()} title="Connect a board" data-tip-key="⌘K"><Icon name="plug" size={14} />Connect…</button>
    </div>
  {:else}
    {@const info = board.status.info}
    <div class="panel-header">
      <span class="ellipsis">{board.label}</span>
      <span class="spacer"></span>
      <button class="ghost" title="Refresh info" onclick={() => boards.refresh(board!.id)}><Icon name="refresh" size={14} /></button>
      <button class="ghost" title="Disconnect" onclick={() => boards.disconnect(board!.id)}><Icon name="unplug" size={14} /></button>
    </div>
    <div class="section card">
      {#if !board.connected}
        <div class="row"><span class="badge">disconnected</span><span class="grow"></span><button onclick={() => boards.reconnect(board!.id)} title="Reconnect to {board.status.endpoint.address}">Reconnect</button></div>
      {:else if info}
        <div class="grid">
          <span class="dim">Firmware</span><span class="mono">MicroPython {info.version}</span>
          <span class="dim">Board</span><span class="ellipsis" title={info.machine}>{info.machine}</span>
          <span class="dim">Port</span><span>{board.status.target?.platform_name}</span>
          <span class="dim">Heap</span><span class="mono">{formatBytes(info.mem_free)} free / {formatBytes(info.mem_free + info.mem_alloc)}</span>
          {#if info.fs_total > 0}
            <span class="dim">Storage</span>
            <span class="meter" title="{formatBytes(info.fs_free)} free of {formatBytes(info.fs_total)}">
              <i style="width:{100 - (info.fs_free / info.fs_total) * 100}%"></i>
            </span>
          {/if}
        </div>
      {:else}
        <span class="dim">Connected via {board.status.endpoint.transport}. Device info unavailable.</span>
      {/if}
      {#if board.connected}
        {@const id = board.id}
        <div class="row actions">
          <button onclick={() => attempt(() => api.softReset(id))} title="Ctrl-D soft reboot"><Icon name="reset" size={13} />Soft reset</button>
          <button onclick={() => attempt(() => api.hardReset(id))} title="Hardware reset"><Icon name="bolt" size={13} />Hard reset</button>
          <button onclick={() => rescueBoard(board!)} title="Break out of a program that never returns (e.g. a runaway main.py)"><Icon name="lifebuoy" size={13} />Rescue…</button>
          {#if board.status.target?.capabilities.includes("ble")}
            <button onclick={installBle} title="Install a BLE REPL on the board"><Icon name="bluetooth" size={13} />BLE REPL</button>
          {/if}
        </div>
      {/if}
    </div>
    {#if board.connected && board.status.target?.capabilities.includes("esp32_partitions")}
      <Esp32Section {board} />
    {/if}
    <div class="panel-header">
      Files <span class="faint mono" style="text-transform:none; margin-left:4px">{root}</span>
      <span class="spacer"></span>
      {#if tree && board.connected && !board.running}
        {@const t = tree}
        <div class="actions">
          <button class="ghost" title="New file" onclick={() => t.startCreate(root, false)}><Icon name="file-plus" size={14} /></button>
          <button class="ghost" title="New folder" onclick={() => t.startCreate(root, true)}><Icon name="folder-plus" size={14} /></button>
          <button class="ghost" title="Upload files…" onclick={() => uploadTo(root)}><Icon name="upload" size={14} /></button>
          <button class="ghost" title="Refresh" onclick={() => t.reload()} disabled={t.loading}><Icon name="refresh" size={14} /></button>
          <button class="ghost" title="Collapse folders" onclick={() => t.collapseAll()}><Icon name="collapse" size={14} /></button>
        </div>
      {/if}
    </div>
    {#if board.running}
      <div class="empty">A program is running. Stop it to browse files.</div>
    {:else if tree && board.connected}
      {@const id = board.id}
      <FileTree {tree} {activePath} showSizes onopen={(e) => editor.openDevice(id, e.path)} oncontext={contextMenu} />
    {/if}
  {/if}
</div>

<style>
  .card .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 3px 10px;
  }
  .actions {
    margin-top: 8px;
    flex-wrap: wrap;
  }
  .meter {
    height: 4px;
    margin-top: 6px;
    background: var(--bg-active);
    overflow: hidden;
  }
  .meter i {
    display: block;
    height: 100%;
    background: var(--accent);
  }
</style>
