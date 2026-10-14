// The language server client: CodeMirror's LSP client talking to the Rust
// bridge (which runs ty or another provider with MicroPython stubs).
//
// The server starts lazily, when the first file is opened, and is restarted
// only on an explicit change (project folder, provider). Stubs follow the
// active board; with no board connected they follow the user's setting.
import {
  LSPClient,
  findReferencesKeymap,
  formatKeymap,
  hoverTooltips,
  jumpToDefinitionKeymap,
  renameKeymap,
  serverCompletion,
  signatureHelp,
  type Transport,
} from "@codemirror/lsp-client";
import { keymap } from "@codemirror/view";
import { appCacheDir } from "@tauri-apps/api/path";
import type { Extension } from "@codemirror/state";
import { api, Channel, events, type StubRequest, type StubSet } from "../ipc";
import { notices } from "../state/notices.svelte";
import { settings } from "../state/settings.svelte";
import type { Tab } from "../state/editor.svelte";
import { lspDiagnostics } from "./diagnostics";

export type LspStatus = "off" | "starting" | "ready" | "error";

function fileUri(path: string) {
  const p = path.replace(/\\/g, "/");
  return "file://" + encodeURI(p.startsWith("/") ? p : "/" + p);
}

export function describeStubs(req: StubRequest | null): string {
  if (!req) return "no stubs";
  if (req.port === "generic") return "generic MicroPython";
  return [req.port, req.board, req.version].filter(Boolean).join(" · ");
}

class LanguageService {
  status = $state<LspStatus>("off");
  /** What the stubs are for, e.g. "rp2 · RPI_PICO2 · 1.29.0"; shown in the status bar. */
  stubLabel = $state("");
  /** Bumped whenever the client is replaced; editors reconfigure on change. */
  generation = $state(0);
  client: LSPClient | null = null;
  #cache = "";
  #root: string | null = null;
  #wanted: StubRequest | null = null;
  #applied = "";
  #stubs: StubSet | null = null;
  /** Serializes starts so overlapping requests can't race. */
  #queue: Promise<unknown> = Promise.resolve();

  async init() {
    this.#cache = (await appCacheDir()).replace(/\\/g, "/");
    await events.lsp((e) => {
      if (e.kind === "exited" && this.status === "ready") {
        this.status = "error";
        notices.push("warn", "The language server exited unexpectedly", {
          actions: [{ label: "Restart", run: () => this.restart() }],
        });
      }
    });
  }

  uriFor(tab: Tab): string {
    switch (tab.source.kind) {
      case "local":
        return fileUri(tab.source.path);
      case "device":
        return fileUri(`${this.#cache}/device/${tab.source.boardId}${tab.source.path.replace(/^\.?\//, "/")}`);
      default:
        return fileUri(`${this.#cache}/scratch/${tab.title}`);
    }
  }

  /** Start the server if it isn't running yet (called when a file opens). */
  ensureStarted(): Promise<unknown> {
    if (this.status !== "off") return this.#queue;
    return this.#enqueue(() => this.#start());
  }

  /** Restart with the current project root/provider, if the server is in use. */
  restart(root: string | null = this.#root): Promise<unknown> {
    this.#root = root;
    if (this.status === "off") return this.#queue;
    return this.#enqueue(() => this.#start());
  }

  setRoot(root: string | null) {
    if (root === this.#root) return;
    this.#root = root;
    if (this.status !== "off") this.restart(root);
  }

  /** Choose which stubs the server should use; applied now or on start. */
  setStubs(req: StubRequest | null) {
    this.#wanted = req;
    this.stubLabel = describeStubs(req);
    if (this.status === "ready") this.#enqueue(() => this.#applyStubs());
  }

  #enqueue(fn: () => Promise<void>): Promise<unknown> {
    this.#queue = this.#queue.then(fn, fn);
    return this.#queue;
  }

  async #start() {
    this.status = "starting";
    try {
      this.client?.disconnect();
    } catch {}
    this.client = null;
    const handlers = new Set<(v: string) => void>();
    const channel = new Channel<string>();
    channel.onmessage = (m) => handlers.forEach((h) => h(m));
    try {
      await api.lspStart(settings.value.languageServer, this.#stubs, channel);
    } catch (e) {
      this.status = "error";
      notices.push("warn", `Language server unavailable: ${e}`);
      return;
    }
    const transport: Transport = {
      send: (m) => void api.lspSend(m).catch(() => {}),
      subscribe: (h) => handlers.add(h),
      unsubscribe: (h) => handlers.delete(h),
    };
    this.client = new LSPClient({
      rootUri: this.#root ? fileUri(this.#root) : undefined,
      // languageServerExtensions(), with diagnostics merged with the analysis engine's.
      extensions: [
        serverCompletion(),
        hoverTooltips(),
        keymap.of([...formatKeymap, ...renameKeymap, ...jumpToDefinitionKeymap, ...findReferencesKeymap]),
        signatureHelp(),
        lspDiagnostics(),
      ],
      timeout: 10000,
    }).connect(transport);
    this.status = "ready";
    this.generation++;
    await this.#applyStubs();
  }

  async #applyStubs() {
    const req = this.#wanted;
    const key = JSON.stringify(req);
    if (key === this.#applied) return;
    this.#applied = key;
    if (!req) {
      this.#stubs = null;
      await api.lspSetStubs(null).catch(() => {});
      return;
    }
    try {
      this.stubLabel = `${describeStubs(req)} (loading…)`;
      this.#stubs = await api.stubsEnsure(req);
      await api.lspSetStubs(this.#stubs);
      const fromBoard = this.#stubs.packages.find((p) => p.startsWith("from the board: "));
      this.stubLabel = describeStubs(req) + (fromBoard ? " + board modules" : "");
    } catch (e) {
      this.#applied = "";
      this.stubLabel = `${describeStubs(req)} (unavailable)`;
      notices.push("warn", `Could not load MicroPython stubs: ${e}`);
    }
  }

  plugin(tab: Tab): Extension {
    return this.client ? this.client.plugin(this.uriFor(tab), "python") : [];
  }
}

export const lsp = new LanguageService();
