<script lang="ts">
  // Live plot of numeric lines from the active board, with the raw feed alongside.
  import { onMount } from "svelte";
  import uPlot from "uplot";
  import Icon from "./Icon.svelte";
  import { LineBuffer, parseLine } from "../plot/parse";
  import type { Board } from "../state/boards.svelte";
  import { cssVar, onThemeChange } from "../util/theme";
  import { PALETTE } from "../util/palette";

  let { board }: { board: Board } = $props();
  let host: HTMLDivElement;
  let paused = $state(false);
  let showRaw = $state(true);
  let windowSize = $state(500);
  let raw = $state<string[]>([]);
  let names = $state<string[]>([]);
  let latest = $state<Record<string, number>>({});

  // Plain arrays on purpose: uPlot reads them directly; reactivity would only add overhead.
  const data = { xs: [] as number[], series: {} as Record<string, (number | null)[]> };
  let plot: uPlot | null = null;
  let t0 = performance.now();

  function rebuild() {
    plot?.destroy();
    const rect = host.getBoundingClientRect();
    const axis = { stroke: cssVar("--text-dim"), grid: { stroke: cssVar("--border"), width: 1 } };
    plot = new uPlot(
      {
        width: Math.max(rect.width, 100),
        height: Math.max(rect.height, 80),
        legend: { show: false },
        cursor: { drag: { x: true, y: false } },
        scales: { x: { time: false } },
        axes: [axis, axis],
        series: [{}, ...names.map((n, i) => ({ label: n, stroke: PALETTE[i % PALETTE.length], width: 1.5, spanGaps: true }))],
      },
      [data.xs, ...names.map((n) => data.series[n])] as uPlot.AlignedData,
      host,
    );
  }

  function add(sample: Record<string, number>) {
    let changed = false;
    for (const k of Object.keys(sample)) {
      if (!data.series[k]) {
        data.series[k] = data.xs.map(() => null);
        names = [...names, k];
        changed = true;
      }
    }
    data.xs.push((performance.now() - t0) / 1000);
    for (const n of names) data.series[n].push(sample[n] ?? null);
    while (data.xs.length > windowSize) {
      data.xs.shift();
      for (const n of names) data.series[n].shift();
    }
    latest = sample;
    if (changed) rebuild();
  }

  function clear() {
    data.xs = [];
    data.series = {};
    names = [];
    raw = [];
    t0 = performance.now();
    rebuild();
  }

  function exportCsv() {
    const rows = [["t", ...names].join(",")];
    data.xs.forEach((x, i) => rows.push([x.toFixed(3), ...names.map((n) => data.series[n][i] ?? "")].join(",")));
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([rows.join("\n")], { type: "text/csv" }));
    a.download = "plot.csv";
    a.click();
  }

  onMount(() => {
    rebuild();
    const buf = new LineBuffer();
    let frame = 0;
    const unsub = board.subscribe((ev) => {
      if (ev.kind === "structured") return;
      if (paused) return;
      for (const line of buf.push(ev.data)) {
        raw.push(line);
        const s = parseLine(line);
        if (s) add(s);
      }
      if (raw.length > 400) raw = raw.slice(-300);
      if (!frame)
        frame = requestAnimationFrame(() => {
          frame = 0;
          plot?.setData([data.xs, ...names.map((n) => data.series[n])] as uPlot.AlignedData);
        });
    });
    const ro = new ResizeObserver(() => {
      const r = host.getBoundingClientRect();
      plot?.setSize({ width: Math.max(r.width, 100), height: Math.max(r.height, 80) });
    });
    ro.observe(host);
    const stopTheme = onThemeChange(rebuild);
    return () => {
      unsub();
      stopTheme();
      ro.disconnect();
      plot?.destroy();
    };
  });
</script>

<div class="plotter">
  <div class="toolbar row">
    <button class="ghost" onclick={() => (paused = !paused)} title={paused ? "Resume" : "Pause"}>
      <Icon name={paused ? "play" : "stop"} size={13} />{paused ? "Resume" : "Pause"}
    </button>
    <button class="ghost" onclick={clear} title="Clear the plot and raw feed"><Icon name="trash" size={13} />Clear</button>
    <button class="ghost" onclick={exportCsv} disabled={!names.length} title={names.length ? "Export the plotted samples as CSV" : "Nothing plotted yet"}><Icon name="download" size={13} />CSV</button>
    <label class="row">Window <input type="number" min="50" max="20000" step="50" bind:value={windowSize} style="width:80px" /></label>
    <label class="row"><input type="checkbox" bind:checked={showRaw} /> Raw feed</label>
    <span class="grow"></span>
    {#each names as n, i}
      <span class="legend"><i style="background:{PALETTE[i % PALETTE.length]}"></i>{n} <b class="mono">{latest[n] ?? "–"}</b></span>
    {/each}
  </div>
  <div class="body">
    <div class="chart" bind:this={host}>
      {#if !names.length}<div class="hint faint">Print numbers from your program, e.g. <code>print("temp:", t, "hum:", h)</code> or <code>print(a, b, c)</code></div>{/if}
    </div>
    {#if showRaw}
      <pre class="raw mono">{raw.slice(-200).join("\n")}</pre>
    {/if}
  </div>
</div>

<style>
  .plotter {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-elev);
  }
  .toolbar {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    flex-wrap: wrap;
    gap: 6px;
  }
  .legend {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
  }
  .legend i {
    width: 10px;
    height: 3px;
    border-radius: 2px;
    display: inline-block;
  }
  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .chart {
    flex: 1;
    min-width: 0;
    position: relative;
  }
  .hint {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    text-align: center;
    padding: 12px;
  }
  .raw {
    width: 32%;
    max-width: 420px;
    margin: 0;
    padding: 6px 8px;
    overflow: auto;
    font-size: 11px;
    border-left: 1px solid var(--border);
    background: var(--bg-sunken);
    user-select: text;
  }
</style>
