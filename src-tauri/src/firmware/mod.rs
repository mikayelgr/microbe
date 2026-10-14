//! Firmware discovery, download and flashing.
//!
//! Flow: detect → resolve platform/board → fetch the board's firmware list →
//! pick a build and a flash method the host supports → download → put the
//! board in the right mode → flash → the frontend reconnects.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::progress::Reporter;
use crate::state::AppState;

pub mod builder;
pub mod detect;
pub mod esp32;
pub mod flashers;
pub mod index;

use detect::{Candidate, DetectContext, FlashTarget};
use flashers::FlashJob;
use index::{BoardFirmware, BoardIndex};

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct FlashRequest {
    /// The connected session for the board, if any (it is closed for flashing).
    #[serde(default)]
    pub session_id: Option<String>,
    pub platform: String,
    pub board_id: Option<String>,
    /// Variant suffix (`SPIRAM_OCT`); `None` for the standard build.
    #[serde(default)]
    pub variant: Option<String>,
    /// Exact version; `None` picks the latest release (or preview if allowed).
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub allow_preview: bool,
    pub target: FlashTarget,
    #[serde(default)]
    pub chip: Option<String>,
    #[serde(default)]
    pub erase: bool,
    /// Flash this local file instead of downloading.
    #[serde(default)]
    pub local_file: Option<String>,
    /// Use this flash method (e.g. `esp32_ota`) instead of the first that fits.
    #[serde(default)]
    #[ts(optional)]
    pub method: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct FlashResult {
    pub file: String,
    pub version: Option<String>,
    pub method: String,
    /// Serial port to reconnect to, if known.
    pub port: Option<String>,
}

#[tauri::command]
pub async fn firmware_index(state: State<'_, AppState>, force: bool) -> Result<BoardIndex> {
    index::load_index(&state.http, &state.cache_dir("firmware")?, force).await
}

#[tauri::command]
pub async fn firmware_board(state: State<'_, AppState>, board_id: String) -> Result<BoardFirmware> {
    index::board_firmware(&state.http, &state.cache_dir("firmware")?, &board_id).await
}

#[tauri::command]
pub async fn firmware_detect(state: State<'_, AppState>, deep: bool) -> Result<Vec<Candidate>> {
    let platforms = state.platforms();
    let serial = crate::transport::serial::SerialProvider;
    use crate::transport::TransportProvider;
    let ports = serial.discover(&platforms).await.unwrap_or_default();
    let idx = index::load_index(&state.http, &state.cache_dir("firmware")?, false).await.ok();
    let ctx = DetectContext {
        platforms: &platforms,
        sessions: state.all_sessions(),
        serial_ports: ports,
        index: idx.as_ref(),
        deep,
    };
    Ok(state.detectors.detect(&ctx).await)
}

#[tauri::command]
pub async fn firmware_flash(app: AppHandle, state: State<'_, AppState>, request: FlashRequest) -> Result<FlashResult> {
    let reporter = Reporter::new(&app, "flash", "Preparing…");
    let platforms = state.platforms();
    let profile = platforms
        .get(&request.platform)
        .ok_or_else(|| Error::NotFound(format!("platform {}", request.platform)))?
        .clone();
    let cache = state.cache_dir("firmware")?;

    // 1. Firmware file and flash method.
    let (file, version, method, flasher) = match &request.local_file {
        Some(path) => {
            let ext = PathBuf::from(path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let chosen = match &request.method {
                Some(id) => state.flashers.by_method(&profile, id).filter(|(m, _)| m.ext.contains(&ext)),
                None => state.flashers.choose(&profile, &[ext.clone()], request.chip.as_deref()).map(|(m, f, _)| (m, f)),
            };
            let (m, f) = chosen.ok_or_else(|| Error::Unsupported(format!("no way to flash .{ext} files on {}", profile.name)))?;
            (PathBuf::from(path), None, m.clone(), f)
        }
        None => {
            let board = request.board_id.as_deref().ok_or_else(|| Error::other("no board selected"))?;
            reporter.set_label(format!("Looking up firmware for {board}…"));
            let fw = index::board_firmware(&state.http, &cache, board).await?;
            let variant = fw
                .variant(request.variant.as_deref())
                .or_else(|| fw.variants.first())
                .ok_or_else(|| Error::NotFound(format!("no firmware for {board}")))?;
            let build = match &request.version {
                Some(v) => variant.builds.iter().find(|b| &b.version == v),
                None => variant.builds.iter().find(|b| request.allow_preview || !b.preview),
            }
            .ok_or_else(|| Error::NotFound("requested firmware version not found".into()))?;
            let forced = match &request.method {
                Some(id) => Some(state.flashers.by_method(&profile, id).ok_or_else(|| Error::Unsupported(format!("{id} isn't available for {}", profile.name)))?),
                None => None,
            };
            let preferred = forced.as_ref().map(|(m, _)| &m.ext).unwrap_or(&profile.firmware_ext);
            let available: Vec<String> =
                preferred.iter().filter(|e| build.files.iter().any(|f| &f.ext == *e)).cloned().collect();
            let choice = match forced {
                Some((m, f)) => available.first().map(|e| (m, f, e.clone())),
                None => state.flashers.choose(&profile, &available, request.chip.as_deref()),
            };
            let (m, f, ext) = choice
                .ok_or_else(|| {
                    Error::Unsupported(format!(
                        "no supported flash method for {} ({})",
                        profile.name,
                        available.join(", ")
                    ))
                })?;
            let file = build.pick(&[ext]).expect("chosen ext exists");
            reporter.set_label(format!("Downloading {}…", file.file_name));
            let path = index::download(&state.http, &cache, file, &|d, t| reporter.update(d, t)).await?;
            (path, Some(build.version.clone()), m.clone(), f)
        }
    };

    // 2. Put the board in the right mode.
    let mut target = request.target.clone();
    let session = match (&request.session_id, &target) {
        (Some(id), _) | (None, FlashTarget::Session { session_id: id }) => state.session(id).ok(),
        (None, FlashTarget::Serial { port, .. }) => state.session_on(port),
        _ => None,
    };
    let via_session = matches!(target, FlashTarget::Session { .. });
    if flasher.uses_session() && session.is_none() {
        return Err(Error::Unsupported(format!("{} needs the board connected", method.method)));
    }
    if let Some(s) = session.as_ref().filter(|_| !flasher.uses_session()) {
        let on_port = matches!(&target, FlashTarget::Serial { port, .. } if *port == s.endpoint.address);
        if via_session || on_port {
            if flasher.needs_serial_port() {
                if s.endpoint.transport != crate::transport::serial::ID {
                    return Err(Error::Unsupported("flashing needs a USB/serial connection".into()));
                }
                target = FlashTarget::Serial { port: s.endpoint.address.clone(), vid: None, pid: None };
            } else if let Some(entry) = &profile.bootloader_entry {
                reporter.set_label("Rebooting into the bootloader…");
                let _ = s.exec_detached(entry).await;
            }
            let _ = s.close().await;
            state.remove_session(&s.id);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }
    // Session flashers check images against the running chip ("ESP32S3").
    let chip_name = session
        .as_ref()
        .filter(|_| flasher.uses_session())
        .and_then(|s| s.info.lock().unwrap().as_ref().and_then(|i| crate::analysis::chip_of(&i.machine).map(str::to_string)));
    if matches!(target, FlashTarget::Session { .. }) && flasher.needs_serial_port() {
        return Err(Error::other("no serial port selected"));
    }

    // 3. Flash.
    let job = FlashJob {
        method: &method,
        profile: &profile,
        firmware: &file,
        target: &target,
        chip: request.chip.as_deref().or(chip_name.as_deref()),
        erase: request.erase,
        reporter: &reporter,
        http: &state.http,
        cache: &state.paths.cache,
        session: if flasher.uses_session() { session.clone() } else { None },
    };
    flasher.flash(job).await?;
    reporter.set_label("Done");

    Ok(FlashResult {
        file: file.display().to_string(),
        version,
        method: method.method.clone(),
        port: match target {
            FlashTarget::Serial { port, .. } => Some(port),
            _ => None,
        },
    })
}
