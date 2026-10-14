<script lang="ts">
  // Detect a board, fetch the right MicroPython firmware and flash it.
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import Icon from "../../components/Icon.svelte";
  import SearchInput from "../../components/SearchInput.svelte";
  import { api, type BoardFirmware, type BoardIndex, type Candidate, type Endpoint } from "../../ipc";
  import { boards } from "../../state/boards.svelte";
  import { notices, notifyError } from "../../state/notices.svelte";
  import { firmwareUi } from "./firmware.svelte";
  import { dialogs } from "../../state/dialogs.svelte";
  import { baseName } from "../../util/path";
  import BuildSection from "./BuildSection.svelte";

  let index = $state<BoardIndex | null>(null);
  let candidates = $state<Candidate[]>([]);
  let detecting = $state(false);
  let deep = $state(false);
  let query = $state("");
  let fw = $state<BoardFirmware | null>(null);
  let variant = $state<string | null>(null);
  let version = $state<string | null>(null);
  let previews = $state(false);
  let erase = $state(false);
  let flashing = $state(false);
  let ports = $state<Endpoint[]>([]);
  let localFile = $state<string | null>(null);

  const sel = $derived(firmwareUi.selected);
  const builds = $derived(fw?.variants.find((v) => v.variant === variant)?.builds.filter((b) => previews || !b.preview) ?? []);
  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!index || q.length < 2) return [];
    return index.boards.filter((b) => b.id.toLowerCase().includes(q) || b.name.toLowerCase().includes(q) || b.vendor.toLowerCase().includes(q)).slice(0, 30);
  });

  // Scan once when the panel opens, then quietly every few seconds so boards
  // appear and disappear as they are plugged in. Deep (ESP ROM) probing resets
  // boards, so it only ever runs on explicit request.
  onMount(() => {
    api.firmwareIndex().then((i) => (index = i)).catch((e) => notifyError(e, "Firmware index"));
    detect(false);
    const timer = setInterval(() => detect(true), 3000);
    return () => clearInterval(timer);
  });

  $effect(() => {
    const id = sel?.board_id;
    fw = null;
    if (id) {
      api.firmwareBoard(id).then((f) => {
        fw = f;
        variant = firmwareUi.variant ?? f.variants[0]?.variant ?? null;
        version = null;
      }).catch((e) => notifyError(e));
    }
  });

  let scanInFlight = false;

  async function detect(quiet: boolean) {
    if (scanInFlight) return;
    scanInFlight = true;
    if (!quiet) detecting = true;
    try {
      const found = await api.firmwareDetect(deep && !quiet);
      // A quiet scan never drops ESP chips found by an explicit deep probe.
      candidates = quiet && deep ? mergeDeep(found) : found;
      ports = await api.discover("serial");
      const cur = firmwareUi.selected;
      if (cur && cur.source !== "Manual" && !candidates.some((c) => sameCandidate(c, cur))) firmwareUi.selected = null;
      if (!firmwareUi.selected && candidates[0]) firmwareUi.select(candidates[0]);
    } catch (e) {
      if (!quiet) notifyError(e, "Detection failed");
    } finally {
      scanInFlight = false;
      detecting = false;
    }
  }

  function mergeDeep(found: Candidate[]) {
    const probed = candidates.filter((c) => c.source === "ESP ROM" && found.some((f) => sameTarget(f, c)));
    return [...probed, ...found.filter((f) => !probed.some((p) => sameTarget(p, f)))];
  }

  function sameTarget(a: Candidate, b: Candidate) {
    return JSON.stringify(a.target) === JSON.stringify(b.target);
  }

  function sameCandidate(a: Candidate, b: Candidate) {
    return a.board_id === b.board_id && a.platform === b.platform && sameTarget(a, b);
  }

  function pickBoard(id: string, port: string | null) {
    const target = firmwareUi.selected?.target ?? (ports[0] ? { kind: "serial" as const, port: ports[0].address, vid: null, pid: null } : { kind: "dfu" as const });
    firmwareUi.select({ platform: port ?? "generic", board_id: id, confidence: 100, source: "Manual", detail: null, target, chip: null });
    query = "";
  }

  function setPort(port: string) {
    if (!sel) return;
    firmwareUi.select({ ...sel, target: port === "__volume" ? { kind: "volume", path: "", label: "" } : { kind: "serial", port, vid: null, pid: null } });
  }

  async function pickFile() {
    const f = await open({ multiple: false, title: "Choose a firmware file", filters: [{ name: "Firmware", extensions: ["bin", "uf2", "dfu", "hex", "app-bin"] }] });
    localFile = typeof f === "string" ? f : null;
  }

  async function flash() {
    if (!sel) return;
    const what = localFile ? baseName(localFile) : `MicroPython ${version ?? "(latest release)"} for ${sel.board_id}`;
    const ok = await dialogs.confirm({
      title: `Flash ${what}?`,
      message: erase
        ? "The entire flash will be erased first — all files on the board will be lost."
        : "Files on the board's filesystem are usually kept, but back up anything important.",
      confirmLabel: erase ? "Erase and flash" : "Flash",
      danger: erase,
    });
    if (!ok) return;
    flashing = true;
    const prevBoard = sel.target.kind === "session" ? boards.get(sel.target.session_id) : null;
    if (prevBoard) boards.expectDisconnect(prevBoard.id);
    for (const b of boards.list) if (sel.target.kind === "serial" && b.status.endpoint.address === sel.target.port) boards.expectDisconnect(b.id);
    try {
      const r = await api.firmwareFlash({
        session_id: sel.target.kind === "session" ? sel.target.session_id : null,
        platform: sel.platform,
        board_id: sel.board_id,
        variant,
        version,
        allow_preview: previews,
        target: sel.target,
        chip: sel.chip,
        erase,
        local_file: localFile,
      });
      notices.push("success", `Flashed ${r.version ? "MicroPython " + r.version : "firmware"} (${r.method})`);
      if (prevBoard) boards.forget(prevBoard.id);
      // Reconnect once the board re-enumerates.
      setTimeout(async () => {
        if (prevBoard) {
          const b = await boards.reconnect(prevBoard.id);
          if (b) notices.push("success", `Reconnected — running MicroPython ${b.status.info?.version ?? ""}`);
        } else if (r.port) {
          const ep = (await api.discover("serial")).find((e) => e.address === r.port);
          if (ep) await boards.connect(ep);
        }
      }, 2500);
    } catch (e) {
      notifyError(e, "Flashing failed");
    } finally {
      flashing = false;
    }
  }

  function targetLabel(c: Candidate) {
    const t = c.target;
    if (t.kind === "serial") return t.port;
    if (t.kind === "volume") return `drive ${t.label}`;
    if (t.kind === "session") return boards.get(t.session_id)?.label ?? "connected board";
    return "DFU";
  }
</script>

<div class="panel scroll">
  <div class="panel-header">Firmware<span class="spacer"></span>
    <button class="ghost" title="Scan again" onclick={() => detect(false)} disabled={detecting}><Icon name="refresh" size={14} /></button>
  </div>

  <div class="section col">
    <div class="row">
      <b class="grow">Connected boards{candidates.length ? ` (${candidates.length})` : ""}</b>
      <label class="row small" title="Identify ESP chips through their ROM bootloader (resets the board)"><input type="checkbox" bind:checked={deep} onchange={() => detect(false)} />Probe ESP chips</label>
    </div>
    {#each candidates as c}
      <button class="cand" class:active={sel === c || (sel?.board_id === c.board_id && sel?.source === c.source)} onclick={() => firmwareUi.select(c)} title="Flash this board ({c.source}, {c.confidence}% sure)">
        <Icon name={c.target.kind === "volume" ? "download" : "chip"} size={16} />
        <div class="grow">
          <div class="ellipsis">{c.board_id ?? `Unknown ${c.platform} board`}</div>
          <div class="faint small ellipsis">{c.source} · {targetLabel(c)}{c.detail ? ` · ${c.detail}` : ""}</div>
        </div>
        <span class="badge" class:ok={c.confidence >= 80}>{c.confidence}%</span>
      </button>
    {:else}
      <div class="faint small">
        {#if detecting}Scanning…{:else}No boards found. Plug one in — hold BOOT/BOOTSEL for a board without MicroPython — or choose a board below to flash a file or drive manually.{/if}
      </div>
    {/each}
    <SearchInput bind:value={query} placeholder="Find a board ({index?.boards.length ?? '…'} known)" />
    {#each matches as b}
      <button class="cand" onclick={() => pickBoard(b.id, b.port)} title="Use firmware for {b.name} ({b.id})">
        <div class="grow"><div class="ellipsis">{b.name}</div><div class="faint small">{b.vendor} · {b.id}{b.port ? ` · ${b.port}` : ""}</div></div>
      </button>
    {/each}
  </div>

  {#if sel}
    <div class="section col form">
      <div class="row"><span class="dim lbl">Board</span><b class="grow ellipsis">{fw?.name ?? sel.board_id ?? "—"}</b><span class="badge">{sel.platform}</span></div>
      {#if sel.target.kind !== "session"}
        <div class="row"><span class="dim lbl">Target</span>
          <select class="grow" value={sel.target.kind === "serial" ? sel.target.port : "__volume"} onchange={(e) => setPort(e.currentTarget.value)}>
            {#each ports as p}<option value={p.address}>{p.label} ({p.address})</option>{/each}
            <option value="__volume">Boot drive (UF2 / DAPLink)</option>
          </select>
        </div>
      {/if}
      {#if fw && !localFile}
        {#if fw.variants.length > 1}
          <div class="row"><span class="dim lbl">Variant</span>
            <select class="grow" bind:value={variant}>
              {#each fw.variants as v}<option value={v.variant}>{v.title.replace(/^Firmware\s*/, "") || "Standard"}</option>{/each}
            </select>
          </div>
        {/if}
        <div class="row"><span class="dim lbl">Version</span>
          <select class="grow" bind:value={version}>
            <option value={null}>Latest {previews ? "(including previews)" : "release"}</option>
            {#each builds as b}<option value={b.version}>{b.version} · {b.date}</option>{/each}
          </select>
        </div>
        <label class="row small"><input type="checkbox" bind:checked={previews} /> Show preview builds</label>
      {/if}
      <div class="row small">
        <button class="ghost" onclick={pickFile} title={localFile ? `Flashing ${localFile}` : "Flash a firmware file from disk instead of downloading"}><Icon name="file" size={13} />{localFile ? baseName(localFile) : "Use a local file…"}</button>
        {#if localFile}<button class="ghost" onclick={() => (localFile = null)} title="Use downloaded firmware instead"><Icon name="x" size={12} /></button>{/if}
      </div>
      <label class="row small"><input type="checkbox" bind:checked={erase} /> Erase entire flash first (removes all files)</label>
      <button class="primary flash" onclick={flash} disabled={flashing || (!sel.board_id && !localFile)} title={flashing ? "Flashing in progress" : !sel.board_id && !localFile ? "Pick a board or a firmware file first" : "Download (if needed) and flash this firmware"}>
        <Icon name="bolt" size={14} />{flashing ? "Flashing…" : "Flash MicroPython"}
      </button>
      {#if fw?.features.length}<div class="faint small">Features: {fw.features.join(", ")}</div>{/if}
    </div>
  {/if}

  <BuildSection
    platform={sel?.platform ?? null}
    boardId={sel?.board_id ?? null}
    {variant}
    versions={(fw?.variants[0]?.builds ?? []).filter((b) => !b.preview).map((b) => b.version)}
    onuse={(path) => (localFile = path)}
  />
</div>

<style>
  .cand {
    width: 100%;
    height: 38px;
    text-align: left;
    gap: 8px;
    padding: 0 8px;
    border-color: var(--border);
  }
  .cand.active {
    border-color: var(--accent);
    background: var(--bg-select);
  }
  .form .lbl {
    width: 56px;
    flex: none;
  }
  .form select {
    min-width: 0;
  }
  .flash {
    justify-content: center;
    padding: 7px;
  }
</style>
