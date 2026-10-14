<script lang="ts">
  // One CodeMirror view; each tab keeps its own EditorState.
  import { onMount } from "svelte";
  import { EditorView, keymap } from "@codemirror/view";
  import { Compartment, EditorState, Prec } from "@codemirror/state";
  import { baseExtensions, completionBehaviour } from "../editor/setup";
  import { settings } from "../state/settings.svelte";
  import { lsp } from "../editor/lsp.svelte";
  import { editor, type Tab } from "../state/editor.svelte";
  import { boards } from "../state/boards.svelte";
  import { analysis, refreshAnalysis, type AnalysisTarget } from "../editor/diagnostics";
  import { breakpoints, setBreakpoints, showDebugLine } from "../editor/breakpoints";
  import { debug } from "../state/debug.svelte";

  let { onrun, onsave }: { onrun: () => void; onsave: () => void } = $props();
  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let current: Tab | null = null;
  const lspSlot = new Compartment();
  const lspGen = new WeakMap<Tab, number>();
  // Each tab's state remembers the completion mode it was configured with.
  const acSlot = new Compartment();
  const acMode = new WeakMap<Tab, boolean>();

  /** The board a tab's code is checked against: its own board, else the active one. */
  function analysisTarget(tab: Tab): AnalysisTarget {
    const b = tab.source.kind === "device" ? boards.get(tab.source.boardId) : boards.active;
    const t = b?.status.target;
    return {
      platform: t?.platform ?? null,
      board_id: t?.board_id ?? null,
      machine: b?.status.info?.machine ?? null,
      disabled: [...settings.value.disabledChecks],
    };
  }

  const isPython = (tab: Tab) => /\.pyi?$/i.test(tab.title) || !tab.title.includes(".");

  function stateFor(tab: Tab): EditorState {
    lspGen.set(tab, lsp.generation);
    acMode.set(tab, settings.value.autocomplete);
    return EditorState.create({
      doc: tab.saved,
      extensions: baseExtensions([
        Prec.high(
          keymap.of([
            { key: "Mod-s", run: () => (onsave(), true) },
            { key: "Mod-Enter", run: () => (onrun(), true) },
          ]),
        ),
        lspSlot.of(lsp.plugin(tab)),
        acSlot.of(completionBehaviour(settings.value.autocomplete)),
        isPython(tab) ? analysis(() => analysisTarget(tab)) : [],
        isPython(tab) ? breakpoints(debug.breakpoints[tab.id] ?? [], (lines) => debug.setBreakpoints(tab.id, lines)) : [],
        EditorView.updateListener.of((u) => {
          if (u.docChanged && current === tab) {
            tab.dirty = u.state.doc.toString() !== tab.saved;
          }
        }),
      ]),
    });
  }

  function show(tab: Tab | null) {
    if (!view) return;
    if (current && current !== tab) current.state = view.state;
    current = tab;
    if (!tab) {
      view.setState(EditorState.create({ doc: "" }));
      return;
    }
    // First file opened: start the language server; the editor reconfigures
    // when it's ready (generation bump).
    void lsp.ensureStarted();
    if (!tab.state) tab.state = stateFor(tab);
    view.setState(tab.state);
    if (lspGen.get(tab) !== lsp.generation) {
      lspGen.set(tab, lsp.generation);
      view.dispatch({ effects: lspSlot.reconfigure(lsp.plugin(tab)) });
    }
    if (acMode.get(tab) !== settings.value.autocomplete) {
      acMode.set(tab, settings.value.autocomplete);
      view.dispatch({ effects: acSlot.reconfigure(completionBehaviour(settings.value.autocomplete)) });
    }
    syncDebugLine();
    view.focus();
  }

  onMount(() => {
    view = new EditorView({ parent: host });
    debug.applyBreakpoints = (tabId, lines) => {
      const effects = setBreakpoints(lines);
      if (current?.id === tabId && view) view.dispatch({ effects });
      else {
        const t = editor.tabs.find((x) => x.id === tabId);
        if (t?.state) t.state = t.state.update({ effects }).state;
      }
    };
    editor.docOf = (t) => (t === current && view ? view.state.doc.toString() : (t.state?.doc.toString() ?? t.saved));
    show(editor.active);
    return () => {
      if (current && view) current.state = view.state;
      view?.destroy();
      view = null;
    };
  });

  // Reveal a requested line (from the test runner, debugger…) once its tab shows.
  $effect(() => {
    const p = editor.pendingLine;
    if (!p || !view || editor.active?.id !== p.tabId || current?.id !== p.tabId) return;
    editor.pendingLine = null;
    const doc = view.state.doc;
    const line = doc.line(Math.min(Math.max(1, p.line), doc.lines));
    view.dispatch({ selection: { anchor: line.from }, effects: EditorView.scrollIntoView(line.from, { y: "center" }) });
    view.focus();
  });

  // Show where the debugger stopped (in the debugged tab only).
  function syncDebugLine() {
    if (!view || !current) return;
    const line = debug.stopped?.line ?? null;
    showDebugLine(view, current.id === debug.session?.tabId ? line : null);
  }

  $effect(() => {
    void debug.stopped;
    void debug.session;
    syncDebugLine();
  });

  // Re-check the open file when the board or the enabled checks change.
  $effect(() => {
    const b = boards.active;
    void [b?.status.target?.platform, b?.status.target?.board_id, b?.status.info?.machine, settings.value.disabledChecks.join()];
    if (view && current) refreshAnalysis(view);
  });

  $effect(() => {
    const tab = editor.active;
    void lsp.generation;
    void settings.value.autocomplete;
    show(tab);
  });
</script>

<div class="editor" bind:this={host} class:hidden={!editor.active}></div>
{#if !editor.active}
  <div class="welcome">
    <div class="mark">microbe — MicroPython IDE</div>
    <table>
      <tbody>
        <tr><td>Connect board</td><td><kbd>⌘K</kbd></td></tr>
        <tr><td>Run file on board</td><td><kbd>⌘↵</kbd> / <kbd>F5</kbd></td></tr>
        <tr><td>Stop program</td><td><kbd>⌘.</kbd> / <kbd>⇧F5</kbd></td></tr>
        <tr><td>Debug file on board</td><td><kbd>F6</kbd></td></tr>
        <tr><td>Save</td><td><kbd>⌘S</kbd></td></tr>
        <tr><td>Toggle sidebar</td><td><kbd>⌘B</kbd></td></tr>
        <tr><td>Toggle autocomplete while typing</td><td><kbd>⌘⇧Space</kbd></td></tr>
        <tr><td>Show completions</td><td><kbd>⌃Space</kbd></td></tr>
      </tbody>
    </table>
  </div>
{/if}

<style>
  .editor {
    height: 100%;
    overflow: hidden;
  }
  .hidden {
    display: none;
  }
  .welcome {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    background: var(--bg-elev);
    color: var(--text-faint);
  }
  .mark {
    font-family: var(--font-mono);
    font-size: 15px;
    color: var(--text-dim);
    margin-bottom: 18px;
    letter-spacing: 0.02em;
  }
  table {
    border-collapse: collapse;
    font-size: 12px;
  }
  td {
    padding: 3px 10px;
  }
  td:first-child {
    text-align: right;
    color: var(--text-dim);
  }
  kbd {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-dim);
  }
</style>
