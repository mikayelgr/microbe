// The effective light/dark theme, reactive.
import { onThemeChange } from "../util/theme";

function isDark(): boolean {
  const forced = document.documentElement.dataset.theme;
  if (forced) return forced === "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

class Theme {
  dark = $state(typeof document !== "undefined" ? isDark() : true);

  constructor() {
    if (typeof document !== "undefined") onThemeChange(() => (this.dark = isDark()));
  }
}

export const theme = new Theme();
