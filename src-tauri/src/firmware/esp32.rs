//! ESP32 partition tables and app images.
//!
//! The table can come from the running board (`esp32.Partition`) or from a
//! full firmware image (the binary table at flash offset 0x8000). OTA updates
//! (`flashers::ota`) write an *app* image into the next OTA partition; a full
//! image's app is found through its table.

use serde::{Deserialize, Serialize};
use tauri::State;
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Partition {
    pub label: String,
    /// `app` or `data` (or the numeric type).
    pub kind: String,
    #[serde(rename = "type_id")]
    pub type_id: u8,
    pub subtype: u8,
    /// `factory`, `ota_0`, `nvs`, `fat`…
    pub subtype_name: String,
    #[ts(type = "number")]
    pub offset: u32,
    #[ts(type = "number")]
    pub size: u32,
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct PartitionTable {
    pub partitions: Vec<Partition>,
    /// Label of the partition the firmware runs from.
    pub running: Option<String>,
    /// Where an OTA update would go (`None`: no OTA partitions).
    pub next_update: Option<String>,
    /// The table as an ESP-IDF partitions CSV.
    pub csv: String,
}

pub fn partition(type_id: u8, subtype: u8, offset: u32, size: u32, label: &str, encrypted: bool) -> Partition {
    let kind = match type_id {
        0 => "app".to_string(),
        1 => "data".to_string(),
        t => format!("0x{t:02x}"),
    };
    let subtype_name = match (type_id, subtype) {
        (0, 0x00) => "factory".into(),
        (0, s @ 0x10..=0x1f) => format!("ota_{}", s - 0x10),
        (0, 0x20) => "test".into(),
        (1, 0x00) => "ota".into(),
        (1, 0x01) => "phy".into(),
        (1, 0x02) => "nvs".into(),
        (1, 0x03) => "coredump".into(),
        (1, 0x04) => "nvs_keys".into(),
        (1, 0x05) => "efuse".into(),
        (1, 0x81) => "fat".into(),
        (1, 0x82) => "spiffs".into(),
        (1, 0x83) => "littlefs".into(),
        (_, s) => format!("0x{s:02x}"),
    };
    Partition { label: label.into(), kind, type_id, subtype, subtype_name, offset, size, encrypted }
}

/// Parse a binary partition table (32-byte entries starting with `AA 50`).
pub fn parse_table(bytes: &[u8]) -> Vec<Partition> {
    bytes
        .chunks_exact(32)
        .take_while(|e| e[0] == 0xAA && e[1] == 0x50)
        .map(|e| {
            let u32_at = |i: usize| u32::from_le_bytes([e[i], e[i + 1], e[i + 2], e[i + 3]]);
            let label: String = e[12..28].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
            partition(e[2], e[3], u32_at(4), u32_at(8), &label, u32_at(28) & 1 != 0)
        })
        .collect()
}

/// ESP-IDF partitions CSV.
pub fn to_csv(parts: &[Partition]) -> String {
    let mut out = String::from("# Name,   Type, SubType, Offset,  Size, Flags\n");
    for p in parts {
        let flags = if p.encrypted { "encrypted" } else { "" };
        out.push_str(&format!("{},{},{},0x{:x},0x{:x},{}\n", p.label, p.kind, p.subtype_name, p.offset, p.size, flags));
    }
    out
}

/// The app inside a full firmware image (bootloader + table + app), using the
/// image's own partition table. The image starts at flash offset 0x0 or 0x1000
/// depending on the chip; the table is always at 0x8000.
pub fn app_from_full_image(image: &[u8]) -> Option<&[u8]> {
    [0x8000usize, 0x7000, 0x6000].iter().find_map(|&at| {
        let table = parse_table(image.get(at..at + 0xC00)?);
        let base = 0x8000usize.checked_sub(at)?;
        let app = table.iter().find(|p| p.type_id == 0 && (p.subtype == 0x10 || p.subtype == 0x00))?;
        let start = (app.offset as usize).checked_sub(base)?;
        let data = image.get(start..)?;
        (data.first() == Some(&0xE9)).then_some(data)
    })
}

/// Chip id in an app image's extended header (offset 12), if it is one.
pub fn image_chip_id(image: &[u8]) -> Option<u16> {
    (image.len() > 24 && image[0] == 0xE9).then(|| u16::from_le_bytes([image[12], image[13]]))
}

#[derive(Deserialize)]
struct BoardTable {
    partitions: Vec<(u8, u8, u32, u32, String, bool)>,
    running: Option<String>,
    next_update: Option<String>,
}

pub fn parse_board_table(json: &str) -> Result<PartitionTable> {
    let t: BoardTable = serde_json::from_str(json.trim()).map_err(|e| Error::Protocol(format!("partition list: {e}")))?;
    let mut partitions: Vec<Partition> =
        t.partitions.iter().map(|(ty, st, off, size, label, enc)| partition(*ty, *st, *off, *size, label, *enc)).collect();
    partitions.sort_by_key(|p| p.offset);
    Ok(PartitionTable { csv: to_csv(&partitions), partitions, running: t.running, next_update: t.next_update })
}

/// The partition table of a connected ESP32 board.
#[tauri::command]
pub async fn esp32_partitions(state: State<'_, AppState>, session_id: String) -> Result<PartitionTable> {
    let session = state.session(&session_id)?;
    let code = crate::scripts::render(&session.profile(), "partitions", &[])?;
    let mut raw = session.raw().await?;
    let out = raw.exec_str(&code).await;
    parse_board_table(&raw.finish_with(out).await?)
}

/// The partition table inside a firmware image file.
#[tauri::command]
pub async fn esp32_image_partitions(path: String) -> Result<PartitionTable> {
    let image = std::fs::read(&path)?;
    let partitions = [0x8000usize, 0x7000, 0x6000]
        .iter()
        .map(|&at| image.get(at..at + 0xC00).map(parse_table).unwrap_or_default())
        .find(|t| !t.is_empty())
        .ok_or_else(|| Error::NotFound("no partition table in this image".into()))?;
    Ok(PartitionTable { csv: to_csv(&partitions), partitions, running: None, next_update: None })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn entry(ty: u8, st: u8, off: u32, size: u32, label: &str) -> Vec<u8> {
        let mut e = vec![0xAA, 0x50, ty, st];
        e.extend(off.to_le_bytes());
        e.extend(size.to_le_bytes());
        let mut l = label.as_bytes().to_vec();
        l.resize(16, 0);
        e.extend(l);
        e.extend(0u32.to_le_bytes());
        e
    }

    /// A full image starting at flash 0x1000 (classic ESP32): table at file
    /// offset 0x7000, app (ota_0 at 0x10000) at file offset 0xF000.
    pub fn full_image(app: &[u8]) -> Vec<u8> {
        let mut img = vec![0xE9u8; 16];
        img.resize(0x7000, 0xFF);
        img.extend(entry(1, 2, 0x9000, 0x6000, "nvs"));
        img.extend(entry(1, 0, 0xF000, 0x2000, "otadata"));
        img.extend(entry(0, 0x10, 0x10000, 0x180000, "ota_0"));
        img.extend(entry(0, 0x11, 0x190000, 0x180000, "ota_1"));
        img.extend([0xEB, 0xEB]);
        img.resize(0xF000, 0xFF);
        img.extend(app);
        img
    }

    #[test]
    fn tables_and_csv() {
        let mut t = entry(1, 2, 0x9000, 0x6000, "nvs");
        t.extend(entry(0, 0x10, 0x10000, 0x180000, "ota_0"));
        t.extend([0xFF; 32]);
        let parts = parse_table(&t);
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[1].kind.as_str(), parts[1].subtype_name.as_str(), parts[1].offset), ("app", "ota_0", 0x10000));
        assert_eq!(to_csv(&parts).lines().nth(2).unwrap(), "ota_0,app,ota_0,0x10000,0x180000,");
    }

    #[test]
    fn app_extraction_and_chip_id() {
        let mut app = vec![0xE9, 3, 2, 0x20, 0, 0, 0, 0, 0xEE, 0, 0, 0, 9, 0];
        app.resize(64, 0);
        let img = full_image(&app);
        assert_eq!(app_from_full_image(&img), Some(&app[..]));
        assert_eq!(image_chip_id(&app), Some(9));
        assert!(app_from_full_image(&app).is_none());
    }

    #[test]
    fn board_table() {
        let t = parse_board_table(r#"{"partitions": [[0, 16, 65536, 1572864, "ota_0", false], [1, 2, 36864, 24576, "nvs", false], [0, 17, 1638400, 1572864, "ota_1", false]], "running": "ota_0", "next_update": "ota_1"}"#).unwrap();
        assert_eq!(t.partitions.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), vec!["nvs", "ota_0", "ota_1"]);
        assert_eq!(t.next_update.as_deref(), Some("ota_1"));
        assert!(t.csv.contains("nvs,data,nvs,0x9000,0x6000,"));
    }
}
