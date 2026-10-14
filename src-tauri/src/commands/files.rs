//! Device filesystem and project-sync commands.

use std::path::Path;

use tauri::{AppHandle, State};

use crate::error::Result;
use crate::fs::{self, DeviceEntry};
use crate::progress::Reporter;
use crate::state::AppState;
use crate::sync::{self, SyncOptions, SyncReport};

#[tauri::command]
pub async fn fs_list(state: State<'_, AppState>, session_id: String, path: String) -> Result<Vec<DeviceEntry>> {
    fs::list(&*state.session(&session_id)?, &path).await
}

#[tauri::command]
pub async fn fs_read_text(state: State<'_, AppState>, session_id: String, path: String) -> Result<String> {
    fs::read_text(&*state.session(&session_id)?, &path).await
}

#[tauri::command]
pub async fn fs_write_text(app: AppHandle, state: State<'_, AppState>, session_id: String, path: String, content: String) -> Result<()> {
    let r = Reporter::new(&app, "fs-write", format!("Saving {path}"));
    fs::write(&*state.session(&session_id)?, &path, content.as_bytes(), &|d, t| r.update(d, t)).await
}

/// Upload local files or folders into `remote_dir`.
#[tauri::command]
pub async fn fs_upload(app: AppHandle, state: State<'_, AppState>, session_id: String, local_paths: Vec<String>, remote_dir: String) -> Result<Vec<String>> {
    let session = state.session(&session_id)?;
    let r = Reporter::new(&app, "fs-upload", "Uploading…");
    let mut jobs = Vec::new();
    for lp in &local_paths {
        let p = Path::new(lp);
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if p.is_dir() {
            for (rel, path) in sync::local_files(p, &[])? {
                jobs.push((fs::join(&fs::join(&remote_dir, &name), &rel), path));
            }
        } else {
            jobs.push((fs::join(&remote_dir, &name), p.to_path_buf()));
        }
    }
    let profile = session.profile();
    let mut done = Vec::new();
    let mut raw = session.raw().await?;
    let res = async {
        for (remote, local) in jobs {
            r.set_label(format!("Uploading {remote}"));
            let data = std::fs::read(&local)?;
            fs::write_in(&mut raw, &profile, &remote, &data, &|d, t| r.update(d, t)).await?;
            done.push(remote);
        }
        Ok(done)
    }
    .await;
    raw.finish_with(res).await
}

#[tauri::command]
pub async fn fs_download(app: AppHandle, state: State<'_, AppState>, session_id: String, remote_path: String, local_path: String) -> Result<()> {
    let session = state.session(&session_id)?;
    let r = Reporter::new(&app, "fs-download", format!("Downloading {remote_path}"));
    let profile = session.profile();
    let mut raw = session.raw().await?;
    let res = download_in(&mut raw, &profile, &remote_path, Path::new(&local_path), &r).await;
    raw.finish_with(res).await
}

async fn download_in(
    raw: &mut crate::repl::raw::RawRepl,
    profile: &crate::platform::PlatformProfile,
    remote: &str,
    local: &Path,
    r: &Reporter,
) -> Result<()> {
    let is_dir = fs::stat_in(raw, profile, remote).await?.is_some_and(|s| s.is_dir);
    if !is_dir {
        let data = fs::read_in(raw, profile, remote, &|d, t| r.update(d, t)).await?;
        if let Some(parent) = local.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(local, data)?;
        return Ok(());
    }
    for f in fs::walk_in(raw, profile, remote, false).await? {
        let rel = f.path.strip_prefix(remote).unwrap_or(&f.path).trim_start_matches('/');
        let target = local.join(rel);
        if f.is_dir {
            std::fs::create_dir_all(&target)?;
        } else {
            r.set_label(format!("Downloading {}", f.path));
            let data = fs::read_in(raw, profile, &f.path, &|d, t| r.update(d, t)).await?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, data)?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn fs_mkdir(state: State<'_, AppState>, session_id: String, path: String) -> Result<()> {
    fs::mkdir(&*state.session(&session_id)?, &path).await
}

#[tauri::command]
pub async fn fs_remove(state: State<'_, AppState>, session_id: String, path: String) -> Result<()> {
    fs::remove(&*state.session(&session_id)?, &path).await
}

#[tauri::command]
pub async fn fs_rename(state: State<'_, AppState>, session_id: String, src: String, dst: String) -> Result<()> {
    fs::rename(&*state.session(&session_id)?, &src, &dst).await
}

#[tauri::command]
pub async fn project_sync(app: AppHandle, state: State<'_, AppState>, session_id: String, options: SyncOptions) -> Result<SyncReport> {
    let r = Reporter::new(&app, "sync", "Syncing project…");
    sync::sync(&*state.session(&session_id)?, &options, &r).await
}
