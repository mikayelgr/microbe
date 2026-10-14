//! Device filesystem operations.
//!
//! The `*_in` functions work inside an existing raw-REPL session, so batch
//! operations (sync, package install) enter raw REPL once. The session-level
//! functions wrap a single op and use a transport's out-of-band file transfer
//! (WebREPL) when available.

use base64::Engine;
use serde::Serialize;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::platform::PlatformProfile;
use crate::repl::raw::RawRepl;
use crate::repl::session::Session;
use crate::scripts::{self, py_bytes, Py};
use crate::transport::Progress;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DeviceEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[ts(type = "number")]
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DeviceFile {
    pub path: String,
    pub is_dir: bool,
    #[ts(type = "number")]
    pub size: u64,
    pub sha256: Option<String>,
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub fn parent(path: &str) -> &str {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

// ---- inside raw REPL -------------------------------------------------------

pub async fn list_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str) -> Result<Vec<DeviceEntry>> {
    let out = repl.exec_str(&scripts::render(p, "ls", &[("path", Py::Str(path))])?).await?;
    let mut entries: Vec<DeviceEntry> = out
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let name = it.next()?.to_string();
            let is_dir = it.next()? == "d";
            let size = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            Some(DeviceEntry { path: join(path, &name), name, is_dir, size })
        })
        .collect();
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

pub async fn walk_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str, hash: bool) -> Result<Vec<DeviceFile>> {
    let code = scripts::render(p, "walk", &[("path", Py::Str(path)), ("hash", Py::Bool(hash))])?;
    let out = repl.exec_str(&code).await?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let path = it.next()?.to_string();
            let is_dir = it.next()? == "d";
            let size = it.next()?.trim().parse().unwrap_or(0);
            let h = it.next().map(str::trim).filter(|h| *h != "-").map(str::to_string);
            Some(DeviceFile { path, is_dir, size, sha256: h })
        })
        .collect())
}

pub async fn stat_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str) -> Result<Option<DeviceFile>> {
    let out = repl.exec_str(&scripts::render(p, "stat", &[("path", Py::Str(path))])?).await?;
    let mut it = out.trim().split('\t');
    match it.next() {
        Some(kind @ ("d" | "f")) => Ok(Some(DeviceFile {
            path: path.into(),
            is_dir: kind == "d",
            size: it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            sha256: None,
        })),
        _ => Ok(None),
    }
}

pub async fn hash_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str) -> Result<String> {
    Ok(repl.exec_str(&scripts::render(p, "hash", &[("path", Py::Str(path))])?).await?.trim().to_string())
}

pub async fn read_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str, progress: Progress<'_>) -> Result<Vec<u8>> {
    let chunk = repl.chunk.max(64) as i64;
    let code = scripts::render(p, "get", &[("path", Py::Str(path)), ("chunk", Py::Int(chunk))])?;
    let engine = base64::engine::general_purpose::STANDARD;
    let mut total: Option<u64> = None;
    let mut data = Vec::new();
    let mut line = Vec::new();
    let mut bad = None;
    repl.exec_with(&code, |bytes| {
        for &b in bytes {
            if b != b'\n' {
                line.push(b);
                continue;
            }
            let l = std::mem::take(&mut line);
            let l = String::from_utf8_lossy(&l);
            let l = l.trim();
            if total.is_none() {
                total = Some(l.parse().unwrap_or(0));
            } else if !l.is_empty() {
                match engine.decode(l) {
                    Ok(d) => data.extend_from_slice(&d),
                    Err(e) => bad = Some(e.to_string()),
                }
                progress(data.len() as u64, total.unwrap_or(0));
            }
        }
    })
    .await?;
    if let Some(e) = bad {
        return Err(Error::Protocol(format!("corrupt transfer: {e}")));
    }
    Ok(data)
}

pub async fn write_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str, data: &[u8], progress: Progress<'_>) -> Result<()> {
    let dir = parent(path);
    if dir != "/" {
        mkdir_in(repl, p, dir).await?;
    }
    let opened = repl.exec_str(&scripts::render(p, "put_open", &[("path", Py::Str(path))])?).await?;
    let b64 = opened.trim() == "1";
    let engine = base64::engine::general_purpose::STANDARD;
    let total = data.len() as u64;
    let mut done = 0u64;
    let res = async {
        for part in data.chunks(repl.chunk.max(64)) {
            let code = if b64 {
                format!("_mb_w(_mb_a({}))", py_bytes(engine.encode(part).as_bytes()))
            } else {
                format!("_mb_w({})", py_bytes(part))
            };
            repl.exec(&code).await?;
            done += part.len() as u64;
            progress(done, total);
        }
        Ok::<_, Error>(())
    }
    .await;
    let closed = repl.exec(&scripts::render(p, "put_close", &[])?).await;
    progress(total, total);
    res.and(closed.map(|_| ()))
}

pub async fn mkdir_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str) -> Result<()> {
    repl.exec(&scripts::render(p, "mkdir", &[("path", Py::Str(path))])?).await.map(|_| ())
}

pub async fn remove_in(repl: &mut RawRepl, p: &PlatformProfile, path: &str) -> Result<()> {
    repl.exec(&scripts::render(p, "rm", &[("path", Py::Str(path))])?).await.map(|_| ())
}

pub async fn rename_in(repl: &mut RawRepl, p: &PlatformProfile, src: &str, dst: &str) -> Result<()> {
    let code = scripts::render(p, "rename", &[("src", Py::Str(src)), ("dst", Py::Str(dst))])?;
    repl.exec(&code).await.map(|_| ())
}

// ---- session level ---------------------------------------------------------

/// Run one raw-REPL op and leave raw REPL afterwards.
macro_rules! raw_op {
    ($s:expr, |$r:ident, $p:ident| $body:expr) => {{
        let $p = $s.profile();
        let mut guard = $s.raw().await?;
        let $r: &mut RawRepl = &mut guard;
        let res = $body;
        guard.finish_with(res).await
    }};
}
pub(crate) use raw_op;

pub async fn list(s: &Session, path: &str) -> Result<Vec<DeviceEntry>> {
    raw_op!(s, |r, p| list_in(r, &p, path).await)
}

pub async fn walk(s: &Session, path: &str, hash: bool) -> Result<Vec<DeviceFile>> {
    raw_op!(s, |r, p| walk_in(r, &p, path, hash).await)
}

pub async fn read(s: &Session, path: &str, progress: Progress<'_>) -> Result<Vec<u8>> {
    if let Some(ft) = s.transport().file_transfer() {
        return ft.get(path, progress).await;
    }
    raw_op!(s, |r, p| read_in(r, &p, path, progress).await)
}

pub async fn write(s: &Session, path: &str, data: &[u8], progress: Progress<'_>) -> Result<()> {
    if let Some(ft) = s.transport().file_transfer() {
        let dir = parent(path);
        if dir != "/" {
            raw_op!(s, |r, p| mkdir_in(r, &p, dir).await)?;
        }
        return ft.put(path, data, progress).await;
    }
    raw_op!(s, |r, p| write_in(r, &p, path, data, progress).await)
}

pub async fn mkdir(s: &Session, path: &str) -> Result<()> {
    raw_op!(s, |r, p| mkdir_in(r, &p, path).await)
}

pub async fn remove(s: &Session, path: &str) -> Result<()> {
    raw_op!(s, |r, p| remove_in(r, &p, path).await)
}

pub async fn rename(s: &Session, src: &str, dst: &str) -> Result<()> {
    raw_op!(s, |r, p| rename_in(r, &p, src, dst).await)
}

pub async fn read_text(s: &Session, path: &str) -> Result<String> {
    let data = read(s, path, &crate::progress::none).await?;
    String::from_utf8(data).map_err(|_| Error::Unsupported(format!("{path} is not a text file")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        assert_eq!(join("/", "a.py"), "/a.py");
        assert_eq!(join("/lib", "a.py"), "/lib/a.py");
        assert_eq!(parent("/lib/a.py"), "/lib");
        assert_eq!(parent("/a.py"), "/");
        assert_eq!(parent("/flash/lib/x/"), "/flash/lib");
    }
}
