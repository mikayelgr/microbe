<script lang="ts">
  // App-wide hover hints. Any element with `title` (or `data-tip`) gets a
  // styled tooltip; `data-tip-key` adds a shortcut. Native title tooltips are
  // suppressed by moving `title` into `data-tip` on first hover. Pointer
  // tracking uses elementFromPoint so disabled buttons (which WebKit doesn't
  // send mouse events to) can still explain why they're disabled.
  import { onMount, tick } from "svelte";

  const SHOW_DELAY = 450;
  const WARM_MS = 400; // move between controls quickly → show instantly

  let text = $state<string | null>(null);
  let key = $state<string | null>(null);
  let x = $state(0);
  let y = $state(0);
  let placeAbove = $state(false);
  let box: HTMLDivElement | null = $state(null);

  let current: Element | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let hiddenAt = 0;

  /** Find the hinted element at a point, migrating `title` → `data-tip`. */
  function hinted(el: Element | null): HTMLElement | null {
    const target = el?.closest<HTMLElement>("[title], [data-tip]") ?? null;
    if (!target || target.closest(".cm-editor, .xterm")) return null;
    const t = target.getAttribute("title");
    if (t !== null) {
      target.dataset.tip = t;
      target.removeAttribute("title");
      // Keep an accessible name for icon-only controls.
      if (!target.getAttribute("aria-label") && !target.textContent?.trim()) target.setAttribute("aria-label", t);
    }
    return target.dataset.tip ? target : null;
  }

  function hide() {
    clearTimeout(timer);
    if (text) hiddenAt = performance.now();
    text = null;
    current = null;
  }

  async function show(target: HTMLElement) {
    text = target.dataset.tip ?? null;
    key = target.dataset.tipKey ?? null;
    if (!text) return;
    const r = target.getBoundingClientRect();
    x = r.left + r.width / 2;
    y = r.bottom + 6;
    placeAbove = false;
    await tick();
    if (!box) return;
    const b = box.getBoundingClientRect();
    if (y + b.height > window.innerHeight - 4) {
      placeAbove = true;
      y = r.top - 6 - b.height;
    }
    x = Math.min(Math.max(x, b.width / 2 + 6), window.innerWidth - b.width / 2 - 6);
  }

  function hover(target: HTMLElement | null) {
    if (target === current) return;
    hide();
    current = target;
    if (!target) return;
    const warm = performance.now() - hiddenAt < WARM_MS;
    timer = setTimeout(() => current === target && show(target), warm ? 0 : SHOW_DELAY);
  }

  onMount(() => {
    let frame = 0;
    let px = 0;
    let py = 0;
    const move = (e: PointerEvent) => {
      px = e.clientX;
      py = e.clientY;
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        hover(hinted(document.elementFromPoint(px, py)));
      });
    };
    const focus = (e: FocusEvent) => {
      const el = e.target as HTMLElement;
      if (el?.matches?.(":focus-visible")) {
        const t = hinted(el);
        if (t) (hover(t), clearTimeout(timer), show(t));
      }
    };
    const leave = (e: PointerEvent) => !e.relatedTarget && hide();
    const opts = { capture: true, passive: true };
    document.addEventListener("pointermove", move, opts);
    document.addEventListener("pointerdown", hide, opts);
    document.addEventListener("keydown", hide, opts);
    document.addEventListener("wheel", hide, opts);
    document.addEventListener("focusin", focus, true);
    document.addEventListener("focusout", hide, true);
    document.documentElement.addEventListener("pointerleave", leave);
    window.addEventListener("blur", hide);
    return () => {
      document.removeEventListener("pointermove", move, opts);
      document.removeEventListener("pointerdown", hide, opts);
      document.removeEventListener("keydown", hide, opts);
      document.removeEventListener("wheel", hide, opts);
      document.removeEventListener("focusin", focus, true);
      document.removeEventListener("focusout", hide, true);
      document.documentElement.removeEventListener("pointerleave", leave);
      window.removeEventListener("blur", hide);
    };
  });
</script>

{#if text}
  <div bind:this={box} class="tip" class:above={placeAbove} role="tooltip" style="left:{x}px; top:{y}px">
    <span>{text}</span>
    {#if key}<kbd>{key}</kbd>{/if}
  </div>
{/if}

<style>
  .tip {
    position: fixed;
    z-index: 90;
    transform: translateX(-50%);
    max-width: min(360px, calc(100vw - 12px));
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    font-size: 11.5px;
    line-height: 1.35;
    color: var(--text);
    background: var(--bg-elev);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: 0 4px 14px rgb(0 0 0 / 0.25);
    pointer-events: none;
    white-space: normal;
    word-break: break-word;
    animation: in 0.08s ease-out;
  }
  kbd {
    flex: none;
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--text-dim);
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    padding: 0 4px;
    line-height: 16px;
  }
  @keyframes in {
    from {
      opacity: 0;
    }
  }
</style>
