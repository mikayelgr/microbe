// Update-on-connect: offer newer firmware for a connected board.
import { api } from "../../ipc";
import type { Board } from "../../state/boards.svelte";
import { notices } from "../../state/notices.svelte";
import { firmwareUi } from "./firmware.svelte";

export function compareVersions(a: string, b: string): number {
  const parse = (v: string) => v.split("-")[0].split(".").map((n) => parseInt(n, 10) || 0);
  const [x, y] = [parse(a), parse(b)];
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d) return d;
  }
  return 0;
}

const checked = new Set<string>();

export async function checkForUpdate(board: Board, openPanel: () => void) {
  const info = board.status.info;
  const target = board.status.target;
  if (!info || !target?.board_id || checked.has(board.id)) return;
  checked.add(board.id);
  const [boardId, variant] = target.board_id.split("-");
  try {
    const fw = await api.firmwareBoard(boardId);
    const v = fw.variants.find((x) => x.variant === (variant ?? null)) ?? fw.variants[0];
    const latest = v?.builds.find((b) => !b.preview);
    if (!latest || compareVersions(latest.version, info.version) <= 0) return;
    notices.push("info", `MicroPython ${latest.version} is available for ${fw.name} (you have ${info.version}).`, {
      sticky: true,
      actions: [
        {
          label: "Update…",
          run: () => {
            firmwareUi.select(
              {
                platform: target.platform,
                board_id: boardId,
                confidence: 100,
                source: "Connected board",
                detail: null,
                target: { kind: "session", session_id: board.id },
                chip: null,
              },
              variant ?? null,
            );
            openPanel();
          },
        },
      ],
    });
  } catch {
    // Offline or unknown board: nothing to offer.
  }
}
