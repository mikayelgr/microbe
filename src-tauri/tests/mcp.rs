//! The MCP server end to end: real HTTP requests, a real session on the
//! MicroPython unix port. Skipped when no `micropython` executable is installed.
#![cfg(unix)]

use std::sync::{Arc, Mutex};

use microbe_lib::mcp::{Activity, Host, McpOptions};
use microbe_lib::platform::PlatformRegistry;
use microbe_lib::repl::session::Session;
use microbe_lib::state::{AppState, Paths};
use microbe_lib::transport::{local, Endpoint};
use serde_json::{json, Value};

struct TestHost {
    state: AppState,
    activity: Mutex<Vec<Activity>>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

impl Host for TestHost {
    fn state(&self) -> &AppState {
        &self.state
    }
    fn activity(&self, a: Activity) {
        self.activity.lock().unwrap().push(a);
    }
}

fn host_with_board() -> Option<(Arc<TestHost>, tempfile::TempDir)> {
    let exe = local::find_micropython()?;
    let (data, cache) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let state = AppState::new(Paths { data: data.path().into(), cache: cache.path().into() }).unwrap();
    let sandbox = tempfile::tempdir().unwrap();
    let conn = local::spawn_pty(&exe, sandbox.path()).unwrap();
    let profile = PlatformRegistry::load(None).unwrap().get("unix").unwrap().clone();
    let endpoint = Endpoint {
        transport: local::ID.into(),
        address: exe,
        label: "test".into(),
        detail: None,
        usb: None,
        platform_hint: Some("unix".into()),
        role: None,
    };
    let id = state.new_session_id();
    let session = Session::start(id, endpoint, conn, profile, Arc::new(|_| {}), Arc::new(|_| {}));
    state.insert_session(session);
    let host = TestHost { state, activity: Mutex::new(Vec::new()), _dirs: (data, cache) };
    Some((Arc::new(host), sandbox))
}

/// A minimal streamable-HTTP MCP client.
struct Client {
    http: reqwest::Client,
    url: String,
    token: String,
    session: Option<String>,
    next_id: u64,
}

impl Client {
    async fn connect(url: &str, token: &str) -> Self {
        let mut c = Client { http: reqwest::Client::new(), url: url.into(), token: token.into(), session: None, next_id: 1 };
        let init = c
            .post(json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": { "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }
            }))
            .send()
            .await
            .unwrap();
        assert!(init.status().is_success(), "initialize: {}", init.status());
        c.session = init.headers().get("mcp-session-id").map(|v| v.to_str().unwrap().to_string());
        let _ = init.text().await;
        let ack = c.post(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).send().await.unwrap();
        assert!(ack.status().is_success());
        c
    }

    fn post(&self, body: Value) -> reqwest::RequestBuilder {
        let mut r = self
            .http
            .post(&self.url)
            .bearer_auth(&self.token)
            .header("accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(s) = &self.session {
            r = r.header("mcp-session-id", s);
        }
        r
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let res = self.post(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).send().await.unwrap();
        assert!(res.status().is_success(), "{method}: {}", res.status());
        let text = res.text().await.unwrap();
        // Either a JSON body or an SSE stream whose `data:` lines carry messages.
        let messages: Vec<Value> = if text.trim_start().starts_with('{') {
            vec![serde_json::from_str(&text).unwrap()]
        } else {
            text.lines().filter_map(|l| l.strip_prefix("data:")).filter_map(|d| serde_json::from_str(d.trim()).ok()).collect()
        };
        messages.into_iter().find(|m| m["id"] == json!(id)).unwrap_or_else(|| panic!("no response in {text:?}"))
    }

    async fn call(&mut self, tool: &str, args: Value) -> Value {
        self.request("tools/call", json!({ "name": tool, "arguments": args })).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn agents_drive_a_board_over_mcp() {
    let Some((host, sandbox)) = host_with_board() else { return };
    let mcp = &host.state().mcp;
    let status = mcp.start(host.clone(), &McpOptions { port: 0, allow_write: true }).await.unwrap();
    let url = status.url.clone().unwrap();
    assert!(url.starts_with("http://127.0.0.1:"));

    // No token, a wrong token, or a browser Origin: rejected.
    let http = reqwest::Client::new();
    let body = json!({ "jsonrpc": "2.0", "id": 0, "method": "tools/list" });
    assert_eq!(http.post(&url).json(&body).send().await.unwrap().status(), 401);
    assert_eq!(http.post(&url).bearer_auth("nope").json(&body).send().await.unwrap().status(), 401);
    let from_page = http
        .post(&url)
        .bearer_auth(&status.token)
        .header("origin", "https://evil.example")
        .header("accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(from_page.status().is_client_error(), "{}", from_page.status());

    let mut c = Client::connect(&url, &status.token).await;
    let tools = c.request("tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"exec") && names.contains(&"list_boards"), "{names:?}");

    let boards = c.call("list_boards", json!({})).await;
    assert_eq!(boards["result"]["structuredContent"]["boards"].as_array().unwrap().len(), 1);

    // exec: output, tracebacks, and a runaway loop interrupted at the deadline.
    let r = c.call("exec", json!({ "code": "x = 6 * 7\nprint(x)" })).await;
    assert_eq!(r["result"]["structuredContent"]["stdout"].as_str().unwrap().trim(), "42");
    let r = c.call("exec", json!({ "code": "print(x + 1)\n1/0" })).await;
    let out = &r["result"]["structuredContent"];
    assert_eq!(out["stdout"].as_str().unwrap().trim(), "43", "globals persist");
    assert!(out["stderr"].as_str().unwrap().contains("ZeroDivisionError"));
    let r = c.call("exec", json!({ "code": "while True:\n    pass", "timeout_s": 1 })).await;
    let out = &r["result"]["structuredContent"];
    assert_eq!(out["timed_out"], json!(true));
    assert!(out["stderr"].as_str().unwrap().contains("KeyboardInterrupt"), "{out}");
    // The board is usable afterwards.
    let r = c.call("exec", json!({ "code": "print('ok')" })).await;
    assert_eq!(r["result"]["structuredContent"]["stdout"].as_str().unwrap().trim(), "ok");

    // Files, relative to the board's root (the sandbox for the unix port).
    let r = c.call("write_file", json!({ "path": "/lib/hello.py", "content": "MSG = 'hi'\n" })).await;
    assert_eq!(r["result"]["isError"], json!(false), "{r}");
    assert_eq!(std::fs::read_to_string(sandbox.path().join("lib/hello.py")).unwrap(), "MSG = 'hi'\n");
    let r = c.call("read_file", json!({ "path": "lib/hello.py" })).await;
    assert_eq!(r["result"]["content"][0]["text"], json!("MSG = 'hi'\n"));
    let r = c.call("list_files", json!({ "path": "lib" })).await;
    assert_eq!(r["result"]["structuredContent"]["entries"][0]["name"], json!("hello.py"));
    let r = c.call("delete_path", json!({ "path": "/" })).await;
    assert_eq!(r["result"]["isError"], json!(true));
    let r = c.call("delete_path", json!({ "path": "lib" })).await;
    assert_eq!(r["result"]["isError"], json!(false));
    assert!(!sandbox.path().join("lib").exists());
    assert!(host.activity.lock().unwrap().iter().any(|a| a.tool == "write_file" && a.path.as_deref() == Some("./lib/hello.py")));

    // A running program and its output.
    let r = c.call("run_program", json!({ "code": "import time\nfor i in range(3):\n    print('tick', i)\n    time.sleep_ms(50)\n" })).await;
    let from = r["result"]["structuredContent"]["output_from"].as_u64().unwrap();
    let mut seen = String::new();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let r = c.call("read_output", json!({ "after": from })).await;
        seen = r["result"]["structuredContent"]["text"].as_str().unwrap().to_string();
        if seen.contains("tick 2") && r["result"]["structuredContent"]["running"] == json!(false) {
            break;
        }
    }
    assert!(seen.contains("tick 0") && seen.contains("tick 2"), "{seen:?}");

    // Static checks.
    let r = c.call("check_code", json!({ "code": "import threading\n" })).await;
    let f = &r["result"]["structuredContent"]["findings"][0];
    assert_eq!((f["line"].clone(), f["rule"].clone()), (json!(1), json!("compat-imports")));

    // Read-only access hides and refuses tools that change the board.
    mcp.start(host.clone(), &McpOptions { port: 0, allow_write: false }).await.unwrap();
    let url = mcp.status().await.unwrap().url.unwrap();
    let mut c = Client::connect(&url, &status.token).await;
    let tools = c.request("tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(!names.contains(&"exec") && names.contains(&"read_file"), "{names:?}");
    let r = c.call("exec", json!({ "code": "print(1)" })).await;
    assert!(r["error"]["message"].as_str().unwrap().contains("read-only"), "{r}");

    // A new token locks out the old one.
    mcp.regenerate_token().unwrap();
    mcp.start(host.clone(), &McpOptions { port: 0, allow_write: false }).await.unwrap();
    let url = mcp.status().await.unwrap().url.unwrap();
    assert_eq!(http.post(&url).bearer_auth(&status.token).json(&body).send().await.unwrap().status(), 401);

    mcp.stop().await;
    assert!(!mcp.status().await.unwrap().running);
}
