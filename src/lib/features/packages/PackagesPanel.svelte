<script lang="ts">
  // micropython-lib / GitHub packages, installed host-side with mip.
  import Icon from "../../components/Icon.svelte";
  import SearchInput from "../../components/SearchInput.svelte";
  import { api, type PackageInfo, type DeviceEntry } from "../../ipc";
  import { boards } from "../../state/boards.svelte";
  import { notices, notifyError } from "../../state/notices.svelte";

  let index = $state<PackageInfo[]>([]);
  let query = $state("");
  let custom = $state("");
  let installing = $state<string | null>(null);
  let installed = $state<DeviceEntry[]>([]);
  let mpy = $state(true);
  const board = $derived(boards.active);
  const libDir = $derived(`${(board?.status.target?.fs_root ?? "/").replace(/\/$/, "")}/lib`);

  const results = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = q ? index.filter((p) => p.name.toLowerCase().includes(q) || p.description.toLowerCase().includes(q)) : index;
    return list.slice(0, 200);
  });

  $effect(() => {
    if (!index.length) api.packagesIndex().then((i) => (index = i)).catch((e) => notifyError(e, "Package index"));
  });

  $effect(() => {
    if (board?.connected && !board.running) refreshInstalled();
  });

  async function refreshInstalled() {
    if (!board) return;
    try {
      installed = await api.fsList(board.id, libDir);
    } catch {
      installed = [];
    }
  }

  async function install(pkg: string) {
    if (!board) return notices.push("info", "Connect a board first");
    installing = pkg;
    try {
      const r = await api.packagesInstall(board.id, { package: pkg, version: null, target: null, mpy, index: null });
      notices.push("success", `Installed ${pkg}: ${r.installed.length} file(s)${r.skipped.length ? `, ${r.skipped.length} already up to date` : ""}`);
      await refreshInstalled();
    } catch (e) {
      notifyError(e, `Installing ${pkg}`);
    } finally {
      installing = null;
    }
  }
</script>

<div class="panel">
  <div class="panel-header">Packages<span class="spacer"></span>
    <button class="ghost" title="Refresh index" onclick={() => api.packagesIndex(true).then((i) => (index = i)).catch(notifyError)}><Icon name="refresh" size={14} /></button>
  </div>
  <div class="section col">
    <SearchInput bind:value={query} placeholder="Search micropython-lib" />
    <div class="row">
      <input class="grow mono" placeholder="github:org/repo or URL" bind:value={custom} onkeydown={(e) => e.key === "Enter" && custom && install(custom)} />
      <button disabled={!custom || !!installing || !board} onclick={() => install(custom)} title={!board ? "Connect a board first" : !custom ? "Enter a package, github:org/repo or URL" : `Install ${custom} on the board`}>Install</button>
    </div>
    <label class="row small"><input type="checkbox" bind:checked={mpy} /> Prefer precompiled .mpy</label>
  </div>
  <div class="list">
    {#each results as p (p.name)}
      <div class="pkg">
        <div class="grow">
          <div class="row"><b>{p.name}</b><span class="faint small">{p.version}</span></div>
          {#if p.description}<div class="dim small desc">{p.description}</div>{/if}
        </div>
        <button disabled={!board?.connected || !!installing} onclick={() => install(p.name)} title={board?.connected ? `Install ${p.name} ${p.version} into the board's lib folder` : "Connect a board first"}>
          {installing === p.name ? "…" : "Install"}
        </button>
      </div>
    {:else}
      <div class="empty">{index.length ? "No matches" : "Loading package index…"}</div>
    {/each}
  </div>
  {#if board}
    <div class="panel-header">On board <span class="faint mono" style="text-transform:none">{libDir}</span></div>
    <div class="installed">
      {#each installed as e}<span class="badge">{e.name}</span>{:else}<span class="faint small" style="padding:0 10px">Nothing installed</span>{/each}
    </div>
  {/if}
</div>

<style>
  .list {
    flex: 1;
    overflow: auto;
  }
  .pkg {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
  }
  .desc {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .installed {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    padding: 0 10px 10px;
    max-height: 120px;
    overflow: auto;
  }
</style>
