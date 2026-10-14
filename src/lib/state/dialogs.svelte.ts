// In-app modal dialogs. The webview doesn't implement window.prompt/confirm,
// so every confirmation and text prompt goes through here.

interface Base {
  title: string;
  message?: string;
  confirmLabel?: string;
  danger?: boolean;
}
export type ConfirmOptions = Base;
export interface PromptOptions extends Base {
  value?: string;
  placeholder?: string;
  /** Return an error message to keep the dialog open. */
  validate?: (value: string) => string | null;
}

export interface FormOptions extends Base {
  checks: { id: string; label: string; hint?: string; checked: boolean }[];
}

export type DialogRequest =
  | ({ kind: "confirm"; resolve: (ok: boolean) => void } & ConfirmOptions)
  | ({ kind: "prompt"; resolve: (value: string | null) => void } & PromptOptions)
  | ({ kind: "form"; resolve: (values: Record<string, boolean> | null) => void } & FormOptions);

class Dialogs {
  current = $state<DialogRequest | null>(null);

  confirm(opts: ConfirmOptions): Promise<boolean> {
    this.close(null);
    return new Promise((resolve) => (this.current = { kind: "confirm", ...opts, resolve }));
  }

  prompt(opts: PromptOptions): Promise<string | null> {
    this.close(null);
    return new Promise((resolve) => (this.current = { kind: "prompt", ...opts, resolve }));
  }

  /** A confirmation with checkboxes; resolves to their values, or null if cancelled. */
  form(opts: FormOptions): Promise<Record<string, boolean> | null> {
    this.close(null);
    return new Promise((resolve) => (this.current = { kind: "form", ...opts, checks: opts.checks.map((c) => ({ ...c })), resolve }));
  }

  close(result: boolean | string | Record<string, boolean> | null) {
    const c = this.current;
    this.current = null;
    if (!c) return;
    if (c.kind === "confirm") c.resolve(result === true);
    else if (c.kind === "prompt") c.resolve(typeof result === "string" ? result : null);
    else c.resolve(result && typeof result === "object" ? result : null);
  }
}

export const dialogs = new Dialogs();

/** Validates a single file or folder name. */
export function validateFileName(name: string): string | null {
  if (!name.trim()) return "Enter a name";
  if (/[\\/:*?"<>|]/.test(name)) return "Names can't contain / \\ : * ? \" < > |";
  if (name === "." || name === "..") return "Invalid name";
  return null;
}
