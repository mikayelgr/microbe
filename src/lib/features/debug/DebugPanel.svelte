<script lang="ts">
  // Debugger controls, call stack, variables and breakpoints.
  import Icon from "../../components/Icon.svelte";
  import { boards } from "../../state/boards.svelte";
  import { editor } from "../../state/editor.svelte";
  import { debug } from "../../state/debug.svelte";

  let expr = $state("");
  let showGlobals = $state(true);
  const s = $derived(debug.session);
  const stop = $derived(debug.stopped);
  const board = $derived(boards.active);
  const tab = $derived(editor.active);
  const startBlocked = $derived(
    !tab ? "Open a file to debug it" : !board?.connected ? "Connect a board first" : board.running ? "A program is running" : null,
  );
  const bpTabs = $derived(
    Object.entries(debug.breakpoints)
      .map(([id, lines]) => ({ tab: editor.tabs.find((t) => t.id === id), lines: [...lines].sort((a, b) => a - b) }))
      .filter((x) => x.tab && x.lines.length),
  );

  function goto(tabId: string, line: number) {
    editor.activeId = tabId;
    editor.pendingLine = { tabId, line };
  }

  function evaluate(e: KeyboardEvent) {
    if (e.key !== "Enter") return;
    debug.evaluate(expr);
    expr = "";
  }
</script>

<div class="panel scroll">
  <div class="panel-header">Debug</div>
  <div class="section col">
    {#if !s}
      <button class="primary" disabled={!!startBlocked} onclick={() => debug.start(tab, board?.id ?? null)} title={startBlocked ?? "Run the current file on the board under the debugger"} data-tip-key="F6">
        <Icon name="bug-play" size={13} />Debug {tab?.title ?? "file"}
      </button>
      <div class="faint small">
        Works on stock firmware: the file is instrumented before it runs, so it runs slower. Click the gutter left of the line numbers to set breakpoints. Locals are shown for this file's functions; imported modules run normally.
      </div>
    {:else}
      <div class="controls row">
        <button class="ghost" disabled={!stop} onclick={() => debug.continue()} title={stop ? "Continue" : "Running"} data-tip-key="F8"><Icon name="play" size={14} /></button>
        <button class="ghost" disabled={!!stop} onclick={() => debug.pause()} title={stop ? "Already paused" : "Pause at the next line"}><Icon name="dot" size={14} /></button>
        <button class="ghost" disabled={!stop} onclick={() => debug.stepOver()} title={stop ? "Step over" : "Pause first"} data-tip-key="F10"><Icon name="step-over" size={14} /></button>
        <button class="ghost" disabled={!stop} onclick={() => debug.stepInto()} title={stop ? "Step into" : "Pause first"} data-tip-key="F11"><Icon name="step-into" size={14} /></button>
        <button class="ghost" disabled={!stop} onclick={() => debug.stepOut()} title={stop ? "Step out" : "Pause first"} data-tip-key="⇧F11"><Icon name="step-out" size={14} /></button>
        <button class="ghost danger" onclick={() => debug.stop()} title="Stop debugging" data-tip-key="⇧F5"><Icon name="stop" size={14} /></button>
      </div>
      <div class="status small">
        {#if stop}
          <span class="mono">Paused at {s.title}:{stop.line}</span> <span class="faint">({stop.reason})</span>
        {:else}
          <span class="faint">Running {s.title}…</span>
        {/if}
      </div>
    {/if}
  </div>

  {#if stop}
    <div class="section col">
      <b>Call stack</b>
      {#each [...stop.stack].reverse() as frame, i}
        <div class="mono small frame" class:top={i === 0}>{frame}{i === 0 ? ` · line ${stop.line}` : ""}</div>
      {/each}
    </div>
    <div class="section col">
      <b>Locals</b>
      {#each Object.entries(stop.locals) as [k, v] (k)}
        <div class="var mono small"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>
      {:else}
        <div class="faint small">{stop.stack.length > 1 ? "No local variables bound yet" : "At module level: see globals"}</div>
      {/each}
    </div>
    <div class="section col">
      <button class="ghost head" onclick={() => (showGlobals = !showGlobals)} title={showGlobals ? "Hide globals" : "Show globals"}>
        <b>Globals</b><span class="faint small">{Object.keys(stop.globals).length}</span>
      </button>
      {#if showGlobals}
        {#each Object.entries(stop.globals) as [k, v] (k)}
          <div class="var mono small"><span class="k">{k}</span><span class="v" title={v}>{v}</span></div>
        {/each}
      {/if}
    </div>
    <div class="section col">
      <b>Evaluate</b>
      <input class="mono" placeholder="expression (global scope), Enter" bind:value={expr} onkeydown={evaluate} />
      {#each [...debug.evals].reverse() as ev, i (i)}
        <div class="var mono small"><span class="k">{ev.expr}</span><span class="v" class:err={!!ev.error} title={ev.error ?? ev.result ?? ""}>{ev.error ?? ev.result}</span></div>
      {/each}
    </div>
  {/if}

  <div class="section col">
    <div class="row">
      <b class="grow">Breakpoints</b>
      {#if bpTabs.length}<button class="ghost" title="Remove all breakpoints" onclick={() => debug.clearBreakpoints()}><Icon name="trash" size={13} /></button>{/if}
    </div>
    {#each bpTabs as { tab: t, lines } (t!.id)}
      {#each lines as line (line)}
        <div class="bp row small">
          <span class="dot"></span>
          <button class="ghost mono name" title="Go to line {line}" onclick={() => goto(t!.id, line)}>{t!.title}:{line}</button>
          <button class="ghost x" title="Remove this breakpoint" onclick={() => debug.removeBreakpoint(t!.id, line)}><Icon name="x" size={12} /></button>
        </div>
      {/each}
    {:else}
      <div class="faint small">None. Click left of a line number to add one.</div>
    {/each}
  </div>
</div>

<style>
  .controls {
    gap: 2px;
  }
  .controls button {
    width: 26px;
    justify-content: center;
    padding: 0;
  }
  .danger {
    color: var(--danger);
  }
  .status {
    min-height: 16px;
  }
  .frame {
    padding-left: 4px;
    color: var(--text-dim);
  }
  .frame.top {
    color: var(--text);
    border-left: 2px solid var(--accent);
    padding-left: 2px;
  }
  .var {
    display: flex;
    gap: 8px;
    min-width: 0;
  }
  .k {
    color: var(--tok-prop);
    flex: none;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .v {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }
  .v.err {
    color: var(--danger);
  }
  .head {
    justify-content: flex-start;
    gap: 6px;
    padding: 0;
  }
  .bp {
    height: var(--row);
  }
  .bp .name {
    flex: 1;
    justify-content: flex-start;
    padding: 0 2px;
  }
  .bp .x {
    opacity: 0;
    padding: 0 2px;
  }
  .bp:hover .x {
    opacity: 1;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--danger);
    flex: none;
  }
</style>
