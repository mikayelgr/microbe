<script lang="ts">
  import Icon from "./Icon.svelte";
  import { menu } from "../state/menu.svelte";
</script>

<svelte:window onclick={() => menu.close()} onblur={() => menu.close()} onkeydown={(e) => e.key === "Escape" && menu.close()} />

{#if menu.open}
  <div class="menu" style="left:{menu.x}px; top:{menu.y}px" role="menu">
    {#each menu.items as item}
      <button class="ghost item" class:danger={item.danger} disabled={item.disabled} role="menuitem" onclick={() => (menu.close(), item.run())} title={item.hint ?? item.label}>
        <span class="ic">{#if item.icon}<Icon name={item.icon} size={14} />{/if}</span>{item.label}
      </button>
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 60;
    min-width: 190px;
    padding: 3px 0;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.25);
    display: flex;
    flex-direction: column;
  }
  .item {
    justify-content: flex-start;
    width: 100%;
    height: 24px;
    padding: 0 10px 0 6px;
    border-radius: 0;
    color: var(--text);
  }
  .item:hover:not(:disabled) {
    background: var(--accent) !important;
    color: #fff;
  }
  .danger {
    color: var(--danger);
  }
  .ic {
    width: 18px;
    display: inline-flex;
    opacity: 0.8;
  }
</style>
