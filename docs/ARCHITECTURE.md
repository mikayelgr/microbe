# Architecture

Microbe is a Tauri 2 app. A Rust core owns all device I/O, downloads and processes. A Svelte 5 frontend renders the UI inside the OS webview (WebKit on macOS, WebView2 on Windows, WebKitGTK on Linux). The two talk through Tauri commands, events, and `ipc::Channel` streams.

```
┌──────────────────────────── Svelte frontend ─────────────────────────────┐
│ features/*  (panels)   components/*   state/* (rune stores)   editor/*   │
│        │ invoke (typed via ipc.ts + ts-rs bindings)  ▲ events / channels │
└────────┼─────────────────────────────────────────────┼───────────────────┘
         ▼                                             │
┌──────────────────────────────── Rust core ───────────┴───────────────────┐
│ commands/*  →  AppState { platforms, transports, flashers, detectors,    │
│                           sessions: id → Session, lsp, http, paths }     │
│                                                                          │
│  Session ──► RawRepl ──► dyn Transport ◄── TransportProvider registry    │
│     │  routes device bytes: terminal ⇄ raw-REPL owner                    │
│     ├─ fs / sync / mip / device_info   (py/*.py templates via scripts)   │
│     └─ run → FollowParser → Splitter → TermEvent::{Run, Structured}      │
│                                                                          │
│  firmware: index (micropython.org) · Detector registry · Flasher reg.    │
│            esp32 partitions · OTA flasher · Docker builder (builders.toml)│
│  lsp: provider registry · stdio bridge · stubs → custom typeshed         │
│  analysis: tree-sitter → LintRule / DiagnosticFilter registry            │
│  debug: instrumentation · testing: discovery (both tree-sitter)          │
│  mcp: rmcp over HTTP (127.0.0.1, bearer token) → McpTool registry        │
│  platform: TOML profiles → ResolvedTarget { capabilities, fs_root… }     │
└──────────────────────────────────────────────────────────────────────────┘
```

## Data-driven platforms

Each MicroPython port is a TOML profile (`src-tauri/platforms/*.toml`) holding:
- USB ids and their roles (runtime, bootloader, UART bridge)
- capabilities
- firmware file extensions in preference order
- flash methods, which name registered flashers, plus their parameters (e.g. ESP offsets per chip)
- the bootloader-entry statement
- the stub package port
- the filesystem root
- serial reset sequence
- transfer chunk size
- detection hints
- script overrides
- GPIO maps (`[[pins]]`) for the pin check, selected by board id and chip

`PlatformRegistry::resolve(DeviceInfo)` merges a profile with what the running firmware reports (modules present, `_build`, `_mpy`) into a `ResolvedTarget`. The rest of the app consults that target's capabilities.

## Talking to a board

1. **Transport.** A `TransportProvider` discovers endpoints and opens a `Connection`, which is a writer (`dyn Transport`) plus a channel of incoming bytes. Serial and PTY transports use a reader thread (`spawn_reader`); WebREPL and BLE use async streams.
2. **Session.** A pump task routes every incoming chunk either to the terminal (friendly REPL) or to the `RawRepl` of whichever operation holds the session.
3. **Raw REPL.** `RawRepl` implements Ctrl‑A raw mode and raw‑paste flow control, with a fallback to plain raw mode. Behaviour mirrors `mpremote`/`pyboard.py`.
4. **Operations.** `session.raw()` returns a `RawSession` guard. File ops, sync, mip and device info run Python templates through it, then `finish_with` leaves raw REPL. If the guard is dropped early (an error), it exits raw REPL in the background.
5. **Run.** The program is sent with raw‑paste, after optional *preludes*. Preludes are helper scripts executed first in the same namespace. The `FollowParser` splits stdout from stderr. The `Splitter` lifts `\x1e<TAG> <json>` lines out of stdout as `TermEvent::Structured`, which frontend `runTaps` route to features. Keystrokes are forwarded as stdin, and Stop uses `Transport::interrupt` (Ctrl‑C on boards, SIGINT for the unix port).

Several sessions can be open at once. Every command carries a `session_id`, and events are wrapped in `SessionEnvelope { session_id, event }`.

## Files and sync

`fs.rs` provides each operation twice: a session-level function, and an `*_in` variant that reuses an open raw session for batches. Transfers use base64 chunks sized per profile (`transfer_chunk`). A transport's `FileTransferExt` (WebREPL's binary protocol) takes over when present. `sync.rs` walks the board once with SHA‑256 (or sizes, without `hashlib`) and uploads only changed files.

## Firmware

1. **Index.** `firmware/index.rs` scrapes micropython.org: the board cards, the port filter pages, and each board's page. Firmware sections and file names (`BOARD[-VARIANT]-DATE-vX.Y.Z[-preview…].ext`) become variants and builds. Results are cached for 24h with an offline fallback (`cache::cached_json`).
2. **Detect.** `Detector`s each produce candidates with a confidence: running MicroPython (`_build`), UF2/DAPLink boot volumes (removable drives or ones with `INFO_UF2.TXT` only), the ESP ROM bootloader (opt-in, since it resets the board), and USB ids.
3. **Flash.** The profile's `[[flash]]` methods are matched to the build's file types and to available flashers (espflash, esptool, volume copy, dfu‑util). The board is put in the right mode (bootloader entry, or the serial session closed), flashed, and the frontend reconnects.

## Language server

The frontend's `@codemirror/lsp-client` sends JSON‑RPC through `lsp_send`, and the bridge relays server output back over a `Channel`. The bridge answers `workspace/configuration` itself, so the server gets the stub settings and nothing is written into the user's project.

**Stubs:**
- `lsp/stubs.rs` downloads the port or board stub wheel and `micropython-stdlib-stubs` from PyPI.
- It merges them into one custom typeshed: everything goes under `stdlib/`, and `VERSIONS` is regenerated.
- Port modules must sit in the stdlib folder, because type checkers only resolve built-in names (`time`, `gc`…) from the stdlib.
- The "generic" set keeps only the modules shared by rp2 and esp32, for when no board is known.

**Lifecycle:** the server starts lazily (first opened file), starts are serialized, and generations distinguish deliberate restarts from crashes.

## Static analysis

`analysis/` parses the open file with tree-sitter-python and runs two kinds of registered checks:

- **`LintRule`s** report findings: `compat-imports` (CPython-only modules and attributes, with the MicroPython replacement), `compat-syntax` (`match`, `except*`, type aliases), and `pins` (GPIO numbers checked against the target's `[[pins]]` map: missing, input-only, or reserved for flash, PSRAM or the radio).
- **`DiagnosticFilter`s** return regions where the language server is wrong for MicroPython. `inline-code` covers `@rp2.asm_pio`, `@micropython.asm_thumb`/`asm_xtensa` and `@micropython.viper` functions, whose bodies use names MicroPython injects.

The rules' tables live in `src-tauri/analysis.toml`. Offsets are converted to UTF-16, so they are CodeMirror positions.

In the editor, `editor/diagnostics.ts` owns the lint state. It replaces lsp-client's `serverDiagnostics()` with a handler that stores the server's report. A view plugin runs the analysis (debounced) against the tab's board. Every update dispatches the merged list: server diagnostics, minus those inside a suppression region that name an injected identifier, plus the engine's findings. The analysis reruns when the active board or the enabled checks change.

## MCP server

`mcp/` serves the Model Context Protocol (rmcp, streamable HTTP, mounted in axum) when the user turns it on.

**Security:**
- It binds 127.0.0.1 only, and rmcp's `Host` check stops DNS rebinding.
- Every request needs `Authorization: Bearer <token>`. The token is in `<app-data>/mcp-token` (mode 0600) and can be regenerated.
- Requests with an `Origin` header are refused, so web pages can't call it.
- In read-only mode, tools that run code or change files are neither listed nor callable.

**Tools:** each tool is an `McpTool` in `mcp/tools.rs`. Tools call the same session APIs the UI uses (`fs::*`, `Session::run`/`stop`/`exec_capture`, `analysis::run`).
- When exactly one board is connected, tools use it by default.
- `Session::exec_capture` runs a snippet with a deadline and interrupts it if it overruns.
- `Session.output` keeps the last 64 KiB of terminal output. `read_output` reads it from a position, so agents can follow `run_program`.
- Tools that change files emit an `mcp` event, and the board's file tree reloads.

## Frontend structure

- **Stores** (`lib/state/*.svelte.ts`) hold app state as Svelte 5 runes: boards, editor tabs, project, settings, notices, dialogs, menu, theme.
- **Panels** are listed in `features/registry.ts`, optionally gated by capabilities.
- **The editor** is one `EditorView`. Each tab keeps its own `EditorState`, and Compartments swap the LSP plugin and the completion behaviour per tab.
- **Overlays:** `TooltipHost` turns every `title` into a styled tooltip, and `DialogHost` replaces the browser's prompt/confirm dialogs.
- **Icons:** Lucide for UI icons and the Material Icon Theme for file types, both from npm packages.

## Runtime features

Features that observe a running program use the structured channel. A *prelude* (`py/<name>.py`) runs in the program's namespace first and prints `\x1e<TAG> <json>` records. A prelude can name a *cleanup* script, which `Session::run_with` executes after the program ends, including after Ctrl-C, to undo it: stop a timer, unwrap a function. The frontend chooses preludes per run (`runtime.preludes()`), and `runTaps` routes the records to the feature that wants them.

| Feature | Prelude → records | Frontend |
|---|---|---|
| Heap map | `heap.py` (idle, `micropython.mem_info(1)`) | `runtime/heap.ts`, Memory tab |
| Heap sampling | `mem_watch.py` / `mem_watch_end.py` (a `machine.Timer`) → `MEM` | Memory tab chart |
| asyncio inspector | `aio_watch.py` / `aio_watch_end.py` (wraps `create_task`) → `TASKS` | Tasks tab |
| Test runner | `testrunner.py` / `testrunner_end.py` → `TEST`, `TESTS` | Tests panel (`state/tests.svelte.ts`) |
| Debugger | `debugger.py` → `DBG`; commands on stdin | Debug panel, breakpoint gutter |

**Tests.** `testing.rs` finds tests on the host with tree-sitter: `test_*` functions and `Test*`/`TestCase` methods, in source order. Each file runs as one program, followed by `__mb_tests.run(globals(), names)`. The project is synced first by default.

**Debugger.** Stock firmware has no `sys.settrace`, so `debug.rs` instruments the file instead. Every statement gets a hook placed on its own line: `hook; stmt`, or inside a header as `if (hook) or (cond):`. Line numbers and tracebacks are therefore unchanged. Inside functions, `locals()` is the globals on MicroPython, so locals are passed as `("x", lambda: x)` pairs, which are only created when the debugger stops. The call stack is tracked from function ids and entry hooks.

While stopped, the device reads one-line commands from stdin: `c`, `n`, `s`, `o`, `b <lines>`, `e <expr>`. While running, it polls stdin at each hook, so `p` (pause) and breakpoint changes work without a stop.

## Network transports

`transport/tcp.rs` registers three providers:

- **`tcp`:** a raw socket.
- **`telnet`:** option negotiation, IAC escaping, CR-NUL handling outside binary mode, and the WiPy-style login prompt.
- **`rfc2217`:** Telnet plus COM-PORT-OPTION. It sets the baud rate, data bits, parity and stop bits, and drives DTR/RTS. That gives a remote serial port the same `esp_classic` hard-reset sequence as a local one.

## Stubs from the board

When a board is connected, `stubs_ensure` can be given its session (`introspect_session`).

1. `help("modules")` lists the board's built-in and frozen modules.
2. Modules missing from the published stub set's `VERSIONS` are described by `py/introspect.py`: names, kinds, classes and their members.
3. `lsp/introspect.rs` writes loose `.pyi` files (`*args, **kwargs -> Any`, plus a module `__getattr__`) into a copy of the typeshed.

The result is cached per stub set, firmware build and module list. Modules whose import has side effects (`boot`, `main`, `*_setup`…) are skipped.

## ESP32 partitions and OTA

`firmware/esp32.rs` reads partition tables from the running board (`esp32.Partition`) or from the binary table in an image, and exports ESP-IDF CSV. `flashers/ota.rs` is a `Flasher` that returns `uses_session() == true`, so the flash flow keeps the session open instead of rebooting into a bootloader. It works like this:

1. Check the app image's magic byte, and its chip id against the profile's `chip_ids`.
2. Write it in 4 KiB blocks to the next OTA partition with `writeblocks`.
3. Read the image back and compare SHA-256 hashes.
4. Call `set_boot()` and reset the board.

A full `.bin` image is accepted too; its app is located through the image's own partition table. Requests choose the method with `FlashRequest.method = "esp32_ota"`. After reconnecting, the UI can call `mark_app_valid_cancel_rollback()`.

## Firmware builder

`firmware/builder.rs` turns a `BuildRequest` into a `BuildPlan`: the image, the `docker run` arguments, and a shell script. Recipes live in `builders.toml`: images (Docker Hub, or Microbe's own built from a Dockerfile), make arguments, build directory, artifacts, and ESP-IDF versions by MicroPython version. One container does all the work:

1. Clones the tag into `<cache>/build/src/`, where it stays cached.
2. Builds `mpy-cross` and the submodules.
3. Builds the firmware, optionally with a generated `FROZEN_MANIFEST` (the board's own manifest plus the project's modules) and `USER_C_MODULES`.
4. Copies the artifacts to `<cache>/build/out/<id>/`.

Output streams to the UI line by line, and cancelling kills the container. A built file is flashed through the normal flow as a local file. ESP32 builds also produce `firmware.app-bin` for OTA.
