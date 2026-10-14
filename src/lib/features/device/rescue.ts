// Rescue a board whose program (often main.py) never returns to the REPL.
import { api } from "../../ipc";
import { boards, type Board } from "../../state/boards.svelte";
import { dialogs } from "../../state/dialogs.svelte";
import { notices, notifyError } from "../../state/notices.svelte";

export async function rescueBoard(board: Board) {
  const opts = await dialogs.form({
    title: `Rescue ${board.label}`,
    message:
      "Microbe will send Ctrl-C repeatedly for a few seconds to break into the REPL before boot.py / main.py take over again.",
    checks: [
      { id: "reset", label: "Reset the board first", hint: "Uses the reset line when available; otherwise press RESET (or replug) when asked.", checked: true },
      { id: "disable_main", label: "Rename main.py to main.py.disabled", hint: "Stops the program from starting again at the next boot.", checked: true },
    ],
    confirmLabel: "Rescue",
  });
  if (!opts) return;
  const toast = notices.push("info", opts.reset ? "Resetting and interrupting… If nothing happens, press the board's RESET button now." : "Interrupting the board…", { sticky: true });
  try {
    let report;
    try {
      report = await api.rescue(board.id, { reset: opts.reset, disable_main: opts.disable_main, window_ms: 8000 });
    } catch (e) {
      // Native-USB boards re-enumerate on reset: reconnect and try again without resetting.
      if (!String(e).includes("closed") && !String(e).includes("not connected")) throw e;
      const again = await boards.reconnect(board.id, { quiet: true });
      if (!again) throw e;
      report = await api.rescue(again.id, { reset: false, disable_main: opts.disable_main, window_ms: 8000 });
    }
    notices.dismiss(toast);
    if (!report.reached_repl) {
      notices.push("warn", report.needs_manual_reset ? "The board didn't stop. Press its RESET button while rescuing, or hold BOOT to enter the bootloader and reflash." : "The board didn't respond to interrupts.");
      return;
    }
    notices.push("success", report.disabled.length ? `Back at the REPL. Disabled ${report.disabled.join(", ")}.` : "Back at the REPL.");
    await boards.refresh(board.id);
  } catch (e) {
    notices.dismiss(toast);
    notifyError(e, "Rescue failed");
  }
}
