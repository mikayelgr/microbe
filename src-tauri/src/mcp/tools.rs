//! MCP tools: thin wrappers over the session, fs and analysis APIs.
//!
//! Add a tool by implementing [`McpTool`] and listing it in
//! [`ToolRegistry::with_defaults`]. Tools that run code or change the board
//! return `true` from `mutates`; they're hidden when the user grants
//! read-only access.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::{Activity, Host};
use crate::error::{Error, Result};
use crate::repl::session::{Session, SessionState};

pub type Args = Map<String, Value>;

#[async_trait]
pub trait McpTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema of the arguments object.
    fn schema(&self) -> Value;
    /// Runs code on, or changes, a board.
    fn mutates(&self) -> bool;
    /// A string result becomes text content; anything else is returned as
    /// structured content (and as JSON text).
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value>;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn McpTool>>,
}

impl ToolRegistry {
    pub fn with_defaults() -> Self {
        Self {
            tools: vec![
                Box::new(ListBoards),
                Box::new(BoardInfo),
                Box::new(Exec),
                Box::new(RunProgram),
                Box::new(StopProgram),
                Box::new(ReadOutput),
                Box::new(ListFiles),
                Box::new(ReadFile),
                Box::new(WriteFile),
                Box::new(MakeDir),
                Box::new(DeletePath),
                Box::new(Reset),
                Box::new(CheckCode),
            ],
        }
    }

    pub fn visible(&self, allow_write: bool) -> impl Iterator<Item = &dyn McpTool> {
        self.tools.iter().map(|t| t.as_ref()).filter(move |t| allow_write || !t.mutates())
    }

    pub fn get(&self, name: &str, allow_write: bool) -> Option<&dyn McpTool> {
        self.visible(allow_write).find(|t| t.name() == name)
    }
}

// ---- argument helpers ------------------------------------------------------

fn str_arg<'a>(args: &'a Args, key: &str) -> Result<&'a str> {
    opt_str(args, key).ok_or_else(|| Error::other(format!("missing string argument `{key}`")))
}

fn opt_str<'a>(args: &'a Args, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn board_prop() -> Value {
    json!({ "type": "string", "description": "Board id from list_boards. Optional when exactly one board is connected." })
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

/// The board named by `board`, or the only connected one.
fn session(host: &dyn Host, args: &Args) -> Result<Arc<Session>> {
    let state = host.state();
    if let Some(id) = opt_str(args, "board") {
        return state.session(id).map_err(|_| Error::NotFound(format!("no board `{id}`; call list_boards")));
    }
    let all: Vec<_> = state.all_sessions().into_iter().filter(|s| s.state() != SessionState::Disconnected).collect();
    match all.as_slice() {
        [] => Err(Error::other("no board is connected; connect one in Microbe first")),
        [one] => Ok(one.clone()),
        many => {
            let ids: Vec<_> = many.iter().map(|s| s.id.as_str()).collect();
            Err(Error::other(format!("several boards are connected ({}); pass `board`", ids.join(", "))))
        }
    }
}

/// A path relative to the board's filesystem root (`/` on most ports, `.` on
/// the unix port). Absolute paths already under a non-`/` root are kept.
pub fn device_path(root: &str, path: &str) -> String {
    let p = path.trim();
    if p.is_empty() || p == "/" || p == "." {
        return root.to_string();
    }
    if root != "/" && (p == root || p.starts_with(&format!("{root}/"))) {
        return p.to_string();
    }
    crate::fs::join(root, p.trim_start_matches("./").trim_start_matches('/'))
}

fn path_arg(session: &Session, args: &Args, key: &str) -> Result<String> {
    Ok(device_path(&session.profile().fs_root, str_arg(args, key)?))
}

// ---- tools -----------------------------------------------------------------

struct ListBoards;

#[async_trait]
impl McpTool for ListBoards {
    fn name(&self) -> &'static str {
        "list_boards"
    }
    fn description(&self) -> &'static str {
        "List the MicroPython boards connected in Microbe, with their port, firmware version and state."
    }
    fn schema(&self) -> Value {
        schema(json!({}), &[])
    }
    fn mutates(&self) -> bool {
        false
    }
    async fn call(&self, host: &dyn Host, _: &Args) -> Result<Value> {
        let boards: Vec<Value> = host
            .state()
            .all_sessions()
            .iter()
            .map(|s| {
                let info = s.info.lock().unwrap().clone();
                let target = s.target.lock().unwrap().clone();
                json!({
                    "board": s.id,
                    "address": s.endpoint.address,
                    "transport": s.endpoint.transport,
                    "state": s.state(),
                    "platform": target.as_ref().map(|t| t.platform.clone()),
                    "board_id": target.as_ref().and_then(|t| t.board_id.clone()),
                    "machine": info.as_ref().map(|i| i.machine.clone()),
                    "version": info.as_ref().map(|i| i.version.clone()),
                })
            })
            .collect();
        Ok(json!({ "boards": boards }))
    }
}

struct BoardInfo;

#[async_trait]
impl McpTool for BoardInfo {
    fn name(&self) -> &'static str {
        "board_info"
    }
    fn description(&self) -> &'static str {
        "Details of one board: firmware, memory, filesystem size, available modules and capabilities."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop() }), &[])
    }
    fn mutates(&self) -> bool {
        false
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        Ok(serde_json::to_value(crate::commands::device::status_of(&s))?)
    }
}

struct Exec;

#[async_trait]
impl McpTool for Exec {
    fn name(&self) -> &'static str {
        "exec"
    }
    fn description(&self) -> &'static str {
        "Run a MicroPython snippet on the board and return what it printed. A raised exception comes back in \
         `stderr`. Code still running at the timeout is interrupted (Ctrl-C). Globals persist between calls \
         until the board is reset. For long-running programs use run_program."
    }
    fn schema(&self) -> Value {
        schema(
            json!({
                "board": board_prop(),
                "code": { "type": "string", "description": "MicroPython source to execute." },
                "timeout_s": { "type": "number", "description": "Seconds before the code is interrupted (default 10, max 120)." },
            }),
            &["code"],
        )
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let code = str_arg(args, "code")?;
        let secs = args.get("timeout_s").and_then(Value::as_f64).unwrap_or(10.0).clamp(0.5, 120.0);
        host.activity(Activity::new(self.name(), &s.id));
        let out = s.exec_capture(code, Duration::from_secs_f64(secs)).await?;
        Ok(json!({ "stdout": out.stdout, "stderr": out.stderr, "timed_out": out.timed_out }))
    }
}

struct RunProgram;

#[async_trait]
impl McpTool for RunProgram {
    fn name(&self) -> &'static str {
        "run_program"
    }
    fn description(&self) -> &'static str {
        "Start a program on the board without waiting for it, as Microbe's Run button does. Its output shows \
         in Microbe's terminal; read it with read_output starting from the returned `output_from`. Stop it \
         with stop_program."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "code": { "type": "string" } }), &["code"])
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let code = str_arg(args, "code")?.to_string();
        let from = s.output.lock().unwrap().since(u64::MAX).1;
        host.activity(Activity::new(self.name(), &s.id));
        s.run(code, Vec::new()).await?;
        Ok(json!({ "started": true, "output_from": from }))
    }
}

struct StopProgram;

#[async_trait]
impl McpTool for StopProgram {
    fn name(&self) -> &'static str {
        "stop_program"
    }
    fn description(&self) -> &'static str {
        "Interrupt the program running on the board (Ctrl-C)."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop() }), &[])
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let running = s.state() == SessionState::Running;
        s.stop(running).await?;
        Ok(json!({ "was_running": running }))
    }
}

struct ReadOutput;

#[async_trait]
impl McpTool for ReadOutput {
    fn name(&self) -> &'static str {
        "read_output"
    }
    fn description(&self) -> &'static str {
        "Read recent terminal output of the board (REPL and running program; the last 64 KiB are kept). Pass \
         the `next` value of a previous call as `after` to get only newer output."
    }
    fn schema(&self) -> Value {
        schema(
            json!({
                "board": board_prop(),
                "after": { "type": "integer", "description": "Return output after this position (from `next` or `output_from`)." },
            }),
            &[],
        )
    }
    fn mutates(&self) -> bool {
        false
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let after = args.get("after").and_then(Value::as_u64).unwrap_or(0);
        let (text, next) = s.output.lock().unwrap().since(after);
        Ok(json!({ "text": text, "next": next, "running": s.state() == SessionState::Running }))
    }
}

struct ListFiles;

#[async_trait]
impl McpTool for ListFiles {
    fn name(&self) -> &'static str {
        "list_files"
    }
    fn description(&self) -> &'static str {
        "List a directory on the board's filesystem. Paths are relative to the filesystem root."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "path": { "type": "string", "description": "Directory (default: root)." } }), &[])
    }
    fn mutates(&self) -> bool {
        false
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let path = device_path(&s.profile().fs_root, opt_str(args, "path").unwrap_or("/"));
        Ok(json!({ "path": path, "entries": crate::fs::list(&s, &path).await? }))
    }
}

struct ReadFile;

#[async_trait]
impl McpTool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read a file from the board. Text files are returned as text, other files as base64."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "path": { "type": "string" } }), &["path"])
    }
    fn mutates(&self) -> bool {
        false
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let path = path_arg(&s, args, "path")?;
        let data = crate::fs::read(&s, &path, &crate::progress::none).await?;
        Ok(match String::from_utf8(data) {
            Ok(text) => Value::String(text),
            Err(e) => {
                use base64::Engine;
                json!({ "path": path, "base64": base64::engine::general_purpose::STANDARD.encode(e.as_bytes()) })
            }
        })
    }
}

struct WriteFile;

#[async_trait]
impl McpTool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        "Create or overwrite a text file on the board. `main.py` runs at every boot."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "path": { "type": "string" }, "content": { "type": "string" } }), &["path", "content"])
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let path = path_arg(&s, args, "path")?;
        let content = str_arg(args, "content")?;
        crate::fs::write(&s, &path, content.as_bytes(), &crate::progress::none).await?;
        host.activity(Activity::new(self.name(), &s.id).path(&path));
        Ok(json!({ "path": path, "bytes": content.len() }))
    }
}

struct MakeDir;

#[async_trait]
impl McpTool for MakeDir {
    fn name(&self) -> &'static str {
        "make_dir"
    }
    fn description(&self) -> &'static str {
        "Create a directory (and missing parents) on the board."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "path": { "type": "string" } }), &["path"])
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let path = path_arg(&s, args, "path")?;
        crate::fs::mkdir(&s, &path).await?;
        host.activity(Activity::new(self.name(), &s.id).path(&path));
        Ok(json!({ "path": path }))
    }
}

struct DeletePath;

#[async_trait]
impl McpTool for DeletePath {
    fn name(&self) -> &'static str {
        "delete_path"
    }
    fn description(&self) -> &'static str {
        "Delete a file, or a directory with everything in it, on the board."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "path": { "type": "string" } }), &["path"])
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let path = path_arg(&s, args, "path")?;
        if path == s.profile().fs_root {
            return Err(Error::other("refusing to delete the filesystem root"));
        }
        crate::fs::remove(&s, &path).await?;
        host.activity(Activity::new(self.name(), &s.id).path(&path));
        Ok(json!({ "deleted": path }))
    }
}

struct Reset;

#[async_trait]
impl McpTool for Reset {
    fn name(&self) -> &'static str {
        "reset"
    }
    fn description(&self) -> &'static str {
        "Reset the board. A soft reset restarts MicroPython and clears globals; a hard reset reboots the chip \
         (USB boards may briefly disconnect and Microbe reconnects them)."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "hard": { "type": "boolean", "description": "Hard reset (default false)." } }), &[])
    }
    fn mutates(&self) -> bool {
        true
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let s = session(host, args)?;
        let hard = args.get("hard").and_then(Value::as_bool).unwrap_or(false);
        host.activity(Activity::new(self.name(), &s.id));
        if hard {
            s.hard_reset().await?;
        } else {
            s.soft_reset().await?;
        }
        Ok(json!({ "reset": if hard { "hard" } else { "soft" } }))
    }
}

struct CheckCode;

#[async_trait]
impl McpTool for CheckCode {
    fn name(&self) -> &'static str {
        "check_code"
    }
    fn description(&self) -> &'static str {
        "Statically check MicroPython source for CPython-only modules and syntax, and for pins the board \
         doesn't have. Pass `board` (or connect exactly one) for board-specific pin checks."
    }
    fn schema(&self) -> Value {
        schema(json!({ "board": board_prop(), "code": { "type": "string" } }), &["code"])
    }
    fn mutates(&self) -> bool {
        false
    }
    async fn call(&self, host: &dyn Host, args: &Args) -> Result<Value> {
        let code = str_arg(args, "code")?;
        let mut request = crate::analysis::AnalysisRequest {
            source: code.to_string(),
            platform: None,
            board_id: None,
            machine: None,
            disabled: Vec::new(),
        };
        if let Ok(s) = session(host, args) {
            let target = s.target.lock().unwrap().clone();
            request.platform = target.as_ref().map(|t| t.platform.clone());
            request.board_id = target.and_then(|t| t.board_id);
            request.machine = s.info.lock().unwrap().as_ref().map(|i| i.machine.clone());
        }
        let result = crate::analysis::run(host.state(), &request)?;
        let lines = LineIndex::new(code);
        let findings: Vec<Value> = result
            .findings
            .iter()
            .map(|f| {
                let (line, column) = lines.at(f.from);
                json!({ "line": line, "column": column, "severity": f.severity, "rule": f.rule, "message": f.message })
            })
            .collect();
        Ok(json!({ "findings": findings }))
    }
}

/// UTF-16 offset → 1-based (line, column).
struct LineIndex(Vec<u16>);

impl LineIndex {
    fn new(s: &str) -> Self {
        Self(s.encode_utf16().collect())
    }

    fn at(&self, offset: u32) -> (usize, usize) {
        let before = &self.0[..(offset as usize).min(self.0.len())];
        let line = before.iter().filter(|&&c| c == b'\n' as u16).count() + 1;
        let col = before.iter().rev().take_while(|&&c| c != b'\n' as u16).count() + 1;
        (line, col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_paths() {
        assert_eq!(device_path("/", "main.py"), "/main.py");
        assert_eq!(device_path("/", "/lib/a.py"), "/lib/a.py");
        assert_eq!(device_path("/", ""), "/");
        assert_eq!(device_path(".", "/main.py"), "./main.py");
        assert_eq!(device_path(".", "./lib"), "./lib");
        assert_eq!(device_path("/flash", "/flash/main.py"), "/flash/main.py");
        assert_eq!(device_path("/flash", "main.py"), "/flash/main.py");
    }

    #[test]
    fn line_index() {
        let l = LineIndex::new("ab\ncd\n");
        assert_eq!(l.at(0), (1, 1));
        assert_eq!(l.at(4), (2, 2));
    }

    #[test]
    fn read_only_hides_mutating_tools() {
        let r = ToolRegistry::with_defaults();
        assert!(r.get("exec", false).is_none());
        assert!(r.get("exec", true).is_some());
        assert!(r.get("read_file", false).is_some());
        let names: Vec<_> = r.visible(true).map(|t| t.name()).collect();
        let mut dedup = names.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(names.len(), dedup.len(), "tool names are unique");
    }
}
