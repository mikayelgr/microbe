//! Version-ranged protocol quirks (`src-tauri/quirks.toml`).

use serde::Deserialize;

use crate::error::{Error, Result};

const BUILTIN: &str = include_str!("../../quirks.toml");

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Quirk {
    #[serde(default)]
    pub description: String,
    pub versions: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub raw_paste: Option<bool>,
    #[serde(default)]
    pub transfer_chunk: Option<usize>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    quirk: Vec<Quirk>,
}

/// The combined effect of every quirk matching a board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Effective {
    pub raw_paste: Option<bool>,
    pub transfer_chunk: Option<usize>,
}

pub fn load() -> Result<Vec<Quirk>> {
    toml::from_str::<File>(BUILTIN)
        .map(|f| f.quirk)
        .map_err(|e| Error::other(format!("quirks.toml: {e}")))
}

pub fn effective(quirks: &[Quirk], platform: &str, version: &str) -> Effective {
    let mut out = Effective::default();
    for q in quirks {
        let platform_ok = q.platforms.is_empty() || q.platforms.iter().any(|p| p == platform);
        if platform_ok && version_matches(version, &q.versions) {
            out.raw_paste = q.raw_paste.or(out.raw_paste);
            out.transfer_chunk = match (out.transfer_chunk, q.transfer_chunk) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
    }
    out
}

fn parse_version(v: &str) -> Option<Vec<u32>> {
    let core = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let parts: Option<Vec<u32>> = core.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

fn cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    (0..n).map(|i| a.get(i).unwrap_or(&0).cmp(b.get(i).unwrap_or(&0))).find(|o| o.is_ne()).unwrap_or(std::cmp::Ordering::Equal)
}

/// `version_matches("1.13.0", "<1.14")`; comparisons are comma-separated and all must hold.
pub fn version_matches(version: &str, spec: &str) -> bool {
    let Some(v) = parse_version(version) else { return false };
    spec.split(',').map(str::trim).filter(|s| !s.is_empty()).all(|clause| {
        let (op, rest) = ["<=", ">=", "==", "<", ">", "="]
            .iter()
            .find_map(|op| clause.strip_prefix(op).map(|r| (*op, r)))
            .unwrap_or(("==", clause));
        let Some(t) = parse_version(rest) else { return false };
        let o = cmp(&v, &t);
        match op {
            "<" => o.is_lt(),
            "<=" => o.is_le(),
            ">" => o.is_gt(),
            ">=" => o.is_ge(),
            _ => o.is_eq(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_specs() {
        assert!(version_matches("1.13.0", "<1.14"));
        assert!(!version_matches("1.14.0", "<1.14"));
        assert!(version_matches("1.21.0", ">=1.20, <1.23"));
        assert!(version_matches("1.30.0-preview", ">=1.30"));
        assert!(!version_matches("garbage", "<1.14"));
    }

    #[test]
    fn builtin_quirks_apply() {
        let q = load().unwrap();
        assert_eq!(effective(&q, "rp2", "1.13.0").raw_paste, Some(false));
        assert_eq!(effective(&q, "rp2", "1.29.0"), Effective::default());
    }

    #[test]
    fn platform_filter_and_merging() {
        let q = vec![
            Quirk { versions: "<2".into(), platforms: vec!["esp8266".into()], transfer_chunk: Some(256), ..Default::default() },
            Quirk { versions: "<2".into(), transfer_chunk: Some(512), ..Default::default() },
        ];
        assert_eq!(effective(&q, "esp8266", "1.0").transfer_chunk, Some(256));
        assert_eq!(effective(&q, "rp2", "1.0").transfer_chunk, Some(512));
    }
}
