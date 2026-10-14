//! Local project folder commands (the host side of the file explorer).

use std::path::Path;

use serde::Serialize;
use ts_rs::TS;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct LocalEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

const HIDDEN: &[&str] = &[".git", ".microbe", "__pycache__", ".DS_Store", "node_modules", ".venv"];

#[tauri::command]
pub fn project_list(dir: String) -> Result<Vec<LocalEntry>> {
    let mut out: Vec<LocalEntry> = std::fs::read_dir(&dir)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            let is_dir = e.file_type().ok()?.is_dir();
            Some(LocalEntry { path: e.path().to_string_lossy().into_owned(), name, is_dir })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[tauri::command]
pub fn project_read(path: String) -> Result<String> {
    let data = std::fs::read(&path)?;
    String::from_utf8(data).map_err(|_| Error::Unsupported(format!("{path} is not a text file")))
}

#[tauri::command]
pub fn project_write(path: String, content: String) -> Result<()> {
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::write(path, content)?)
}

#[tauri::command]
pub fn project_create(path: String, is_dir: bool) -> Result<()> {
    let p = Path::new(&path);
    if p.exists() {
        return Err(Error::other(format!("{path} already exists")));
    }
    if is_dir {
        std::fs::create_dir_all(p)?;
    } else {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(p, "")?;
    }
    Ok(())
}

#[tauri::command]
pub fn project_remove(path: String) -> Result<()> {
    let p = Path::new(&path);
    if p.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[tauri::command]
pub fn project_rename(src: String, dst: String) -> Result<()> {
    Ok(std::fs::rename(src, dst)?)
}
