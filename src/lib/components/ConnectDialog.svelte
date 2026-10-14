<script lang="ts">
  // Pick a transport and endpoint to connect to. Built from the backend's
  // transport registry, so new transports appear automatically.
  import Icon from "./Icon.svelte";
  import { api, type Endpoint, type TransportInfo } from "../ipc";
  import { boards } from "../state/boards.svelte";
  import { settings } from "../state/settings.svelte";
  import { notifyError } from "../state/notices.svelte";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  const ICONS: Record<string, string> = { serial: "usb", webrepl: "wifi", ble: "bluetooth", local: "terminal", tcp: "network", telnet: "network", rfc2217: "cable" };
  let transports = $state<TransportInfo[]>([]);
  let tab = $state("serial");
  let endpoints = $state<Endpoint[]>([]);
  let scanning = $state(false);
  let manual = $state("");
  let password = $state("");
  let baud = $state(settings.value.baud);

  const current = $derived(transports.find((t) => t.id === tab));
  const list = $derived(endpoints.filter((e) => e.transport === tab));

  $effect(() => {
    if (open) init();
  });

  async function init() {
    try {
      if (!transports.length) transports = await api.transports();
      await scan(false);
    } catch (e) {
      notifyError(e);
    }
  }

  async function scan(includeSlow: boolean) {
    scanning = true;
    try {
      const found = includeSlow ? await api.discover(tab) : await api.discover();
      endpoints = includeSlow ? [...endpoints.filter((e) => e.transport !== tab), ...found] : found;
    } catch (e) {
      notifyError(e, "Scan failed");
    } finally {
      scanning = false;
    }
  }

  async function connect(ep: Endpoint) {
    const b = await boards.connect(ep, { baud: current?.supports_baud ? baud : null, password: password || null });
    if (b) {
      open = false;
      if (ep.transport === "webrepl" || ep.transport === "serial") {
        const recent = [ep.address, ...settings.value.recentAddresses.filter((a) => a !== ep.address)].slice(0, 8);
        await settings.set("recentAddresses", recent);
      }
      if (current?.supports_baud) await settings.set("baud", baud);
    }
  }

  function connectManual() {
    if (!manual.trim()) return;
    connect({
      transport: tab,
      address: manual.trim(),
      label: manual.trim(),
      detail: null,
      usb: null,
      platform_hint: null,
      role: null,
    });
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") open = false;
  }
</script>

<svelte:window onkeydown={open ? onkey : undefined} />

{#if open}
  <div class="backdrop" role="presentation" onclick={(e) => e.target === e.currentTarget && (open = false)}>
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Connect to a board">
      <header class="row">
        <h3 class="grow">Connect to a board</h3>
        <button class="ghost" onclick={() => (open = false)} title="Close" data-tip-key="Esc"><Icon name="x" /></button>
      </header>
      <nav class="tabs">
        {#each transports as t}
          <button class="tab" class:active={tab === t.id} onclick={() => (tab = t.id)} title="Connect over {t.label}">
            <Icon name={ICONS[t.id] ?? "plug"} size={14} />{t.label}
          </button>
        {/each}
      </nav>

      <div class="body">
        <div class="row list-head">
          <span class="dim grow">
            {#if tab === "ble"}Boards advertising the Nordic UART service{:else if tab === "webrepl"}Boards running WebREPL on your network{:else}Detected devices{/if}
          </span>
          <button onclick={() => scan(tab === "ble")} disabled={scanning} title={tab === "ble" ? "Scan for Bluetooth LE boards (takes a few seconds)" : "Look for devices again"}>
            <Icon name="refresh" size={13} />{scanning ? "Scanning…" : tab === "ble" ? "Scan" : "Refresh"}
          </button>
        </div>

        <div class="list">
          {#each list as ep (ep.address)}
            <button class="ep" onclick={() => connect(ep)} disabled={boards.connecting} title="Connect to {ep.label} ({ep.address})">
              <Icon name={ICONS[ep.transport] ?? "plug"} size={18} />
              <div class="grow">
                <div class="ellipsis">{ep.label}</div>
                <div class="faint ellipsis mono small">{ep.detail ?? ep.address}</div>
              </div>
              {#if ep.platform_hint}<span class="badge ok">{ep.platform_hint}</span>{/if}
              {#if ep.role === "bootloader"}<span class="badge">bootloader</span>{/if}
            </button>
          {:else}
            <div class="empty">
              {#if scanning}Looking for devices…
              {:else if tab === "serial"}No USB serial devices found. Plug in a board, or check the driver (CP210x / CH34x).
              {:else if tab === "ble"}Press Scan to look for BLE REPL boards.
              {:else if tab === "local"}Install the MicroPython unix port (e.g. <code>brew install micropython</code>) to use the simulator.
              {:else}Enter the board's address below.{/if}
            </div>
          {/each}
        </div>

        {#if current?.manual_address}
          <div class="manual col">
            <label for="addr">Address</label>
            <div class="row">
              <input id="addr" class="grow mono" list="recent" placeholder={current.manual_address} bind:value={manual} onkeydown={(e) => e.key === "Enter" && connectManual()} />
              <datalist id="recent">
                {#each settings.value.recentAddresses as a}<option value={a}></option>{/each}
              </datalist>
              <button class="primary" onclick={connectManual} disabled={!manual.trim() || boards.connecting} title={manual.trim() ? `Connect to ${manual.trim()}` : "Enter an address first"} data-tip-key="↵">Connect</button>
            </div>
          </div>
        {/if}
        <div class="row opts">
          {#if current?.needs_password}
            <label class="row">Password <input type="password" bind:value={password} style="width:160px" /></label>
          {/if}
          {#if current?.supports_baud}
            <label class="row">Baud
              <select bind:value={baud}>
                {#each [9600, 57600, 115200, 230400, 460800, 921600] as b}<option value={b}>{b}</option>{/each}
              </select>
            </label>
          {/if}
          <span class="grow"></span>
          <label class="row" title="Stop a running program on connect so the IDE can query the board">
            <input type="checkbox" checked={settings.value.interruptOnConnect} onchange={(e) => settings.set("interruptOnConnect", e.currentTarget.checked)} />
            Interrupt running program
          </label>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.3);
    display: grid;
    place-items: start center;
    padding-top: 12vh;
    z-index: 40;
  }
  .dialog {
    width: min(600px, calc(100vw - 24px));
    max-height: 76vh;
    display: flex;
    flex-direction: column;
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.35);
  }
  header {
    height: 32px;
    padding: 0 4px 0 12px;
    border-bottom: 1px solid var(--border);
  }
  h3 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
  }
  .tabs {
    display: flex;
    padding: 0 8px;
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    background: var(--bg);
  }
  .tab {
    height: 30px;
    border: none;
    border-radius: 0;
    background: transparent !important;
    padding: 0 10px;
    color: var(--text-faint);
    box-shadow: inset 0 -1px 0 transparent;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.active {
    color: var(--text);
    box-shadow: inset 0 -2px 0 var(--accent);
  }
  .body {
    padding: 10px 12px 12px;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .list {
    display: flex;
    flex-direction: column;
    min-height: 84px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-sunken);
  }
  .ep {
    width: 100%;
    height: 40px;
    text-align: left;
    padding: 0 10px;
    gap: 10px;
    border: none;
    border-radius: 0;
    border-bottom: 1px solid var(--border);
    background: transparent;
  }
  .ep:hover:not(:disabled) {
    background: var(--bg-select);
  }
  .opts {
    flex-wrap: wrap;
  }
  .list .empty {
    font-size: 12px;
  }
</style>
