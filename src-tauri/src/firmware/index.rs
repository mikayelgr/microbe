//! The micropython.org firmware catalogue.
//!
//! micropython.org has no JSON API, so we read the download pages:
//! * `/download/` lists every board as `<a class="board-card" href="BOARD_ID">`,
//! * `/download/?port=<port>` lists the boards of one port,
//! * `/download/<BOARD_ID>/` has `<h2>Firmware (variant)</h2>` sections whose
//!   links follow `/resources/firmware/<BOARD>[-VARIANT]-<date>-v<ver>.<ext>`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use regex::Regex;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::cache;
use crate::error::{Error, Result};
use crate::tools::get_text;

pub const BASE: &str = "https://micropython.org";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BoardSummary {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub port: Option<String>,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BoardIndex {
    pub boards: Vec<BoardSummary>,
    pub ports: Vec<String>,
    #[ts(type = "number")]
    pub fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FirmwareFile {
    pub ext: String,
    pub url: String,
    pub file_name: String,
}

/// One version of one variant, available in several file formats.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FirmwareBuild {
    pub version: String,
    pub date: String,
    pub preview: bool,
    pub files: Vec<FirmwareFile>,
}

impl FirmwareBuild {
    /// The first file matching the preferred extensions.
    pub fn pick(&self, prefs: &[String]) -> Option<&FirmwareFile> {
        prefs.iter().find_map(|e| self.files.iter().find(|f| &f.ext == e))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FirmwareVariant {
    /// e.g. "Firmware (Support for Octal-SPIRAM)".
    pub title: String,
    /// Variant suffix in file names (`SPIRAM_OCT`), `None` for the standard build.
    pub variant: Option<String>,
    pub builds: Vec<FirmwareBuild>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BoardFirmware {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub port: Option<String>,
    pub features: Vec<String>,
    pub variants: Vec<FirmwareVariant>,
}

impl BoardFirmware {
    pub fn variant(&self, v: Option<&str>) -> Option<&FirmwareVariant> {
        self.variants.iter().find(|x| x.variant.as_deref() == v)
    }
}

pub async fn load_index(http: &reqwest::Client, cache: &Path, force: bool) -> Result<BoardIndex> {
    cache::cached_json(&cache.join("board-index.json"), cache::DAY, force, || fetch_index(http)).await
}

async fn fetch_index(http: &reqwest::Client) -> Result<BoardIndex> {
    let html = get_text(http, &format!("{BASE}/download/")).await?;
    let mut boards = parse_board_cards(&html);
    let ports = parse_ports(&html);
    // Map boards to ports with one request per port.
    let mut tasks = Vec::new();
    for port in &ports {
        let http = http.clone();
        let port = port.clone();
        tasks.push(tokio::spawn(async move {
            let html = get_text(&http, &format!("{BASE}/download/?port={port}")).await.ok()?;
            Some((port, parse_board_cards(&html).into_iter().map(|b| b.id).collect::<Vec<_>>()))
        }));
    }
    let mut port_of: BTreeMap<String, String> = BTreeMap::new();
    for t in tasks {
        if let Ok(Some((port, ids))) = t.await {
            for id in ids {
                port_of.insert(id, port.clone());
            }
        }
    }
    for b in &mut boards {
        b.port = port_of.get(&b.id).cloned();
    }
    let fetched_at = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Ok(BoardIndex { boards, ports, fetched_at })
}

pub fn parse_board_cards(html: &str) -> Vec<BoardSummary> {
    let doc = Html::parse_document(html);
    let card = Selector::parse("a.board-card").unwrap();
    let product = Selector::parse(".board-product").unwrap();
    let vendor = Selector::parse(".board-vendor").unwrap();
    let img = Selector::parse("img").unwrap();
    let text = |e: scraper::ElementRef, s: &Selector| {
        e.select(s).next().map(|x| x.text().collect::<String>().trim().to_string()).unwrap_or_default()
    };
    doc.select(&card)
        .filter_map(|c| {
            let id = c.value().attr("href")?.trim_matches('/').to_string();
            Some(BoardSummary {
                name: text(c, &product),
                vendor: text(c, &vendor),
                image: c.select(&img).next().and_then(|i| i.value().attr("src")).map(str::to_string),
                port: None,
                id,
            })
        })
        .collect()
}

fn parse_ports(html: &str) -> Vec<String> {
    let re = Regex::new(r#"href="\?port=([A-Za-z0-9_-]+)""#).unwrap();
    let mut ports: Vec<String> = re.captures_iter(html).map(|c| c[1].to_string()).collect();
    ports.sort();
    ports.dedup();
    ports
}

pub async fn board_firmware(http: &reqwest::Client, cache: &Path, id: &str) -> Result<BoardFirmware> {
    cache::cached_json(&cache.join(format!("board-{id}.json")), cache::DAY, false, || async {
        parse_board_page(id, &get_text(http, &format!("{BASE}/download/{id}/")).await?)
    })
    .await
}

pub fn parse_board_page(id: &str, html: &str) -> Result<BoardFirmware> {
    let doc = Html::parse_document(html);
    let h2 = Selector::parse("h2").unwrap();
    let name = doc
        .select(&h2)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_else(|| id.to_string());

    let strong_field = |label: &str| -> Option<String> {
        let re = Regex::new(&format!(r"<strong>{label}:</strong>\s*([^<]*)")).unwrap();
        re.captures(html).map(|c| c[1].trim().to_string())
    };
    let vendor = strong_field("Vendor").unwrap_or_default();
    let features = strong_field("Features")
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let port = Regex::new(r"/ports/([A-Za-z0-9_-]+)/boards/")
        .unwrap()
        .captures(html)
        .map(|c| c[1].to_string());

    // Split at each "Firmware" section heading.
    let section_re = Regex::new(r"<h2>\s*(Firmware[^<]*)</h2>").unwrap();
    let starts: Vec<(usize, String)> = section_re
        .captures_iter(html)
        .map(|c| (c.get(0).unwrap().start(), c[1].trim().to_string()))
        .collect();
    let link_re = Regex::new(r#"href="(/resources/firmware/([^"]+))""#).unwrap();
    let file_re = Regex::new(&format!(
        r"^{}(?:-([A-Za-z0-9_]+))?-(\d{{8}})-v(\d+\.\d+\.\d+(?:-preview\.\d+\.g[0-9a-f]+)?)\.(meta\.zip|[a-z0-9-]+)$",
        regex::escape(id)
    ))
    .unwrap();

    let mut variants: Vec<FirmwareVariant> = Vec::new();
    for (i, (start, title)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map(|s| s.0).unwrap_or(html.len());
        let section = &html[*start..end];
        let mut builds: Vec<FirmwareBuild> = Vec::new();
        let mut variant_name: Option<Option<String>> = None;
        for c in link_re.captures_iter(section) {
            let file_name = c[2].to_string();
            let Some(m) = file_re.captures(&file_name) else { continue };
            let ext = m[4].to_string();
            if ext.ends_with("meta.zip") || ext == "elf" || ext == "map" {
                continue;
            }
            variant_name.get_or_insert_with(|| m.get(1).map(|v| v.as_str().to_string()));
            let version = m[3].to_string();
            let date = format!("{}-{}-{}", &m[2][..4], &m[2][4..6], &m[2][6..]);
            let file = FirmwareFile { ext, url: format!("{BASE}{}", &c[1]), file_name };
            match builds.iter_mut().find(|b| b.version == version) {
                Some(b) => b.files.push(file),
                None => builds.push(FirmwareBuild {
                    preview: version.contains("preview"),
                    version,
                    date,
                    files: vec![file],
                }),
            }
        }
        if !builds.is_empty() {
            variants.push(FirmwareVariant { title: title.clone(), variant: variant_name.flatten(), builds });
        }
    }
    if variants.is_empty() {
        return Err(Error::NotFound(format!("no firmware listed for {id}")));
    }
    Ok(BoardFirmware { id: id.into(), name, vendor, port, features, variants })
}

/// Download a firmware file into the cache (files are immutable, so cached forever).
pub async fn download(
    http: &reqwest::Client,
    cache: &Path,
    file: &FirmwareFile,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<std::path::PathBuf> {
    let dest = cache.join(&file.file_name);
    if dest.is_file() && std::fs::metadata(&dest)?.len() > 0 {
        return Ok(dest);
    }
    crate::tools::download_to(http, &file.url, &dest, progress).await?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"
<h2> ESP32-S3 </h2>
<div><strong>Vendor:</strong> Espressif</div>
<div><strong>Features:</strong> BLE, External Flash, WiFi</div>
<div><strong>Source on GitHub:</strong> <a href="https://github.com/micropython/micropython/tree/master/ports/esp32/boards/ESP32_GENERIC_S3">x</a></div>
<h2>Installation instructions</h2>
<h2>Firmware</h2>
<h3>Releases</h3>
<a href="/resources/firmware/ESP32_GENERIC_S3-20260824-v1.29.0.uf2">v1.29.0</a>
/ <a href="/resources/firmware/ESP32_GENERIC_S3-20260824-v1.29.0.app-bin">[.app-bin]</a>
/ <a href="/resources/firmware/ESP32_GENERIC_S3-20260824-v1.29.0.bin">[.bin]</a>
/ <a href="/resources/firmware/ESP32_GENERIC_S3-20260824-v1.29.0.meta.zip">[.elf]</a>
<a href="/resources/firmware/ESP32_GENERIC_S3-20260406-v1.28.0.bin">v1.28.0</a>
<h3>Preview builds</h3>
<a href="/resources/firmware/ESP32_GENERIC_S3-20261013-v1.30.0-preview.127.g4a1fa4a563.bin">p</a>
<a href="/resources/firmware/ESP32_GENERIC_S3-20261013-v1.30.0-preview.127.g4a1fa4a563.elf">p</a>
<h2>Firmware (Support for Octal-SPIRAM)</h2>
<a href="/resources/firmware/ESP32_GENERIC_S3-SPIRAM_OCT-20260824-v1.29.0.bin">v1.29.0</a>
"#;

    #[test]
    fn parses_board_page() {
        let fw = parse_board_page("ESP32_GENERIC_S3", PAGE).unwrap();
        assert_eq!(fw.name, "ESP32-S3");
        assert_eq!(fw.vendor, "Espressif");
        assert_eq!(fw.port.as_deref(), Some("esp32"));
        assert_eq!(fw.features, vec!["BLE", "External Flash", "WiFi"]);
        assert_eq!(fw.variants.len(), 2);
        let std = fw.variant(None).unwrap();
        assert_eq!(std.builds.len(), 3);
        assert_eq!(std.builds[0].version, "1.29.0");
        assert_eq!(std.builds[0].date, "2026-08-24");
        assert_eq!(std.builds[0].files.len(), 3);
        assert_eq!(std.builds[0].pick(&["bin".into()]).unwrap().ext, "bin");
        assert!(std.builds[2].preview);
        assert_eq!(std.builds[2].version, "1.30.0-preview.127.g4a1fa4a563");
        assert_eq!(std.builds[2].files.len(), 1);
        let oct = fw.variant(Some("SPIRAM_OCT")).unwrap();
        assert!(oct.title.contains("Octal"));
    }

    #[test]
    fn parses_cards_and_ports() {
        let html = r#"<a href="?port=rp2" class="filter-option">rp2</a>
            <a class="board-card" href="RPI_PICO2"><img src="x.jpg">
            <div class="board-product">Pico 2</div><div class="board-vendor">Raspberry Pi</div></a>"#;
        let cards = parse_board_cards(html);
        assert_eq!(cards[0].id, "RPI_PICO2");
        assert_eq!(cards[0].name, "Pico 2");
        assert_eq!(parse_ports(html), vec!["rp2"]);
    }
}
