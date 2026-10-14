<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./lib/components/Icon.svelte";
  import Editor from "./lib/components/Editor.svelte";
  import EditorTabs from "./lib/components/EditorTabs.svelte";
  import Terminal from "./lib/components/Terminal.svelte";
  import Plotter from "./lib/components/Plotter.svelte";
  import MemoryView from "./lib/components/MemoryView.svelte";
  import TasksView from "./lib/components/TasksView.svelte";
  import StatusBar from "./lib/components/StatusBar.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import ConnectDialog from "./lib/components/ConnectDialog.svelte";
  import ContextMenu from "./lib/components/ContextMenu.svelte";
  import DialogHost from "./lib/components/DialogHost.svelte";
  import TooltipHost from "./lib/components/TooltipHost.svelte";
  import { features, isAvailable } from "./lib/features/registry";
  import { checkForUpdate } from "./lib/features/firmware/updates";
  import { rescueBoard } from "./lib/features/device/rescue";
  import { api, events, type PlatformProfile } from "./lib/ipc";
  import { boards } from "./lib/state/boards.svelte";
  import { editor } from "./lib/state/editor.svelte";
  import { project } from "./lib/state/project.svelte";
  import { settings } from "./lib/state/settings.svelte";
  import { attempt, notices, notifyError } from "./lib/state/notices.svelte";
  import { lsp } from "./lib/editor/lsp.svelte";
  import { mcp } from "./lib/state/mcp.svelte";
  import { runtime } from "./lib/runtime/runtime.svelte";
  import { debug } from "./lib/state/debug.svelte";

  let activeFeature = $state("files");
  let sidebarOpen = $state(true);
  let sidebarWidth = $state(290);
  let bottomHeight = $state(260);
  let bottomTab = $state<"terminal" | "plotter" | "memory" | "tasks">("terminal");
  let connectOpen = $state(false);
  let platforms: PlatformProfile[] = [];

  const board = $derived(boards.active);
  const caps = $derived(board?.status.target?.capabilities);
  const feature = $derived(features.find((f) => f.id === activeFeature) ?? features[0]);

  onMount(() => {
    (async () => {
      await settings.init();
      await lsp.init();
      await boards.init();
      boards.onUnresponsive = rescueBoard;
      await events.progress((e) => notices.onProgress(e));
      await events.notice((m) => notices.push("warn", m));
      platforms = await api.platforms().catch(() => []);
      await project.init();
      await mcp.init();
      ready = true;
    })().catch((e) => notifyError(e, "Startup"));
  });
  let ready = $state(false);

  // The language server follows the project folder (it starts lazily on the
  // first opened file; see Editor).
  $effect(() => {
    lsp.setRoot(project.root);
  });

  // Stubs follow the active board; without one, the user's setting decides.
  $effect(() => {
    if (!ready) return;
    const b = boards.active;
    const t = b?.status.target;
    if (b && t) {
      const port = platforms.find((p) => p.id === t.platform)?.stubs?.port;
      if (port) {
        const req = { port, board: t.board_id, version: b.status.info?.version ?? null };
        lsp.setStubs(settings.value.boardStubs ? { ...req, introspect_session: b.id } : req);
        if (JSON.stringify(req) !== JSON.stringify(settings.value.lastBoardStubs)) settings.set("lastBoardStubs", req);
      }
      if (settings.value.checkFirmwareUpdates) checkForUpdate(b, () => openFeature("firmware"));
      return;
    }
    const pref = settings.value.stubsWhenDisconnected;
    const last = settings.value.lastBoardStubs;
    if (pref === "last") lsp.setStubs(last ?? { port: "generic", board: null, version: null });
    else lsp.setStubs({ port: pref, board: null, version: null });
  });

  function openFeature(id: string) {
    if (activeFeature === id && sidebarOpen) sidebarOpen = false;
    else {
      activeFeature = id;
      sidebarOpen = true;
    }
  }

  function targetBoardId(): string | null {
    const t = editor.active;
    if (t?.source.kind === "device") return t.source.boardId;
    return boards.active?.id ?? null;
  }

  async function run() {
    const tab = editor.active;
    const id = targetBoardId();
    if (!tab) return notices.push("info", "Open a file to run");
    if (!id) return (connectOpen = true);
    runtime.startRun(id);
    await attempt(() => api.run(id, editor.docOf(tab), runtime.preludes()), "Run");
  }

  async function stop() {
    const id = targetBoardId();
    if (id) await attempt(() => api.stop(id, boards.get(id)?.running === false), "Stop");
  }

  async function toggleAutocomplete() {
    const on = !settings.value.autocomplete;
    await settings.set("autocomplete", on);
    notices.push("info", on ? "Autocomplete while typing: on" : "Autocomplete while typing: off — press ⌃Space for suggestions");
  }

  function startDebug() {
    activeFeature = "debug";
    sidebarOpen = true;
    void debug.start(editor.active, targetBoardId());
  }

  function onkey(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (debug.active && ["F8", "F10", "F11"].includes(e.key)) {
      e.preventDefault();
      if (e.key === "F8") debug.continue();
      else if (e.key === "F10") debug.stepOver();
      else if (e.shiftKey) debug.stepOut();
      else debug.stepInto();
    } else if (e.key === "F6") (e.preventDefault(), startDebug());
    else if (e.key === "F5" && !e.shiftKey) (e.preventDefault(), run());
    else if ((e.key === "F5" && e.shiftKey) || (mod && e.key === ".")) (e.preventDefault(), stop());
    else if (mod && e.key.toLowerCase() === "k") (e.preventDefault(), (connectOpen = true));
    else if (mod && e.key.toLowerCase() === "s" && !(e.target as HTMLElement)?.closest?.(".cm-editor")) (e.preventDefault(), editor.save());
    else if (mod && e.key.toLowerCase() === "b") (e.preventDefault(), (sidebarOpen = !sidebarOpen));
    else if (mod && e.shiftKey && e.code === "Space") (e.preventDefault(), toggleAutocomplete());
  }

  function drag(kind: "side" | "bottom") {
    return (e: PointerEvent) => {
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);
      const start = kind === "side" ? e.clientX : e.clientY;
      const init = kind === "side" ? sidebarWidth : bottomHeight;
      const move = (ev: PointerEvent) => {
        if (kind === "side") sidebarWidth = Math.min(600, Math.max(200, init + ev.clientX - start));
        else bottomHeight = Math.min(window.innerHeight - 200, Math.max(80, init - (ev.clientY - start)));
      };
      const up = () => {
        el.removeEventListener("pointermove", move);
        el.removeEventListener("pointerup", up);
      };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
    };
  }
</script>

<svelte:window onkeydown={onkey} />

<div class="app" style="--side:{sidebarOpen ? sidebarWidth : 0}px; --bottom:{bottomHeight}px">
  <nav class="activity">
    {#each features.filter((f) => !f.bottom) as f}
      <button
        class="act"
        class:active={sidebarOpen && activeFeature === f.id}
        title={isAvailable(f, caps) ? f.title : `${f.title} (not supported by the connected board)`}
        disabled={!isAvailable(f, caps)}
        onclick={() => openFeature(f.id)}><Icon name={f.icon} size={18} /></button
      >
    {/each}
    <span class="grow"></span>
    {#each features.filter((f) => f.bottom) as f}
      <button class="act" class:active={sidebarOpen && activeFeature === f.id} title={f.title} onclick={() => openFeature(f.id)}><Icon name={f.icon} size={18} /></button>
    {/each}
  </nav>

  {#if sidebarOpen}
    <aside class="sidebar">
      <feature.panel onconnect={() => (connectOpen = true)} />
    </aside>
    <div class="split-v" role="separator" aria-orientation="vertical" onpointerdown={drag("side")}></div>
  {/if}

  <main class="main">
    <div class="toolbar">
      <div class="boards">
        {#each boards.list as b (b.id)}
          <button class="board-chip" class:active={b.id === boards.activeId} class:off={!b.connected} onclick={() => (boards.activeId = b.id)} title={b.status.endpoint.address}>
            <span class="dot" class:run={b.running}></span>{b.label}
            <span class="x" role="button" tabindex="-1" onclick={(e) => (e.stopPropagation(), boards.disconnect(b.id))} onkeydown={() => {}}><Icon name="x" size={11} /></span>
          </button>
        {/each}
        <button class="ghost add-board" onclick={() => (connectOpen = true)} title="Connect a board" data-tip-key="⌘K">
          <Icon name="plus" size={14} />{boards.list.length ? "" : "Connect board"}
        </button>
      </div>
      <span class="grow"></span>
      <button class="ghost" onclick={() => editor.save()} disabled={!editor.active?.dirty} title="Save the current file" data-tip-key="⌘S"><Icon name="save" size={14} /></button>
      {#if (targetBoardId() && boards.get(targetBoardId()!)?.running)}
        <button class="danger-btn" onclick={stop} title="Stop the running program" data-tip-key="⌘."><Icon name="stop" size={12} />Stop</button>
      {:else}
        <button class="ghost" onclick={startDebug} disabled={!editor.active || debug.active} title={debug.active ? "Already debugging" : editor.active ? "Debug the current file on the board" : "Open a file to debug it"} data-tip-key="F6"><Icon name="bug-play" size={14} /></button>
        <button class="primary" onclick={run} disabled={!editor.active} title={editor.active ? "Run the current file on the board" : "Open a file to run it"} data-tip-key="⌘↵"><Icon name="play" size={12} />Run</button>
      {/if}
    </div>

    <EditorTabs />
    <div class="editor-area"><Editor onrun={run} onsave={() => editor.save()} /></div>

    <div class="split-h" role="separator" aria-orientation="horizontal" onpointerdown={drag("bottom")}></div>
    <section class="bottom">
      <div class="bottom-tabs">
        <button class="btab" class:active={bottomTab === "terminal"} onclick={() => (bottomTab = "terminal")} title="Interactive REPL of the active board">REPL</button>
        <button class="btab" class:active={bottomTab === "plotter"} onclick={() => (bottomTab = "plotter")} title="Plot numbers printed by the board">Plotter</button>
        <button class="btab" class:active={bottomTab === "memory"} onclick={() => (bottomTab = "memory")} title="Heap map and memory use of the active board">Memory</button>
        <button class="btab" class:active={bottomTab === "tasks"} onclick={() => (bottomTab = "tasks")} title="asyncio tasks of the running program">Tasks</button>
        <span class="grow"></span>
        {#if board}
          <button class="ghost" title="Interrupt the running program" data-tip-key="Ctrl-C" onclick={() => attempt(() => api.interrupt(board!.id))}><Icon name="stop" size={13} /></button>
          <button class="ghost" title="Soft reset the board" data-tip-key="Ctrl-D" onclick={() => attempt(() => api.softReset(board!.id))}><Icon name="reset" size={13} /></button>
        {/if}
      </div>
      <div class="bottom-body">
        {#each boards.list as b (b.id)}
          <div class="pane" class:shown={b.id === boards.activeId && bottomTab === "terminal"}><Terminal board={b} /></div>
          {#if b.id === boards.activeId && bottomTab === "plotter"}
            <div class="pane shown"><Plotter board={b} /></div>
          {:else if b.id === boards.activeId && bottomTab === "memory"}
            <div class="pane shown"><MemoryView board={b} /></div>
          {:else if b.id === boards.activeId && bottomTab === "tasks"}
            <div class="pane shown"><TasksView board={b} /></div>
          {/if}
        {/each}
        {#if !boards.list.length}
          <div class="empty">
            <div>No board connected. <button class="ghost" style="color:var(--accent)" onclick={() => (connectOpen = true)} title="Connect a board" data-tip-key="⌘K">Connect…</button></div>
            <div class="faint">No hardware: connect to "Local (unix port)" to use the MicroPython simulator.</div>
          </div>
        {/if}
      </div>
    </section>
  </main>
</div>
<StatusBar onconnect={() => (connectOpen = true)} />

<ConnectDialog bind:open={connectOpen} />
<ContextMenu />
<DialogHost />
<TooltipHost />
<Toasts />

<style>
  :global(#app) {
    display: flex;
    flex-direction: column;
  }
  .app {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .activity {
    width: 40px;
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    padding: 4px 0;
    background: var(--bg-sunken);
    border-right: 1px solid var(--border);
  }
  .act {
    height: 40px;
    justify-content: center;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--text-faint);
    border-left: 2px solid transparent;
  }
  .act:hover:not(:disabled) {
    color: var(--text);
    background: transparent;
  }
  .act.active {
    color: var(--text);
    border-left-color: var(--accent);
  }
  .sidebar {
    width: var(--side);
    flex: none;
    background: var(--sidebar-bg);
    overflow: hidden;
  }
  .split-v {
    width: 5px;
    margin: 0 -3px 0 -2px;
    cursor: col-resize;
    z-index: 2;
    border-left: 1px solid var(--border);
    flex: none;
  }
  .split-v:hover,
  .split-h:hover {
    border-color: var(--accent);
  }
  .split-h {
    height: 5px;
    margin: -2px 0 -3px;
    cursor: row-resize;
    z-index: 2;
    border-top: 1px solid var(--border);
    flex: none;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .toolbar {
    display: flex;
    align-items: stretch;
    height: 32px;
    padding: 0 6px 0 0;
    gap: 4px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }
  .toolbar > button {
    align-self: center;
  }
  .boards {
    display: flex;
    align-items: stretch;
    overflow-x: auto;
    min-width: 0;
  }
  .board-chip {
    height: auto;
    border: none;
    border-right: 1px solid var(--border);
    border-radius: 0;
    background: transparent;
    padding: 0 6px 0 10px;
    color: var(--text-dim);
    font-family: var(--font-mono);
    font-size: 11px;
  }
  .board-chip.active {
    color: var(--text);
    background: var(--bg-elev);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .board-chip.off {
    color: var(--text-faint);
  }
  .board-chip .dot {
    width: 6px;
    height: 6px;
    background: var(--ok);
  }
  .board-chip.off .dot {
    background: var(--text-faint);
  }
  .board-chip .dot.run {
    background: var(--warn);
  }
  .board-chip .x {
    display: inline-flex;
    padding: 2px;
    border-radius: 2px;
    opacity: 0;
  }
  .board-chip:hover .x {
    opacity: 0.7;
  }
  .board-chip .x:hover {
    opacity: 1;
    background: var(--bg-hover);
  }
  .add-board {
    align-self: center;
  }
  .danger-btn {
    background: transparent;
    border-color: var(--danger);
    color: var(--danger);
  }
  .danger-btn:hover {
    background: var(--danger) !important;
    color: #fff;
  }
  .editor-area {
    flex: 1;
    min-height: 0;
  }
  .bottom {
    height: var(--bottom);
    flex: none;
    display: flex;
    flex-direction: column;
    background: var(--term-bg);
  }
  .bottom-tabs {
    display: flex;
    align-items: stretch;
    height: 28px;
    padding: 0 6px 0 4px;
    gap: 0;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
  }
  .bottom-tabs > button.ghost {
    align-self: center;
    height: 22px;
    width: 22px;
    justify-content: center;
    padding: 0;
  }
  .btab {
    height: auto;
    border: none;
    background: transparent;
    border-radius: 0;
    padding: 0 10px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-faint);
    box-shadow: inset 0 -1px 0 transparent;
  }
  .btab:hover {
    background: transparent !important;
    color: var(--text);
  }
  .btab.active {
    color: var(--text);
    box-shadow: inset 0 -1px 0 var(--accent);
  }
  .bottom-body {
    flex: 1;
    min-height: 0;
    position: relative;
  }
  .pane {
    position: absolute;
    inset: 0;
    visibility: hidden;
  }
  .pane.shown {
    visibility: visible;
  }
  .bottom-body .empty {
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.8;
  }
</style>
