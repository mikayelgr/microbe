//! Board detection.
//!
//! Each [`Detector`] looks at one source of evidence (a running MicroPython, a
//! UF2 boot volume, an ESP ROM bootloader, USB ids) and proposes candidates.
//! Results from all detectors are merged and ranked by confidence.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::index::BoardIndex;
use crate::error::Result;
use crate::platform::{PlatformRegistry, UsbRole};
use crate::repl::session::Session;
use crate::transport::Endpoint;

/// Where to send firmware for a detected board.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum FlashTarget {
    /// A serial port (ESP ROM bootloader, or a running board to reboot).
    Serial { port: String, vid: Option<u16>, pid: Option<u16> },
    /// A mounted mass-storage boot volume (UF2, DAPLink).
    Volume { path: String, label: String },
    /// A USB DFU device.
    Dfu,
    /// A connected session; the flasher will put it in bootloader mode.
    Session { session_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Candidate {
    pub platform: String,
    pub board_id: Option<String>,
    /// 0..=100.
    pub confidence: u8,
    pub source: String,
    pub detail: Option<String>,
    pub target: FlashTarget,
    /// Chip name reported by a ROM bootloader (e.g. `esp32s3`).
    pub chip: Option<String>,
}

pub struct DetectContext<'a> {
    pub platforms: &'a PlatformRegistry,
    pub sessions: Vec<Arc<Session>>,
    pub serial_ports: Vec<Endpoint>,
    pub index: Option<&'a BoardIndex>,
    /// Allow detectors that reset the board (ESP ROM probing).
    pub deep: bool,
}

#[async_trait]
pub trait Detector: Send + Sync {
    fn id(&self) -> &'static str;
    async fn detect(&self, ctx: &DetectContext<'_>) -> Vec<Candidate>;
}

pub struct DetectorRegistry {
    detectors: Vec<Arc<dyn Detector>>,
}

impl DetectorRegistry {
    pub fn with_defaults() -> Self {
        let mut r = Self { detectors: vec![] };
        r.register(Arc::new(RunningMicroPython));
        r.register(Arc::new(BootVolume));
        r.register(Arc::new(super::flashers::espflash::EspRomDetector));
        r.register(Arc::new(UsbIds));
        r
    }

    pub fn register(&mut self, d: Arc<dyn Detector>) {
        self.detectors.push(d);
    }

    pub async fn detect(&self, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        let mut all = Vec::new();
        for d in &self.detectors {
            let mut c = d.detect(ctx).await;
            all.append(&mut c);
        }
        // Keep the best candidate per (platform, board, target).
        all.sort_by(|a, b| b.confidence.cmp(&a.confidence));
        let mut out: Vec<Candidate> = Vec::new();
        for c in all {
            let dup = out.iter().any(|o| {
                o.platform == c.platform
                    && (o.board_id == c.board_id || c.board_id.is_none())
                    && same_device(&o.target, &c.target)
            });
            if !dup {
                out.push(c);
            }
        }
        out
    }
}

fn same_device(a: &FlashTarget, b: &FlashTarget) -> bool {
    match (a, b) {
        (FlashTarget::Serial { port: x, .. }, FlashTarget::Serial { port: y, .. }) => x == y,
        (FlashTarget::Session { .. }, FlashTarget::Serial { .. })
        | (FlashTarget::Serial { .. }, FlashTarget::Session { .. }) => true,
        _ => a == b,
    }
}

/// Fuzzy-match `uname().machine` ("Raspberry Pi Pico2 with RP2350") against
/// board names in the index.
pub fn match_machine(index: &BoardIndex, port: Option<&str>, machine: &str) -> Option<String> {
    let norm = |s: &str| s.to_lowercase().replace([' ', '-', '_'], "");
    let m = norm(machine.split(" with ").next().unwrap_or(machine));
    index
        .boards
        .iter()
        .filter(|b| port.is_none() || b.port.as_deref() == port)
        .filter(|b| {
            let n = norm(&b.name);
            !n.is_empty() && (m.ends_with(&n) || n.ends_with(&m))
        })
        .max_by_key(|b| b.name.len())
        .map(|b| b.id.clone())
}

struct RunningMicroPython;

#[async_trait]
impl Detector for RunningMicroPython {
    fn id(&self) -> &'static str {
        "running"
    }

    async fn detect(&self, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        ctx.sessions.iter().filter_map(|s| running_candidate(ctx, s)).collect()
    }
}

fn running_candidate(ctx: &DetectContext<'_>, s: &Arc<Session>) -> Option<Candidate> {
    {
        let (Some(info), Some(target)) = (s.info.lock().unwrap().clone(), s.target.lock().unwrap().clone()) else {
            return None;
        };
        let (board, confidence) = match info.board_id() {
            // `_build` may include a variant: "ESP32_GENERIC_S3-SPIRAM_OCT".
            Some(b) => (Some(b), 95),
            None => match ctx.index.and_then(|i| match_machine(i, Some(&target.platform), &info.machine)) {
                Some(b) => (Some(b), 60),
                None => (None, 40),
            },
        };
        Some(Candidate {
            platform: target.platform,
            board_id: board,
            confidence,
            source: "MicroPython".into(),
            detail: Some(format!("{} — MicroPython {}", info.machine, info.version)),
            target: FlashTarget::Session { session_id: s.id.clone() },
            chip: None,
        })
    }
}

/// Mounted UF2 / DAPLink boot volumes.
pub struct BootVolume;

/// Mounted drives that could be a board's boot volume: removable media, or any
/// drive carrying a UF2 bootloader's `INFO_UF2.TXT`. System volumes (macOS
/// `Preboot`, `/`, ...) never qualify, whatever their label.
pub fn boot_volumes() -> Vec<(String, PathBuf)> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| could_be_board(d.mount_point(), d.is_removable()))
        .map(|d| (volume_label(d), d.mount_point().to_path_buf()))
        .collect()
}

fn volume_label(d: &sysinfo::Disk) -> String {
    // The mount directory carries the volume label on every desktop OS
    // (/Volumes/RPI-RP2, /media/user/RPI-RP2); Windows uses the disk name.
    d.mount_point()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| d.name().to_string_lossy().into_owned())
}

pub fn could_be_board(mount: &Path, removable: bool) -> bool {
    !is_system_mount(mount) && (removable || mount.join("INFO_UF2.TXT").is_file())
}

fn is_system_mount(mount: &Path) -> bool {
    mount == Path::new("/")
        || ["/System", "/private", "/Library", "/boot", "/usr", "/var", "/snap"]
            .iter()
            .any(|p| mount.starts_with(p))
}

/// Whether a volume fits a `volume_copy` flash method: its label matches and,
/// if the method names an info file (`INFO_UF2.TXT`), that file is present.
pub fn method_accepts(m: &crate::platform::FlashMethod, label: &str, mount: &Path) -> bool {
    m.param_str_list("volume_labels").iter().any(|pat| label_matches(label, pat))
        && m.param_str("info_file").is_none_or(|f| mount.join(f).is_file())
}

pub fn label_matches(label: &str, pattern: &str) -> bool {
    let (l, p) = (label.to_uppercase(), pattern.to_uppercase());
    match (p.strip_prefix('*'), p.strip_suffix('*')) {
        (Some(suffix), _) => l.ends_with(suffix),
        (_, Some(prefix)) => l.starts_with(prefix),
        _ => l == p,
    }
}

/// Parse `Board-ID: RPI-RP2` style lines from INFO_UF2.TXT.
pub fn uf2_board_id(info: &str) -> Option<String> {
    info.lines()
        .find_map(|l| l.strip_prefix("Board-ID:"))
        .map(|s| s.trim().to_string())
}

#[async_trait]
impl Detector for BootVolume {
    fn id(&self) -> &'static str {
        "volume"
    }

    async fn detect(&self, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        let vols = tokio::task::spawn_blocking(boot_volumes).await.unwrap_or_default();
        let mut out = Vec::new();
        for (label, mount) in vols {
            let info = std::fs::read_to_string(Path::new(&mount).join("INFO_UF2.TXT")).ok();
            let board_key = info.as_deref().and_then(uf2_board_id);
            for p in ctx.platforms.all() {
                for m in p.flash.iter().filter(|m| m.method == "volume_copy") {
                    if !method_accepts(m, &label, &mount) {
                        continue;
                    }
                    let hinted = board_key.as_deref().and_then(|k| {
                        p.detect.iter().find(|d| k.starts_with(&d.key)).map(|d| d.board.clone())
                    });
                    out.push(Candidate {
                        platform: p.id.clone(),
                        confidence: if hinted.is_some() { 85 } else { 55 },
                        board_id: hinted,
                        source: "Boot volume".into(),
                        detail: Some(match &board_key {
                            Some(k) => format!("{label} ({k})"),
                            None => label.clone(),
                        }),
                        target: FlashTarget::Volume { path: mount.display().to_string(), label: label.clone() },
                        chip: None,
                    });
                }
            }
        }
        out
    }
}

/// Serial ports with known USB ids.
struct UsbIds;

#[async_trait]
impl Detector for UsbIds {
    fn id(&self) -> &'static str {
        "usb"
    }

    async fn detect(&self, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        let mut out = Vec::new();
        for ep in &ctx.serial_ports {
            let Some(usb) = &ep.usb else { continue };
            for (p, m) in ctx.platforms.by_usb(usb.vid, usb.pid) {
                out.push(Candidate {
                    platform: p.id.clone(),
                    board_id: None,
                    confidence: match m.role {
                        UsbRole::Runtime | UsbRole::Bootloader => 35,
                        UsbRole::UartBridge => 15,
                    },
                    source: "USB id".into(),
                    detail: Some(format!("{} ({:04x}:{:04x})", ep.label, usb.vid, usb.pid)),
                    target: FlashTarget::Serial { port: ep.address.clone(), vid: Some(usb.vid), pid: Some(usb.pid) },
                    chip: None,
                });
            }
        }
        out
    }
}

/// Ensure the per-run results reflect only reachable devices.
pub fn serial_endpoints(all: Vec<Endpoint>) -> Vec<Endpoint> {
    all.into_iter().filter(|e| e.transport == crate::transport::serial::ID).collect()
}

#[allow(dead_code)]
pub type DetectResult = Result<Vec<Candidate>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firmware::index::BoardSummary;

    #[test]
    fn labels_and_uf2_info() {
        assert!(label_matches("RPI-RP2", "RPI-RP2"));
        assert!(label_matches("FEATHERBOOT", "*BOOT"));
        assert!(label_matches("RT1060-EVK", "RT10*"));
        assert!(!label_matches("Macintosh HD", "*BOOT"));
        assert_eq!(
            uf2_board_id("UF2 Bootloader v3.0\nModel: Raspberry Pi RP2\nBoard-ID: RPI-RP2\n").as_deref(),
            Some("RPI-RP2")
        );
    }

    #[test]
    fn system_volumes_are_never_boards() {
        assert!(!could_be_board(Path::new("/System/Volumes/Preboot"), true));
        assert!(!could_be_board(Path::new("/System/Volumes/iSCPreboot"), true));
        assert!(!could_be_board(Path::new("/"), true));
        assert!(could_be_board(Path::new("/Volumes/RPI-RP2"), true));
        assert!(could_be_board(Path::new("/media/me/RPI-RP2"), true));
        // A fixed (non-removable) disk only counts if it carries a UF2 info file.
        assert!(!could_be_board(Path::new("/Volumes/Backup"), false));
    }

    #[test]
    fn methods_require_their_info_file() {
        let reg = crate::platform::PlatformRegistry::load(None).unwrap();
        let samd = &reg.get("samd").unwrap().flash[0];
        let tmp = tempfile::tempdir().unwrap();
        assert!(!method_accepts(samd, "FEATHERBOOT", tmp.path()), "no INFO_UF2.TXT");
        std::fs::write(tmp.path().join("INFO_UF2.TXT"), "").unwrap();
        assert!(method_accepts(samd, "FEATHERBOOT", tmp.path()));
        assert!(!method_accepts(samd, "Macintosh HD", tmp.path()));
    }

    #[test]
    fn machine_matching() {
        let b = |id: &str, name: &str| BoardSummary {
            id: id.into(),
            name: name.into(),
            vendor: String::new(),
            port: Some("rp2".into()),
            image: None,
        };
        let idx = BoardIndex {
            boards: vec![b("RPI_PICO", "Pico"), b("RPI_PICO2", "Pico 2"), b("RPI_PICO_W", "Pico W")],
            ports: vec![],
            fetched_at: 0,
        };
        assert_eq!(match_machine(&idx, Some("rp2"), "Raspberry Pi Pico2 with RP2350").as_deref(), Some("RPI_PICO2"));
        assert_eq!(match_machine(&idx, Some("rp2"), "Raspberry Pi Pico W with RP2040").as_deref(), Some("RPI_PICO_W"));
    }
}
