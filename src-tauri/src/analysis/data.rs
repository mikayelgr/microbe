//! Tables for the analysis rules (`src-tauri/analysis.toml`).

use serde::Deserialize;

use crate::error::{Error, Result};

const BUILTIN: &str = include_str!("../../analysis.toml");

#[derive(Debug, Default, Deserialize)]
pub struct Data {
    #[serde(default, rename = "module")]
    pub modules: Vec<ModuleEntry>,
    #[serde(default, rename = "attribute")]
    pub attributes: Vec<AttributeEntry>,
    #[serde(default)]
    pub syntax: Vec<SyntaxEntry>,
    #[serde(default)]
    pub inline: Vec<InlineEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ModuleEntry {
    pub name: String,
    pub hint: String,
    #[serde(default)]
    pub except_platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct AttributeEntry {
    pub module: String,
    pub names: Vec<String>,
    pub hint: String,
    #[serde(default)]
    pub except_platforms: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SyntaxEntry {
    pub node: String,
    #[serde(default)]
    pub token: Option<String>,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct InlineEntry {
    pub decorators: Vec<String>,
    #[serde(default)]
    pub names: Vec<String>,
    #[serde(default)]
    pub any: bool,
}

impl Data {
    pub fn builtin() -> Result<Self> {
        toml::from_str(BUILTIN).map_err(|e| Error::other(format!("analysis.toml: {e}")))
    }

    /// The entry for an imported module (`RPi` covers `RPi.GPIO`).
    pub fn module(&self, name: &str, platform: Option<&str>) -> Option<&ModuleEntry> {
        self.modules.iter().find(|m| {
            let hit = name == m.name || name.strip_prefix(&m.name).is_some_and(|r| r.starts_with('.'));
            hit && !excepted(&m.except_platforms, platform)
        })
    }

    pub fn attribute(&self, module: &str, name: &str, platform: Option<&str>) -> Option<&AttributeEntry> {
        self.attributes.iter().find(|a| {
            a.module == module && a.names.iter().any(|n| n == name) && !excepted(&a.except_platforms, platform)
        })
    }
}

fn excepted(list: &[String], platform: Option<&str>) -> bool {
    platform.is_some_and(|p| list.iter().any(|x| x == p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tables_parse() {
        let d = Data::builtin().unwrap();
        assert!(d.module("RPi.GPIO", None).is_some());
        assert!(d.module("RPiX", None).is_none());
        assert!(d.attribute("os", "getenv", Some("unix")).is_none());
        assert!(d.attribute("os", "getenv", Some("rp2")).is_some());
        assert!(d.inline.iter().any(|i| i.decorators.contains(&"asm_pio".to_string())));
    }
}
