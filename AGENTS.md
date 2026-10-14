# AGENTS.md

Guidance for coding agents and contributors working on Microbe. Read this before changing code; read [`docs/DECISIONS.md`](docs/DECISIONS.md) before changing architecture.

## Ground rules

1. **Do not create `CLAUDE.md`.** Repository guidance lives here, in `AGENTS.md`.
2. **Record decisions.** Any non-obvious architectural or UX decision goes into `docs/DECISIONS.md`, using the template at the top of that file. If you're about to reverse a recorded decision, say so explicitly and add a new entry that supersedes it.
3. **Extend through registries and data, not `if port == …`.** Port differences belong in `src-tauri/platforms/*.toml`. New behaviour goes in a registered trait implementation (see [Extension recipes](#extension-recipes)). Core code asks for *capabilities*, never port names.
4. **Reuse before you write.** There is a shared helper for most recurring patterns (see [Shared building blocks](#shared-building-blocks)). If you're writing a second copy of something, extract it instead.
5. **Keep the UI developer-grade.** The UI should be dense and neutral, like VS Code, Zed or JetBrains, not playful. Use the tokens in `src/app.css`. Avoid gradients, pill shapes and emoji.
6. **Verify, don't assume.** Run the checks below. For UI behaviour, add or extend a Playwright test in `e2e/` (WebKit = the macOS app's engine).

## Commands

| Task | Command |
|---|---|
| Install | `npm install && npm run sidecars` |
| Run the app | `npm run tauri dev` |
| Frontend types + a11y | `npm run check` |
| Frontend unit tests | `npm test` |
| UI tests (WebKit) | `npm run test:e2e` |
| Rust tests (unit + e2e vs unix port) | `cd src-tauri && cargo test` |
| Hardware/network diagnostics | `cd src-tauri && cargo test --test hardware -- --ignored --nocapture` |
| Regenerate TS bindings | `cd src-tauri && cargo test` (ts-rs exports on test) |
| Release build | `npm run tauri build` |

Before you finish a change, all of these must pass: `npm run check`, `npm test`, `npm run test:e2e`, `cargo test`, `npx vite build`.

### Build environment

- **Rust 1.88 (MSRV).** `Cargo.toml` sets `rust-version = "1.88"` and `resolver = "3"`, so Cargo picks dependency versions that build on 1.88. That is why some crates are older than the latest. Don't bump the MSRV casually; see D-012.
- **Tauri JS and Rust versions must match minor versions.** The npm `@tauri-apps/*` packages are pinned to the minor versions of the resolved Rust crates (e.g. `tauri 2.11` ↔ `@tauri-apps/api 2.11.x`). The Tauri CLI refuses to run on a mismatch. If you upgrade one side, upgrade the other.
- **macOS SDK mismatch.** On some machines the Command Line Tools' `ld` can't parse the newest SDK's `.tbd` files. `npm run tauri` (`scripts/tauri.mjs`) probes for this and falls back automatically. For raw `cargo`:
  ```sh
  export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
  export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS="-C link-arg=-isysroot -C link-arg=$SDKROOT"
  ```
- **Sidecars.** `ty` is fetched by `scripts/fetch-sidecars.mjs` into `src-tauri/binaries/` (gitignored). Tauri bundles it via `externalBin`.

## Repository map

```
src-tauri/src/
  platform/        PlatformRegistry + TOML profile schema, resolve() → ResolvedTarget
  transport/       Transport / TransportProvider traits + serial, webrepl, ble, tcp (raw/telnet/rfc2217), local (PTY)
  repl/raw.rs      raw REPL + raw-paste protocol (mirrors mpremote's pyboard.py)
  repl/session.rs  one connection: output routing, RawSession guard, run/stop/stdin
  repl/structured.rs  `\x1e<TAG> <json>` record splitter (structured run channel)
  fs.rs, sync.rs   device filesystem ops (*_in variants run inside one raw session), diff sync
  scripts.rs       device-side Python templates (py/*.py), per-profile overrides
  device_info.rs   facts read from a running board
  firmware/        index (micropython.org scraper), detect (Detector registry), flashers/ (Flasher registry, incl. ota),
                   esp32.rs (partition tables), builder.rs (Docker builds; recipes in builders.toml)
  lsp/             LanguageServerProvider + stdio bridge; stubs.rs builds custom typesheds; introspect.rs (board stubs)
  debug.rs         debugger instrumentation (py/debugger.py runs on the board)
  testing.rs       test discovery (py/testrunner.py runs on the board)
  analysis/        tree-sitter checks: LintRule + DiagnosticFilter registry (tables in analysis.toml)
  mcp/             MCP server (rmcp over HTTP); tools.rs holds the McpTool registry
  mip.rs           host-side mip installer
  cache.rs, tools.rs, progress.rs   on-disk JSON cache, HTTP/download/tool cache, progress reporting
  commands/        Tauri command handlers (thin; logic lives in the modules above)
src/
  App.svelte       shell: activity bar, sidebar, toolbar, editor, bottom panel
  lib/features/    sidebar panels; registry.ts lists them (files, device, debug, tests, packages, firmware, settings)
  lib/runtime/     live run data: heap parsing, MEM/TASKS records, which preludes a run gets
  lib/components/  Editor, FileTree, Terminal, Plotter, dialogs, tooltips, icons…
  lib/state/       stores (boards, editor, project, settings, tree, dialogs, notices, menu, theme)
  lib/editor/      setup.ts (CodeMirror config), completion.ts (ranking), lsp.svelte.ts, diagnostics.ts (merged lint state)
  lib/icons/       ui.ts (Lucide map), files.ts (Material Icon Theme resolver)
  lib/bindings/    GENERATED by ts-rs. Never edit by hand.
```

## Shared building blocks

Use these instead of re-implementing them:

| Need | Use |
|---|---|
| Raw-REPL work on a board | `let mut raw = session.raw().await?; let r = op(&mut raw).await; raw.finish_with(r).await` |
| One-shot device fs op | `fs::list/read/write/...` (session level) or `fs::*_in` inside an existing raw session |
| Device-side Python | a template in `src-tauri/py/` rendered with `scripts::render` / `render_json` |
| Cached downloads with offline fallback | `cache::cached_json(path, ttl, force, fetch)` |
| HTTP | `tools::get_text / get_bytes / get_json_opt / download_to` |
| External tool binaries | `tools::ensure(http, cache, &SPEC, progress)`, `tools::which` |
| Progress events | `progress::Reporter` (`::new` for the UI, `::with_sink` elsewhere), `progress::none` |
| Blocking I/O transports | `transport::spawn_reader`, `transport::blocking` |
| Frontend error toasts | `attempt(() => …, "What failed")` from `state/notices.svelte.ts` |
| Confirm / prompt | `dialogs.confirm(...)` / `dialogs.prompt(...)` (never `window.confirm/prompt`) |
| File trees | `TreeModel` (state/tree.svelte.ts) with a `TreeBackend` + `<FileTree>` |
| Paths, sizes, theme colours | `util/path.ts`, `util/format.ts`, `util/theme.ts` (`cssVar`, `onThemeChange`) |
| Icons | `<Icon name>` (Lucide, `icons/ui.ts`), `<FileIcon name isDir open>` (Material) |
| Search fields | `<SearchInput bind:value>` |
| Run a bundled device script and get its output | `api.script(boardId, "heap", args)` (Rust: `device_script`) |
| Run a program with helpers | `Prelude { script, args, cleanup }`; `Session::run_with(code, preludes, cleanups)` |
| Wait for a program to end (frontend) | `boards.waitRunFinished(id, { quiet })` |
| Open a file at a line | `editor.openLocalAt(path, line)` / `editor.pendingLine` |
| Parse Python on the host | `analysis::parse(source)` (tree-sitter) |
| Run a snippet with a deadline | `session.exec_capture(code, limit)` (interrupts on timeout, returns stdout/stderr) |
| Recent terminal output | `session.output.lock().unwrap().since(pos)` |
| Editor diagnostics | `publish` in `editor/diagnostics.ts` (never `setDiagnostics` directly) |

## Extension recipes

### Add a MicroPython port
Create `src-tauri/platforms/<id>.toml`. The schema is `PlatformProfile` in `platform/mod.rs`, and `rp2.toml` / `esp32.toml` are good templates. Set `sys_platform` (as reported by `sys.platform`), `usb` ids, `capabilities`, `firmware_ext`, `[[flash]]` methods (by flasher id), `stubs.port`, `fs_root`, and optional `[[detect]]` hints and `[scripts]` overrides. Users can also drop profiles into `<app-data>/platforms/`, with no rebuild needed.

### Add a transport
Implement `TransportProvider` + `Transport` in `src-tauri/src/transport/<name>.rs` and register it in `TransportRegistry::with_defaults`. Override `interrupt`, `hard_reset` and `max_write` when the link differs, and `file_transfer` for an out-of-band file protocol. The connect dialog picks the transport up automatically; add an icon for its id in `ConnectDialog.svelte`'s `ICONS`.

### Add a flasher or detector
Implement `Flasher` (`firmware/flashers/`) or `Detector` (`firmware/detect.rs`) and register it in the matching `with_defaults`. A flasher is selected when a profile's `[[flash]] method` equals its `id()`.

### Add a language server
Implement `LanguageServerProvider` in `lsp/mod.rs` (command, plus the reply to `workspace/configuration` given a `StubSet`) and add it to `LspManager::new`. It appears in Settings automatically.

### Add a MicroPython check
For a CPython-only module or attribute, unsupported syntax, or a decorator whose body MicroPython compiles itself, add a row to `src-tauri/analysis.toml`. For a pin map, add `[[pins]]` to the port's profile. For new logic, implement `LintRule` (findings) or `DiagnosticFilter` (regions where language-server diagnostics are dropped) in `src-tauri/src/analysis/` and register it in `Analyzer::with_defaults`. It shows up in Settings → Code checks automatically. Test it in `analysis/mod.rs`.

### Add an MCP tool
Implement `McpTool` in `src-tauri/src/mcp/tools.rs` and add it to `ToolRegistry::with_defaults`. Return `true` from `mutates` if it runs code or changes the board (read-only mode hides it). Resolve the board with `session(host, args)` and device paths with `path_arg`. Call `host.activity(...)` with the path when it changes files. Cover it in `src-tauri/tests/mcp.rs`.

### Add a Tauri command
Write a thin `#[tauri::command]` that calls into a module, register it in `lib.rs`'s `generate_handler!`, and add a typed wrapper to `src/lib/ipc.ts`. Types crossing the boundary derive `TS` with `#[ts(export)]`. Annotate `u64` fields with `#[ts(type = "number")]`, because JSON carries plain numbers. Run `cargo test` to regenerate `src/lib/bindings/`.

### Add a sidebar feature
Create `src/lib/features/<name>/<Name>Panel.svelte` and add it to `features` in `features/registry.ts`, with `requires: [...]` capabilities if it needs specific board support.

### Add a runtime feature on the structured channel
Write a device-side prelude in `src-tauri/py/<name>.py` that prints `\x1e<TAG> <json>` lines. If it leaves anything behind (timers, wrapped functions, globals), add `<name>_end.py` and pass it as `cleanup`. Run it with `api.run(boardId, code, [{ script: "<name>", args, cleanup: "<name>_end" }])` (or add it to `runtime.preludes()` behind a setting), and subscribe in the frontend with `runTaps.on("<TAG>", (boardId, payload) => …)`. Test it on the unix port in `src-tauri/tests/runtime.rs`.

### Add a firmware build recipe
Add a `[[builder]]` to `src-tauri/builders.toml`: `ports`, `image` (a Docker Hub image, or `local:<name>` with an `[images.<name>]` Dockerfile), `make_args`, `build_dir`, `c_modules` and `artifacts`. For ESP-IDF, add `[[builder.idf]]` rows when MicroPython moves to a new IDF. `builder.rs` tests check that the plan parses.

### Add a UI icon
Import it from `@lucide/svelte/icons/<kebab-name>` and add an entry to `UI_ICONS` in `src/lib/icons/ui.ts`. `npm test` fails if the import doesn't resolve.

## UI conventions (enforced or relied upon)

- **Every `<button>` has a hover hint**: a `title` (or `data-tip`), plus `data-tip-key` for a shortcut. `src/lib/ui-lint.test.ts` fails otherwise. Disabled buttons should explain *why* they're disabled.
- **No `window.prompt` / `confirm` / `alert`.** The Tauri webview doesn't implement them: they silently return null or false. Use `dialogs`.
- **Create and rename inline** in trees (TreeModel), not via dialogs.
- **Selection is blue** everywhere (`--sel-bg`, `--sel-bg-inactive`, `--sel-match`). CodeMirror's defaults are overridden in `editor/setup.ts`.
- **Editor tooltips mount on `<body>`** and are bounded by the window (`editor/setup.ts`, `app.css`).
- **Completion ranking:** `_private`, `__mangled` and `__dunder__` names sink to the bottom (`editor/completion.ts`).
- **Density:** 22px rows, 12px UI text, 3px radii; actions on panel headers appear on hover (`.panel-header .actions`).

## Gotchas

- **Dev hot-reload keeps old editor state.** Each tab caches its `EditorState`, extensions and theme included. After changing `editor/setup.ts`, reopen tabs or restart the app before judging the result.
- **The unix port only interrupts on SIGINT.** The local transport's `interrupt()` sends a signal; boards get Ctrl-C bytes. Use `Transport::interrupt`, not a raw `\x03` write, to stop programs.
- **Boot-volume detection** must ignore system volumes (macOS `Preboot`…). Use `firmware::detect::boot_volumes()` / `method_accepts()`.
- **The LSP starts lazily**, on the first opened file, and launches are generation-guarded. Don't start it at app launch.
- **One owner for lint state.** `setDiagnostics` replaces all diagnostics, so the LSP and the analysis engine both go through `editor/diagnostics.ts` (D-020).
- **The debugger instruments only the debugged file**, inside existing lines. Never insert lines (tracebacks must keep their numbers); see `debug.rs` tests.
- **Firmware build inputs reach a shell.** `builder::plan` validates board, variant and version; keep it that way.
- **OTA keeps the session.** Flashers with `uses_session()` get `FlashJob.session`; others get the board put in its bootloader and the session closed.
- **The MCP server is a security boundary.** Keep it loopback-only, token-checked and Origin-rejecting (D-021).
- **ts-rs bindings are generated.** Edit the Rust type, then regenerate.

## Testing map

| Layer | Where |
|---|---|
| Protocol, parsers, registries | `#[cfg(test)]` modules in `src-tauri/src/**` |
| Real device behaviour (unix port in a PTY) | `src-tauri/tests/local_device.rs` (skipped if `micropython` isn't installed) |
| MCP server over real HTTP + unix port | `src-tauri/tests/mcp.rs` (skipped if `micropython` isn't installed) |
| Debugger, test runner, asyncio, heap, board stubs, OTA (fake `esp32` module) | `src-tauri/tests/runtime.rs` (unix port) |
| TCP / Telnet / RFC 2217 (bridged to the unix port) | `src-tauri/tests/tcp.rs` |
| Docker builder orchestration (fake `docker`) | `src-tauri/tests/builder.rs`; a real build is in `tests/hardware.rs` (`--ignored`) |
| Real hardware / network (opt-in) | `src-tauri/tests/hardware.rs` (`--ignored`) |
| Frontend logic | `src/**/*.test.ts` (vitest) |
| UI behaviour in WebKit | `e2e/*.spec.ts` (Playwright; backend stubbed in `e2e/pages/app.html`, answer specific commands via `window.__stubs[cmd]` in `addInitScript`; `window.__emit(event, payload)` and `window.__channel(id, message)` fake backend events and Channel messages; `connectBoard()` / `openProject(dir)` hooks in `e2e/pages/app.ts`) |
