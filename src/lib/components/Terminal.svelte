<script lang="ts">
  // xterm.js view of one board's REPL. Keystrokes go to the device.
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { api } from "../ipc";
  import type { Board } from "../state/boards.svelte";
  import { settings } from "../state/settings.svelte";
  import { cssVar, onThemeChange } from "../util/theme";

  let { board }: { board: Board } = $props();
  let host: HTMLDivElement;

  function themeColors() {
    return {
      background: cssVar("--term-bg"),
      foreground: cssVar("--term-fg"),
      cursor: cssVar("--accent"),
      selectionBackground: cssVar("--sel-bg"),
      selectionInactiveBackground: cssVar("--sel-bg-inactive"),
      red: cssVar("--danger"),
      brightRed: cssVar("--danger"),
    };
  }

  onMount(() => {
    const term = new Terminal({
      fontFamily: cssVar("--font-mono"),
      fontSize: settings.value.fontSize,
      cursorBlink: true,
      convertEol: false,
      scrollback: 5000,
      theme: themeColors(),
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(host);
    fit.fit();

    const unsub = board.subscribe((ev) => {
      if (ev.kind === "data") term.write(new Uint8Array(ev.data));
      else if (ev.kind === "run") {
        if (ev.stderr) term.write("\x1b[31m");
        // Program output uses bare \n on some ports; normalise for xterm.
        const text = new TextDecoder().decode(new Uint8Array(ev.data)).replace(/\r?\n/g, "\r\n");
        term.write(text);
        if (ev.stderr) term.write("\x1b[0m");
      }
    });
    const input = term.onData((d) => {
      api.write(board.id, d).catch(() => {});
    });

    const ro = new ResizeObserver(() => {
      try {
        fit.fit();
      } catch {}
    });
    ro.observe(host);
    const stopTheme = onThemeChange(() => (term.options.theme = themeColors()));

    return () => {
      unsub();
      input.dispose();
      ro.disconnect();
      stopTheme();
      term.dispose();
    };
  });
</script>

<div class="term" bind:this={host}></div>

<style>
  .term {
    height: 100%;
    width: 100%;
    padding: 4px 0 0 8px;
    background: var(--term-bg);
  }
  .term :global(.xterm-viewport) {
    background: var(--term-bg) !important;
  }
</style>
