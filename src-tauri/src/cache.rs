//! On-disk caching of downloaded JSON (board index, package index...).
//!
//! `cached_json` returns a fresh cached copy, otherwise fetches and stores a
//! new one, and falls back to a stale copy when offline.

use std::future::Future;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::Result;

pub const DAY: Duration = Duration::from_secs(24 * 3600);

/// Whether `path` exists and was written less than `ttl` ago.
pub fn is_fresh(path: &Path, ttl: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age < ttl)
}

pub fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec(value)?)?;
    Ok(())
}

pub async fn cached_json<T, F, Fut>(path: &Path, ttl: Duration, force: bool, fetch: F) -> Result<T>
where
    T: Serialize + DeserializeOwned,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    if !force && is_fresh(path, ttl) {
        if let Some(v) = read(path) {
            return Ok(v);
        }
    }
    match fetch().await {
        Ok(v) => {
            write(path, &v)?;
            Ok(v)
        }
        Err(e) => read(path).ok_or(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[tokio::test]
    async fn fresh_then_stale_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.json");
        let v: Vec<u32> = cached_json(&p, DAY, false, || async { Ok(vec![1, 2]) }).await.unwrap();
        assert_eq!(v, vec![1, 2]);
        // Fresh: the fetcher isn't called.
        let v: Vec<u32> = cached_json(&p, DAY, false, || async { Err(Error::other("unused")) }).await.unwrap();
        assert_eq!(v, vec![1, 2]);
        // Forced refresh that fails falls back to the cached copy.
        let v: Vec<u32> = cached_json(&p, DAY, true, || async { Err(Error::other("offline")) }).await.unwrap();
        assert_eq!(v, vec![1, 2]);
        // Nothing cached and the fetch fails: the error surfaces.
        let q = dir.path().join("y.json");
        assert!(cached_json::<Vec<u32>, _, _>(&q, DAY, false, || async { Err(Error::other("offline")) }).await.is_err());
    }
}
