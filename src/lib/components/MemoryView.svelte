<script lang="ts">
  // Heap of the active board: a block map from micropython.mem_info(1) (taken
  // while idle) and, while a program runs, sampled free/allocated memory.
  import { onMount } from "svelte";
  import uPlot from "uplot";
  import Icon from "./Icon.svelte";
  import { api } from "../ipc";
  import type { Board } from "../state/boards.svelte";
  import { attempt, notices } from "../state/notices.svelte";
  import { settings } from "../state/settings.svelte";
  import { runtime } from "../runtime/runtime.svelte";
  import { BLOCK_KINDS, fragmentation, kindOf, objectAt, parseHeap, type HeapMap } from "../runtime/heap";
  import { formatBytes } from "../util/format";
  import { cssVar, onThemeChange } from "../util/theme";
  import { PALETTE } from "../util/palette";

  let { board }: { board: Board } = $props();
  let heap = $state<HeapMap | null>(null);
  let takenAt = $state<string | null>(null);
  let loading = $state(false);
  let hover = $state<string | null>(null);
  let canvas: HTMLCanvasElement;
  let mapHost: HTMLDivElement;
  let chartHost = $state<HTMLDivElement>();
  let plot: uPlot | null = null;
  let layout = { cell: 1, cols: 1 };

  const samples = $derived(runtime.mem[board.id] ?? []);
  const blocked = $derived(!board.connected ? "Connect the board first" : board.running ? "Stop the program to read the heap" : board.busy ? "The board is busy" : null);
  const legend = $derived.by(() => {
    if (!heap) return [];
    const seen = new Set(heap.blocks.replace(/=/g, ""));
    return Object.entries(BLOCK_KINDS).filter(([c]) => seen.has(c));
  });

  function colorOf(kind: string) {
    const k = kindOf(kind);
    return k.color < 0 ? cssVar("--bg-active") : PALETTE[k.color % PALETTE.length];
  }

  async function snapshot() {
    loading = true;
    const out = await attempt(() => api.script(board.id, "heap"), "Reading the heap");
    loading = false;
    if (out === undefined) return;
    heap = parseHeap(out);
    takenAt = new Date().toLocaleTimeString();
    if (!heap) notices.push("warn", "This firmware doesn't report a heap map (micropython.mem_info)");
    draw();
  }

  function draw() {
    if (!canvas || !mapHost) return;
    const { width, height } = mapHost.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.floor(width * dpr));
    canvas.height = Math.max(1, Math.floor(height * dpr));
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx || !heap?.blocks.length) return ctx?.clearRect(0, 0, canvas.width, canvas.height);
    const n = heap.blocks.length;
    // Largest square cell that fits every block.
    let cell = Math.max(1, Math.floor(Math.sqrt((width * height) / n)));
    while (cell > 1 && Math.floor(width / cell) * Math.floor(height / cell) < n) cell--;
    const cols = Math.max(1, Math.floor(width / cell));
    layout = { cell, cols };
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const gap = cell >= 4 ? 1 : 0;
    let head = ".";
    for (let i = 0; i < n; i++) {
      const c = heap.blocks[i];
      if (c !== "=") head = c;
      ctx.fillStyle = colorOf(head);
      ctx.fillRect((i % cols) * cell, Math.floor(i / cols) * cell, cell - gap, cell - gap);
    }
  }

  function onmove(e: MouseEvent) {
    if (!heap) return;
    const r = canvas.getBoundingClientRect();
    const i = Math.floor((e.clientY - r.top) / layout.cell) * layout.cols + Math.floor((e.clientX - r.left) / layout.cell);
    if (i < 0 || i >= heap.blocks.length) return void (hover = null);
    const o = objectAt(heap.blocks, i);
    const bytes = o.length * heap.blockSize;
    hover = `${kindOf(o.kind).label} · ${o.length} block${o.length === 1 ? "" : "s"} (${formatBytes(bytes)}) at +0x${(o.head * heap.blockSize).toString(16)}`;
  }

  function chart() {
    plot?.destroy();
    plot = null;
    if (!chartHost) return;
    const r = chartHost.getBoundingClientRect();
    const axis = { stroke: cssVar("--text-dim"), grid: { stroke: cssVar("--border"), width: 1 }, values: (_: uPlot, v: number[]) => v.map((x) => formatBytes(x)) };
    plot = new uPlot(
      {
        width: Math.max(r.width, 100),
        height: Math.max(r.height, 60),
        legend: { show: false },
        scales: { x: { time: false } },
        axes: [{ stroke: cssVar("--text-dim"), grid: { stroke: cssVar("--border"), width: 1 } }, axis],
        series: [{}, { label: "free", stroke: PALETTE[0], width: 1.5 }, { label: "allocated", stroke: PALETTE[2], width: 1.5 }],
      },
      [[], [], []],
      chartHost,
    );
    update();
  }

  function update() {
    plot?.setData([samples.map((s) => s.t), samples.map((s) => s.free), samples.map((s) => s.alloc)]);
  }

  $effect(() => {
    void samples.length;
    if (chartHost && !plot) chart();
    else update();
  });

  onMount(() => {
    const ro = new ResizeObserver(() => {
      draw();
      if (chartHost && plot) {
        const r = chartHost.getBoundingClientRect();
        plot.setSize({ width: Math.max(r.width, 100), height: Math.max(r.height, 60) });
      }
    });
    ro.observe(mapHost);
    const stopTheme = onThemeChange(() => (draw(), chart()));
    return () => {
      ro.disconnect();
      stopTheme();
      plot?.destroy();
    };
  });
</script>

<div class="memory">
  <div class="toolbar row">
    <button class="ghost" onclick={snapshot} disabled={!!blocked || loading} title={blocked ?? "Collect garbage and read the heap block map"}>
      <Icon name="refresh" size={13} />Snapshot
    </button>
    {#if takenAt}<span class="faint small">taken {takenAt}</span>{/if}
    <label class="row" title="Adds a small timer to your program that reports heap use (needs machine.Timer)">
      <input type="checkbox" checked={settings.value.sampleMemory} onchange={(e) => settings.set("sampleMemory", e.currentTarget.checked)} />
      Sample during runs
    </label>
    <span class="grow"></span>
    {#if hover}<span class="mono small">{hover}</span>{/if}
  </div>
  <div class="body">
    <div class="stats">
      {#if heap}
        <table class="mono small">
          <tbody>
            <tr><td>total</td><td>{formatBytes(heap.total)}</td></tr>
            <tr><td>used</td><td>{formatBytes(heap.used)} ({Math.round((heap.used / heap.total) * 100)}%)</td></tr>
            <tr><td>free</td><td>{formatBytes(heap.free)}</td></tr>
            <tr><td>largest free</td><td>{formatBytes(heap.maxFreeBlocks * heap.blockSize)}</td></tr>
            <tr><td>fragmentation</td><td>{Math.round(fragmentation(heap) * 100)}%</td></tr>
            <tr><td>block</td><td>{heap.blockSize} B × {heap.blocks.length}</td></tr>
          </tbody>
        </table>
        <div class="legend">
          {#each legend as [c, k] (c)}
            <span class="row small"><i style="background:{colorOf(c)}"></i>{k.label}</span>
          {/each}
        </div>
      {:else}
        <div class="faint small">Take a snapshot to see how the heap is laid out: which objects use it and how fragmented the free space is. A large allocation fails when no single free run is big enough, even with plenty free in total.</div>
      {/if}
    </div>
    <div class="map" bind:this={mapHost}>
      <canvas bind:this={canvas} onmousemove={onmove} onmouseleave={() => (hover = null)}></canvas>
    </div>
    {#if settings.value.sampleMemory || samples.length}
      <div class="chart" bind:this={chartHost}>
        {#if !samples.length}<div class="hint faint small">Run a program to sample its memory use.</div>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .memory {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-elev);
  }
  .toolbar {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    gap: 8px;
    min-height: 30px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .stats {
    width: 220px;
    flex: none;
    padding: 8px;
    border-right: 1px solid var(--border);
    overflow: auto;
  }
  td {
    padding: 1px 6px 1px 0;
  }
  td:first-child {
    color: var(--text-dim);
  }
  .legend {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .legend i {
    width: 9px;
    height: 9px;
    display: inline-block;
  }
  .map {
    flex: 1;
    min-width: 0;
    padding: 0;
    overflow: hidden;
  }
  canvas {
    display: block;
  }
  .chart {
    width: 36%;
    flex: none;
    border-left: 1px solid var(--border);
    position: relative;
  }
  .hint {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
</style>
