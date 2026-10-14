// Toast notifications and long-running progress.
import type { ProgressEvent } from "../ipc";

export type NoticeKind = "info" | "success" | "warn" | "error";
export interface NoticeAction {
  label: string;
  run: () => void;
}
export interface Notice {
  id: number;
  kind: NoticeKind;
  text: string;
  actions?: NoticeAction[];
  sticky?: boolean;
}

let nextId = 1;

class Notices {
  list = $state<Notice[]>([]);
  progress = $state<Record<string, ProgressEvent>>({});

  push(kind: NoticeKind, text: string, opts: { actions?: NoticeAction[]; sticky?: boolean } = {}) {
    const n: Notice = { id: nextId++, kind, text, ...opts };
    this.list.push(n);
    if (!n.sticky) setTimeout(() => this.dismiss(n.id), kind === "error" ? 9000 : 5000);
    return n.id;
  }

  dismiss(id: number) {
    this.list = this.list.filter((n) => n.id !== id);
  }

  onProgress(e: ProgressEvent) {
    if (e.finished) {
      const { [e.id]: _, ...rest } = this.progress;
      this.progress = rest;
    } else {
      this.progress = { ...this.progress, [e.id]: e };
    }
  }
}

export const notices = new Notices();

export function notifyError(e: unknown, prefix = "") {
  const msg = typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
  notices.push("error", prefix ? `${prefix}: ${msg}` : msg);
}

/**
 * Run an action, reporting failures as a toast. Resolves to the result, or
 * `undefined` if it failed. Use for user-triggered actions.
 */
export async function attempt<T>(fn: () => Promise<T>, what = ""): Promise<T | undefined> {
  try {
    return await fn();
  } catch (e) {
    notifyError(e, what);
    return undefined;
  }
}
