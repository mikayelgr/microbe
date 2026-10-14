<script lang="ts">
  import Icon from "../../components/Icon.svelte";
  import { api, type CheckInfo, type LspProviderInfo } from "../../ipc";
  import { settings } from "../../state/settings.svelte";
  import { lsp, describeStubs } from "../../editor/lsp.svelte";
  import { project } from "../../state/project.svelte";
  import { mcp, claudeCodeCommand, jsonConfig } from "../../state/mcp.svelte";
  import { attempt, notices } from "../../state/notices.svelte";

  let providers = $state<LspProviderInfo[]>([]);
  let checks = $state<CheckInfo[]>([]);
  $effect(() => {
    api.lspProviders().then((p) => (providers = p)).catch(() => {});
    api.analysisChecks().then((c) => (checks = c)).catch(() => {});
  });

  async function toggleCheck(id: string, on: boolean) {
    const rest = settings.value.disabledChecks.filter((c) => c !== id);
    await settings.set("disabledChecks", on ? rest : [...rest, id]);
  }

  async function setMcp<K extends "mcpEnabled" | "mcpPort" | "mcpAllowWrite">(key: K, v: (typeof settings.value)[K]) {
    await settings.set(key, v);
    if (key === "mcpEnabled" || settings.value.mcpEnabled) await mcp.apply();
  }

  async function copy(text: string, what: string) {
    if ((await attempt(() => navigator.clipboard.writeText(text), "Copy")) !== undefined) notices.push("success", `Copied ${what}`);
  }

  const PORTS = ["rp2", "esp32", "esp8266", "stm32", "samd", "nrf", "mimxrt", "renesas-ra", "unix"];

  async function setServer(id: string) {
    await settings.set("languageServer", id);
    await lsp.restart(project.root);
  }
</script>

<div class="panel scroll">
  <div class="panel-header">Settings</div>
  <div class="section col">
    <b>Appearance</b>
    <label class="row">Theme
      <select value={settings.value.theme} onchange={(e) => settings.set("theme", e.currentTarget.value as "system" | "light" | "dark")}>
        <option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option>
      </select>
    </label>
    <label class="row">Font size
      <input type="number" min="10" max="24" value={settings.value.fontSize} onchange={(e) => settings.set("fontSize", +e.currentTarget.value)} style="width:70px" />
    </label>
  </div>
  <div class="section col">
    <b>Editor</b>
    <label class="row">
      <input type="checkbox" checked={settings.value.autocomplete} onchange={(e) => settings.set("autocomplete", e.currentTarget.checked)} />
      Autocomplete while typing <span class="faint mono small">⌘⇧Space</span>
    </label>
    <div class="faint small">When off, press ⌃Space to show completions.</div>
  </div>
  <div class="section col">
    <b>Python language server</b>
    {#each providers as p}
      <label class="row" class:faint={!p.available}>
        <input type="radio" name="ls" value={p.id} checked={settings.value.languageServer === p.id} disabled={!p.available} onchange={() => setServer(p.id)} />
        {p.label}{#if !p.available}<span class="small">(not installed)</span>{/if}
      </label>
    {/each}
    <label class="col">Stubs when no board is connected
      <select value={settings.value.stubsWhenDisconnected} onchange={(e) => settings.set("stubsWhenDisconnected", e.currentTarget.value)}>
        <option value="last">Last connected board{settings.value.lastBoardStubs ? ` (${describeStubs(settings.value.lastBoardStubs)})` : ""}</option>
        <option value="generic">Generic MicroPython (modules common to all ports)</option>
        <optgroup label="Specific port">
          {#each PORTS as p}<option value={p}>{p}</option>{/each}
        </optgroup>
      </select>
    </label>
    <label class="row">
      <input type="checkbox" checked={settings.value.boardStubs} onchange={(e) => settings.set("boardStubs", e.currentTarget.checked)} />
      Generate stubs from the board for modules without published stubs
    </label>
    <div class="faint small">Active: {lsp.status === "off" ? "starts when you open a file" : lsp.stubLabel || "none"}</div>
  </div>
  <div class="section col">
    <b>Code checks</b>
    {#each checks as c (c.id)}
      <label class="row">
        <input type="checkbox" checked={!settings.value.disabledChecks.includes(c.id)} onchange={(e) => toggleCheck(c.id, e.currentTarget.checked)} />
        {c.description}
      </label>
    {/each}
    <div class="faint small">MicroPython-specific checks shown alongside the language server's. Pin checks use the connected board's GPIO map.</div>
  </div>
  <div class="section col">
    <b>MCP server</b>
    <label class="row">
      <input type="checkbox" checked={settings.value.mcpEnabled} onchange={(e) => setMcp("mcpEnabled", e.currentTarget.checked)} />
      Let AI agents use the connected boards
    </label>
    <label class="row">
      <input type="checkbox" checked={settings.value.mcpAllowWrite} onchange={(e) => setMcp("mcpAllowWrite", e.currentTarget.checked)} />
      Allow running code and changing files
    </label>
    <label class="row">Port
      <input type="number" min="1024" max="65535" value={settings.value.mcpPort} onchange={(e) => setMcp("mcpPort", +e.currentTarget.value)} style="width:80px" />
    </label>
    {#if mcp.status?.running && mcp.status.url}
      {@const st = mcp.status}
      <div class="row small">
        <span class="badge ok">on</span>
        <span class="mono ellipsis grow" title={st.url}>{st.url}</span>
        <button class="ghost" title="Copy the server URL" onclick={() => copy(st.url!, "the server URL")}><Icon name="copy" size={13} /></button>
      </div>
      <div class="row small">
        <span class="faint">Token</span>
        <span class="mono ellipsis grow">{st.token.slice(0, 6)}…</span>
        <button class="ghost" title="Copy the bearer token" onclick={() => copy(st.token, "the token")}><Icon name="copy" size={13} /></button>
        <button class="ghost" title="Create a new token; agents using the old one lose access" onclick={() => mcp.regenerateToken()}><Icon name="refresh" size={13} /></button>
      </div>
      <div class="row">
        <button title="Copy a `claude mcp add` command that registers this server with Claude Code" onclick={() => copy(claudeCodeCommand(st), "the Claude Code command")}>Copy for Claude Code</button>
        <button title="Copy an mcpServers JSON entry (Cursor, VS Code and other clients)" onclick={() => copy(jsonConfig(st), "the JSON config")}>Copy JSON</button>
      </div>
      <div class="faint small">{st.tools.length} tools: {st.tools.join(", ")}</div>
    {/if}
    <div class="faint small">Served on this computer only (127.0.0.1) and protected by the token. Agents work on the same sessions you see here.</div>
  </div>
  <div class="section col">
    <b>Boards</b>
    <label class="row"><input type="checkbox" checked={settings.value.interruptOnConnect} onchange={(e) => settings.set("interruptOnConnect", e.currentTarget.checked)} />Interrupt running program on connect</label>
    <label class="row"><input type="checkbox" checked={settings.value.autoReconnect} onchange={(e) => settings.set("autoReconnect", e.currentTarget.checked)} />Reconnect automatically when a board comes back</label>
    <label class="row"><input type="checkbox" checked={settings.value.checkFirmwareUpdates} onchange={(e) => settings.set("checkFirmwareUpdates", e.currentTarget.checked)} />Offer firmware updates on connect</label>
  </div>
</div>

<style>
  label.row {
    color: var(--text);
  }
</style>
