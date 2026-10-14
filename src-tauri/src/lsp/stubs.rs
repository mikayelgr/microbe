//! MicroPython type stubs for the language server.
//!
//! Downloads `micropython-<port>[-<board>]-stubs` and `micropython-stdlib-stubs`
//! wheels from PyPI and merges them into one custom *typeshed* directory:
//! on MicroPython every module is built in, and type checkers resolve built-in
//! module names (`time`, `gc`, `math`...) only from the stdlib path.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::tools;

const PYPI: &str = "https://pypi.org/pypi";
/// Port name for board-neutral stubs: the stdlib plus the modules every major
/// port ships (`machine`, `time`, `network`...), without port-only modules.
pub const GENERIC: &str = "generic";
/// Port stub packages whose common modules make up the generic set.
const GENERIC_FROM: &[&str] = &["micropython-rp2-stubs", "micropython-esp32-stubs"];

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StubSet {
    pub key: String,
    /// Directory containing `stdlib/` (pass as the type checker's typeshed path).
    pub typeshed: String,
    pub packages: Vec<String>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StubRequest {
    /// Port name (`rp2`, `esp32`...), or [`GENERIC`] for board-neutral stubs.
    pub port: String,
    #[serde(default)]
    pub board: Option<String>,
    /// MicroPython version on the device (`1.29.0`), `None` for latest.
    #[serde(default)]
    pub version: Option<String>,
    /// Session of a connected board to generate stubs from for modules the
    /// published stubs don't cover (see `introspect`).
    #[serde(default)]
    #[ts(optional)]
    pub introspect_session: Option<String>,
}

#[derive(Deserialize)]
struct PypiProject {
    info: PypiInfo,
    releases: std::collections::BTreeMap<String, Vec<PypiFile>>,
}

#[derive(Deserialize)]
struct PypiInfo {
    version: String,
}

#[derive(Deserialize, Clone)]
struct PypiFile {
    url: String,
    packagetype: String,
    filename: String,
    #[serde(default)]
    upload_time_iso_8601: Option<String>,
}

/// Board ids map to package names lower-cased and without variant suffix:
/// `ESP32_GENERIC_S3-SPIRAM_OCT` → `esp32_generic_s3`.
pub fn board_package(port: &str, board: &str) -> String {
    let base = board.split('-').next().unwrap_or(board).to_lowercase();
    format!("micropython-{port}-{base}-stubs")
}

async fn project(http: &reqwest::Client, name: &str) -> Result<Option<PypiProject>> {
    tools::get_json_opt(http, &format!("{PYPI}/{name}/json")).await
}

/// Latest wheel whose version starts with `want` (else the latest release).
fn pick_wheel(p: &PypiProject, want: Option<&str>) -> Option<(String, PypiFile)> {
    let wheel = |files: &Vec<PypiFile>| files.iter().find(|f| f.packagetype == "bdist_wheel").cloned();
    if let Some(want) = want {
        let mut matches: Vec<(String, PypiFile)> = p
            .releases
            .iter()
            .filter(|(v, _)| v.as_str() == want || v.starts_with(&format!("{want}.")))
            .filter_map(|(v, files)| wheel(files).map(|f| (v.clone(), f)))
            .collect();
        matches.sort_by(|a, b| a.1.upload_time_iso_8601.cmp(&b.1.upload_time_iso_8601));
        if let Some(m) = matches.pop() {
            return Some(m);
        }
    }
    p.releases.get(&p.info.version).and_then(wheel).map(|f| (p.info.version.clone(), f))
}

pub async fn ensure(http: &reqwest::Client, cache: &Path, req: &StubRequest) -> Result<StubSet> {
    let version = req.version.as_deref().map(|v| v.split('-').next().unwrap_or(v).to_string());
    let generic = req.port == GENERIC;
    let mut names: Vec<String> = Vec::new();
    if !generic {
        if let Some(b) = &req.board {
            names.push(board_package(&req.port, b));
        }
        names.push(format!("micropython-{}-stubs", req.port));
    }

    let key_base = format!(
        "{}-{}-{}",
        req.port,
        req.board.as_deref().map(|b| b.to_lowercase()).unwrap_or_else(|| "any".into()),
        version.clone().unwrap_or_else(|| "latest".into())
    );
    let set_dir = cache.join("sets").join(&key_base);
    let marker = set_dir.join("stubset.json");
    if let Ok(data) = std::fs::read(&marker) {
        if let Ok(set) = serde_json::from_slice::<StubSet>(&data) {
            return Ok(set);
        }
    }

    // Port stubs: the most specific package that exists, or for the generic
    // set, every package whose common modules we keep.
    let mut port_pkgs: Vec<(String, String, PypiFile)> = Vec::new();
    if generic {
        for name in GENERIC_FROM {
            let p = project(http, name).await?.ok_or_else(|| Error::NotFound(name.to_string()))?;
            let (v, f) = pick_wheel(&p, version.as_deref()).ok_or_else(|| Error::NotFound(format!("{name} wheel")))?;
            port_pkgs.push((name.to_string(), v, f));
        }
    } else {
        for name in &names {
            if let Some(p) = project(http, name).await? {
                if let Some((v, f)) = pick_wheel(&p, version.as_deref()) {
                    port_pkgs.push((name.clone(), v, f));
                    break;
                }
            }
        }
        if port_pkgs.is_empty() {
            return Err(Error::NotFound(format!("no stubs published for port {}", req.port)));
        }
    }
    let stdlib = project(http, "micropython-stdlib-stubs")
        .await?
        .ok_or_else(|| Error::NotFound("micropython-stdlib-stubs".into()))?;
    let (std_ver, std_file) = pick_wheel(&stdlib, version.as_deref())
        .ok_or_else(|| Error::NotFound("micropython-stdlib-stubs wheel".into()))?;

    let wheels = cache.join("wheels");
    let mut paths = Vec::new();
    for f in std::iter::once(&std_file).chain(port_pkgs.iter().map(|(_, _, f)| f)) {
        let dest = wheels.join(&f.filename);
        if !dest.is_file() {
            tools::download_to(http, &f.url, &dest, &crate::progress::none).await?;
        }
        paths.push(dest);
    }

    let tmp = set_dir.with_extension("tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    let stdlib_dir = tmp.join("stdlib");
    let std_whl = paths[0].clone();
    let port_whls: Vec<PathBuf> = paths[1..].to_vec();
    let sd = stdlib_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        // stdlib wheel: stdlib/* and _mpy_shed/* → stdlib/
        tools::extract_zip_filtered(&std_whl, &sd.join("..").join("_std"), |p| {
            p.starts_with("stdlib/") || p.starts_with("_mpy_shed/")
        })?;
        let std_root = sd.join("..").join("_std");
        copy_dir(&std_root.join("stdlib"), &sd)?;
        copy_dir(&std_root.join("_mpy_shed"), &sd.join("_mpy_shed"))?;
        std::fs::remove_dir_all(&std_root)?;
        // Port stubs win over stdlib ones. With several packages (generic set)
        // only modules present in all of them are kept.
        let common = common_modules(&port_whls)?;
        if let Some(first) = port_whls.first() {
            tools::extract_zip_filtered(first, &sd, |p| {
                let stub = !p.contains(".dist-info/") && (p.ends_with(".pyi") || p.ends_with(".py") || p.ends_with("py.typed"));
                stub && common.contains(top_module(p))
            })?;
        }
        write_versions(&sd)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::other(e.to_string()))??;

    let _ = std::fs::remove_dir_all(&set_dir);
    std::fs::rename(&tmp, &set_dir)?;
    let mut packages: Vec<String> = port_pkgs.iter().map(|(n, v, _)| format!("{n}=={v}")).collect();
    packages.push(format!("micropython-stdlib-stubs=={std_ver}"));
    let set = StubSet {
        key: key_base,
        typeshed: set_dir.display().to_string(),
        packages,
        version: port_pkgs.first().map(|(_, v, _)| v.clone()).unwrap_or(std_ver),
    };
    std::fs::write(&marker, serde_json::to_vec_pretty(&set)?)?;
    Ok(set)
}

/// Top-level module of a wheel entry: `machine.pyi` → `machine`, `rp2/asm.pyi` → `rp2`.
fn top_module(entry: &str) -> &str {
    let first = entry.split('/').next().unwrap_or(entry);
    first.split('.').next().unwrap_or(first)
}

/// Module names present in every wheel.
fn common_modules(wheels: &[PathBuf]) -> Result<std::collections::HashSet<String>> {
    let mut common: Option<std::collections::HashSet<String>> = None;
    for w in wheels {
        let zip = zip::ZipArchive::new(std::fs::File::open(w)?)?;
        let mods: std::collections::HashSet<String> = zip
            .file_names()
            .filter(|n| !n.contains(".dist-info/"))
            .map(|n| top_module(n).to_string())
            .collect();
        common = Some(match common {
            None => mods,
            Some(c) => c.intersection(&mods).cloned().collect(),
        });
    }
    Ok(common.unwrap_or_default())
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), to)?;
        }
    }
    Ok(())
}

/// typeshed's VERSIONS file: every top-level module present, available "3.0-".
pub fn write_versions(stdlib: &Path) -> Result<()> {
    let mut mods: Vec<String> = std::fs::read_dir(stdlib)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(m) = name.strip_suffix(".pyi") {
                Some(m.to_string())
            } else if e.path().is_dir() && !name.contains('.') {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    mods.sort();
    mods.dedup();
    let body: String = mods.iter().map(|m| format!("{m}: 3.0-\n")).collect();
    std::fs::write(stdlib.join("VERSIONS"), body)?;
    Ok(())
}

pub fn typeshed_path(set: &StubSet) -> PathBuf {
    PathBuf::from(&set.typeshed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names() {
        assert_eq!(board_package("rp2", "RPI_PICO2"), "micropython-rp2-rpi_pico2-stubs");
        assert_eq!(board_package("esp32", "ESP32_GENERIC_S3-SPIRAM_OCT"), "micropython-esp32-esp32_generic_s3-stubs");
    }

    #[test]
    fn versions_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("machine.pyi"), "").unwrap();
        std::fs::write(d.path().join("time.pyi"), "").unwrap();
        std::fs::create_dir(d.path().join("rp2")).unwrap();
        write_versions(d.path()).unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("VERSIONS")).unwrap(), "machine: 3.0-\nrp2: 3.0-\ntime: 3.0-\n");
    }
}
