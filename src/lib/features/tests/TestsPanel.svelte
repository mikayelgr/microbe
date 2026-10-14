<script lang="ts">
  // Tests in the project, run on the active board.
  import Icon from "../../components/Icon.svelte";
  import type { TestFile } from "../../ipc";
  import { boards } from "../../state/boards.svelte";
  import { editor } from "../../state/editor.svelte";
  import { project } from "../../state/project.svelte";
  import { settings } from "../../state/settings.svelte";
  import { resultKey, tests, type TestStatus } from "../../state/tests.svelte";

  let expanded = $state<Record<string, boolean>>({});
  const board = $derived(boards.active);
  const summary = $derived(tests.summary);
  const blocked = $derived(!project.root ? "Open a project folder first" : !board?.connected ? "Connect a board first" : board.running && !tests.running ? "A program is running" : null);

  $effect(() => {
    void project.root;
    tests.discover();
  });

  const ICON: Record<TestStatus, string> = {
    pending: "test-pending",
    running: "test-pending",
    pass: "test-pass",
    fail: "test-fail",
    error: "test-error",
    skip: "test-skip",
  };

  function fileStatus(f: TestFile): TestStatus | null {
    const st = f.tests.map((t) => tests.results[resultKey(f.path, t.name)]?.status);
    if (st.includes("running")) return "running";
    if (st.includes("error")) return "error";
    if (st.includes("fail")) return "fail";
    if (st.length && st.every((s) => s === "pass" || s === "skip")) return "pass";
    return null;
  }
</script>

<div class="panel">
  <div class="panel-header">
    <span>Tests</span>
    <span class="spacer"></span>
    <div class="actions">
      {#if tests.running}
        <button class="ghost" title="Stop running tests" onclick={() => tests.stop()}><Icon name="stop" size={14} /></button>
      {:else}
        <button class="ghost" title={blocked ?? "Run all tests on the board"} disabled={!!blocked} onclick={() => tests.run()}><Icon name="play" size={14} /></button>
      {/if}
      <button class="ghost" title="Find tests again" disabled={!project.root || tests.discovering} onclick={() => tests.discover()}><Icon name="refresh" size={14} /></button>
    </div>
  </div>
  <div class="section col">
    {#if summary.total}
      <div class="row small">
        <span class="mono">{summary.total} tests</span>
        {#if summary.pass}<span class="ok">{summary.pass} passed</span>{/if}
        {#if summary.fail}<span class="bad">{summary.fail} failed</span>{/if}
        {#if summary.error}<span class="bad">{summary.error} errors</span>{/if}
        {#if summary.skip}<span class="faint">{summary.skip} skipped</span>{/if}
      </div>
    {/if}
    <label class="row small">
      <input type="checkbox" checked={settings.value.syncBeforeTests} onchange={(e) => settings.set("syncBeforeTests", e.currentTarget.checked)} />
      Sync changed project files first
    </label>
  </div>
  <div class="list scroll">
    {#each tests.files as f (f.path)}
      {@const fs = fileStatus(f)}
      <div class="file row" role="treeitem" aria-selected="false" aria-expanded={expanded[f.path] !== false}>
        <button class="ghost twisty" class:open={expanded[f.path] !== false} title={expanded[f.path] === false ? "Expand" : "Collapse"} onclick={() => (expanded[f.path] = expanded[f.path] === false)}>
          <Icon name="chevron" size={12} />
        </button>
        {#if fs}<span class="st {fs}"><Icon name={ICON[fs]} size={13} /></span>{/if}
        <button class="name ghost mono ellipsis" title="Open {f.rel}" onclick={() => editor.openLocal(f.path)}>{f.rel}</button>
        <button class="ghost run" title={blocked ?? `Run the tests in ${f.rel}`} disabled={!!blocked || tests.running} onclick={() => tests.run([{ file: f }])}><Icon name="play" size={12} /></button>
      </div>
      {#if expanded[f.path] !== false}
        {#each f.tests as t (t.name)}
          {@const r = tests.results[resultKey(f.path, t.name)]}
          <div class="test row" class:open={!!r?.message && (r.status === "fail" || r.status === "error")}>
            <span class="st {r?.status ?? 'none'}"><Icon name={ICON[r?.status ?? "pending"]} size={13} /></span>
            <button class="name ghost mono ellipsis" title="Go to {t.name} (line {t.line})" onclick={() => editor.openLocalAt(f.path, t.line)}>{t.name}</button>
            {#if r?.ms !== undefined}<span class="faint small mono">{r.ms} ms</span>{/if}
            <button class="ghost run" title={blocked ?? `Run ${t.name}`} disabled={!!blocked || tests.running} onclick={() => tests.run([{ file: f, names: [t.name] }])}><Icon name="play" size={12} /></button>
          </div>
          {#if r?.message && (r.status === "fail" || r.status === "error")}
            <pre class="failure mono" title={r.traceback ?? undefined}>{r.message}{r.traceback ? `\n${r.traceback.trim()}` : ""}</pre>
          {/if}
        {/each}
      {/if}
    {:else}
      <div class="empty faint small">
        {project.root ? "No tests found. Add test_*.py files with test_* functions (or Test* classes); they run on the board." : "Open a project folder to find its tests."}
      </div>
    {/each}
  </div>
</div>

<style>
  .list {
    flex: 1;
    overflow: auto;
    padding: 2px 0;
  }
  .file,
  .test {
    height: var(--row);
    padding: 0 6px 0 4px;
    gap: 4px;
  }
  .test {
    padding-left: 22px;
  }
  .file:hover,
  .test:hover {
    background: var(--tree-hover);
  }
  .name {
    flex: 1;
    min-width: 0;
    justify-content: flex-start;
    padding: 0 2px;
    height: 20px;
    color: var(--tree-fg);
  }
  .twisty {
    width: 16px;
    padding: 0;
    justify-content: center;
  }
  .twisty :global(svg) {
    transition: transform 0.1s;
  }
  .twisty.open :global(svg) {
    transform: rotate(90deg);
  }
  .run {
    opacity: 0;
    width: 20px;
    padding: 0;
    justify-content: center;
  }
  .file:hover .run,
  .test:hover .run {
    opacity: 1;
  }
  .st {
    display: inline-flex;
    color: var(--text-faint);
  }
  .st.pass {
    color: var(--ok);
  }
  .st.fail,
  .st.error {
    color: var(--danger);
  }
  .st.running {
    color: var(--accent);
  }
  .ok {
    color: var(--ok);
  }
  .bad {
    color: var(--danger);
  }
  .failure {
    margin: 0 8px 4px 40px;
    padding: 4px 6px;
    font-size: 11px;
    white-space: pre-wrap;
    color: var(--danger);
    background: var(--bg-sunken);
    border-left: 2px solid var(--danger);
    max-height: 160px;
    overflow: auto;
    user-select: text;
  }
  .empty {
    padding: 10px 12px;
  }
</style>
