//! ESP32-family flashing and chip detection via the `espflash` library.

use std::time::Duration;

use async_trait::async_trait;
use espflash::connection::{Connection, ResetAfterOperation, ResetBeforeOperation};
use espflash::flasher::Flasher as EspFlasher;
use espflash::target::ProgressCallbacks;
use serialport::{SerialPortType, UsbPortInfo};

use super::{FlashJob, Flasher};
use crate::error::{Error, Result};
use crate::firmware::detect::{Candidate, DetectContext, Detector, FlashTarget};
use crate::platform::UsbRole;

pub struct EspFlash;

const FLASH_BAUD: u32 = 460_800;

fn esp_err(e: espflash::Error) -> Error {
    Error::other(format!("espflash: {e}"))
}

fn usb_info(port: &str) -> UsbPortInfo {
    serialport::available_ports()
        .ok()
        .and_then(|ports| {
            ports.into_iter().find(|p| p.port_name == port).and_then(|p| match p.port_type {
                SerialPortType::UsbPort(u) => Some(u),
                _ => None,
            })
        })
        .unwrap_or(UsbPortInfo { vid: 0, pid: 0, serial_number: None, manufacturer: None, product: None })
}

fn open(port: &str, after: ResetAfterOperation) -> Result<EspFlasher> {
    let serial = serialport::new(port, 115_200)
        .timeout(Duration::from_secs(3))
        .open_native()?;
    let conn = Connection::new(serial, usb_info(port), after, ResetBeforeOperation::DefaultReset, 115_200);
    EspFlasher::connect(conn, true, true, false, None, Some(FLASH_BAUD)).map_err(esp_err)
}

pub struct ChipInfo {
    pub chip: String,
    pub features: Vec<String>,
    pub flash_size: String,
}

/// Reset into the ROM bootloader, read chip facts, and reset back.
pub fn probe(port: &str) -> Result<ChipInfo> {
    let mut f = open(port, ResetAfterOperation::HardReset)?;
    let info = f.device_info().map_err(esp_err)?;
    let chip = f.chip();
    let _ = f.connection().reset_after(true, chip);
    Ok(ChipInfo {
        chip: chip.to_string(),
        features: info.features.clone(),
        flash_size: info.flash_size.to_string(),
    })
}

struct Progress<'a> {
    reporter: &'a crate::progress::Reporter,
    total: u64,
}

impl ProgressCallbacks for Progress<'_> {
    fn init(&mut self, _addr: u32, total: usize) {
        self.total = total as u64;
    }
    fn update(&mut self, current: usize) {
        self.reporter.update(current as u64, self.total);
    }
    fn verifying(&mut self) {
        self.reporter.set_label("Verifying…");
    }
    fn finish(&mut self, _skipped: bool) {}
}

#[async_trait]
impl Flasher for EspFlash {
    fn id(&self) -> &'static str {
        "espflash"
    }

    fn needs_serial_port(&self) -> bool {
        true
    }

    async fn flash(&self, job: FlashJob<'_>) -> Result<()> {
        let FlashTarget::Serial { port, .. } = job.target else {
            return Err(Error::Unsupported("espflash needs a serial port".into()));
        };
        let data = tokio::fs::read(job.firmware).await?;
        let reporter = job.reporter;
        let method = job.method;
        let erase = job.erase;
        tokio::task::block_in_place(|| {
            reporter.set_label("Connecting to the ROM bootloader…");
            let mut f = open(port, ResetAfterOperation::HardReset)?;
            let chip = f.chip();
            let chip_name = chip.to_string();
            let offset = method
                .offset_for(&chip_name)
                .ok_or_else(|| Error::Unsupported(format!("no firmware offset known for {chip_name}")))?;
            if erase {
                reporter.set_label(format!("Erasing {chip_name} flash…"));
                f.erase_flash().map_err(esp_err)?;
            }
            reporter.set_label(format!("Writing firmware to {chip_name} at {offset:#x}…"));
            let mut progress = Progress { reporter, total: data.len() as u64 };
            f.write_bin_to_flash(offset, &data, &mut progress).map_err(esp_err)?;
            reporter.set_label("Resetting…");
            f.connection().reset_after(true, chip).map_err(esp_err)?;
            Ok(())
        })
    }
}

/// Probes serial ports that could be ESP chips (only when deep detection is on,
/// since it resets the board).
pub struct EspRomDetector;

#[async_trait]
impl Detector for EspRomDetector {
    fn id(&self) -> &'static str {
        "esp_rom"
    }

    async fn detect(&self, ctx: &DetectContext<'_>) -> Vec<Candidate> {
        if !ctx.deep {
            return vec![];
        }
        let busy: Vec<String> = ctx.sessions.iter().map(|s| s.endpoint.address.clone()).collect();
        let mut out = Vec::new();
        for ep in &ctx.serial_ports {
            let Some(usb) = &ep.usb else { continue };
            let esp_like = ctx.platforms.by_usb(usb.vid, usb.pid).iter().any(|(p, m)| {
                p.capabilities.iter().any(|c| c == "esp_rom")
                    && matches!(m.role, UsbRole::UartBridge | UsbRole::Bootloader | UsbRole::Runtime)
            });
            if !esp_like || busy.contains(&ep.address) {
                continue;
            }
            let port = ep.address.clone();
            let Ok(Ok(info)) = tokio::task::spawn_blocking(move || probe(&port)).await else { continue };
            let psram = info.features.iter().any(|f| f.to_uppercase().contains("PSRAM"));
            let keys = if psram { vec![format!("{}+psram", info.chip), info.chip.clone()] } else { vec![info.chip.clone()] };
            let hit = keys.iter().find_map(|k| ctx.platforms.board_for_hint(k));
            if let Some((profile, board)) = hit {
                out.push(Candidate {
                    platform: profile.id.clone(),
                    board_id: Some(board.to_string()),
                    confidence: 80,
                    source: "ESP ROM".into(),
                    detail: Some(format!("{} — {} flash, {}", info.chip, info.flash_size, info.features.join(", "))),
                    target: FlashTarget::Serial { port: ep.address.clone(), vid: Some(usb.vid), pid: Some(usb.pid) },
                    chip: Some(info.chip.clone()),
                });
            }
        }
        out
    }
}
