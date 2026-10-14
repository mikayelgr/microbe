//! MCP server: lets AI agents drive the connected boards.
//!
//! Off by default. When enabled it serves the Model Context Protocol over
//! streamable HTTP at `http://127.0.0.1:<port>/mcp`:
//! * bound to loopback only, with rmcp's loopback `Host` check (DNS rebinding);
//! * every request needs `Authorization: Bearer <token>`; the token lives in
//!   `<app-data>/mcp-token` and can be regenerated;
//! * requests carrying a browser `Origin` are rejected, so web pages can't
//!   reach it;
//! * "read-only" access hides tools that run code or change files.
//!
//! Tools are registered in [`tools::ToolRegistry`]. They use the same session
//! APIs as the UI, so an agent's work shows up in the app (busy state, terminal,
//! file trees refresh on `mcp` activity events).

pub mod tools;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Request, State as AxState};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::state::AppState;
use tools::ToolRegistry;

/// What the server needs from the app; implemented by `AppHandle` and by tests.
pub trait Host: Send + Sync + 'static {
    fn state(&self) -> &AppState;
    /// Report that an agent touched a board (the UI shows it and refreshes).
    fn activity(&self, _activity: Activity) {}
}

impl Host for AppHandle {
    fn state(&self) -> &AppState {
        Manager::state::<AppState>(self).inner()
    }

    fn activity(&self, activity: Activity) {
        let _ = self.emit("mcp", activity);
    }
}

/// Sent to the frontend as an `mcp` event.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Activity {
    pub tool: String,
    pub session_id: String,
    /// A device path the tool changed.
    pub path: Option<String>,
}

impl Activity {
    pub fn new(tool: &str, session_id: &str) -> Self {
        Self { tool: tool.into(), session_id: session_id.into(), path: None }
    }

    pub fn path(mut self, path: &str) -> Self {
        self.path = Some(path.into());
        self
    }
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct McpOptions {
    pub port: u16,
    /// Expose tools that run code and change files.
    pub allow_write: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct McpStatus {
    pub running: bool,
    pub url: Option<String>,
    pub token: String,
    pub allow_write: bool,
    pub tools: Vec<String>,
}

struct Running {
    url: String,
    options: McpOptions,
    cancel: CancellationToken,
}

pub struct McpServer {
    token_file: PathBuf,
    tools: Arc<ToolRegistry>,
    running: tokio::sync::Mutex<Option<Running>>,
}

impl McpServer {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self {
            token_file: data_dir.join("mcp-token"),
            tools: Arc::new(ToolRegistry::with_defaults()),
            running: tokio::sync::Mutex::new(None),
        }
    }

    /// The bearer token, created on first use.
    pub fn token(&self) -> Result<String> {
        match std::fs::read_to_string(&self.token_file) {
            Ok(t) if !t.trim().is_empty() => Ok(t.trim().to_string()),
            _ => self.regenerate_token(),
        }
    }

    pub fn regenerate_token(&self) -> Result<String> {
        let bytes: [u8; 32] = rand::random();
        let token = hex::encode(bytes);
        std::fs::write(&self.token_file, &token)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&self.token_file, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(token)
    }

    pub async fn status(&self) -> Result<McpStatus> {
        let running = self.running.lock().await;
        let allow_write = running.as_ref().is_some_and(|r| r.options.allow_write);
        Ok(McpStatus {
            running: running.is_some(),
            url: running.as_ref().map(|r| r.url.clone()),
            token: self.token()?,
            allow_write,
            tools: self.tools.visible(allow_write).map(|t| t.name().to_string()).collect(),
        })
    }

    /// Start serving, replacing a running server. Port 0 picks a free port.
    pub async fn start(&self, host: Arc<dyn Host>, opts: &McpOptions) -> Result<McpStatus> {
        self.stop().await;
        let token: Arc<str> = self.token()?.into();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", opts.port))
            .await
            .map_err(|e| Error::other(format!("can't listen on port {}: {e}", opts.port)))?;
        let addr = listener.local_addr()?;
        let cancel = CancellationToken::new();

        let handler = Handler { host, tools: self.tools.clone(), allow_write: opts.allow_write };
        let config = StreamableHttpServerConfig::default()
            .with_cancellation_token(cancel.clone())
            .enforce_origin_validation();
        let service: StreamableHttpService<Handler, LocalSessionManager> =
            StreamableHttpService::new(move || Ok(handler.clone()), Default::default(), config);
        let router = axum::Router::new()
            .nest_service("/mcp", service)
            .layer(axum::middleware::from_fn_with_state(token, require_token));
        let shutdown = cancel.clone();
        tokio::spawn(async move {
            let served = axum::serve(listener, router).with_graceful_shutdown(shutdown.cancelled_owned()).await;
            if let Err(e) = served {
                log::warn!("MCP server stopped: {e}");
            }
        });

        // Keep the port actually bound, so a restart reuses it even when 0 was asked for.
        let options = McpOptions { port: addr.port(), allow_write: opts.allow_write };
        *self.running.lock().await = Some(Running { url: format!("http://{addr}/mcp"), options, cancel });
        self.status().await
    }

    /// The options of the running server, if any.
    pub async fn options(&self) -> Option<McpOptions> {
        self.running.lock().await.as_ref().map(|r| r.options.clone())
    }

    pub async fn stop(&self) {
        if let Some(r) = self.running.lock().await.take() {
            r.cancel.cancel();
        }
    }
}

async fn require_token(AxState(token): AxState<Arc<str>>, req: Request, next: Next) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if constant_time_eq(given.as_bytes(), token.as_bytes()) {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "missing or wrong bearer token (see Microbe → Settings → MCP server)").into_response()
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Clone)]
struct Handler {
    host: Arc<dyn Host>,
    tools: Arc<ToolRegistry>,
    allow_write: bool,
}

const INSTRUCTIONS: &str = "Microbe is a MicroPython IDE. These tools act on the boards connected in it. \
Call list_boards first; pass `board` when more than one is connected. Use exec for short snippets and \
run_program + read_output for programs that keep running. The user sees the board's terminal and files \
in the IDE while you work.";

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("microbe", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        let tools = self
            .tools
            .visible(self.allow_write)
            .map(|t| {
                let schema = match t.schema() {
                    serde_json::Value::Object(m) => m,
                    _ => Default::default(),
                };
                Tool::new(t.name(), t.description(), schema)
                    .with_annotations(ToolAnnotations::new().read_only(!t.mutates()).destructive(t.mutates()))
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let Some(tool) = self.tools.get(&request.name, self.allow_write) else {
            let hint = if self.tools.get(&request.name, true).is_some() {
                " (Microbe grants read-only access; enable \"Allow running code and changing files\" in Settings)"
            } else {
                ""
            };
            return Err(ErrorData::invalid_params(format!("unknown tool `{}`{hint}", request.name), None));
        };
        let args = request.arguments.unwrap_or_default();
        let result = match tool.call(self.host.as_ref(), &args).await {
            Ok(serde_json::Value::String(text)) => CallToolResult::success(vec![ContentBlock::text(text)]),
            Ok(value) => CallToolResult::structured(value),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.to_string())]),
        };
        Ok(result.into())
    }
}

// ---- commands --------------------------------------------------------------

#[tauri::command]
pub async fn mcp_status(state: tauri::State<'_, AppState>) -> Result<McpStatus> {
    state.mcp.status().await
}

#[tauri::command]
pub async fn mcp_start(app: AppHandle, state: tauri::State<'_, AppState>, options: McpOptions) -> Result<McpStatus> {
    state.mcp.start(Arc::new(app.clone()), &options).await
}

#[tauri::command]
pub async fn mcp_stop(state: tauri::State<'_, AppState>) -> Result<McpStatus> {
    state.mcp.stop().await;
    state.mcp.status().await
}

/// New token; a running server is restarted so the old one stops working.
#[tauri::command]
pub async fn mcp_regenerate_token(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<McpStatus> {
    state.mcp.regenerate_token()?;
    match state.mcp.options().await {
        Some(opts) => state.mcp.start(Arc::new(app.clone()), &opts).await,
        None => state.mcp.status().await,
    }
}
