import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const { editor } = await import("./editor.svelte");

function tab(path: string, boardId?: string) {
  const source = boardId ? ({ kind: "device", boardId, path } as const) : ({ kind: "local", path } as const);
  editor.add({ id: editor.key(source), title: path.split("/").pop()!, source, saved: "", state: null, dirty: false });
}

describe("editor tabs follow file changes", () => {
  it("closes tabs of a deleted file or folder, leaving others", () => {
    editor.tabs = [];
    tab("/p/main.py");
    tab("/p/lib/a.py");
    tab("/p/lib/sub/b.py");
    tab("/p/libx.py");
    tab("/lib/a.py", "s1"); // same path on a board: untouched by a local delete
    editor.activeId = editor.key({ kind: "local", path: "/p/lib/a.py" });

    editor.closeDeleted("/p/lib");

    expect(editor.tabs.map((t) => t.source.kind !== "scratch" && t.source.path)).toEqual(["/p/main.py", "/p/libx.py", "/lib/a.py"]);
    expect(editor.activeId).not.toBeNull();
    expect(editor.tabs.some((t) => t.id === editor.activeId)).toBe(true);

    editor.closeDeleted("/lib/a.py", "s1");
    expect(editor.tabs).toHaveLength(2);
  });

  it("retargets tabs inside a renamed folder", () => {
    editor.tabs = [];
    tab("/p/lib/a.py");
    editor.activeId = editor.tabs[0].id;
    editor.retarget("/p/lib", "/p/drivers");
    expect(editor.tabs[0].source).toMatchObject({ path: "/p/drivers/a.py" });
    expect(editor.activeId).toBe(editor.tabs[0].id);
  });
});
