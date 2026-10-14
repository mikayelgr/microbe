//! ESP32 over-the-air update through the REPL connection (serial, WebREPL,
//! BLE or network): the app image is written into the next OTA partition with
//! `esp32.Partition.writeblocks`, read back and hashed, then made the boot
//! partition and the board reset. Needs firmware with OTA partitions (the
//! `OTA` variants on micropython.org).

use async_trait::async_trait;
use base64::Engine;
use sha2::{Digest, Sha256};

use super::{FlashJob, Flasher};
use crate::error::{Error, Result};
use crate::firmware::esp32::{app_from_full_image, image_chip_id};
use crate::scripts::{render, Py};

pub struct Esp32Ota;

#[async_trait]
impl Flasher for Esp32Ota {
    fn id(&self) -> &'static str {
        "esp32_ota"
    }

    fn needs_serial_port(&self) -> bool {
        false
    }

    fn uses_session(&self) -> bool {
        true
    }

    async fn flash(&self, job: FlashJob<'_>) -> Result<()> {
        let session = job.session.ok_or_else(|| Error::Unsupported("OTA updates need a connected board running MicroPython".into()))?;
        let file = std::fs::read(job.firmware)?;
        let is_app = job.firmware.to_string_lossy().ends_with(".app-bin");
        let app = if is_app { &file[..] } else { app_from_full_image(&file).ok_or_else(|| Error::other("no app image found in this firmware file"))? };
        let expected = job
            .chip
            .map(|c| c.to_ascii_uppercase())
            .and_then(|c| job.method.params.get("chip_ids").and_then(|ids| ids.get(&c)).and_then(|v| v.as_u64()));
        match (image_chip_id(app), expected) {
            (None, _) => return Err(Error::other("not an ESP32 app image (bad magic byte)")),
            (Some(got), Some(want)) if u64::from(got) != want => {
                return Err(Error::other(format!("this image is for a different chip (id {got}, the board's is {want})")))
            }
            _ => {}
        }

        let profile = job.profile;
        let reporter = job.reporter;
        reporter.set_label("Preparing the OTA partition…");
        let mut raw = session.raw().await?;
        let res = async {
            let info = raw.exec_str(&render(profile, "ota_begin", &[])?).await.map_err(|e| match e {
                Error::Device(m) => Error::Unsupported(format!(
                    "this firmware can't be updated over the air ({m}); flash an OTA build once over USB"
                )),
                e => e,
            })?;
            let mut it = info.split_whitespace();
            let label = it.next().unwrap_or("?").to_string();
            let size: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let block: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(4096);
            if app.len() > size {
                return Err(Error::other(format!("the image ({} KB) doesn't fit in {label} ({} KB)", app.len() / 1024, size / 1024)));
            }
            reporter.set_label(format!("Writing {label}…"));
            let total = app.len() as u64;
            for (n, chunk) in app.chunks(block).enumerate() {
                let mut buf = chunk.to_vec();
                buf.resize(block, 0xFF);
                let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
                let code = render(profile, "ota_block", &[("block", Py::Int(n as i64)), ("data", Py::Str(&b64))])?;
                raw.exec(&code).await?;
                reporter.update(((n + 1) * block).min(app.len()) as u64, total);
            }
            reporter.set_label("Verifying…");
            let got = raw.exec_str(&render(profile, "ota_finish", &[("length", Py::Int(app.len() as i64))])?).await?;
            let want = hex::encode(Sha256::digest(app));
            if got.trim() != want {
                return Err(Error::other("verification failed: the written image doesn't match; the board still boots the old firmware"));
            }
            raw.exec("_mb_ota.set_boot()\ndel _mb_ota").await?;
            Ok(label)
        }
        .await;
        let label = raw.finish_with(res).await?;
        reporter.set_label(format!("Rebooting into {label}…"));
        let _ = session.exec_detached("import machine\nmachine.reset()\n").await;
        Ok(())
    }
}
