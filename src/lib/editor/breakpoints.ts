// Breakpoint gutter and the debugger's current-line highlight. Breakpoints
// live in each tab's EditorState, so they follow edits; the debug store is
// told about changes through `onChange`.
import { Prec, RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, GutterMarker, gutter, type DecorationSet } from "@codemirror/view";

class BreakpointMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-breakpoint";
    return el;
  }
}
const marker = new BreakpointMarker();

/** Sizes the gutter; not a breakpoint. */
class SpacerMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-breakpoint-spacer";
    return el;
  }
}

const toggle = StateEffect.define<number>();
const setAll = StateEffect.define<number[]>();

function fromLines(state: EditorState, lines: readonly number[]) {
  const doc = state.doc;
  const starts = [...new Set(lines.filter((l) => l >= 1 && l <= doc.lines))].sort((a, b) => a - b).map((l) => doc.line(l).from);
  return RangeSet.of(starts.map((s) => marker.range(s)));
}

const breakpointField = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(set, tr) {
    set = set.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setAll)) set = fromLines(tr.state, e.value);
      if (e.is(toggle)) {
        let has = false;
        set.between(e.value, e.value, () => void (has = true));
        set = has ? set.update({ filter: (from) => from !== e.value }) : set.update({ add: [marker.range(e.value)] });
      }
    }
    return set;
  },
});

export function breakpointLines(state: EditorState): number[] {
  const field = state.field(breakpointField, false);
  if (!field) return [];
  const out: number[] = [];
  const iter = field.iter();
  while (iter.value) {
    out.push(state.doc.lineAt(iter.from).number);
    iter.next();
  }
  return [...new Set(out)];
}

/** Effect that replaces a state's breakpoints. */
export function setBreakpoints(lines: number[]) {
  return setAll.of(lines);
}

const setDebugLine = StateEffect.define<number | null>();
const debugLineDeco = Decoration.line({ class: "cm-debug-line" });

const debugLineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    deco = deco.map(tr.changes);
    for (const e of tr.effects)
      if (e.is(setDebugLine)) {
        const doc = tr.state.doc;
        deco = e.value && e.value <= doc.lines ? Decoration.set([debugLineDeco.range(doc.line(e.value).from)]) : Decoration.none;
      }
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** Highlight the line the debugger stopped at (null clears it). */
export function showDebugLine(view: EditorView, line: number | null) {
  const effects: StateEffect<unknown>[] = [setDebugLine.of(line)];
  if (line && line <= view.state.doc.lines) effects.push(EditorView.scrollIntoView(view.state.doc.line(line).from, { y: "center" }));
  view.dispatch({ effects });
}

const theme = EditorView.baseTheme({
  ".cm-breakpoint-gutter .cm-gutterElement": { cursor: "pointer", width: "14px", display: "flex", alignItems: "center", justifyContent: "center" },
  ".cm-breakpoint-spacer": { width: "8px", display: "inline-block" },
  ".cm-breakpoint": { width: "8px", height: "8px", borderRadius: "50%", background: "var(--danger)", display: "inline-block" },
  ".cm-breakpoint-gutter .cm-gutterElement:hover:empty::after": {
    content: '""', width: "8px", height: "8px", borderRadius: "50%", background: "var(--danger)", opacity: "0.35",
  },
  ".cm-debug-line": { backgroundColor: "var(--debug-line, rgba(229, 192, 60, 0.22)) !important" },
});

export function breakpoints(initial: readonly number[], onChange: (lines: number[]) => void): Extension {
  return [
    breakpointField.init((state) => fromLines(state, initial)),
    debugLineField,
    // Left of the line numbers, as in other IDEs.
    Prec.highest(
      gutter({
        class: "cm-breakpoint-gutter",
        markers: (v) => v.state.field(breakpointField),
        initialSpacer: () => new SpacerMarker(),
        renderEmptyElements: true,
        domEventHandlers: {
          mousedown(view, line) {
            view.dispatch({ effects: toggle.of(line.from) });
            return true;
          },
        },
      }),
    ),
    EditorView.updateListener.of((u) => {
      if (u.state.field(breakpointField) !== u.startState.field(breakpointField)) onChange(breakpointLines(u.state));
    }),
    theme,
  ];
}
