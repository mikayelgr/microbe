//! Firmware flashers.
//!
//! A platform profile lists flash methods by name (`method = "espflash"`); each
//! name maps to a registered [`Flasher`]. Supporting a new flashing technology
//! means implementing the trait and registering it here.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use super::detect::FlashTarget;
use crate::error::Result;
use crate::platform::{FlashMethod, PlatformProfile};
use crate::progress::Reporter;

pub mod dfu;
pub mod espflash;
pub mod esptool;
pub mod ota;
pub mod volume;

pub struct FlashJob<'a> {
    pub method: &'a FlashMethod,
    pub profile: &'a PlatformProfile,
    pub firmware: &'a Path,
    pub target: &'a FlashTarget,
    /// Chip reported by a detector, if any (e.g. `esp32s3`).
    pub chip: Option<&'a str>,
    pub erase: bool,
    pub reporter: &'a Reporter,
    pub http: &'a reqwest::Client,
    pub cache: &'a Path,
    /// The board's session, for flashers that work through MicroPython.
    pub session: Option<std::sync::Arc<crate::repl::session::Session>>,
}

#[async_trait]
pub trait Flasher: Send + Sync {
    /// Matches `FlashMethod::method` in platform profiles.
    fn id(&self) -> &'static str;
    /// Whether the flasher wants a serial port (vs. a boot volume / USB device).
    fn needs_serial_port(&self) -> bool;
    /// Whether the flasher works through the running MicroPython session (OTA)
    /// instead of a bootloader; the session then stays open.
    fn uses_session(&self) -> bool {
        false
    }
    /// Whether the flasher can run on this host right now (tools present...).
    fn available(&self) -> bool {
        true
    }
    async fn flash(&self, job: FlashJob<'_>) -> Result<()>;
}

pub struct FlasherRegistry {
    flashers: Vec<Arc<dyn Flasher>>,
}

impl FlasherRegistry {
    pub fn with_defaults() -> Self {
        let mut r = Self { flashers: vec![] };
        r.register(Arc::new(espflash::EspFlash));
        r.register(Arc::new(esptool::EspTool));
        r.register(Arc::new(volume::VolumeCopy));
        r.register(Arc::new(dfu::DfuUtil));
        r.register(Arc::new(ota::Esp32Ota));
        r
    }

    pub fn register(&mut self, f: Arc<dyn Flasher>) {
        self.flashers.push(f);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Flasher>> {
        self.flashers.iter().find(|f| f.id() == id).cloned()
    }

    /// The profile's method named `id`, if its flasher is available.
    pub fn by_method<'p>(&self, profile: &'p PlatformProfile, id: &str) -> Option<(&'p FlashMethod, Arc<dyn Flasher>)> {
        let m = profile.flash.iter().find(|m| m.method == id)?;
        Some((m, self.get(id).filter(|f| f.available())?))
    }

    /// First method in the profile with a registered, available flasher that
    /// accepts one of `exts`; returns it with the chosen extension.
    pub fn choose<'p>(
        &self,
        profile: &'p PlatformProfile,
        exts: &[String],
        chip: Option<&str>,
    ) -> Option<(&'p FlashMethod, Arc<dyn Flasher>, String)> {
        profile.flash.iter().find_map(|m| {
            let f = self.get(&m.method).filter(|f| f.available())?;
            // A method with chip offsets only applies to chips it knows.
            if let (Some(chip), Some(_)) = (chip, m.params.get("offsets")) {
                m.offset_for(chip)?;
            }
            let ext = m.ext.iter().find(|e| exts.contains(e))?;
            Some((m, f, ext.clone()))
        })
    }
}
