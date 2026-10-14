<script lang="ts">
  import Icon from "./Icon.svelte";
  let { value = $bindable(""), placeholder = "Search" }: { value?: string; placeholder?: string } = $props();
</script>

<label class="search row">
  <Icon name="search" size={13} />
  <input class="grow" {placeholder} bind:value spellcheck="false" />
  {#if value}<button class="ghost clear" onclick={() => (value = "")} title="Clear search"><Icon name="x" size={12} /></button>{/if}
</label>

<style>
  .search {
    gap: 4px;
    background: var(--bg-sunken);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    padding: 0 2px 0 6px;
    color: var(--text-faint);
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  input {
    border: none;
    background: transparent;
    padding: 0 2px;
  }
  .clear {
    height: 18px;
    width: 18px;
    justify-content: center;
    padding: 0;
  }
</style>
