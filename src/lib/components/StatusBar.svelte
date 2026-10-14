<script lang="ts">
  import Icon from "./Icon.svelte";
  import { boards } from "../state/boards.svelte";
  import { notices } from "../state/notices.svelte";
  import { lsp } from "../editor/lsp.svelte";
  import { formatBytes } from "../util/format";
  import { settings } from "../state/settings.svelte";
  import { mcp } from "../state/mcp.svelte";

  let { onconnect }: { onconnect: () => void } = $props();
  const b = $derived(boards.active);
  const info = $derived(b?.status.info);
  const progress = $derived(Object.values(notices.progress));

</script>

<footer class="status">
  <button class="item conn" class:on={b?.connected} onclick={onconnect} title={b?.connected ? `Connected to ${b.status.endpoint.address} — connect another board` : "Connect a board"} data-tip-key="⌘K">
    <Icon name={b?.connected ? "plug" : "unplug"} size={12} />
    {#if b}{b.status.endpoint.transport}:{b.status.endpoint.address.split("/").pop()}{:else}no board{/if}
  </button>
  {#if b?.connected}
    <span class="item mono">{b.status.state}</span>
    {#if info}
      <span class="item mono">v{info.version}</span>
      {#if b.status.target}<span class="item mono">{b.status.target.platform}{b.status.target.board_id ? `/${b.status.target.board_id}` : ""}</span>{/if}
      <span class="item mono" title="Free heap at last refresh">heap {formatBytes(info.mem_free)}</span>
    {/if}
  {/if}
  <span class="grow"></span>
  {#each progress as p (p.id)}
    <span class="item progress">
      <span class="ellipsis">{p.label}</span>
      {#if p.total > 0}<span class="bar"><i style="width:{Math.min(100, (p.done / p.total) * 100)}%"></i></span>{/if}
    </span>
  {/each}
  {#if !settings.value.autocomplete}
    <button class="item toggle" title="Autocomplete while typing is off (⌘⇧Space to turn on)" onclick={() => settings.set("autocomplete", true)}>
      autocomplete off
    </button>
  {/if}
  {#if mcp.status?.running}
    <span class="item mono" title="MCP server: {mcp.status.url}{mcp.last ? ` · last agent action: ${mcp.last.tool}` : ''}">
      <Icon name="bot" size={12} />{mcp.last ? mcp.last.tool : "mcp"}
    </span>
  {/if}
  {#if lsp.status !== "off"}
    <span class="item mono" title="Language server: {lsp.status} · stubs: {lsp.stubLabel}">
      <span class="dot {lsp.status}"></span>{lsp.stubLabel || "python"}
    </span>
  {/if}
  <span class="item mono">MicroPython</span>
</footer>

<style>
  .status {
    display: flex;
    align-items: stretch;
    height: 22px;
    font-size: 11px;
    background: var(--bg-sunken);
    border-top: 1px solid var(--border);
    color: var(--text-dim);
    overflow: hidden;
    flex: none;
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 8px;
    white-space: nowrap;
  }
  .conn {
    height: auto;
    border: none;
    border-radius: 0;
    background: var(--bg-active);
    color: var(--text-dim);
  }
  .conn.on {
    background: var(--accent);
    color: #fff;
  }
  .conn.on:hover {
    background: var(--accent-strong) !important;
  }
  .mono {
    font-family: var(--font-mono);
  }
  .toggle {
    height: auto;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--warn);
    font-family: var(--font-mono);
  }
  .progress {
    max-width: 380px;
  }
  .bar {
    width: 80px;
    height: 3px;
    background: var(--bg-active);
    overflow: hidden;
    flex: none;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-faint);
  }
  .dot.ready {
    background: var(--ok);
  }
  .dot.starting {
    background: var(--warn);
  }
  .dot.error {
    background: var(--danger);
  }
</style>
