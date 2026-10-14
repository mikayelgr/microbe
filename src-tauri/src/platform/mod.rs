//! Declarative platform profiles.
//!
//! Every MicroPython port is described by a TOML file in `src-tauri/platforms/`
//! (embedded at build time) or in `<app-data>/platforms/` (user overrides, no
//! rebuild needed). The rest of the app never branches on port names: it asks a
//! [`ResolvedTarget`] for capabilities, flash methods, scripts and so on.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::device_info::DeviceInfo;
use crate::error::{Error, Result};

static BUILTIN: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/platforms");

pub const GENERIC: &str = "generic";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlatformProfile {
    pub id: String,
    pub name: String,
    /// Values of `sys.platform` that identify this port.
    #[serde(default)]
    pub sys_platform: Vec<String>,
    #[serde(default)]
    pub usb: Vec<UsbMatch>,
    /// Capability flags. Free-form strings so new features need no schema change.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Firmware file extensions in order of preference.
    #[serde(default)]
    pub firmware_ext: Vec<String>,
    /// Flash methods in order of preference; each names a registered `Flasher`.
    #[serde(default)]
    pub flash: Vec<FlashMethod>,
    /// Python statement that reboots the board into its bootloader.
    #[serde(default)]
    pub bootloader_entry: Option<String>,
    #[serde(default)]
    pub stubs: Option<StubSpec>,
    #[serde(default = "default_fs_root")]
    pub fs_root: String,
    #[serde(default)]
    pub serial: SerialSpec,
    /// Bytes per raw-REPL write when transferring files.
    #[serde(default)]
    pub transfer_chunk: Option<u32>,
    /// Overrides for device-side script templates, keyed by script name.
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    /// Detection hints: a detector-specific key (UF2 board id, ROM chip name...)
    /// mapped to a micropython.org board id.
    #[serde(default)]
    pub detect: Vec<DetectHint>,
    /// GPIO maps for the static pin check, most specific first.
    #[serde(default)]
    pub pins: Vec<PinMap>,
}

impl PlatformProfile {
    /// The first pin map matching the board id and chip (`machine` reports
    /// "<board> with <chip>"). Maps without `boards`/`chips` match anything.
    pub fn pin_map(&self, board_id: Option<&str>, chip: Option<&str>) -> Option<&PinMap> {
        self.pins.iter().find(|m| {
            let board_ok = m.boards.is_empty() || board_id.is_some_and(|b| m.boards.iter().any(|p| glob(p, b)));
            let chip_ok = m.chips.is_empty() || chip.is_some_and(|c| m.chips.iter().any(|p| p.eq_ignore_ascii_case(c)));
            board_ok && chip_ok
        })
    }
}

/// Which GPIO numbers exist on a board or chip, and which are best avoided.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PinMap {
    /// Board ids (`*` wildcards) this map applies to; empty for any board.
    #[serde(default)]
    pub boards: Vec<String>,
    /// Chip names this map applies to; empty for any chip.
    #[serde(default)]
    pub chips: Vec<String>,
    /// GPIO numbers that exist, e.g. `"0-19,21-23"`.
    pub gpio: String,
    /// GPIO numbers that can't drive an output.
    #[serde(default)]
    pub input_only: String,
    #[serde(default)]
    pub reserved: Vec<ReservedPins>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReservedPins {
    pub pins: String,
    /// Completes "GPIO n …", e.g. "is connected to the SPI flash".
    pub reason: String,
}

/// Parse a pin list such as `"0-5,9,12-16"`.
pub fn parse_pins(spec: &str) -> Result<Vec<std::ops::RangeInclusive<u32>>> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|part| {
            let (a, b) = part.split_once('-').unwrap_or((part, part));
            match (a.trim().parse(), b.trim().parse()) {
                (Ok(a), Ok(b)) if a <= b => Ok(a..=b),
                _ => Err(Error::other(format!("bad pin list `{spec}`"))),
            }
        })
        .collect()
}

pub fn pins_contain(spec: &str, pin: u32) -> bool {
    parse_pins(spec).is_ok_and(|r| r.iter().any(|r| r.contains(&pin)))
}

/// Match `text` against a pattern where `*` stands for any run of characters.
fn glob(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern.eq_ignore_ascii_case(text);
    }
    let (text, mut pos) = (text.to_ascii_uppercase(), 0);
    for (i, part) in parts.iter().enumerate() {
        let part = part.to_ascii_uppercase();
        if i == 0 {
            if !text.starts_with(&part) {
                return false;
            }
            pos = part.len();
        } else if i == parts.len() - 1 {
            return text.len() >= pos + part.len() && text.ends_with(&part);
        } else {
            match text[pos..].find(&part) {
                Some(at) => pos += at + part.len(),
                None => return false,
            }
        }
    }
    true
}

fn default_fs_root() -> String {
    "/".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum UsbRole {
    /// MicroPython itself enumerates with this id.
    Runtime,
    /// A ROM / UF2 / DFU bootloader.
    Bootloader,
    /// A generic USB-UART bridge; could be any port.
    UartBridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsbMatch {
    pub vid: u16,
    #[serde(default)]
    pub pid: Option<u16>,
    pub role: UsbRole,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FlashMethod {
    pub method: String,
    #[serde(default)]
    pub ext: Vec<String>,
    /// Flasher-specific parameters (offsets, volume labels, ...).
    #[serde(flatten)]
    #[ts(skip)]
    pub params: BTreeMap<String, serde_json::Value>,
}

impl FlashMethod {
    pub fn param_str_list(&self, key: &str) -> Vec<String> {
        match self.params.get(key) {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => vec![],
        }
    }

    pub fn param_str(&self, key: &str) -> Option<String> {
        self.params.get(key).and_then(|v| v.as_str()).map(str::to_string)
    }

    pub fn offset_for(&self, chip: &str) -> Option<u32> {
        self.params
            .get("offsets")?
            .get(chip)?
            .as_u64()
            .map(|v| v as u32)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StubSpec {
    /// Port name used by the `micropython-<port>-stubs` packages.
    pub port: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SerialSpec {
    #[serde(default)]
    pub baud: Option<u32>,
    /// Name of a hard-reset line sequence understood by the serial transport.
    #[serde(default)]
    pub hard_reset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DetectHint {
    #[serde(rename = "match")]
    pub key: String,
    pub board: String,
}

/// A device matched against a profile, plus facts learned from the device.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ResolvedTarget {
    pub platform: String,
    pub platform_name: String,
    pub board_id: Option<String>,
    pub capabilities: Vec<String>,
    pub fs_root: String,
}

impl ResolvedTarget {
    pub fn has(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

pub struct PlatformRegistry {
    profiles: Vec<PlatformProfile>,
}

impl PlatformRegistry {
    /// Built-in profiles, overridden or extended by `*.toml` files in `user_dir`.
    pub fn load(user_dir: Option<&Path>) -> Result<Self> {
        let mut profiles: Vec<PlatformProfile> = Vec::new();
        for file in BUILTIN.files() {
            if file.path().extension().is_some_and(|e| e == "toml") {
                let text = file
                    .contents_utf8()
                    .ok_or_else(|| Error::other("profile is not UTF-8"))?;
                profiles.push(parse(text, &file.path().display().to_string())?);
            }
        }
        if let Some(dir) = user_dir {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "toml") {
                        match std::fs::read_to_string(&path)
                            .map_err(Error::from)
                            .and_then(|t| parse(&t, &path.display().to_string()))
                        {
                            Ok(p) => {
                                profiles.retain(|q| q.id != p.id);
                                profiles.push(p);
                            }
                            Err(e) => log::warn!("skipping profile {}: {e}", path.display()),
                        }
                    }
                }
            }
        }
        profiles.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Self { profiles })
    }

    pub fn all(&self) -> &[PlatformProfile] {
        &self.profiles
    }

    pub fn get(&self, id: &str) -> Option<&PlatformProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn generic(&self) -> &PlatformProfile {
        self.get(GENERIC).expect("generic profile is built in")
    }

    pub fn by_sys_platform(&self, sys_platform: &str) -> Option<&PlatformProfile> {
        self.profiles
            .iter()
            .find(|p| p.sys_platform.iter().any(|s| s == sys_platform))
    }

    /// Profiles whose USB ids match, best (most specific) first.
    pub fn by_usb(&self, vid: u16, pid: u16) -> Vec<(&PlatformProfile, &UsbMatch)> {
        let mut hits: Vec<(&PlatformProfile, &UsbMatch, u8)> = Vec::new();
        for p in &self.profiles {
            for u in &p.usb {
                if u.vid != vid {
                    continue;
                }
                let score = match (u.pid, u.role) {
                    (Some(x), _) if x != pid => continue,
                    (Some(_), UsbRole::UartBridge) => 1,
                    (Some(_), _) => 3,
                    (None, _) => 2,
                };
                hits.push((p, u, score));
            }
        }
        hits.sort_by(|a, b| b.2.cmp(&a.2));
        hits.into_iter().map(|(p, u, _)| (p, u)).collect()
    }

    /// Profile whose detection hints contain `key` (e.g. a ROM chip name).
    pub fn board_for_hint(&self, key: &str) -> Option<(&PlatformProfile, &str)> {
        self.profiles.iter().find_map(|p| {
            p.detect
                .iter()
                .find(|d| d.key.eq_ignore_ascii_case(key))
                .map(|d| (p, d.board.as_str()))
        })
    }

    /// Resolve a running device into a target with its effective capabilities.
    pub fn resolve(&self, info: &DeviceInfo) -> ResolvedTarget {
        let profile = self
            .by_sys_platform(&info.sys_platform)
            .unwrap_or_else(|| self.generic());
        self.resolve_with(profile, info.board_id(), &info.derived_capabilities())
    }

    pub fn resolve_with(
        &self,
        profile: &PlatformProfile,
        board_id: Option<String>,
        extra_caps: &[String],
    ) -> ResolvedTarget {
        let mut caps: BTreeSet<String> = profile.capabilities.iter().cloned().collect();
        caps.extend(extra_caps.iter().cloned());
        if profile.flash.iter().any(|f| !f.ext.is_empty()) {
            caps.insert("flash".into());
        }
        ResolvedTarget {
            platform: profile.id.clone(),
            platform_name: profile.name.clone(),
            board_id,
            capabilities: caps.into_iter().collect(),
            fs_root: profile.fs_root.clone(),
        }
    }
}

fn parse(text: &str, origin: &str) -> Result<PlatformProfile> {
    toml::from_str(text).map_err(|e| Error::other(format!("{origin}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_profiles_parse() {
        let reg = PlatformRegistry::load(None).unwrap();
        for id in ["rp2", "esp32", "esp8266", "stm32", "samd", "nrf", "unix", "generic"] {
            assert!(reg.get(id).is_some(), "missing profile {id}");
        }
        let esp = reg.get("esp32").unwrap();
        assert_eq!(esp.flash[0].method, "espflash");
        assert_eq!(esp.flash[0].offset_for("esp32"), Some(0x1000));
        assert_eq!(esp.flash[0].offset_for("esp32s3"), Some(0));
    }

    #[test]
    fn usb_matching_prefers_specific_ids() {
        let reg = PlatformRegistry::load(None).unwrap();
        let hits = reg.by_usb(0x2e8a, 0x0005);
        assert_eq!(hits[0].0.id, "rp2");
        assert_eq!(hits[0].1.role, UsbRole::Runtime);
        // A CP210x bridge is ambiguous between the ESP ports.
        let ids: Vec<_> = reg.by_usb(0x10c4, 0xea60).iter().map(|h| h.0.id.clone()).collect();
        assert!(ids.contains(&"esp32".to_string()) && ids.contains(&"esp8266".to_string()));
    }

    #[test]
    fn detect_hints() {
        let reg = PlatformRegistry::load(None).unwrap();
        assert_eq!(reg.board_for_hint("esp32s3").unwrap().1, "ESP32_GENERIC_S3");
        assert_eq!(reg.board_for_hint("RP2350").unwrap().0.id, "rp2");
    }

    #[test]
    fn pin_maps_parse_and_select() {
        let reg = PlatformRegistry::load(None).unwrap();
        for p in reg.all() {
            for m in &p.pins {
                parse_pins(&m.gpio).unwrap();
                parse_pins(&m.input_only).unwrap();
                for r in &m.reserved {
                    parse_pins(&r.pins).unwrap();
                }
            }
        }
        let rp2 = reg.get("rp2").unwrap();
        assert_eq!(rp2.pin_map(Some("RPI_PICO_W"), Some("RP2040")).unwrap().reserved.len(), 1);
        assert_eq!(rp2.pin_map(Some("RPI_PICO2"), Some("RP2350")).unwrap().gpio, "0-29");
        assert_eq!(rp2.pin_map(Some("OTHER"), Some("RP2350")).unwrap().gpio, "0-47");
        let esp = reg.get("esp32").unwrap();
        assert!(esp.pin_map(Some("ESP32_GENERIC-SPIRAM"), Some("ESP32")).unwrap().reserved.len() == 2);
        assert!(esp.pin_map(Some("ESP32_GENERIC_S3"), Some("ESP32S3")).unwrap().gpio.contains("26-48"));
        assert!(esp.pin_map(None, Some("ESP32H2")).is_none());
        assert!(pins_contain("0-19,21-23", 22) && !pins_contain("0-19,21-23", 20));
        assert!(glob("*-SPIRAM*", "ESP32_GENERIC-SPIRAM") && !glob("*-SPIRAM*", "ESP32_GENERIC"));
    }

    #[test]
    fn user_profile_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("rp2.toml"),
            "id = \"rp2\"\nname = \"Custom RP2\"\nsys_platform = [\"rp2\"]\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("myport.toml"),
            "id = \"myport\"\nname = \"My Port\"\nsys_platform = [\"myport\"]\ncapabilities = [\"wifi\"]\n",
        )
        .unwrap();
        let reg = PlatformRegistry::load(Some(dir.path())).unwrap();
        assert_eq!(reg.get("rp2").unwrap().name, "Custom RP2");
        assert!(reg.by_sys_platform("myport").is_some());
    }
}
