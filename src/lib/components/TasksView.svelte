<script lang="ts">
  // asyncio tasks of the program running on the active board, from the
  // aio_watch prelude's TASKS records.
  import type { Board } from "../state/boards.svelte";
  import { settings } from "../state/settings.svelte";
  import { runtime, type TaskInfo } from "../runtime/runtime.svelte";

  let { board }: { board: Board } = $props();
  const tasks = $derived(runtime.tasks[board.id] ?? []);
  const ORDER: Record<TaskInfo["state"], number> = { running: 0, ready: 1, sleeping: 2, waiting: 3, failed: 4, cancelled: 5, done: 6 };
  const sorted = $derived([...tasks].sort((a, b) => ORDER[a.state] - ORDER[b.state]));

  function detail(t: TaskInfo): string {
    if (t.state === "sleeping") return `wakes in ${t.wake_ms} ms`;
    if (t.state === "waiting") return t.on === "TaskQueue" ? "waiting on an event, lock or task" : `waiting on ${t.on}`;
    if (t.state === "failed") return t.error ?? "";
    return "";
  }
</script>

<div class="tasks">
  <div class="toolbar row">
    <label class="row" title="Wraps asyncio.create_task in your program to report its tasks">
      <input type="checkbox" checked={settings.value.inspectAsyncio} onchange={(e) => settings.set("inspectAsyncio", e.currentTarget.checked)} />
      Inspect asyncio tasks during runs
    </label>
    <span class="grow"></span>
    {#if tasks.length}<span class="faint small">{tasks.filter((t) => !["done", "cancelled", "failed"].includes(t.state)).length} live</span>{/if}
  </div>
  {#if sorted.length}
    <div class="scroll">
      <table class="small">
        <thead><tr><th>Task</th><th>State</th><th>Detail</th></tr></thead>
        <tbody>
          {#each sorted as t (t.id)}
            <tr class:dim={["done", "cancelled"].includes(t.state)}>
              <td class="mono">{t.name}</td>
              <td class="state {t.state}">{t.state}</td>
              <td class="mono ellipsis" title={detail(t)}>{detail(t)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="empty faint small">
      {settings.value.inspectAsyncio ? "Run a program that uses asyncio to see its tasks here." : "Turn on task inspection, then run a program that uses asyncio."}
    </div>
  {/if}
</div>

<style>
  .tasks {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-elev);
  }
  .toolbar {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    min-height: 30px;
  }
  .scroll {
    flex: 1;
    overflow: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    font-weight: normal;
    color: var(--text-dim);
    padding: 3px 8px;
    border-bottom: 1px solid var(--border);
    position: sticky;
    top: 0;
    background: var(--bg-elev);
  }
  td {
    padding: 2px 8px;
    height: var(--row);
    max-width: 480px;
  }
  tr.dim td {
    color: var(--text-faint);
  }
  .state.running {
    color: var(--ok);
  }
  .state.sleeping,
  .state.ready {
    color: var(--accent);
  }
  .state.waiting {
    color: var(--warn);
  }
  .state.failed {
    color: var(--danger);
  }
  .empty {
    padding: 12px;
  }
</style>
