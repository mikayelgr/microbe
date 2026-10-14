//! Upload a local project to the device, transferring only changed files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::error::Result;
use crate::fs::{self, DeviceFile};
use crate::progress::Reporter;
use crate::platform::PlatformProfile;
use crate::repl::raw::RawRepl;
use crate::repl::session::Session;

pub const DEFAULT_IGNORE: &[&str] = &[
    ".git", ".microbe", "__pycache__", ".vscode", ".idea", "node_modules", ".venv", "venv",
    ".DS_Store", "*.pyi", "*.pyc", "ty.toml", "pyrightconfig.json", "pyproject.toml",
];

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct SyncOptions {
    pub local_dir: String,
    pub remote_dir: String,
    /// Extra ignore patterns: exact names or `*.ext`.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Remove device files that don't exist locally.
    #[serde(default)]
    pub delete_extra: bool,
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct SyncReport {
    pub uploaded: Vec<String>,
    pub deleted: Vec<String>,
    pub unchanged: u32,
}

pub fn ignored(name: &str, extra: &[String]) -> bool {
    DEFAULT_IGNORE.iter().copied().chain(extra.iter().map(String::as_str)).any(|pat| {
        match pat.strip_prefix('*') {
            Some(suffix) => name.ends_with(suffix),
            None => name == pat,
        }
    })
}

/// Local files relative to `root`, with forward slashes.
pub fn local_files(root: &Path, extra_ignore: &[String]) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if ignored(&name, extra_ignore) {
                continue;
            }
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

pub async fn sync(session: &Session, opts: &SyncOptions, reporter: &Reporter) -> Result<SyncReport> {
    let root = Path::new(&opts.local_dir);
    let files = local_files(root, &opts.ignore)?;
    let profile = session.profile();
    let remote_root = if opts.remote_dir.is_empty() { profile.fs_root.clone() } else { opts.remote_dir.clone() };
    let mut raw = session.raw().await?;
    let res = sync_in(&mut raw, &profile, &files, &remote_root, opts, reporter).await;
    raw.finish_with(res).await
}

async fn sync_in(
    raw: &mut RawRepl,
    profile: &PlatformProfile,
    files: &[(String, PathBuf)],
    remote_root: &str,
    opts: &SyncOptions,
    reporter: &Reporter,
) -> Result<SyncReport> {
    let mut report = SyncReport::default();
    reporter.set_label("Comparing files…");
    let remote: HashMap<String, DeviceFile> = match fs::walk_in(raw, profile, remote_root, true).await {
        Ok(list) => list.into_iter().map(|f| (f.path.clone(), f)).collect(),
        Err(_) => HashMap::new(), // remote dir doesn't exist yet
    };

    let mut to_upload = Vec::new();
    for (rel, path) in files {
        let remote_path = fs::join(remote_root, rel);
        let data = std::fs::read(path)?;
        let changed = match remote.get(&remote_path) {
            None => true,
            Some(r) if r.is_dir => true,
            Some(r) => match &r.sha256 {
                Some(h) => *h != hex::encode(Sha256::digest(&data)),
                None => r.size != data.len() as u64,
            },
        };
        if changed {
            to_upload.push((remote_path, data));
        } else {
            report.unchanged += 1;
        }
    }

    let local_set: std::collections::HashSet<String> =
        files.iter().map(|(rel, _)| fs::join(remote_root, rel)).collect();
    let mut to_delete: Vec<String> = if opts.delete_extra {
        remote
            .values()
            .filter(|f| !f.is_dir && !local_set.contains(&f.path))
            .map(|f| f.path.clone())
            .collect()
    } else {
        vec![]
    };
    to_delete.sort();

    if !opts.dry_run {
        let total: u64 = to_upload.iter().map(|(_, d)| d.len() as u64).sum();
        let mut base = 0u64;
        for (remote_path, data) in &to_upload {
            reporter.set_label(format!("Uploading {remote_path}"));
            let len = data.len() as u64;
            fs::write_in(raw, profile, remote_path, data, &|d, _| reporter.update(base + d, total)).await?;
            base += len;
            report.uploaded.push(remote_path.clone());
        }
        for path in &to_delete {
            reporter.set_label(format!("Removing {path}"));
            fs::remove_in(raw, profile, path).await?;
            report.deleted.push(path.clone());
        }
    } else {
        report.uploaded = to_upload.into_iter().map(|(p, _)| p).collect();
        report.deleted = to_delete;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignore_patterns() {
        assert!(ignored(".git", &[]));
        assert!(ignored("machine.pyi", &[]));
        assert!(!ignored("main.py", &[]));
        assert!(ignored("secrets.py", &["secrets.py".into()]));
    }

    #[test]
    fn walks_local_tree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("lib/sub")).unwrap();
        std::fs::create_dir_all(dir.path().join("__pycache__")).unwrap();
        std::fs::write(dir.path().join("main.py"), "x").unwrap();
        std::fs::write(dir.path().join("lib/sub/a.py"), "y").unwrap();
        std::fs::write(dir.path().join("__pycache__/z.pyc"), "z").unwrap();
        let rels: Vec<_> = local_files(dir.path(), &[]).unwrap().into_iter().map(|(r, _)| r).collect();
        assert_eq!(rels, vec!["lib/sub/a.py", "main.py"]);
    }
}
