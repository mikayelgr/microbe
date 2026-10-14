//! Facts read from a running device, used to resolve its platform and board.

use serde::Serialize;
use ts_rs::TS;

use crate::error::Result;
use crate::platform::PlatformProfile;
use crate::repl::raw::RawRepl;
use crate::scripts::{self, Py};

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct DeviceInfo {
    pub sys_platform: String,
    pub version: String,
    pub machine: String,
    /// `sys.implementation._build`, e.g. `RPI_PICO2` or `ESP32_GENERIC_S3-SPIRAM_OCT`.
    pub build: String,
    pub mpy: u32,
    pub release: String,
    #[ts(type = "number")]
    pub mem_free: u64,
    #[ts(type = "number")]
    pub mem_alloc: u64,
    #[ts(type = "number")]
    pub fs_total: u64,
    #[ts(type = "number")]
    pub fs_free: u64,
    pub modules: Vec<String>,
}

impl DeviceInfo {
    pub fn parse(text: &str) -> Self {
        let mut d = DeviceInfo::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('\t') else { continue };
            let v = v.trim();
            match k {
                "platform" => d.sys_platform = v.into(),
                "version" => d.version = v.into(),
                "machine" => d.machine = v.into(),
                "build" => d.build = v.into(),
                "mpy" => d.mpy = v.parse().unwrap_or(0),
                "release" => d.release = v.into(),
                "mem_free" => d.mem_free = v.parse().unwrap_or(0),
                "mem_alloc" => d.mem_alloc = v.parse().unwrap_or(0),
                "fs_total" => d.fs_total = v.parse().unwrap_or(0),
                "fs_free" => d.fs_free = v.parse().unwrap_or(0),
                "modules" => d.modules = v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
                _ => {}
            }
        }
        d
    }

    /// micropython.org board id, when the firmware reports one. Board ids are
    /// upper-case; the unix port reports its lower-case variant ("standard").
    pub fn board_id(&self) -> Option<String> {
        let b = self.build.trim();
        (!b.is_empty() && b.chars().any(|c| c.is_ascii_uppercase())).then(|| b.to_string())
    }

    pub fn has_module(&self, m: &str) -> bool {
        self.modules.iter().any(|x| x == m)
    }

    /// The bytecode version (`_mpy & 0xff`) used to pick .mpy packages.
    pub fn mpy_version(&self) -> Option<u32> {
        (self.mpy > 0).then_some(self.mpy & 0xff)
    }

    /// Capabilities implied by the modules present on this firmware.
    pub fn derived_capabilities(&self) -> Vec<String> {
        let mut caps = Vec::new();
        for (module, cap) in [
            ("network", "network"),
            ("bluetooth", "ble"),
            ("webrepl", "webrepl"),
            ("hashlib", "hashlib"),
            ("binascii", "binascii"),
            ("mip", "mip_device"),
            ("machine", "machine"),
            ("esp32", "esp32_partitions"),
        ] {
            if self.has_module(module) {
                caps.push(cap.to_string());
            }
        }
        caps
    }
}

pub async fn read(repl: &mut RawRepl, profile: &PlatformProfile) -> Result<DeviceInfo> {
    let code = scripts::render(profile, "info", &[("root", Py::Str(&profile.fs_root))])?;
    Ok(DeviceInfo::parse(&repl.exec_str(&code).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_info() {
        let d = DeviceInfo::parse(
            "platform\trp2\nversion\t1.29.0\nmachine\tRaspberry Pi Pico2 with RP2350\nbuild\tRPI_PICO2\nmpy\t1286\nmodules\tmachine,hashlib,binascii\n",
        );
        assert_eq!(d.sys_platform, "rp2");
        assert_eq!(d.board_id().as_deref(), Some("RPI_PICO2"));
        assert_eq!(d.mpy_version(), Some(6));
        assert!(d.derived_capabilities().contains(&"hashlib".to_string()));
        assert_eq!(DeviceInfo::parse("build\tstandard\n").board_id(), None);
    }
}
