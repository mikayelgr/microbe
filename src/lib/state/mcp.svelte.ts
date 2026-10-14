// The MCP server that lets AI agents drive the boards (src-tauri/src/mcp).
// It follows the settings: enabled, port, and whether agents may run code
// and change files.
import { api, events, type McpActivity, type McpStatus } from "../ipc";
import { attempt } from "./notices.svelte";
import { settings } from "./settings.svelte";

class McpState {
  status = $state<McpStatus | null>(null);
  /** The most recent tool call that touched a board. */
  last = $state<McpActivity | null>(null);
  #listeners = new Set<(a: McpActivity) => void>();

  async init() {
    await events.mcp((a) => {
      this.last = a;
      for (const l of this.#listeners) l(a);
    });
    if (settings.value.mcpEnabled) await this.apply();
    else this.status = await api.mcpStatus().catch(() => null);
  }

  /** Start, restart or stop the server to match the settings. */
  async apply() {
    const s = settings.value;
    const next = await attempt(
      () => (s.mcpEnabled ? api.mcpStart({ port: s.mcpPort, allow_write: s.mcpAllowWrite }) : api.mcpStop()),
      s.mcpEnabled ? "Starting the MCP server" : "Stopping the MCP server",
    );
    this.status = next ?? (await api.mcpStatus().catch(() => this.status));
  }

  async regenerateToken() {
    this.status = (await attempt(() => api.mcpRegenerateToken(), "New MCP token")) ?? this.status;
  }

  /** Called for every agent action on a board (used to refresh file trees). */
  onActivity(fn: (a: McpActivity) => void): () => void {
    this.#listeners.add(fn);
    return () => this.#listeners.delete(fn);
  }
}

export const mcp = new McpState();

/** A command that registers the server with Claude Code. */
export function claudeCodeCommand(s: McpStatus): string {
  return `claude mcp add --transport http microbe ${s.url} --header "Authorization: Bearer ${s.token}"`;
}

/** A generic JSON config entry (Cursor, VS Code and others accept this shape). */
export function jsonConfig(s: McpStatus): string {
  return JSON.stringify({ mcpServers: { microbe: { type: "http", url: s.url, headers: { Authorization: `Bearer ${s.token}` } } } }, null, 2);
}
