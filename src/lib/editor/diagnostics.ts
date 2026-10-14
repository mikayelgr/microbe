// Diagnostics from two sources, merged into CodeMirror's lint state:
//  - the language server (publishDiagnostics), and
//  - Microbe's analysis engine (src-tauri/src/analysis), which adds its own
//    findings and marks regions where server reports are false positives on
//    MicroPython (names injected by @rp2.asm_pio, inline assembler, viper).
//
// `setDiagnostics` replaces everything, so both sources go through `publish`,
// which keeps the latest of each and dispatches the merged list.
import { StateEffect, StateField, type ChangeDesc, type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import { LSPPlugin, serverDiagnostics, type LSPClientExtension } from "@codemirror/lsp-client";
import { api, type AnalysisResult } from "../ipc";

export interface Region {
  from: number;
  to: number;
  /** Names the region defines; null for any name. */
  names: Set<string> | null;
}

interface Sources {
  server: Diagnostic[];
  analysis: Diagnostic[];
  regions: Region[];
}

type Update = { source: "server"; diagnostics: Diagnostic[] } | { source: "analysis"; diagnostics: Diagnostic[]; regions: Region[] };

const setSource = StateEffect.define<Update>();
/** Ask the analysis plugin to run again (board or settings changed). */
const rerun = StateEffect.define<null>();

function mapSources(v: Sources, changes: ChangeDesc): Sources {
  const diag = (d: Diagnostic) => ({ ...d, from: changes.mapPos(d.from), to: changes.mapPos(d.to, 1) });
  return {
    server: v.server.map(diag),
    analysis: v.analysis.map(diag),
    regions: v.regions.map((r) => ({ ...r, from: changes.mapPos(r.from), to: changes.mapPos(r.to, 1) })),
  };
}

function apply(v: Sources, u: Update): Sources {
  return u.source === "server" ? { ...v, server: u.diagnostics } : { ...v, analysis: u.diagnostics, regions: u.regions };
}

const sourcesField = StateField.define<Sources>({
  create: () => ({ server: [], analysis: [], regions: [] }),
  update(value, tr) {
    let v = tr.docChanged ? mapSources(value, tr.changes) : value;
    for (const e of tr.effects) if (e.is(setSource)) v = apply(v, e.value);
    return v;
  },
});

/** The identifier a diagnostic message is about: `name` (ty, pyrefly) or "name" (pyright). */
export function nameIn(message: string): string | null {
  return /[`"'‘“]([A-Za-z_][A-Za-z0-9_]*)[`"'’”]/.exec(message)?.[1] ?? null;
}

export function isSuppressed(d: Diagnostic, regions: readonly Region[]): boolean {
  const name = nameIn(d.message);
  return regions.some((r) => d.from >= r.from && d.from <= r.to && (r.names === null || (name !== null && r.names.has(name))));
}

export function merge(v: Sources): Diagnostic[] {
  return [...v.server.filter((d) => !isSuppressed(d, v.regions)), ...v.analysis];
}

function publish(view: EditorView, u: Update) {
  if (!view.state.field(sourcesField, false)) view.dispatch({ effects: StateEffect.appendConfig.of(sourcesField) });
  const next = apply(view.state.field(sourcesField), u);
  view.dispatch(setDiagnostics(view.state, merge(next)), { effects: setSource.of(u) });
}

const SEVERITY = ["error", "error", "warning", "info", "hint"] as const;

/**
 * Replaces lsp-client's `serverDiagnostics()`: same capabilities and
 * auto-sync, but diagnostics are merged with the analysis engine's.
 */
export function lspDiagnostics(): LSPClientExtension {
  const base = serverDiagnostics();
  return {
    clientCapabilities: base.clientCapabilities,
    editorExtension: base.editorExtension,
    notificationHandlers: {
      "textDocument/publishDiagnostics": (client, params) => {
        const file = client.workspace.getFile(params.uri);
        if (!file || (params.version != null && params.version != file.version)) return false;
        const view = file.getView();
        const plugin = view && LSPPlugin.get(view);
        if (!view || !plugin) return false;
        const diagnostics: Diagnostic[] = params.diagnostics.map(
          (item: { range: { start: unknown; end: unknown }; severity?: number; message: string; source?: string }) => ({
            from: plugin.unsyncedChanges.mapPos(plugin.fromPosition(item.range.start as never, plugin.syncedDoc)),
            to: plugin.unsyncedChanges.mapPos(plugin.fromPosition(item.range.end as never, plugin.syncedDoc)),
            severity: SEVERITY[item.severity ?? 1] ?? "error",
            message: item.message,
            source: item.source,
          }),
        );
        publish(view, { source: "server", diagnostics });
        return true;
      },
    },
  };
}

export interface AnalysisTarget {
  platform: string | null;
  board_id: string | null;
  machine: string | null;
  disabled: string[];
}

export function toDiagnostics(r: AnalysisResult, docLength: number): { diagnostics: Diagnostic[]; regions: Region[] } {
  const clamp = (n: number) => Math.min(Math.max(0, n), docLength);
  return {
    diagnostics: r.findings.map((f) => ({
      from: clamp(f.from),
      to: clamp(f.to),
      severity: f.severity,
      message: f.message,
      source: f.rule,
    })),
    regions: r.suppressions.map((s) => ({ from: clamp(s.from), to: clamp(s.to), names: s.names.length ? new Set(s.names) : null })),
  };
}

/** Runs the analysis engine on the document (debounced) for the given target. */
export function analysis(target: () => AnalysisTarget): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      timer: ReturnType<typeof setTimeout> | undefined;
      seq = 0;

      constructor(readonly view: EditorView) {
        this.schedule(50);
      }

      update(u: ViewUpdate) {
        if (u.docChanged || u.transactions.some((t) => t.effects.some((e) => e.is(rerun)))) this.schedule(400);
      }

      schedule(ms: number) {
        clearTimeout(this.timer);
        this.timer = setTimeout(() => void this.run(), ms);
      }

      async run() {
        const seq = ++this.seq;
        const doc = this.view.state.doc;
        let result: AnalysisResult;
        try {
          result = await api.analysisRun({ source: doc.toString(), ...target() });
        } catch {
          return; // backend unavailable; the language server's diagnostics still show
        }
        // Superseded, or the text changed meanwhile (a new run is scheduled).
        if (seq !== this.seq || this.view.state.doc !== doc) return;
        publish(this.view, { source: "analysis", ...toDiagnostics(result, doc.length) });
      }

      destroy() {
        clearTimeout(this.timer);
        this.seq++;
      }
    },
  );
  return [sourcesField, plugin];
}

export function refreshAnalysis(view: EditorView) {
  view.dispatch({ effects: rerun.of(null) });
}
