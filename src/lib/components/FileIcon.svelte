<script lang="ts">
  // A file/folder icon from the Material Icon Theme (see icons/files.ts).
  import { iconId, iconUrl } from "../icons/files";
  import { theme } from "../state/theme.svelte";

  let { name, isDir = false, open = false, size = 16 }: { name: string; isDir?: boolean; open?: boolean; size?: number } = $props();
  const src = $derived(iconUrl(iconId({ name, isDir, open, light: !theme.dark })));
</script>

<img {src} width={size} height={size} alt="" draggable="false" />

<style>
  img {
    flex: none;
    display: block;
  }
</style>
