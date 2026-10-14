// Firmware panel selection, shared so other features (update-on-connect) can
// pre-fill it.
import type { Candidate } from "../../ipc";

class FirmwareUi {
  selected = $state<Candidate | null>(null);
  variant = $state<string | null>(null);

  select(c: Candidate, variant: string | null = null) {
    this.selected = c;
    this.variant = variant;
  }
}

export const firmwareUi = new FirmwareUi();
