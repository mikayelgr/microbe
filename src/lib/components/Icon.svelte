<script lang="ts">
  // A UI icon by name (see icons/ui.ts).
  import { UI_ICONS, type UiIconName } from "../icons/ui";

  let { name, size = 16, title = undefined }: { name: UiIconName | (string & {}); size?: number; title?: string } = $props();
  const Glyph = $derived(UI_ICONS[name as UiIconName] ?? UI_ICONS.file);
</script>

<Glyph {size} strokeWidth={1.75} aria-hidden={title ? undefined : "true"} aria-label={title} class="icon" />

<style>
  :global(svg.icon) {
    flex: none;
  }
</style>
