<script lang="ts">
  // ESP32 partition table and over-the-air updates for the connected board.
  import { open, save } from "@tauri-apps/plugin-dialog";
  import Icon from "../../components/Icon.svelte";
  import { api, type PartitionTable } from "../../ipc";
  import { boards, type Board } from "../../state/boards.svelte";
  import { dialogs } from "../../state/dialogs.svelte";
  import { attempt, notices } from "../../state/notices.svelte";
  import { formatBytes } from "../../util/format";
  import { baseName } from "../../util/path";

  let { board }: { board: Board } = $props();
  let open_ = $state(false);
  let table = $state<PartitionTable | null>(null);
  /** Showing a firmware file's table instead of the board's. */
  let imageName = $state<string | null>(null);
  let loading = $state(false);
  let updating = $state(false);
  let confirmNew = $state(true);
  const blocked = $derived(board.running ? "Stop the program first" : board.busy ? "The board is busy" : null);
  const ota = $derived(!imageName && table?.next_update ? table.next_update : null);

  async function load() {
    loading = true;
    imageName = null;
    const t = await attempt(() => api.esp32Partitions(board.id), "Reading partitions");
    loading = false;
    if (t) table = t;
  }

  function toggle() {
    open_ = !open_;
    if (open_ && !table && !blocked) void load();
  }

  async function inspectImage() {
    const f = await open({ multiple: false, title: "Firmware image to inspect", filters: [{ name: "ESP32 firmware", extensions: ["bin"] }] });
    if (typeof f !== "string") return;
    const t = await attempt(() => api.esp32ImagePartitions(f), "Reading the image");
    if (t) [table, imageName] = [t, baseName(f)];
  }

  async function exportCsv() {
    if (!table) return;
    const dest = await save({ defaultPath: "partitions.csv", title: "Save partition table" });
    if (dest && (await attempt(() => api.projectWrite(dest, table!.csv), "Saving CSV")) !== undefined) notices.push("success", `Saved ${baseName(dest)}`);
  }

  async function update(fromFile: boolean) {
    if (!ota) return;
    let local: string | null = null;
    if (fromFile) {
      const f = await open({ multiple: false, title: "ESP32 app image", filters: [{ name: "ESP32 app image", extensions: ["app-bin", "bin"] }] });
      if (typeof f !== "string") return;
      local = f;
    }
    const build = board.status.info?.build ?? "";
    const [boardId, ...variant] = build.split("-");
    const what = local ? baseName(local) : `the latest MicroPython for ${build || "this board"}`;
    const ok = await dialogs.confirm({
      title: `Update over the air?`,
      message: `${what} will be written to ${ota} and the board restarted from it. Files are kept, and the running firmware stays in place until the new one is verified.`,
      confirmLabel: "Update",
    });
    if (!ok) return;
    updating = true;
    const id = board.id;
    boards.expectDisconnect(id);
    const r = await attempt(
      () =>
        api.firmwareFlash({
          session_id: id,
          platform: board.status.target?.platform ?? "esp32",
          board_id: boardId || null,
          variant: variant.length ? variant.join("-") : null,
          version: null,
          allow_preview: false,
          target: { kind: "session", session_id: id },
          chip: null,
          erase: false,
          local_file: local,
          method: "esp32_ota",
        }),
      "OTA update",
    );
    updating = false;
    if (!r) return;
    notices.push("success", `Updated ${r.version ? `to MicroPython ${r.version} ` : ""}over the air; the board is restarting`);
    table = null;
    // Reconnect once it's back, then keep the new firmware.
    for (let i = 0; i < 10; i++) {
      await new Promise((res) => setTimeout(res, 1500));
      const b = await boards.reconnect(id, { quiet: true });
      if (!b) continue;
      if (confirmNew) {
        await api.script(b.id, "ota_valid").catch(() => {}); // fails harmlessly when rollback isn't enabled
      }
      notices.push("success", `Running MicroPython ${b.status.info?.version ?? ""}`);
      return;
    }
    notices.push("warn", "The board didn't come back yet; reconnect it when it's ready");
  }
</script>

<div class="panel-header">
  <button class="ghost head" onclick={toggle} title={open_ ? "Hide partitions" : "Show partitions and OTA updates"}>
    <span class="twisty" class:open={open_}><Icon name="chevron" size={12} /></span>ESP32 partitions
  </button>
  <span class="spacer"></span>
  {#if open_}
    <div class="actions">
      <button class="ghost" title={blocked ?? "Read the board's partition table"} disabled={!!blocked || loading} onclick={load}><Icon name="refresh" size={14} /></button>
      <button class="ghost" title="Show the partition table inside a firmware image" onclick={inspectImage}><Icon name="search" size={14} /></button>
      <button class="ghost" title={table ? "Save as an ESP-IDF partitions CSV" : "Nothing to export yet"} disabled={!table} onclick={exportCsv}><Icon name="download" size={14} /></button>
    </div>
  {/if}
</div>
{#if open_}
  <div class="section col">
    {#if imageName}<div class="small faint">Partition table in <span class="mono">{imageName}</span> · <button class="link" onclick={load} title="Show the board's table again">back to the board</button></div>{/if}
    {#if table}
      <table class="mono small">
        <thead><tr><th>Name</th><th>Type</th><th>Offset</th><th>Size</th></tr></thead>
        <tbody>
          {#each table.partitions as p (p.offset)}
            <tr class:running={p.label === table.running}>
              <td title={p.label === table.running ? "Running firmware" : p.label === table.next_update ? "Next OTA update goes here" : ""}>
                {p.label}{p.label === table.running ? " ●" : p.label === table.next_update ? " ○" : ""}
              </td>
              <td>{p.kind}/{p.subtype_name}</td>
              <td>0x{p.offset.toString(16)}</td>
              <td>{formatBytes(p.size)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if !imageName}
        {#if ota}
          <div class="row wrap">
            <button disabled={!!blocked || updating} onclick={() => update(false)} title={blocked ?? "Download the latest release for this board and install it over the air"}><Icon name="download" size={13} />Update to latest</button>
            <button disabled={!!blocked || updating} onclick={() => update(true)} title={blocked ?? "Install an .app-bin (or full .bin) image over the air"}><Icon name="upload" size={13} />From file…</button>
          </div>
          <label class="row small"><input type="checkbox" bind:checked={confirmNew} />Keep the new firmware once it starts (cancel rollback)</label>
        {:else}
          <div class="faint small">No OTA partitions: flash an OTA build of MicroPython over USB once to enable over-the-air updates.</div>
        {/if}
      {/if}
    {:else if loading}
      <div class="faint small">Reading…</div>
    {:else}
      <div class="faint small">{blocked ?? "Refresh to read the partition table."}</div>
    {/if}
  </div>
{/if}

<style>
  .head {
    padding: 0;
    gap: 4px;
    text-transform: inherit;
    letter-spacing: inherit;
    font: inherit;
    color: inherit;
  }
  .twisty :global(svg) {
    transition: transform 0.1s;
  }
  .twisty.open :global(svg) {
    transform: rotate(90deg);
  }
  table {
    border-collapse: collapse;
    width: 100%;
  }
  th {
    text-align: left;
    font-weight: normal;
    color: var(--text-dim);
  }
  td,
  th {
    padding: 1px 6px 1px 0;
    white-space: nowrap;
  }
  tr.running td {
    color: var(--ok);
  }
  .wrap {
    flex-wrap: wrap;
  }
  .link {
    padding: 0;
    height: auto;
    border: none;
    background: none;
    color: var(--accent);
    font-size: inherit;
  }
</style>
