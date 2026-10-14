// Connected boards. Each board is one backend session with its own terminal
// stream; several can be connected at once.
import { api, Channel, events, type ConnectOptions, type DeviceStatus, type Endpoint, type TermEvent } from "../ipc";
import { attempt, notices, notifyError } from "./notices.svelte";
import { settings } from "./settings.svelte";
import { runTaps } from "../runtaps";

export type TermListener = (ev: TermEvent) => void;

export class Board {
  id: string;
  status = $state<DeviceStatus>(undefined as unknown as DeviceStatus);
  running = $derived(this.status?.state === "running");
  busy = $derived(this.status?.state === "busy");
  connected = $derived(this.status?.connected ?? false);
  lastRunError = $state<string | null>(null);
  #listeners = new Set<TermListener>();
  /** Buffered output until a terminal attaches. */
  backlog: TermEvent[] = [];

  constructor(status: DeviceStatus) {
    this.id = status.session_id;
    this.status = status;
  }

  get label(): string {
    const s = this.status;
    return s.info?.machine?.split(" with ")[0] || s.endpoint.label || s.endpoint.address;
  }

  subscribe(fn: TermListener): () => void {
    this.#listeners.add(fn);
    for (const ev of this.backlog) fn(ev);
    this.backlog = [];
    return () => this.#listeners.delete(fn);
  }

  dispatch(ev: TermEvent) {
    if (ev.kind === "structured") runTaps.dispatch(this.id, ev.tag, ev.payload);
    if (this.#listeners.size === 0) {
      if (ev.kind !== "structured") this.backlog.push(ev);
      if (this.backlog.length > 2000) this.backlog.splice(0, 1000);
    }
    for (const l of this.#listeners) l(ev);
  }
}

class Boards {
  list = $state<Board[]>([]);
  activeId = $state<string | null>(null);
  active = $derived(this.list.find((b) => b.id === this.activeId) ?? null);
  connecting = $state(false);
  /** Last endpoint per board id, for auto-reconnect after flashing/reset. */
  lastEndpoint = new Map<string, { endpoint: Endpoint; options: ConnectOptions }>();

  get(id: string) {
    return this.list.find((b) => b.id === id);
  }

  async init() {
    await events.session(({ session_id, event }) => {
      const b = this.get(session_id);
      if (!b) return;
      if (event.kind === "state") b.status = { ...b.status, state: event.state, connected: event.state !== "disconnected" };
      if (event.kind === "run_finished") {
        b.lastRunError = event.error;
        const waiters = this.#runWaiters.get(b.id) ?? [];
        this.#runWaiters.delete(b.id);
        waiters.forEach((w) => w(event.error));
        if (this.#quietRuns.delete(b.id)) return;
        if (event.error && !event.error.includes("KeyboardInterrupt")) notices.push("warn", event.error);
      }
      if (event.kind === "disconnected") {
        b.status = { ...b.status, connected: false, state: "disconnected" };
        if (this.#expected.delete(b.id)) return;
        if (settings.value.autoReconnect && this.lastEndpoint.has(b.id)) void this.#autoReconnect(b);
        else this.#offerReconnect(b);
      }
      if (event.kind === "unresponsive") {
        notices.push("warn", `${b.label} isn't responding. A program may be stuck in a loop.`, {
          sticky: true,
          actions: [{ label: "Rescue…", run: () => this.onUnresponsive?.(b) }],
        });
      }
    });
  }

  #runWaiters = new Map<string, ((error: string | null) => void)[]>();
  #quietRuns = new Set<string>();

  /**
   * Resolves with the run's error (or null) when the current program on `id`
   * finishes. `quiet`: don't toast the error (the caller reports it).
   */
  waitRunFinished(id: string, { quiet = false } = {}): Promise<string | null> {
    if (quiet) this.#quietRuns.add(id);
    return new Promise((resolve) => {
      const list = this.#runWaiters.get(id) ?? [];
      list.push(resolve);
      this.#runWaiters.set(id, list);
    });
  }

  /** Called when a board stops responding (wired to the rescue flow by the app). */
  onUnresponsive: ((b: Board) => void) | null = null;
  #expected = new Set<string>();

  /** Don't auto-reconnect when this board drops (e.g. it's about to be flashed). */
  expectDisconnect(id: string) {
    this.#expected.add(id);
  }

  #offerReconnect(b: Board) {
    notices.push("info", `${b.label} disconnected`, {
      actions: [{ label: "Reconnect", run: () => this.reconnect(b.id) }],
    });
  }

  /** Wait for a board that dropped off (reset, re-enumeration, Wi-Fi blip) to come back. */
  async #autoReconnect(b: Board) {
    let cancelled = false;
    const toast = notices.push("info", `${b.label} disconnected — waiting for it to come back…`, {
      sticky: true,
      actions: [{ label: "Stop waiting", run: () => (cancelled = true) }],
    });
    const last = this.lastEndpoint.get(b.id)!;
    for (let i = 0; i < 20 && !cancelled; i++) {
      await new Promise((r) => setTimeout(r, 1000));
      if (cancelled || !this.list.includes(b)) break;
      if (last.endpoint.transport === "serial" && !(await this.#serialPresent(last.endpoint))) continue;
      const again = await this.reconnect(b.id, { quiet: true });
      if (again) {
        notices.dismiss(toast);
        notices.push("success", `Reconnected to ${again.label}`);
        return;
      }
    }
    notices.dismiss(toast);
    if (!cancelled && this.list.includes(b)) this.#offerReconnect(b);
  }

  async #serialPresent(ep: Endpoint) {
    const ports = await api.discover("serial").catch(() => []);
    const serial = ep.usb?.serial_number;
    return ports.some((p) => p.address === ep.address || (!!serial && p.usb?.serial_number === serial));
  }

  async connect(endpoint: Endpoint, options: ConnectOptions = { baud: null, password: null }, quiet = false) {
    this.connecting = true;
    const channel = new Channel<TermEvent>();
    let board: Board | null = null;
    const early: TermEvent[] = [];
    channel.onmessage = (ev) => (board ? board.dispatch(ev) : early.push(ev));
    try {
      const status = await api.connect(endpoint, options, settings.value.interruptOnConnect, channel);
      // Replace an existing board on the same endpoint.
      this.list = this.list.filter((b) => b.status.endpoint.address !== endpoint.address);
      board = new Board(status);
      early.forEach((e) => board!.dispatch(e));
      this.list.push(board);
      this.activeId = board.id;
      this.lastEndpoint.set(board.id, { endpoint, options });
      return board;
    } catch (e) {
      if (!quiet) notifyError(e, "Connection failed");
      return null;
    } finally {
      this.connecting = false;
    }
  }

  async reconnect(id: string, { quiet = false } = {}) {
    const last = this.lastEndpoint.get(id);
    if (!last) return null;
    // A re-enumerated USB device may come back on a different port.
    let endpoint = last.endpoint;
    const serial = endpoint.usb?.serial_number;
    if (serial) {
      const found = (await api.discover(endpoint.transport)).find((e) => e.usb?.serial_number === serial);
      if (found) endpoint = found;
    }
    const board = await this.connect(endpoint, last.options, quiet);
    if (board) this.list = this.list.filter((b) => b.id !== id);
    return board;
  }

  async disconnect(id: string) {
    await attempt(() => api.disconnect(id), "Disconnect");
    this.list = this.list.filter((b) => b.id !== id);
    if (this.activeId === id) this.activeId = this.list[0]?.id ?? null;
  }

  async refresh(id: string) {
    const b = this.get(id);
    if (!b) return;
    const status = await attempt(() => api.identify(id), "Reading board info");
    if (status) b.status = status;
  }

  forget(id: string) {
    this.list = this.list.filter((b) => b.id !== id);
    if (this.activeId === id) this.activeId = this.list[0]?.id ?? null;
  }
}

export const boards = new Boards();
