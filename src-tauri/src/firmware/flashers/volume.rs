//! Copy a firmware file onto a mass-storage boot volume (UF2, DAPLink...).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_trait::async_trait;

use super::{FlashJob, Flasher};
use crate::error::{Error, Result};
use crate::firmware::detect::{boot_volumes, method_accepts, FlashTarget};
use crate::platform::FlashMethod;

pub struct VolumeCopy;

const WAIT: Duration = Duration::from_secs(30);

#[async_trait]
impl Flasher for VolumeCopy {
    fn id(&self) -> &'static str {
        "volume_copy"
    }

    fn needs_serial_port(&self) -> bool {
        false
    }

    async fn flash(&self, job: FlashJob<'_>) -> Result<()> {
        let mount = match job.target {
            FlashTarget::Volume { path, .. } => PathBuf::from(path),
            _ => {
                job.reporter.set_label("Waiting for the boot drive…");
                wait_for_volume(job.method).await?
            }
        };
        let name = job.firmware.file_name().ok_or_else(|| Error::other("bad firmware path"))?;
        let dest = mount.join(name);
        job.reporter.set_label(format!("Copying firmware to {}", mount.display()));
        let data = tokio::fs::read(job.firmware).await?;
        let total = data.len() as u64;
        let reporter = job.reporter;
        // Write in chunks for progress; the board reboots (and the volume
        // vanishes) right after the last block, so late I/O errors are expected.
        let res = tokio::task::block_in_place(|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::File::create(&dest)?;
            let mut done = 0u64;
            for chunk in data.chunks(64 * 1024) {
                f.write_all(chunk)?;
                done += chunk.len() as u64;
                reporter.update(done, total);
            }
            f.sync_all()
        });
        match res {
            Ok(()) => Ok(()),
            Err(e) if !mount.exists() => {
                log::info!("volume disappeared after copy (expected): {e}");
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }
}

pub async fn wait_for_volume(method: &FlashMethod) -> Result<PathBuf> {
    let labels = method.param_str_list("volume_labels");
    let start = Instant::now();
    while start.elapsed() < WAIT {
        let vols = tokio::task::spawn_blocking(boot_volumes).await.unwrap_or_default();
        if let Some((_, mount)) = vols.into_iter().find(|(l, m)| method_accepts(method, l, m)) {
            // Give the OS a moment to finish mounting.
            tokio::time::sleep(Duration::from_millis(500)).await;
            return Ok(mount);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    Err(Error::Timeout(format!(
        "a boot drive ({}) — hold BOOT/BOOTSEL while plugging in the board",
        labels.join(", ")
    )))
}
