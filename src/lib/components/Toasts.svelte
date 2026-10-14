<script lang="ts">
  import Icon from "./Icon.svelte";
  import { notices } from "../state/notices.svelte";
  const ICON = { info: "info", success: "check", warn: "alert", error: "alert" } as const;
</script>

<div class="toasts" aria-live="polite">
  {#each notices.list as n (n.id)}
    <div class="toast {n.kind}">
      <span class="ic"><Icon name={ICON[n.kind]} size={14} /></span>
      <div class="grow text">{n.text}</div>
      {#each n.actions ?? [] as a}
        <button onclick={() => (a.run(), notices.dismiss(n.id))} title={a.label}>{a.label}</button>
      {/each}
      <button class="ghost" onclick={() => notices.dismiss(n.id)} title="Dismiss"><Icon name="x" size={13} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 10px;
    bottom: 30px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 50;
    width: min(420px, calc(100vw - 20px));
  }
  .toast {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 6px 8px 10px;
    border-radius: var(--radius);
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    box-shadow: 0 4px 14px rgb(0 0 0 / 0.22);
    user-select: text;
    font-size: 12px;
  }
  .ic {
    display: inline-flex;
    padding-top: 1px;
    color: var(--info);
  }
  .toast.success .ic {
    color: var(--ok);
  }
  .toast.warn .ic {
    color: var(--warn);
  }
  .toast.error .ic {
    color: var(--danger);
  }
  .text {
    white-space: pre-wrap;
    word-break: break-word;
    line-height: 1.45;
    padding-top: 1px;
  }
  .toast button {
    height: 22px;
  }
</style>
