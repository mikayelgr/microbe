//! Language server integration.
//!
//! The frontend's CodeMirror LSP client talks JSON-RPC through this bridge.
//! The bridge spawns the selected [`LanguageServerProvider`] over stdio, frames
//! messages (`Content-Length`), and answers `workspace/configuration` itself so
//! the server sees the MicroPython stubs without touching the user's project.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex as AsyncMutex;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::state::AppState;

pub mod introspect;
pub mod stubs;

use stubs::{StubRequest, StubSet};

pub trait LanguageServerProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    /// Executable and arguments, or `None` if the server isn't installed.
    fn command(&self) -> Option<(PathBuf, Vec<String>)>;
    /// Reply to a `workspace/configuration` item for `section`.
    fn settings(&self, section: Option<&str>, stubs: Option<&StubSet>) -> Value;
}

/// Sidecar binaries sit next to the app executable; in dev they are in
/// `src-tauri/binaries/<name>-<target-triple>`.
pub fn sidecar(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        let p = dir.join(&exe);
        if p.is_file() {
            return Some(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
    std::fs::read_dir(dev)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{name}-"))))
}

pub struct Ty;

impl LanguageServerProvider for Ty {
    fn id(&self) -> &'static str {
        "ty"
    }
    fn label(&self) -> &'static str {
        "ty (Astral)"
    }
    fn command(&self) -> Option<(PathBuf, Vec<String>)> {
        sidecar("ty").or_else(|| crate::tools::which("ty")).map(|p| (p, vec!["server".into()]))
    }
    fn settings(&self, _section: Option<&str>, stubs: Option<&StubSet>) -> Value {
        let mut env = json!({ "python-version": "3.12" });
        if let Some(s) = stubs {
            env["typeshed"] = json!(s.typeshed);
        }
        json!({
            "configuration": {
                "environment": env,
                "src": { "exclude": [".microbe"] },
                // MicroPython code legitimately does things CPython checkers dislike.
                "rules": { "unresolved-reference": "warn" }
            },
            "diagnosticMode": "openFilesOnly",
            "inlayHints": { "variableTypes": false, "callArgumentNames": false },
            "completions": { "autoImport": true }
        })
    }
}

pub struct BasedPyright;

impl LanguageServerProvider for BasedPyright {
    fn id(&self) -> &'static str {
        "basedpyright"
    }
    fn label(&self) -> &'static str {
        "basedpyright"
    }
    fn command(&self) -> Option<(PathBuf, Vec<String>)> {
        crate::tools::which("basedpyright-langserver").map(|p| (p, vec!["--stdio".into()]))
    }
    fn settings(&self, section: Option<&str>, stubs: Option<&StubSet>) -> Value {
        let analysis = json!({
            "typeshedPaths": stubs.map(|s| vec![s.typeshed.clone()]).unwrap_or_default(),
            "typeCheckingMode": "basic",
            "diagnosticMode": "openFilesOnly",
            "reportMissingModuleSource": "none",
        });
        match section {
            Some("basedpyright.analysis") | Some("python.analysis") => analysis,
            _ => json!({ "analysis": analysis }),
        }
    }
}

pub struct Pyrefly;

impl LanguageServerProvider for Pyrefly {
    fn id(&self) -> &'static str {
        "pyrefly"
    }
    fn label(&self) -> &'static str {
        "pyrefly (Meta)"
    }
    fn command(&self) -> Option<(PathBuf, Vec<String>)> {
        crate::tools::which("pyrefly").map(|p| (p, vec!["lsp".into()]))
    }
    fn settings(&self, _section: Option<&str>, stubs: Option<&StubSet>) -> Value {
        json!({ "typeshedPath": stubs.map(|s| s.typeshed.clone()) })
    }
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LspProviderInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
}

struct Running {
    provider: Arc<dyn LanguageServerProvider>,
    stdin: Arc<AsyncMutex<ChildStdin>>,
    child: Child,
    stubs: Arc<Mutex<Option<StubSet>>>,
}

pub struct LspManager {
    providers: Vec<Arc<dyn LanguageServerProvider>>,
    running: AsyncMutex<Option<Running>>,
    /// Bumped on every start and stop, so the exit of a server we replaced or
    /// stopped on purpose isn't reported as a crash.
    generation: Arc<AtomicU64>,
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            providers: vec![Arc::new(Ty), Arc::new(BasedPyright), Arc::new(Pyrefly)],
            running: AsyncMutex::new(None),
            generation: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn register(&mut self, p: Arc<dyn LanguageServerProvider>) {
        self.providers.push(p);
    }

    fn provider(&self, id: &str) -> Option<Arc<dyn LanguageServerProvider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    pub async fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut r) = self.running.lock().await.take() {
            let _ = r.child.kill().await;
        }
    }
}

impl Default for LspManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn write_frame(stdin: &AsyncMutex<ChildStdin>, body: &str) -> Result<()> {
    let mut w = stdin.lock().await;
    w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).await?;
    w.write_all(body.as_bytes()).await?;
    w.flush().await?;
    Ok(())
}

/// Read one `Content-Length`-framed message.
pub async fn read_frame<R: tokio::io::AsyncBufRead + Unpin>(r: &mut R) -> Result<Option<String>> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let len = len.ok_or_else(|| Error::Protocol("LSP frame without Content-Length".into()))?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    String::from_utf8(buf).map(Some).map_err(|_| Error::Protocol("LSP frame is not UTF-8".into()))
}

#[tauri::command]
pub fn lsp_providers(state: State<'_, AppState>) -> Vec<LspProviderInfo> {
    state
        .lsp
        .providers
        .iter()
        .map(|p| LspProviderInfo { id: p.id().into(), label: p.label().into(), available: p.command().is_some() })
        .collect()
}

#[tauri::command]
pub async fn stubs_ensure(state: State<'_, AppState>, request: StubRequest) -> Result<StubSet> {
    let cache = state.cache_dir("stubs")?;
    let base = stubs::ensure(&state.http, &cache, &request).await?;
    // Fill in modules the published stubs don't know (custom firmware).
    let Some(session) = request.introspect_session.as_deref().and_then(|id| state.session(id).ok()) else {
        return Ok(base);
    };
    match introspect::extend(&session, &base, &cache).await {
        Ok(set) => Ok(set),
        Err(e) => {
            log::warn!("board stubs unavailable: {e}");
            Ok(base)
        }
    }
}

/// Start (or restart) the language server. Messages from the server arrive on
/// `channel` as JSON strings.
#[tauri::command]
pub async fn lsp_start(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
    stubs: Option<StubSet>,
    channel: Channel<String>,
) -> Result<()> {
    state.lsp.stop().await;
    let generation = state.lsp.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let p = state.lsp.provider(&provider).ok_or_else(|| Error::NotFound(format!("language server {provider}")))?;
    let (exe, args) = p.command().ok_or_else(|| Error::NotFound(format!("{} is not installed", p.label())))?;
    let mut child = tokio::process::Command::new(&exe)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| Error::other(format!("cannot start {}: {e}", exe.display())))?;
    let stdin = Arc::new(AsyncMutex::new(child.stdin.take().unwrap()));
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stubs = Arc::new(Mutex::new(stubs));

    // stderr → log
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await {
            log::debug!("lsp: {l}");
        }
    });

    // stdout → frontend, answering configuration requests ourselves.
    {
        let stdin = stdin.clone();
        let provider = p.clone();
        let stubs = stubs.clone();
        let current = state.lsp.generation.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_frame(&mut reader).await {
                    Ok(Some(msg)) => {
                        if let Some(reply) = intercept(&msg, provider.as_ref(), &stubs) {
                            let _ = write_frame(&stdin, &reply).await;
                        } else if channel.send(msg).is_err() {
                            break;
                        }
                    }
                    _ => break,
                }
            }
            if current.load(Ordering::SeqCst) == generation {
                let _ = app.emit("lsp", json!({ "kind": "exited" }));
            }
        });
    }

    *state.lsp.running.lock().await = Some(Running { provider: p, stdin, child, stubs });
    Ok(())
}

/// Handle server→client requests the bridge owns. Returns the reply to send.
fn intercept(msg: &str, provider: &dyn LanguageServerProvider, stubs: &Mutex<Option<StubSet>>) -> Option<String> {
    let v: Value = serde_json::from_str(msg).ok()?;
    let method = v.get("method")?.as_str()?;
    let id = v.get("id")?.clone();
    let result = match method {
        "workspace/configuration" => {
            let stubs = stubs.lock().unwrap();
            let items = v["params"]["items"].as_array().cloned().unwrap_or_default();
            Value::Array(items.iter().map(|i| provider.settings(i["section"].as_str(), stubs.as_ref())).collect())
        }
        "client/registerCapability" | "client/unregisterCapability" => Value::Null,
        "window/workDoneProgress/create" => Value::Null,
        _ => return None,
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

#[tauri::command]
pub async fn lsp_send(state: State<'_, AppState>, message: String) -> Result<()> {
    let running = state.lsp.running.lock().await;
    let r = running.as_ref().ok_or_else(|| Error::other("language server not running"))?;
    write_frame(&r.stdin, &message).await
}

/// Switch stub sets (e.g. a different board connected) without a restart.
#[tauri::command]
pub async fn lsp_set_stubs(state: State<'_, AppState>, stubs: Option<StubSet>) -> Result<()> {
    let running = state.lsp.running.lock().await;
    let Some(r) = running.as_ref() else { return Ok(()) };
    *r.stubs.lock().unwrap() = stubs.clone();
    let settings = r.provider.settings(None, stubs.as_ref());
    let note = json!({
        "jsonrpc": "2.0",
        "method": "workspace/didChangeConfiguration",
        "params": { "settings": { r.provider.id(): settings } }
    });
    write_frame(&r.stdin, &note.to_string()).await
}

#[tauri::command]
pub async fn lsp_stop(state: State<'_, AppState>) -> Result<()> {
    state.lsp.stop().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frame_roundtrip() {
        let body = r#"{"jsonrpc":"2.0","id":1,"result":null}"#;
        let raw = format!("Content-Length: {}\r\nContent-Type: x\r\n\r\n{body}Content-Length: 2\r\n\r\n{{}}", body.len());
        let mut r = BufReader::new(raw.as_bytes());
        assert_eq!(read_frame(&mut r).await.unwrap().unwrap(), body);
        assert_eq!(read_frame(&mut r).await.unwrap().unwrap(), "{}");
        assert!(read_frame(&mut r).await.unwrap().is_none());
    }

    #[test]
    fn answers_configuration_requests() {
        let stubs = Mutex::new(Some(StubSet {
            key: "k".into(),
            typeshed: "/ts".into(),
            packages: vec![],
            version: "1.29.0".into(),
        }));
        let req = r#"{"jsonrpc":"2.0","id":7,"method":"workspace/configuration","params":{"items":[{"section":"ty"}]}}"#;
        let reply: Value = serde_json::from_str(&intercept(req, &Ty, &stubs).unwrap()).unwrap();
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"][0]["configuration"]["environment"]["typeshed"], "/ts");
        assert!(intercept(r#"{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{}}"#, &Ty, &stubs).is_none());
    }
}
