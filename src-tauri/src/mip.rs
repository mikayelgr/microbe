//! Host-side `mip` package installation (a port of `mpremote mip`).
//!
//! The host resolves and downloads the package (and its deps) from the
//! micropython-lib index, GitHub or GitLab, then writes the files to the device.
//! This works on boards without networking.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use ts_rs::TS;

use crate::cache;
use crate::error::{Error, Result};
use crate::fs;
use crate::progress;
use crate::tools;
use crate::progress::Reporter;
use crate::state::AppState;

pub const INDEX: &str = "https://micropython.org/pi/v2";
const URL_PREFIXES: &[&str] = &["http://", "https://", "github:", "gitlab:"];

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PackageInfo {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
}

#[derive(Serialize, Deserialize)]
struct IndexJson {
    packages: Vec<PackageInfo>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct InstallOptions {
    /// Index package name, `github:org/repo[/path]`, `gitlab:...`, or an URL.
    pub package: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Destination directory; defaults to `<fs_root>/lib`.
    #[serde(default)]
    pub target: Option<String>,
    /// Install precompiled .mpy files when available (default true).
    #[serde(default = "yes")]
    pub mpy: bool,
    #[serde(default)]
    pub index: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct InstallReport {
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Deserialize, Default)]
struct PackageJson {
    #[serde(default)]
    hashes: Vec<(String, String)>,
    #[serde(default)]
    urls: Vec<(String, String)>,
    #[serde(default)]
    deps: Vec<(String, Option<String>)>,
}

/// A file to put on the device.
struct Planned {
    path: String,
    url: String,
    short_hash: Option<String>,
}

pub fn rewrite_url(url: &str, branch: Option<&str>) -> String {
    let branch = branch.unwrap_or("HEAD");
    if let Some(rest) = url.strip_prefix("github:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            return format!(
                "https://raw.githubusercontent.com/{}/{}/{}/{}",
                parts[0],
                parts[1],
                branch,
                parts[2..].join("/")
            );
        }
    } else if let Some(rest) = url.strip_prefix("gitlab:") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            return format!("https://gitlab.com/{}/{}/-/raw/{}/{}", parts[0], parts[1], branch, parts[2..].join("/"));
        }
    }
    url.to_string()
}

fn is_url(s: &str) -> bool {
    URL_PREFIXES.iter().any(|p| s.starts_with(p))
}

struct Resolver<'a> {
    http: &'a reqwest::Client,
    index: String,
    target: String,
    mpy: String,
    seen: HashSet<String>,
    plan: Vec<Planned>,
}

impl Resolver<'_> {
    async fn package(&mut self, package: &str, version: Option<&str>) -> Result<()> {
        let key = format!("{package}@{}", version.unwrap_or(""));
        if !self.seen.insert(key) {
            return Ok(());
        }
        let json_url = if is_url(package) {
            if package.ends_with(".py") || package.ends_with(".mpy") {
                let name = package.rsplit('/').next().unwrap_or(package);
                self.plan.push(Planned {
                    path: fs::join(&self.target, name),
                    url: rewrite_url(package, version),
                    short_hash: None,
                });
                return Ok(());
            }
            let mut p = package.to_string();
            if !p.ends_with(".json") {
                if !p.ends_with('/') {
                    p.push('/');
                }
                p.push_str("package.json");
            }
            p
        } else {
            format!("{}/package/{}/{}/{}.json", self.index, self.mpy, package, version.unwrap_or("latest"))
        };
        self.json(&json_url, version).await
    }

    async fn json(&mut self, json_url: &str, version: Option<&str>) -> Result<()> {
        let pkg: PackageJson = tools::get_json_opt(self.http, &rewrite_url(json_url, version))
            .await?
            .ok_or_else(|| Error::NotFound(format!("package not found: {json_url}")))?;
        let base_url = json_url.rsplit_once('/').map(|(b, _)| b.to_string()).unwrap_or_default();
        for (path, hash) in &pkg.hashes {
            self.plan.push(Planned {
                path: fs::join(&self.target, path),
                url: format!("{}/file/{}/{}", self.index, &hash[..2.min(hash.len())], hash),
                short_hash: Some(hash.clone()),
            });
        }
        for (path, url) in &pkg.urls {
            let url = if is_url(url) { url.clone() } else { format!("{base_url}/{url}") };
            self.plan.push(Planned { path: fs::join(&self.target, path), url: rewrite_url(&url, version), short_hash: None });
        }
        for (dep, dep_version) in &pkg.deps {
            Box::pin(self.package(dep, dep_version.as_deref())).await?;
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn packages_index(state: State<'_, AppState>, force: bool) -> Result<Vec<PackageInfo>> {
    let path = state.cache_dir("mip")?.join("index.json");
    let http = &state.http;
    let idx: IndexJson = cache::cached_json(&path, cache::DAY, force, || async {
        tools::get_json_opt(http, &format!("{INDEX}/index.json"))
            .await?
            .ok_or_else(|| Error::NotFound("package index".into()))
    })
    .await?;
    Ok(idx.packages)
}

#[tauri::command]
pub async fn packages_install(app: AppHandle, state: State<'_, AppState>, session_id: String, options: InstallOptions) -> Result<InstallReport> {
    let session = state.session(&session_id)?;
    let reporter = Reporter::new(&app, "mip", format!("Resolving {}…", options.package));
    let profile = session.profile();
    let info = session.info.lock().unwrap().clone();
    let mpy = match (options.mpy, info.as_ref().and_then(|i| i.mpy_version())) {
        (true, Some(v)) => v.to_string(),
        _ => "py".into(),
    };
    let target = options.target.clone().unwrap_or_else(|| fs::join(&profile.fs_root, "lib"));
    let mut resolver = Resolver {
        http: &state.http,
        index: options.index.clone().unwrap_or_else(|| INDEX.into()).trim_end_matches('/').to_string(),
        target,
        mpy,
        seen: HashSet::new(),
        plan: vec![],
    };
    resolver.package(&options.package, options.version.as_deref()).await?;
    let plan = std::mem::take(&mut resolver.plan);

    let mut report = InstallReport::default();
    let has_hashlib = info.as_ref().is_some_and(|i| i.has_module("hashlib"));
    let mut raw = session.raw().await?;
    let total = plan.len() as u64;
    let res = async {
    for (i, item) in plan.iter().enumerate() {
        reporter.update(i as u64, total);
        if let (Some(h), true) = (&item.short_hash, has_hashlib) {
            if let Ok(full) = fs::hash_in(&mut raw, &profile, &item.path).await {
                if full.starts_with(h.as_str()) {
                    report.skipped.push(item.path.clone());
                    continue;
                }
            }
        }
        reporter.set_label(format!("Installing {}", item.path));
        let data = tools::get_bytes(&state.http, &item.url).await?;
        fs::write_in(&mut raw, &profile, &item.path, &data, &progress::none).await?;
        report.installed.push(item.path.clone());
    }
    Ok(report)
    }
    .await;
    raw.finish_with(res).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_repo_urls() {
        assert_eq!(
            rewrite_url("github:org/repo/pkg/package.json", None),
            "https://raw.githubusercontent.com/org/repo/HEAD/pkg/package.json"
        );
        assert_eq!(rewrite_url("gitlab:org/repo/a.py", Some("v1")), "https://gitlab.com/org/repo/-/raw/v1/a.py");
        assert_eq!(rewrite_url("https://x/y.py", None), "https://x/y.py");
    }
}
