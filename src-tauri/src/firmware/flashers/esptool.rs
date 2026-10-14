//! Espressif's official `esptool`, downloaded on demand. Used for chips that
//! espflash doesn't support (ESP8266) and as a fallback.

use std::process::Stdio;

use async_trait::async_trait;
use regex::Regex;
use tokio::io::{AsyncBufReadExt, BufReader};

use super::{FlashJob, Flasher};
use crate::error::{Error, Result};
use crate::firmware::detect::FlashTarget;
use crate::tools;

pub struct EspTool;

#[async_trait]
impl Flasher for EspTool {
    fn id(&self) -> &'static str {
        "esptool"
    }

    fn needs_serial_port(&self) -> bool {
        true
    }

    async fn flash(&self, job: FlashJob<'_>) -> Result<()> {
        let FlashTarget::Serial { port, .. } = job.target else {
            return Err(Error::Unsupported("esptool needs a serial port".into()));
        };
        let r = job.reporter;
        r.set_label("Downloading esptool…");
        let exe = tools::ensure(job.http, job.cache, &tools::ESPTOOL, &|d, t| r.update(d, t)).await?;

        // Chip: from detection, else the only chip the method knows about.
        let chip = match job.chip {
            Some(c) => c.to_string(),
            None => {
                let offsets = job.method.params.get("offsets").and_then(|o| o.as_object());
                match offsets {
                    Some(o) if o.len() == 1 => o.keys().next().unwrap().clone(),
                    _ => "auto".into(),
                }
            }
        };
        let offset = job.method.offset_for(&chip).unwrap_or(0);

        if job.erase {
            r.set_label(format!("Erasing flash ({chip})…"));
            run(&exe, &["--chip", &chip, "--port", port, "erase-flash"], r).await?;
        }
        r.set_label(format!("Writing firmware ({chip}) at {offset:#x}…"));
        let fw = job.firmware.to_string_lossy().into_owned();
        let off = format!("{offset:#x}");
        run(
            &exe,
            &["--chip", &chip, "--port", port, "--baud", "460800", "write-flash", "--flash-size", "detect", &off, &fw],
            r,
        )
        .await
    }
}

async fn run(exe: &std::path::Path, args: &[&str], r: &crate::progress::Reporter) -> Result<()> {
    let mut child = tokio::process::Command::new(exe)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pct = Regex::new(r"(\d+(?:\.\d+)?)\s?%").unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut tail = Vec::new();
    while let Some(line) = lines.next_line().await? {
        if let Some(c) = pct.captures(&line) {
            let p: f64 = c[1].parse().unwrap_or(0.0);
            r.update((p * 10.0) as u64, 1000);
        }
        tail.push(line);
        if tail.len() > 20 {
            tail.remove(0);
        }
    }
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(Error::other(format!("esptool failed: {}\n{}", err.trim(), tail.join("\n"))));
    }
    Ok(())
}
