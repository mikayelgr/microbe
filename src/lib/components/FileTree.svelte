<script lang="ts">
  // Renders a TreeModel: rows, chevrons, inline create/rename fields.
  import Icon from "./Icon.svelte";
  import FileIcon from "./FileIcon.svelte";
  import type { TreeEntry, TreeModel } from "../state/tree.svelte";
  import { formatBytes } from "../util/format";

  let {
    tree,
    activePath = null,
    showSizes = false,
    onopen,
    oncontext,
  }: {
    tree: TreeModel;
    activePath?: string | null;
    showSizes?: boolean;
    onopen: (e: TreeEntry) => void;
    oncontext?: (ev: MouseEvent, e: TreeEntry | null) => void;
  } = $props();

  let draft = $state("");
  let error = $state<string | null>(null);

  function focusEdit(node: HTMLInputElement, initial: string) {
    draft = initial;
    error = null;
    node.focus();
    const dot = initial.lastIndexOf(".");
    node.setSelectionRange(0, dot > 0 ? dot : initial.length);
  }

  async function commit() {
    if (!tree.edit) return;
    error = tree.validate(draft.trim());
    if (error && !draft.trim()) return tree.cancelEdit();
    if (!error) await tree.commitEdit(draft);
  }

  function onkey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter") (e.preventDefault(), commit());
    else if (e.key === "Escape") (e.preventDefault(), tree.cancelEdit());
  }
</script>

{#snippet indent(depth: number)}
  {#each { length: depth } as _}<span class="guide"></span>{/each}
{/snippet}

{#snippet editRow(depth: number, isDir: boolean, initial: string)}
  <div class="item editing">
    {@render indent(depth)}
    <span class="twisty"></span>
    <FileIcon name={draft || initial} {isDir} />
    <input
      class="grow"
      class:invalid={!!error}
      use:focusEdit={initial}
      bind:value={draft}
      oninput={() => (error = tree.validate(draft.trim()))}
      onkeydown={onkey}
      onblur={commit}
      spellcheck="false"
      autocomplete="off"
    />
  </div>
  {#if error}<div class="edit-error" style="margin-left:{8 + depth * 12 + 20}px">{error}</div>{/if}
{/snippet}

{#snippet level(dir: string, depth: number)}
  {#if tree.edit?.mode === "create" && tree.edit.dir === dir}
    {@render editRow(depth, tree.edit.isDir, tree.edit.isDir ? "" : "untitled.py")}
  {/if}
  {#each tree.dirs[dir] ?? [] as e (e.path)}
    {#if tree.edit?.mode === "rename" && tree.edit.entry.path === e.path}
      {@render editRow(depth, e.is_dir, e.name)}
    {:else}
      <button
        class="item"
        class:active={activePath === e.path}
        role="treeitem"
        aria-selected={activePath === e.path}
        aria-expanded={e.is_dir ? !!tree.expanded[e.path] : undefined}
        onclick={() => (e.is_dir ? tree.toggle(e.path) : onopen(e))}
        oncontextmenu={(ev) => oncontext?.(ev, e)}
        onkeydown={(ev) => {
          if (ev.key === "F2") tree.startRename(e);
          else if (ev.key === "Delete" || (ev.key === "Backspace" && ev.metaKey)) tree.remove(e);
        }}
        title={e.path}
      >
        {@render indent(depth)}
        <span class="twisty" class:open={tree.expanded[e.path]}>
          {#if e.is_dir}<Icon name="chevron" size={14} />{/if}
        </span>
        <FileIcon name={e.name} isDir={e.is_dir} open={!!tree.expanded[e.path]} />
        <span class="name">{e.name}</span>
        {#if showSizes && !e.is_dir && e.size !== undefined}<span class="meta">{formatBytes(e.size)}</span>{/if}
      </button>
    {/if}
    {#if e.is_dir && tree.expanded[e.path]}{@render level(e.path, depth + 1)}{/if}
  {/each}
{/snippet}

<div class="tree" role="tree" tabindex="-1" oncontextmenu={(ev) => oncontext?.(ev, null)}>
  {#if tree.root}{@render level(tree.root, 0)}{/if}
  {#if tree.root && tree.dirs[tree.root]?.length === 0 && !tree.edit}
    <div class="hint">This folder is empty</div>
  {/if}
</div>

<style>
  .tree {
    flex: 1;
    overflow: auto;
    padding: 2px 0 12px;
    outline: none;
    --guide: transparent;
  }
  .tree:hover,
  .tree:focus-within {
    --guide: var(--tree-guide);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 0;
    width: 100%;
    height: 22px;
    padding: 0 10px 0 8px;
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--tree-fg);
    font-size: 12.5px;
    text-align: left;
    position: relative;
  }
  .item:hover:not(.editing) {
    background: var(--tree-hover);
  }
  /* The file open in the editor: tinted row, bright text, accent edge. */
  .item.active,
  .item.active:hover {
    background: var(--tree-active);
    color: var(--tree-active-fg);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .item.active .twisty,
  .item.active .meta {
    color: inherit;
    opacity: 0.8;
  }
  .item:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }
  /* Indent guides: one 12px column per level, with a hairline at the
     twisty's centre, visible while the pointer is over the tree. */
  .guide {
    flex: none;
    width: 12px;
    align-self: stretch;
    margin-left: 0;
    background: linear-gradient(var(--guide), var(--guide)) 7px 0 / 1px 100% no-repeat;
    transition: background-color 0.1s;
  }
  .twisty {
    flex: none;
    width: 16px;
    height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    transition: transform 0.1s ease-out;
  }
  .twisty.open {
    transform: rotate(90deg);
  }
  .item :global(img) {
    margin: 0 6px 0 2px;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    flex: none;
    margin-left: 8px;
    font-size: 10.5px;
    font-family: var(--font-mono);
    color: var(--text-faint);
  }
  .editing {
    background: var(--tree-active);
  }
  .editing input {
    height: 20px;
    padding: 0 4px;
    font-size: 12.5px;
    border-color: var(--accent);
    background: var(--bg-elev);
    border-radius: 2px;
  }
  .editing input.invalid {
    border-color: var(--danger);
  }
  .edit-error {
    font-size: 11px;
    color: #fff;
    background: var(--danger);
    padding: 2px 6px;
    margin-right: 10px;
    border-radius: 0 0 2px 2px;
  }
  .hint {
    padding: 6px 12px 6px 32px;
    color: var(--text-faint);
    font-style: italic;
  }
</style>
