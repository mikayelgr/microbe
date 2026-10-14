import "@xterm/xterm/css/xterm.css";
import "../../src/app.css";
import { mount } from "svelte";
import App from "../../src/App.svelte";
import { editor } from "../../src/lib/state/editor.svelte";
import { boards } from "../../src/lib/state/boards.svelte";
import { project } from "../../src/lib/state/project.svelte";

mount(App, { target: document.getElementById("app")! });

// Test hooks.
Object.assign(window, {
  openSample(text: string) {
    editor.add({ id: "local:/p/main.py", title: "main.py", source: { kind: "local", path: "/p/main.py" }, saved: text, state: null, dirty: false });
  },
  /** Open a project folder; the test stubs `project_list`. */
  openProject(dir: string) {
    return project.openFolder(dir);
  },
  /** Connect a fake board; the test stubs `device_connect`. */
  connectBoard() {
    return boards.connect({ transport: "serial", address: "/dev/fake", label: "Fake Pico", detail: null, usb: null, platform_hint: "rp2", role: null });
  },
});
