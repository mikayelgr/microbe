<script lang="ts">
  import { dialogs } from "../state/dialogs.svelte";

  let value = $state("");
  let error = $state<string | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  let okBtn = $state<HTMLButtonElement | null>(null);

  $effect(() => {
    const d = dialogs.current;
    if (!d) return;
    value = d.kind === "prompt" ? (d.value ?? "") : "";
    error = null;
    queueMicrotask(() => {
      if (input) {
        input.focus();
        // Select the stem so typing replaces the name but keeps the extension.
        const dot = value.lastIndexOf(".");
        input.setSelectionRange(0, dot > 0 ? dot : value.length);
      } else okBtn?.focus();
    });
  });

  function submit() {
    const d = dialogs.current;
    if (!d) return;
    if (d.kind === "prompt") {
      error = d.validate?.(value) ?? null;
      if (error) return;
      dialogs.close(value);
    } else if (d.kind === "form") {
      dialogs.close(Object.fromEntries(d.checks.map((c) => [c.id, c.checked])));
    } else dialogs.close(true);
  }

  function onkey(e: KeyboardEvent) {
    if (!dialogs.current) return;
    if (e.key === "Escape") (e.preventDefault(), dialogs.close(null));
    else if (e.key === "Enter") (e.preventDefault(), submit());
  }
</script>

<svelte:window onkeydown={onkey} />

{#if dialogs.current}
  {@const d = dialogs.current}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && dialogs.close(null)}>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="dlg-title">
      <div id="dlg-title" class="title">{d.title}</div>
      {#if d.message}<div class="message">{d.message}</div>{/if}
      {#if d.kind === "form"}
        <div class="checks">
          {#each d.checks as c (c.id)}
            <label class="check">
              <input type="checkbox" bind:checked={c.checked} />
              <span><span class="label">{c.label}</span>{#if c.hint}<span class="hint">{c.hint}</span>{/if}</span>
            </label>
          {/each}
        </div>
      {/if}
      {#if d.kind === "prompt"}
        <input bind:this={input} bind:value class="mono" placeholder={d.placeholder} class:invalid={!!error} oninput={() => (error = null)} />
        {#if error}<div class="error">{error}</div>{/if}
      {/if}
      <div class="row actions">
        <span class="grow"></span>
        <button onclick={() => dialogs.close(null)} title="Cancel" data-tip-key="Esc">Cancel</button>
        <button bind:this={okBtn} class:primary={!d.danger} class:destructive={d.danger} onclick={submit} title={d.confirmLabel ?? "OK"} data-tip-key="↵">{d.confirmLabel ?? "OK"}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    background: rgb(0 0 0 / 0.3);
    display: grid;
    place-items: start center;
    padding-top: 18vh;
  }
  .dialog {
    width: min(420px, calc(100vw - 24px));
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.35);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .title {
    font-weight: 600;
  }
  .message {
    color: var(--text-dim);
    white-space: pre-wrap;
    user-select: text;
  }
  input {
    width: 100%;
  }
  input.invalid {
    border-color: var(--danger);
  }
  .error {
    color: var(--danger);
    font-size: 11px;
  }
  .actions {
    margin-top: 4px;
  }
  .checks {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    color: var(--text);
  }
  .check input {
    margin-top: 2px;
  }
  .check .hint {
    display: block;
    color: var(--text-dim);
    font-size: 11px;
  }
  .destructive {
    background: var(--danger);
    border-color: var(--danger);
    color: #fff;
  }
</style>
