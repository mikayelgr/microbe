// Persistent user settings (tauri-plugin-store).
import { load, type Store } from "@tauri-apps/plugin-store";
import type { StubRequest } from "../ipc";

export interface Settings {
  theme: "system" | "light" | "dark";
  fontSize: number;
  languageServer: string;
  /** Stubs when no board is connected: "last" (last board), "generic", or a port name. */
  stubsWhenDisconnected: string;
  /** Stubs of the most recently connected board. */
  lastBoardStubs: StubRequest | null;
  interruptOnConnect: boolean;
  lastProject: string | null;
  recentAddresses: string[];
  baud: number;
  checkFirmwareUpdates: boolean;
  /** Show completions while typing (Ctrl-Space always works). */
  autocomplete: boolean;
  /** Reconnect automatically when a board disappears and comes back. */
  autoReconnect: boolean;
  /** Static checks (analysis engine ids) the user turned off. */
  disabledChecks: string[];
  /** Serve the MCP server so AI agents can drive the boards. */
  mcpEnabled: boolean;
  mcpPort: number;
  /** Expose MCP tools that run code and change files. */
  mcpAllowWrite: boolean;
  /** Sample heap use while programs run (Memory tab). */
  sampleMemory: boolean;
  /** Report asyncio tasks while programs run (Tasks tab). */
  inspectAsyncio: boolean;
  /** Sync the project to the board before running tests. */
  syncBeforeTests: boolean;
  /** Generate stubs from the board for modules without published stubs. */
  boardStubs: boolean;
}

const DEFAULTS: Settings = {
  theme: "system",
  fontSize: 13,
  languageServer: "ty",
  stubsWhenDisconnected: "last",
  lastBoardStubs: null,
  interruptOnConnect: true,
  lastProject: null,
  recentAddresses: [],
  baud: 115200,
  checkFirmwareUpdates: true,
  autocomplete: true,
  autoReconnect: true,
  disabledChecks: [],
  mcpEnabled: false,
  mcpPort: 7357,
  mcpAllowWrite: true,
  sampleMemory: false,
  inspectAsyncio: false,
  syncBeforeTests: true,
  boardStubs: true,
};

class SettingsState {
  value = $state<Settings>({ ...DEFAULTS });
  loaded = $state(false);
  #store: Store | null = null;

  async init() {
    try {
      this.#store = await load("settings.json", { autoSave: 300, defaults: {} });
      const saved = (await this.#store.get<Partial<Settings>>("settings")) ?? {};
      this.value = { ...DEFAULTS, ...saved };
    } catch (e) {
      console.warn("settings unavailable", e);
    }
    this.loaded = true;
    this.applyTheme();
  }

  async set<K extends keyof Settings>(key: K, v: Settings[K]) {
    this.value[key] = v;
    if (key === "theme" || key === "fontSize") this.applyTheme();
    await this.#store?.set("settings", $state.snapshot(this.value));
  }

  applyTheme() {
    const root = document.documentElement;
    if (this.value.theme === "system") delete root.dataset.theme;
    else root.dataset.theme = this.value.theme;
    root.style.setProperty("--editor-font-size", `${this.value.fontSize}px`);
  }
}

export const settings = new SettingsState();
