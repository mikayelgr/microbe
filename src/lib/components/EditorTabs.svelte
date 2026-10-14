<script lang="ts">
  import Icon from "./Icon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import { editor } from "../state/editor.svelte";
  import { boards } from "../state/boards.svelte";

  function where(t: (typeof editor.tabs)[number]) {
    if (t.source.kind === "device") return boards.get(t.source.boardId)?.label ?? "board";
    return null;
  }
</script>

<div class="tabs" role="tablist">
  {#each editor.tabs as t (t.id)}
    <div
      class="tab"
      class:active={t.id === editor.activeId}
      role="tab"
      tabindex="0"
      aria-selected={t.id === editor.activeId}
      title={t.source.kind === "scratch" ? t.title : t.source.path}
      onclick={() => (editor.activeId = t.id)}
      onkeydown={(e) => e.key === "Enter" && (editor.activeId = t.id)}
      onauxclick={(e) => e.button === 1 && editor.close(t.id)}
    >
      <FileIcon name={t.title} size={14} />
      <span class="ellipsis">{t.title}</span>
      {#if where(t)}<span class="faint where" title="On {where(t)}"><Icon name="chip" size={11} />{where(t)}</span>{/if}
      <button class="ghost close" class:dirty={t.dirty} onclick={(e) => (e.stopPropagation(), editor.close(t.id))} title={t.dirty ? "Unsaved changes — close" : "Close"}>
        {#if t.dirty}<span class="dot"></span>{:else}<Icon name="x" size={12} />{/if}
      </button>
    </div>
  {/each}
  <button class="ghost new" onclick={() => editor.newScratch()} title="New scratch file">
    <Icon name="plus" size={14} />
  </button>
</div>

<style>
  .tabs {
    display: flex;
    align-items: stretch;
    height: 30px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    overflow-y: hidden;
    flex: none;
  }
  .tabs::-webkit-scrollbar {
    height: 3px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 4px 0 10px;
    max-width: 220px;
    border-right: 1px solid var(--border);
    color: var(--text-faint);
    white-space: nowrap;
    font-size: 12px;
  }
  .tab:hover {
    color: var(--text-dim);
  }
  .tab.active {
    background: var(--bg-elev);
    color: var(--text);
    box-shadow: inset 0 1px 0 var(--accent);
  }
  .where {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 10px;
    font-family: var(--font-mono);
  }
  .close {
    width: 18px;
    height: 18px;
    padding: 0;
    justify-content: center;
    visibility: hidden;
  }
  .tab:hover .close,
  .tab.active .close,
  .close.dirty {
    visibility: visible;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-dim);
  }
  .new {
    align-self: center;
    margin-left: 2px;
    height: 22px;
    width: 22px;
    justify-content: center;
    padding: 0;
  }
</style>
