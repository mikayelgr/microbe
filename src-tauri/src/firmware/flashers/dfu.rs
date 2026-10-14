//! STM32 DFU flashing through `dfu-util` (if installed on the host).

use std::process::Stdio;

use async_trait::async_trait;

use super::{FlashJob, Flasher};
use crate::error::{Error, Result};
use crate::tools;

pub struct DfuUtil;

#[async_trait]
impl Flasher for DfuUtil {
    fn id(&self) -> &'static str {
        "dfu"
    }

    fn needs_serial_port(&self) -> bool {
        false
    }

    fn available(&self) -> bool {
        tools::which("dfu-util").is_some()
    }

    async fn flash(&self, job: FlashJob<'_>) -> Result<()> {
        let exe = tools::which("dfu-util")
            .ok_or_else(|| Error::Unsupported("dfu-util is not installed (brew/apt install dfu-util)".into()))?;
        job.reporter.set_label("Waiting for DFU device…");
        // Give a board that was just told to enter its bootloader time to enumerate.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        job.reporter.set_label("Flashing over DFU…");
        let out = tokio::process::Command::new(exe)
            .args(["-a", "0", "-D"])
            .arg(job.firmware)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !out.status.success() {
            return Err(Error::other(format!("dfu-util failed: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(())
    }
}
