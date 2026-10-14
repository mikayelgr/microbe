//! Connection, terminal and run commands. Every command addresses one
//! session, so several boards can be connected at once.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use ts_rs::TS;

use crate::device_info::{self, DeviceInfo};
use crate::error::{Error, Result};
use crate::platform::{PlatformProfile, ResolvedTarget};
use crate::repl::session::{Session, SessionEvent, SessionState, TermEvent};
use crate::scripts;
use crate::state::AppState;
use crate::transport::{ConnectOptions, Endpoint, TransportInfo};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DeviceStatus {
    pub session_id: String,
    pub connected: bool,
    pub state: SessionState,
    pub endpoint: Endpoint,
    pub target: Option<ResolvedTarget>,
    pub info: Option<DeviceInfo>,
}

/// Session events are broadcast as `session` events with the session id.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SessionEnvelope {
    pub session_id: String,
    pub event: SessionEvent,
}

/// A device-side helper to run before the user's code.
#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct Prelude {
    /// Bundled script name (`py/<script>.py`), overridable per platform.
    pub script: String,
    /// `{{key}}` substitutions, as Python literals.
    #[serde(default)]
    #[ts(type = "Record<string, string | number | boolean>")]
    pub args: std::collections::BTreeMap<String, serde_json::Value>,
    /// Script run after the program ends (same args), to undo what the prelude set up.
    #[serde(default)]
    #[ts(optional)]
    pub cleanup: Option<String>,
}

pub fn status_of(s: &Arc<Session>) -> DeviceStatus {
    DeviceStatus {
        session_id: s.id.clone(),
        connected: s.state() != SessionState::Disconnected,
        state: s.state(),
        endpoint: s.endpoint.clone(),
        target: s.target.lock().unwrap().clone(),
        info: s.info.lock().unwrap().clone(),
    }
}

#[tauri::command]
pub fn transports_list(state: State<'_, AppState>) -> Vec<TransportInfo> {
    state.transports.all().iter().map(|p| p.info()).collect()
}

#[tauri::command]
pub fn platforms_list(state: State<'_, AppState>) -> Vec<PlatformProfile> {
    state.platforms().all().to_vec()
}

/// Discover endpoints, optionally for a single transport (BLE scans are slow).
#[tauri::command]
pub async fn device_discover(state: State<'_, AppState>, transport: Option<String>) -> Result<Vec<Endpoint>> {
    let platforms = state.platforms();
    let mut out = Vec::new();
    for p in state.transports.all() {
        let id = p.info().id;
        if transport.as_deref().is_some_and(|t| t != id) {
            continue;
        }
        if transport.is_none() && id == crate::transport::ble::ID {
            continue; // BLE only on explicit request
        }
        match p.discover(&platforms).await {
            Ok(mut eps) => out.append(&mut eps),
            Err(e) => log::warn!("discover {id}: {e}"),
        }
    }
    Ok(out)
}

/// Open a session. Reconnecting to an endpoint that is already open replaces
/// that session; other boards stay connected.
pub async fn connect(
    app: &AppHandle,
    state: &AppState,
    endpoint: Endpoint,
    options: &ConnectOptions,
    interrupt: bool,
    term: Arc<dyn Fn(TermEvent) + Send + Sync>,
) -> Result<Arc<Session>> {
    if let Some(old) = state.session_on(&endpoint.address) {
        state.remove_session(&old.id);
        let _ = old.close().await;
    }
    let provider = state
        .transports
        .get(&endpoint.transport)
        .ok_or_else(|| Error::NotFound(format!("transport {}", endpoint.transport)))?;
    let conn = provider.connect(&endpoint, options).await?;

    let platforms = state.platforms();
    let profile = endpoint
        .platform_hint
        .as_deref()
        .and_then(|id| platforms.get(id))
        .unwrap_or_else(|| platforms.generic())
        .clone();

    let id = state.new_session_id();
    let app2 = app.clone();
    let sid = id.clone();
    let event_sink = Arc::new(move |event: SessionEvent| {
        let _ = app2.emit("session", SessionEnvelope { session_id: sid.clone(), event });
    });
    let session = Session::start(id, endpoint, conn, profile, term, event_sink);
    state.insert_session(session.clone());

    if interrupt {
        if let Err(e) = identify(state, &session).await {
            log::warn!("could not identify device: {e}");
            let _ = app.emit("notice", format!("Connected, but could not query the device: {e}"));
        }
    }
    Ok(session)
}

#[tauri::command]
pub async fn device_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: Endpoint,
    options: ConnectOptions,
    interrupt: bool,
    term: Channel<TermEvent>,
) -> Result<DeviceStatus> {
    let sink = Arc::new(move |ev: TermEvent| {
        let _ = term.send(ev);
    });
    let s = connect(&app, &state, endpoint, &options, interrupt, sink).await?;
    Ok(status_of(&s))
}

/// Read device info and resolve the platform profile from it.
pub async fn identify(state: &AppState, session: &Arc<Session>) -> Result<DeviceInfo> {
    let profile = session.profile();
    let mut raw = session.raw().await?;
    let info = device_info::read(&mut raw, &profile).await;
    let info = raw.finish_with(info).await?;
    let platforms = state.platforms();
    let target = platforms.resolve(&info);
    if let Some(p) = platforms.get(&target.platform) {
        *session.profile.lock().unwrap() = p.clone();
    }
    session.apply_quirks(&crate::repl::quirks::effective(&state.quirks, &target.platform, &info.version)).await;
    *session.target.lock().unwrap() = Some(target);
    *session.info.lock().unwrap() = Some(info.clone());
    Ok(info)
}

#[tauri::command]
pub async fn device_identify(state: State<'_, AppState>, session_id: String) -> Result<DeviceStatus> {
    let session = state.session(&session_id)?;
    identify(&state, &session).await?;
    Ok(status_of(&session))
}

#[tauri::command]
pub fn device_sessions(state: State<'_, AppState>) -> Vec<DeviceStatus> {
    state.all_sessions().iter().map(status_of).collect()
}

#[tauri::command]
pub async fn device_disconnect(state: State<'_, AppState>, session_id: String) -> Result<()> {
    if let Some(s) = state.remove_session(&session_id) {
        s.close().await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn device_write(state: State<'_, AppState>, session_id: String, data: String) -> Result<()> {
    state.session(&session_id)?.write(data.as_bytes()).await
}

#[tauri::command]
pub async fn device_interrupt(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.session(&session_id)?.interrupt().await
}

#[tauri::command]
pub async fn device_soft_reset(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.session(&session_id)?.soft_reset().await
}

#[tauri::command]
pub async fn device_hard_reset(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.session(&session_id)?.hard_reset().await
}

pub fn render_preludes(profile: &PlatformProfile, preludes: &[Prelude]) -> Result<Vec<String>> {
    preludes.iter().map(|p| scripts::render_json(profile, &p.script, &p.args)).collect()
}

pub fn render_cleanups(profile: &PlatformProfile, preludes: &[Prelude]) -> Result<Vec<String>> {
    preludes
        .iter()
        .filter_map(|p| p.cleanup.as_ref().map(|c| scripts::render_json(profile, c, &p.args)))
        .collect()
}

#[tauri::command]
pub async fn device_run(
    state: State<'_, AppState>,
    session_id: String,
    code: String,
    preludes: Option<Vec<Prelude>>,
) -> Result<()> {
    let session = state.session(&session_id)?;
    let preludes = preludes.unwrap_or_default();
    let profile = session.profile();
    let cleanups = render_cleanups(&profile, &preludes)?;
    session.run_with(code, render_preludes(&profile, &preludes)?, cleanups).await
}

#[tauri::command]
pub async fn device_stop(state: State<'_, AppState>, session_id: String, force: bool) -> Result<()> {
    state.session(&session_id)?.stop(force).await
}

/// Execute a snippet and return stdout (used by panels such as packages).
#[tauri::command]
pub async fn device_exec(state: State<'_, AppState>, session_id: String, code: String) -> Result<String> {
    let session = state.session(&session_id)?;
    let mut raw = session.raw().await?;
    let out = raw.exec_str(&code).await;
    raw.finish_with(out).await
}

/// Execute a bundled device script (`py/<script>.py`, overridable per port)
/// with `{{key}}` arguments and return what it printed.
#[tauri::command]
pub async fn device_script(
    state: State<'_, AppState>,
    session_id: String,
    script: String,
    args: Option<std::collections::BTreeMap<String, serde_json::Value>>,
) -> Result<String> {
    let session = state.session(&session_id)?;
    let code = scripts::render_json(&session.profile(), &script, &args.unwrap_or_default())?;
    let mut raw = session.raw().await?;
    let out = raw.exec_str(&code).await;
    raw.finish_with(out).await
}

/// Install the BLE REPL helper on the device and enable it from boot.py.
#[tauri::command]
pub async fn ble_helper_install(state: State<'_, AppState>, session_id: String, name: String) -> Result<()> {
    let session = state.session(&session_id)?;
    let target = session.target.lock().unwrap().clone();
    if !target.is_some_and(|t| t.has("ble")) {
        return Err(Error::Unsupported("this firmware has no `bluetooth` module".into()));
    }
    let profile = session.profile();
    let helper = crate::scripts::bundled("ble_repl.py").expect("bundled helper");
    let module = crate::fs::join(&profile.fs_root, "ble_repl.py");
    let boot = crate::fs::join(&profile.fs_root, "boot.py");
    let line = format!("import ble_repl; ble_repl.start({})\n", crate::scripts::py_str(&name));

    let mut raw = session.raw().await?;
    let res = async {
        crate::fs::write_in(&mut raw, &profile, &module, helper.as_bytes(), &crate::progress::none).await?;
        let existing = match crate::fs::stat_in(&mut raw, &profile, &boot).await? {
            Some(_) => crate::fs::read_in(&mut raw, &profile, &boot, &crate::progress::none).await?,
            None => Vec::new(),
        };
        let mut text = String::from_utf8_lossy(&existing).into_owned();
        if !text.contains("ble_repl.start") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&line);
            crate::fs::write_in(&mut raw, &profile, &boot, text.as_bytes(), &crate::progress::none).await?;
        }
        Ok::<_, Error>(())
    }
    .await;
    raw.finish_with(res).await
}

/// Regain control of a board stuck in a runaway program (see `Session::rescue`).
#[tauri::command]
pub async fn device_rescue(
    state: State<'_, AppState>,
    session_id: String,
    options: crate::repl::session::RescueOptions,
) -> Result<crate::repl::session::RescueReport> {
    let session = state.session(&session_id)?;
    let report = session.rescue(&options).await?;
    if report.reached_repl {
        let _ = identify(&state, &session).await;
    }
    Ok(report)
}
