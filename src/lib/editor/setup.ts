// Base CodeMirror configuration shared by all editor tabs.
import { basicSetup } from "codemirror";
import { python } from "@codemirror/lang-python";
import { indentUnit, HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView, keymap, tooltips } from "@codemirror/view";
import { indentWithTab } from "@codemirror/commands";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";
import { autocompletion } from "@codemirror/autocomplete";
import { pythonCompletions } from "./completion";

const theme = EditorView.theme({
  "&": { backgroundColor: "var(--bg-elev)", color: "var(--text)", fontSize: "var(--editor-font-size, 13px)" },
  ".cm-content": { caretColor: "var(--accent)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
  // Selection: blue while focused, dimmer blue when not. These override
  // CodeMirror's defaults (lavender / teal-grey), hence !important.
  ".cm-selectionLayer .cm-selectionBackground": { background: "var(--sel-bg-inactive) !important" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground": {
    background: "var(--sel-bg) !important",
  },
  // Other occurrences of the selected text: a light blue tint (default is green).
  ".cm-selectionMatch": { backgroundColor: "var(--sel-match)" },
  ".cm-gutters": { backgroundColor: "var(--bg-elev)", color: "var(--text-faint)", border: "none" },
  ".cm-activeLine": { backgroundColor: "var(--line-active)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--line-active)", color: "var(--text-dim)" },
  ".cm-tooltip": { backgroundColor: "var(--bg-elev)", border: "1px solid var(--border)", color: "var(--text)" },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--bg-active)", color: "var(--text)" },
  ".cm-panels": { backgroundColor: "var(--bg-sunken)", color: "var(--text)" },
  ".cm-searchMatch": { backgroundColor: "var(--search-bg)" },
  ".cm-matchingBracket": { backgroundColor: "var(--bg-active)", outline: "none" },
  ".cm-foldPlaceholder": { backgroundColor: "var(--bg-active)", border: "none", color: "var(--text-dim)" },
});

const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.definitionKeyword, t.moduleKeyword], color: "var(--tok-keyword)" },
  { tag: [t.string, t.special(t.string)], color: "var(--tok-string)" },
  { tag: [t.number, t.bool, t.null], color: "var(--tok-number)" },
  { tag: [t.comment, t.lineComment], color: "var(--tok-comment)", fontStyle: "italic" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--tok-function)" },
  { tag: [t.definition(t.variableName), t.definition(t.function(t.variableName))], color: "var(--tok-def)" },
  { tag: [t.className, t.typeName], color: "var(--tok-type)" },
  { tag: [t.propertyName], color: "var(--tok-prop)" },
  { tag: [t.self, t.special(t.variableName)], color: "var(--tok-self)" },
  { tag: [t.operator, t.punctuation], color: "var(--text-dim)" },
  { tag: t.meta, color: "var(--tok-meta)" },
]);

// Hover docs, signatures and completion info are mounted on <body> and laid out
// against the whole window, so they're never clipped by the editor pane or the
// panels around it; their size limits live in app.css.
const windowTooltips = tooltips({
  position: "fixed",
  parent: document.body,
  tooltipSpace: () => ({ top: 4, left: 4, right: window.innerWidth - 4, bottom: window.innerHeight - 4 }),
});

export function baseExtensions(extra: Extension[] = []): Extension[] {
  return [
    basicSetup,
    windowTooltips,
    python(),
    indentUnit.of("    "),
    keymap.of([indentWithTab]),
    theme,
    syntaxHighlighting(highlight),
    ...extra,
  ];
}

/**
 * Completion behaviour: our ranked Python source (see completion.ts) and,
 * with `onTyping` off, completions only on Ctrl-Space.
 */
export function completionBehaviour(onTyping: boolean): Extension {
  return autocompletion({ activateOnTyping: onTyping, override: [pythonCompletions] });
}
