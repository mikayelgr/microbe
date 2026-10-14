<script lang="ts">
  // Build MicroPython from source in Docker (src-tauri/src/firmware/builder.rs),
  // optionally freezing the project's modules and adding C modules.
  import { open } from "@tauri-apps/plugin-dialog";
  import Icon from "../../components/Icon.svelte";
  import { api, Channel, type BuilderInfo, type BuildResult } from "../../ipc";
  import { project } from "../../state/project.svelte";
  import { attempt, notices } from "../../state/notices.svelte";
  import { baseName } from "../../util/path";

  let {
    platform,
    boardId,
    variant,
    versions,
    onuse,
  }: { platform: string | null; boardId: string | null; variant: string | null; versions: string[]; onuse: (path: string) => void } = $props();

  let expanded = $state(false);
  let info = $state<BuilderInfo | null>(null);
  let port = $state("");
  let board = $state("");
  let boardVariant = $state("");
  let version = $state("");
  let freeze = $state(false);
  let cmods = $state<string | null>(null);
  let building = $state(false);
  let log = $state<string[]>([]);
  let result = $state<BuildResult | null>(null);
  let logEl = $state<HTMLPreElement>();

  // Follow the board picked above.
  $effect(() => {
    if (platform && platform !== "generic") port = platform;
    if (boardId) [board, boardVariant] = [boardId, variant ?? ""];
  });
  $effect(() => {
    if (!version && versions[0]) version = versions[0];
  });

  async function toggle() {
    expanded = !expanded;
    if (expanded && !info) info = await api.firmwareBuilders().catch((e) => ({ ports: [], docker: false, problem: String(e) }));
  }

  async function pickCmods() {
    const d = await open({ directory: true, multiple: false, title: "Folder with user C modules" });
    cmods = typeof d === "string" ? d : null;
  }

  async function build() {
    building = true;
    result = null;
    log = [];
    const channel = new Channel<string>();
    channel.onmessage = (line) => {
      log.push(line);
      if (log.length > 1500) log.splice(0, 500);
      queueMicrotask(() => logEl?.scrollTo({ top: logEl.scrollHeight }));
    };
    const r = await attempt(
      () =>
        api.firmwareBuild(
          {
            port,
            board: board.trim(),
            variant: boardVariant.trim() || null,
            version: version.trim() || "master",
            freeze_project: freeze ? project.root : null,
            c_modules: cmods,
          },
          channel,
        ),
      "Firmware build",
    );
    building = false;
    if (r) {
      result = r;
      notices.push("success", `Built ${r.artifacts.map((a) => baseName(a)).join(", ")}`);
    }
  }

  const blocked = $derived(
    !info?.docker ? (info?.problem ?? "Checking Docker…") : !port ? "Pick a port" : !board.trim() ? "Enter a board id" : null,
  );
</script>

<div class="panel-header">
  <button class="ghost head" onclick={toggle} title={expanded ? "Hide" : "Build MicroPython from source in Docker"}>
    <span class="twisty" class:open={expanded}><Icon name="chevron" size={12} /></span>Build from source
  </button>
</div>
{#if expanded}
  <div class="section col form">
    {#if info && !info.docker}<div class="small warn">{info.problem}</div>{/if}
    <div class="row"><span class="dim lbl">Port</span>
      <select class="grow" bind:value={port}>
        <option value="" disabled>Choose…</option>
        {#each info?.ports ?? [] as p}<option value={p}>{p}</option>{/each}
      </select>
    </div>
    <div class="row"><span class="dim lbl">Board</span><input class="grow mono" placeholder={port === "unix" ? "standard" : "RPI_PICO"} bind:value={board} /></div>
    <div class="row"><span class="dim lbl">Variant</span><input class="grow mono" placeholder="(none)" bind:value={boardVariant} /></div>
    <div class="row"><span class="dim lbl">Version</span>
      <input class="grow mono" list="mp-versions" placeholder="1.24.1 or master" bind:value={version} />
      <datalist id="mp-versions">{#each versions as v}<option value={v}></option>{/each}<option value="master"></option></datalist>
    </div>
    <label class="row small" title={project.root ? `Freeze the .py modules of ${project.name} (not main.py, boot.py or tests)` : "Open a project folder first"}>
      <input type="checkbox" bind:checked={freeze} disabled={!project.root} />Freeze the project's modules into the firmware
    </label>
    <div class="row small">
      <button class="ghost" onclick={pickCmods} title={cmods ? `C modules from ${cmods}` : "Add user C modules (a folder with micropython.cmake / micropython.mk)"}><Icon name="folder" size={13} />{cmods ? baseName(cmods) : "C modules…"}</button>
      {#if cmods}<button class="ghost" onclick={() => (cmods = null)} title="Don't add C modules"><Icon name="x" size={12} /></button>{/if}
    </div>
    <div class="row">
      {#if building}
        <button class="grow" onclick={() => api.firmwareBuildCancel()} title="Stop the build"><Icon name="stop" size={13} />Cancel build</button>
      {:else}
        <button class="primary grow" onclick={build} disabled={!!blocked} title={blocked ?? "Build in Docker; the first build downloads the toolchain and source"}><Icon name="hammer" size={13} />Build</button>
      {/if}
    </div>
    {#if result}
      {#each result.artifacts as a (a)}
        <div class="row small artifact">
          <span class="mono ellipsis grow" title={a}>{baseName(a)}</span>
          <button onclick={() => onuse(a)} title="Flash this build with the form above">Use for flashing</button>
        </div>
      {/each}
    {/if}
    {#if log.length}
      <pre class="log mono" bind:this={logEl}>{log.join("\n")}</pre>
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
  .lbl {
    width: 56px;
    flex: none;
  }
  .warn {
    color: var(--warn);
  }
  .log {
    margin: 0;
    max-height: 220px;
    overflow: auto;
    font-size: 11px;
    padding: 4px 6px;
    background: var(--bg-sunken);
    border: 1px solid var(--border);
    white-space: pre-wrap;
    user-select: text;
  }
</style>
