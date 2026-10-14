// A single app-wide context menu.
export interface MenuItem {
  label: string;
  icon?: string;
  danger?: boolean;
  disabled?: boolean;
  /** Hover hint; defaults to the label. */
  hint?: string;
  run: () => void;
}

class Menu {
  open = $state(false);
  x = $state(0);
  y = $state(0);
  items = $state<MenuItem[]>([]);

  show(e: MouseEvent, items: MenuItem[]) {
    e.preventDefault();
    e.stopPropagation();
    this.items = items;
    this.x = Math.min(e.clientX, window.innerWidth - 200);
    this.y = Math.min(e.clientY, window.innerHeight - items.length * 30 - 10);
    this.open = true;
  }

  close() {
    this.open = false;
  }
}

export const menu = new Menu();
