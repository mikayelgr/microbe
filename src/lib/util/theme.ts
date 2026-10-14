// Reading design tokens from CSS for non-CSS renderers (xterm, uPlot).

export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Calls `cb` whenever the effective theme changes. Returns an unsubscribe. */
export function onThemeChange(cb: () => void): () => void {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  mq.addEventListener("change", cb);
  const mo = new MutationObserver(cb);
  mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "style"] });
  return () => {
    mq.removeEventListener("change", cb);
    mo.disconnect();
  };
}
