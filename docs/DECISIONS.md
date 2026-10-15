# Decision log

This log records the decisions that shape Microbe. Each entry gives the reasoning, so later changes can build on it or knowingly reverse it. **Add an entry** whenever you make a choice that someone could reasonably have made differently.

Template:

```md
## D-NNN: Title
- **Date:** YYYY-MM-DD · **Status:** accepted | superseded by D-XXX
- **Context:** the problem and constraints.
- **Decision:** what we chose.
- **Consequences:** what follows, including costs and what to watch.
```

---

## D-001: Tauri 2 instead of Electron
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The IDE should be light on RAM and disk.
- **Decision:** Tauri 2, with a Rust core and the OS webview.
- **Consequences:** The UI runs in WebKit on macOS, so behaviour must be checked in WebKit (see D-016). Some browser APIs behave differently there (see D-014).

## D-002: Svelte 5 + CodeMirror 6 + xterm.js + uPlot
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Small bundle, fast startup, good editor extensibility.
- **Decision:** Svelte 5 (runes) for the UI, CodeMirror 6 (not Monaco, which is about 5 MB), xterm.js for the REPL and uPlot for charts.
- **Consequences:** Editor features are composed from CodeMirror extensions. Per-tab behaviour is switched through Compartments.

## D-003: Port differences are data (TOML profiles), behaviour is registered traits
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The IDE must support any MicroPython port without code churn.
- **Decision:** Ports are described in `src-tauri/platforms/*.toml`. Transports, flashers, detectors and language servers are traits held in registries. Core code checks capabilities, never port names.
- **Consequences:** A new port is one file, and users can add profiles without rebuilding. Resist adding `match port {}` branches.

## D-004: Raw REPL protocol implemented natively in Rust, mirroring mpremote
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Python tooling shouldn't be a runtime dependency, but behaviour must match the reference implementation.
- **Decision:** `repl/raw.rs` reimplements raw and raw‑paste mode as `pyboard.py` does, including its fallbacks. Device-side work is done by small Python templates (`py/*.py`) that profiles can override.
- **Consequences:** No Python is needed on the host. Protocol changes in new firmware versions need handling here (a planned `quirks.toml`).

## D-005: The MicroPython unix port as a simulator and test target
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Tests and users without hardware need a real MicroPython.
- **Decision:** A `local` transport runs the unix port in a raw-mode PTY inside a sandbox folder. The end-to-end tests use it.
- **Consequences:** The unix port only interrupts on SIGINT, so `Transport::interrupt` exists. Its filesystem root is `.` (the sandbox), never `/`.

## D-006: ty as the default language server, configured through the bridge
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The language server must understand MicroPython modules, be light, and support pluggable alternatives.
- **Decision:** Astral's ty, bundled as a sidecar. basedpyright and pyrefly are optional providers. The bridge answers `workspace/configuration`, so no config files are written into user projects.
- **Consequences:** The stubs must be shaped as a custom typeshed (D-007).

## D-007: Stubs merged into a custom typeshed
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** ty resolves built-in module names (`time`, `gc`, `math`…) only from the stdlib path, so port stubs on an extra path were ignored.
- **Decision:** Port or board stubs and `micropython-stdlib-stubs` are merged into one `stdlib/` folder with a regenerated `VERSIONS` file. When no board is known, the "generic" set keeps only modules common to rp2 and esp32.
- **Consequences:** Stub sets are cached per port, board and version under the app cache.

## D-008: Stubs follow the board; the LSP starts lazily
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Assuming rp2 with no board connected was wrong, and starting the server at launch raced with project restore.
- **Decision:** Stubs come from the connected board, else the last board, else the generic set (configurable). The server starts on the first opened file, starts are serialized, and generations suppress false "crashed" notices.
- **Consequences:** The server uses no resources until you edit Python.

## D-009: Multiple simultaneous board sessions
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Other tools mix up uploads when several boards are connected.
- **Decision:** `AppState.sessions` maps ids to sessions. Every command takes a `session_id`, and events carry it.
- **Consequences:** The frontend shows one terminal and file tree per board, and a project syncs to the *active* board.

## D-010: Structured run channel and preludes
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The debugger, profilers, test runner and plotter all need machine-readable data from running code.
- **Decision:** Device helpers print `\x1e<TAG> <json>` lines. The session strips them from the output as `TermEvent::Structured`, and frontend `runTaps` route them by tag. Helper code runs as preludes in the same namespace before the user's program.
- **Consequences:** Any new runtime feature reuses this channel; don't invent another one.

## D-011: Typed IPC via ts-rs (not tauri-specta)
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** tauri-specta was still a release candidate.
- **Decision:** ts-rs exports Rust types to `src/lib/bindings/`, and `ipc.ts` holds hand-written typed wrappers.
- **Consequences:** `u64` fields need `#[ts(type = "number")]`. Bindings regenerate on `cargo test`.

## D-012: Rust 1.88 MSRV with MSRV-aware resolution; JS Tauri packages pinned to match
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The machine's toolchain is 1.88, and the newest crates need newer Rust.
- **Decision:** `rust-version = "1.88"` with resolver 3 (Cargo picks compatible crate versions). The npm `@tauri-apps/*` packages are pinned to the resolved crates' minor versions.
- **Consequences:** Upgrading Rust or Tauri means upgrading both sides together.

## D-013: Firmware from micropython.org, scraped and cached
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** There is no official JSON API, but the download pages are regular.
- **Decision:** Parse the board cards, the port filters and each board's firmware sections. Cache everything for 24h with an offline fallback. Use espflash as a library for the ESP32 family and download esptool on demand for ESP8266.
- **Consequences:** Site layout changes break parsing, and the HTML fixtures in the tests catch that. Boot-volume detection only considers removable or UF2 drives, never system volumes.

## D-014: No browser dialogs; inline tree editing; app-wide tooltips
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The webview doesn't implement `prompt`/`confirm`, so actions silently did nothing. Native `title` tooltips are unstyled and don't appear on disabled buttons.
- **Decision:** `dialogs.confirm/prompt` for modals, inline create and rename in `TreeModel`/`FileTree`, and `TooltipHost`, which turns `title` into styled hints and tracks the pointer so disabled buttons are covered too. A lint test requires a hint on every button.
- **Consequences:** Never call `window.prompt/confirm/alert`. Every new button needs a `title`.

## D-015: Developer-grade, dense visual style
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The first UI looked like a toy.
- **Decision:** A VS Code / Zed / JetBrains vocabulary:
  - 22px rows, 12px text and 3px radii
  - neutral grays with a blue accent, with green reserved for status
  - edge markers for the active item
  - Lucide UI icons and Material file icons from npm
  - blue selection everywhere
  - explorer tokens checked for contrast
- **Consequences:** Use the tokens in `app.css`; avoid gradients and pills.

## D-016: UI tests run the real frontend in WebKit with a stubbed backend
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Visual bugs (selection colour, tooltips) need checking in the engine the macOS app uses.
- **Decision:** Playwright tests in `e2e/` load the real `App.svelte` through `e2e/pages/app.html`, which stubs Tauri internals.
- **Consequences:** These tests cover UI behaviour, not backend behaviour (Rust tests cover that). Add a spec whenever a UI bug is fixed.

## D-017: Completion ranking and an autocomplete toggle
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** Python's private and dunder names cluttered the top of completion lists, and some users want completions on demand only.
- **Decision:** One ranked source (the LSP, falling back to lang-python's sources), with boosts that sink `_x`, then `__x`, then `__x__`. ⌘⇧Space toggles completion while typing; ⌃Space always works.
- **Consequences:** Because the boost is a score penalty, typing `_` still surfaces underscore names.

## D-018: Logo is an SVG source of truth
- **Date:** 2026-10-14 · **Status:** accepted
- **Context:** The first app icon was a placeholder that clashed with the developer-grade style (D-015).
- **Decision:** `docs/logo.svg` (dark tile, blue chip outline, hand-drawn "μ" as paths) is the master. The README uses it directly. `src-tauri/app-icon.png` is rendered from it at 1024px, and `npx tauri icon src-tauri/app-icon.png` generates the platform icons (delete the generated `android/` and `ios/` folders).
- **Consequences:** To change the logo, edit the SVG, re-render, and regenerate the icons. Keep the glyph as paths so it renders the same everywhere.

## D-019: Static analysis in Rust with tree-sitter; checks are data
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** The language server checks types against stubs. It can't know that `threading` should become `_thread`, that MicroPython rejects `match`, or that GPIO 34 on an ESP32 is input-only. It also reports names injected by `@rp2.asm_pio` as undefined.
- **Decision:** An `analysis` module parses with tree-sitter-python. It runs a registry of `LintRule`s (findings) and `DiagnosticFilter`s (regions where server diagnostics are dropped). Module and attribute tables, unsupported syntax and inline-code decorators live in `src-tauri/analysis.toml`. GPIO maps live in the platform profiles (`[[pins]]`, first match by board id glob and chip).
- **Consequences:** Most new checks are a table row, not code. Pin checks only run when the board (or the device tab's board) is connected and identified. A chip without a map isn't checked, rather than guessed.

## D-020: One owner for the editor's lint state
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** `@codemirror/lint`'s `setDiagnostics` replaces all diagnostics. lsp-client's `serverDiagnostics()` and a second `linter()` would overwrite each other.
- **Decision:** `editor/diagnostics.ts` replaces `serverDiagnostics()` in the client's extensions, reusing its capabilities and auto-sync. It keeps the latest server and analysis results in a state field and always dispatches the merged list. The suppression filter matches the first quoted identifier in a message, which works for ty, pyright and pyrefly.
- **Consequences:** Any new diagnostic source must go through `publish` in `diagnostics.ts`, not `setDiagnostics`.

## D-021: The MCP server is opt-in, loopback-only and token-protected
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** An MCP server lets agents run arbitrary code on hardware and overwrite files. A local HTTP port can be reached by other local processes and, through DNS rebinding or `fetch`, by web pages.
- **Decision:** It is off by default. When on, it uses streamable HTTP (rmcp + axum) on 127.0.0.1 with rmcp's loopback `Host` check. Every request needs a bearer token (`<app-data>/mcp-token`, 0600, regenerable), and requests carrying an `Origin` header are rejected. A read-only mode hides mutating tools. Tools act on the user's existing sessions rather than opening their own, so the user sees what an agent does.
- **Consequences:** Clients must send the header; the Settings panel copies a ready `claude mcp add` command and a JSON entry. Port 7357 by default, configurable. Tools that change files emit `mcp` events so the UI stays in sync. Add new tools to `ToolRegistry`, and mark them `mutates` if they run code or write.

## D-022: Runtime features are preludes with cleanups
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Heap sampling and the asyncio inspector change the program's environment (a timer, a wrapped `create_task`) in a namespace that persists after the run. Leftovers would print records into the friendly REPL, or wrap the next run twice.
- **Decision:** A `Prelude` can name a `cleanup` script, run with the same arguments. `Session::run_with` executes cleanups in raw REPL after the program ends (normally, on error, or on Ctrl-C), discarding their output. Not after a forced stop, where the board may still be busy. The user turns sampling and inspection on per feature; nothing is injected by default.
- **Consequences:** Every prelude that leaves state behind ships a matching `*_end.py`. The extra code costs a little RAM on small boards, which is why it's opt-in.

## D-023: A debugger by host-side instrumentation
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Release builds of MicroPython don't enable `sys.settrace`, and asking users to build custom firmware defeats a "works on any board" IDE.
- **Decision:** `debug.rs` rewrites the file with tree-sitter, inserting hooks *within* existing lines so line numbers survive. Locals come from per-function lambdas (MicroPython's `locals()` inside functions returns the globals). The debugger talks over the structured channel (`DBG`) and stdin. It polls stdin while running, for pause and live breakpoint changes. Only the file being debugged is instrumented.
- **Consequences:** Instrumented code runs slower and is larger. Imported modules run at full speed, but you can't step into them. Class bodies aren't hooked. Recursion makes the call-stack view approximate (the stack is tracked heuristically). Typing into the terminal while debugging can be read as debugger input.

## D-024: Network serial over TCP, Telnet and RFC 2217
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Boards behind ser2net, ESP-Link or a telnet REPL were unreachable, and remote serial ports had no reset.
- **Decision:** One module, three providers, sharing a client-side Telnet parser. RFC 2217 sets the port parameters on connect and implements `hard_reset` with SET-CONTROL, mirroring the local serial sequence. Telnet answers the WiPy login prompt with `user:password` from the password field (default user `micro`).
- **Consequences:** No discovery: addresses are typed (and remembered). A telnet server without a login prompt delays the connection by up to 3 s while Microbe waits for a prompt.

## D-025: Stubs generated from the board fill gaps only
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Custom firmware (ulab, C modules, frozen drivers) has modules no published stub package describes, so the language server reports them as missing.
- **Decision:** Introspect only the modules missing from the downloaded stub set, with loose signatures and a module-level `__getattr__`, merged into a cached copy of the set. Published stubs always win. It is on by default and can be turned off in Settings.
- **Consequences:** Generated stubs give names and completion but no real signatures. Modules whose import has side effects are skipped by name.

## D-026: ESP32 OTA as a session-based flasher
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Updating a deployed ESP32 shouldn't need USB or the ROM bootloader, and the flash flow assumed every flasher reboots the board out of MicroPython first.
- **Decision:** `Flasher::uses_session()` lets a flasher keep the session. `esp32_ota` writes through `esp32.Partition` over whatever transport is connected, verifies by hash before switching the boot partition, and is chosen only on request (`FlashRequest.method`). Chip ids for image checks are profile data.
- **Consequences:** Needs firmware with OTA partitions (the OTA builds on micropython.org). Transfer speed is that of the REPL link. Rollback confirmation is a separate, optional step after reconnecting.

## D-027: Firmware builds in Docker, recipes in builders.toml
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Building MicroPython needs per-port toolchains (ESP-IDF, arm-none-eabi, esp-open-sdk) that users rarely have installed.
- **Decision:** Build in Docker with one container per build. Images are the official ones where they exist (`espressif/idf`), otherwise small Dockerfiles in `builders.toml` that Microbe builds once. Sources are cloned inside the container into a cached volume. Frozen project modules come from a generated manifest that includes the board's own manifest first. Inputs are validated (`[A-Za-z0-9_.-]`) because they reach a shell.
- **Consequences:** Needs Docker running; the first build of a port downloads a lot. The ESP-IDF version table must follow MicroPython releases (data, not code). The orchestration is tested against a fake `docker`; a real build is an opt-in hardware test.

## D-028: Release tags rebuild only when build inputs changed
- **Date:** 2026-10-15 · **Status:** superseded by D-029
- **Context:** A full four-platform build takes a long time, and some version tags only change docs or tests.
- **Decision:** On a `v*` tag push, `.github/workflows/build.yml` diffs the tagged commit against the tag of the most recent release (drafts included) that has installers. Everything except `docs/`, `e2e/`, `src-tauri/tests/`, `*.md`, `*.test.ts`, `playwright.config.ts` and `.gitignore` counts as a build input. If any input changed, or no earlier release has installers, every platform is built as before. Otherwise the build is skipped and that release's installers are copied to a new draft release for the tag. Manual runs always build.
- **Consequences:** Carried-over installers keep the old version in their file names and in the app, which is correct because the version is only bumped by editing build inputs. If a file outside those exclusions stops affecting the build (or a new test-only path is added), update the exclusion list in the workflow and here.

## D-029: Releases are published from main when the version changes, with stable asset names
- **Date:** 2026-10-15 · **Status:** accepted
- **Context:** Releases should go public without manual steps, and a website needs download links that don't change from one release to the next. Tauri's bundle names contain the version (`Microbe_0.1.0_aarch64.dmg`), and tag-triggered builds only produced drafts.
- **Decision:** The version in `src-tauri/tauri.conf.json` is the release trigger. On every push to `main`, `.github/workflows/build.yml` checks whether release `v<version>` is already published. If it is, nothing is built. If not, the workflow creates a draft release targeting the pushed commit, builds every platform, uploads the installers, adds `SHA256SUMS.txt`, and publishes the release as latest. Publishing creates the tag. The workflow fails if `package.json`, `Cargo.toml` and `tauri.conf.json` disagree on the version. `scripts/release-assets.mjs` renames installers to version-free names (`microbe-<os>-<arch>.<ext>`), so `releases/latest/download/<name>` always serves the newest build. The `v*` tag trigger and D-028's installer carry-over are removed: under this scheme every release bumps the version, which is a build input, so a carry-over could never apply.
- **Consequences:** To release, bump the version in all three files and push to `main`. A push that doesn't bump the version builds nothing; use a manual run to build without publishing. The asset names are a public contract: renaming one breaks links on the website. A failed or cancelled run leaves a draft, and the next push reuses it and points it at the new commit.
