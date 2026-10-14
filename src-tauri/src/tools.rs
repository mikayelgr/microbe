//! Downloading and caching external helper tools (esptool, mpy-cross, ...).
//!
//! A [`ToolSpec`] says where to get a tool for the host OS/arch. Tools are
//! fetched on first use into `<cache>/tools/<name>-<version>/`.

use std::io::Read;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;

use crate::error::{Error, Result};

pub async fn get_text(http: &reqwest::Client, url: &str) -> Result<String> {
    Ok(http.get(url).send().await?.error_for_status()?.text().await?)
}

pub async fn get_bytes(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    Ok(http.get(url).send().await?.error_for_status()?.bytes().await?.to_vec())
}

/// GET and decode JSON; `Ok(None)` on 404.
pub async fn get_json_opt<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<Option<T>> {
    let resp = http.get(url).send().await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    Ok(Some(resp.error_for_status()?.json().await?))
}

pub async fn download_to(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<()> {
    let resp = http.get(url).send().await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let tmp = dest.with_extension("part");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    let mut done = 0u64;
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().await?;
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::other(format!("incomplete download of {url} ({done}/{total} bytes)")));
    }
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

pub struct ToolSpec {
    pub name: &'static str,
    pub version: &'static str,
    /// Download URL for the host, or `None` if unavailable on this platform.
    pub url: fn(&'static str) -> Option<String>,
    /// Executable name inside the archive (without `.exe`).
    pub binary: &'static str,
}

pub const ESPTOOL: ToolSpec = ToolSpec {
    name: "esptool",
    version: "v5.5.0",
    binary: "esptool",
    url: |v| {
        let plat = match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "macos-arm64.tar.gz",
            ("macos", "x86_64") => "macos-amd64.tar.gz",
            ("linux", "x86_64") => "linux-amd64.tar.gz",
            ("linux", "aarch64") => "linux-aarch64.tar.gz",
            ("linux", "arm") => "linux-armv7.tar.gz",
            ("windows", "x86_64") => "windows-amd64.zip",
            _ => return None,
        };
        Some(format!("https://github.com/espressif/esptool/releases/download/{v}/esptool-{v}-{plat}"))
    },
};

/// Path to the tool's executable, downloading it first if needed.
pub async fn ensure(
    http: &reqwest::Client,
    cache: &Path,
    spec: &ToolSpec,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<PathBuf> {
    let dir = cache.join("tools").join(format!("{}-{}", spec.name, spec.version));
    let exe_name = format!("{}{}", spec.binary, std::env::consts::EXE_SUFFIX);
    if let Some(found) = find_file(&dir, &exe_name) {
        return Ok(found);
    }
    let url = (spec.url)(spec.version)
        .ok_or_else(|| Error::Unsupported(format!("{} is not available for this platform", spec.name)))?;
    let archive = dir.with_extension(if url.ends_with(".zip") { "zip" } else { "tar.gz" });
    download_to(http, &url, &archive, progress).await?;
    let (a, d) = (archive.clone(), dir.clone());
    tokio::task::spawn_blocking(move || extract(&a, &d))
        .await
        .map_err(|e| Error::other(e.to_string()))??;
    let _ = std::fs::remove_file(&archive);
    let exe = find_file(&dir, &exe_name)
        .ok_or_else(|| Error::other(format!("{exe_name} not found in {url}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(exe)
}

pub fn extract(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive.to_string_lossy();
    if name.ends_with(".zip") || name.ends_with(".whl") {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        zip.extract(dest)?;
    } else {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        tar::Archive::new(gz).unpack(dest)?;
    }
    Ok(())
}

/// Extract only the entries of a zip (e.g. a wheel) whose path passes `keep`.
pub fn extract_zip_filtered(archive: &Path, dest: &Path, keep: impl Fn(&str) -> bool) -> Result<usize> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if entry.is_dir() || !keep(&rel_str) {
            continue;
        }
        let out = dest.join(&rel);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(out, buf)?;
        n += 1;
    }
    Ok(n)
}

pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            dirs.push(p);
        } else if e.file_name() == name {
            return Some(p);
        }
    }
    dirs.into_iter().find_map(|d| find_file(&d, name))
}

/// Look up an executable on `$PATH`, plus common install locations that GUI
/// apps on macOS don't inherit in their environment.
pub fn which(name: &str) -> Option<PathBuf> {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let path = std::env::var_os("PATH").unwrap_or_default();
    let extra = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"].map(PathBuf::from);
    std::env::split_paths(&path)
        .chain(extra)
        .map(|d| d.join(&exe))
        .find(|p| p.is_file())
}
