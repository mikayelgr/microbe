//! Static analysis of the file being edited.
//!
//! Python is parsed with tree-sitter, then two kinds of registered checks run:
//! * [`LintRule`]s report findings of their own (CPython-only APIs, syntax
//!   MicroPython rejects, pins the target board doesn't have).
//! * [`DiagnosticFilter`]s mark regions where the language server's reports are
//!   wrong for MicroPython (names injected by `@rp2.asm_pio`, inline assembler).
//!   The frontend drops matching server diagnostics.
//!
//! Rules read their tables from `src-tauri/analysis.toml` and the platform
//! profiles' `[[pins]]`, so most new checks are data, not code.

mod data;
mod filters;
mod rules;

use serde::{Deserialize, Serialize};
use tauri::State;
use tree_sitter::{Node, Parser};
use ts_rs::TS;

use crate::error::{Error, Result};
use crate::platform::PlatformProfile;
use crate::state::AppState;

pub use data::Data;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A finding in byte offsets, as rules produce it.
#[derive(Debug, Clone)]
pub struct RawFinding {
    pub severity: Severity,
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// A region where language-server diagnostics about `names` are suppressed.
#[derive(Debug, Clone)]
pub struct RawRegion {
    pub start: usize,
    pub end: usize,
    /// Empty: any undefined name.
    pub names: Vec<String>,
}

/// A finding with UTF-16 offsets (CodeMirror document positions).
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub from: u32,
    pub to: u32,
}

/// Language-server diagnostics inside `from..to` that name one of `names`
/// (or any name, when empty) are false positives on MicroPython.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Suppression {
    pub filter: String,
    pub from: u32,
    pub to: u32,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct AnalysisResult {
    pub findings: Vec<Finding>,
    pub suppressions: Vec<Suppression>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct AnalysisRequest {
    pub source: String,
    /// Platform profile id of the target board, when known.
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub board_id: Option<String>,
    /// The board's `machine` string ("Raspberry Pi Pico with RP2040").
    #[serde(default)]
    pub machine: Option<String>,
    /// Ids of checks to skip.
    #[serde(default)]
    pub disabled: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct CheckInfo {
    pub id: String,
    pub description: String,
    /// Whether it needs a known board to do anything.
    pub needs_board: bool,
}

/// What the code will run on, as far as it's known.
#[derive(Default, Clone, Copy)]
pub struct Target<'a> {
    pub profile: Option<&'a PlatformProfile>,
    pub board_id: Option<&'a str>,
    /// Chip name, from the part of `machine` after " with ".
    pub chip: Option<&'a str>,
}

impl Target<'_> {
    pub fn platform(&self) -> Option<&str> {
        self.profile.map(|p| p.id.as_str())
    }
}

pub fn chip_of(machine: &str) -> Option<&str> {
    machine.rsplit_once(" with ").map(|(_, c)| c.trim()).filter(|c| !c.is_empty())
}

pub struct Context<'a> {
    pub src: &'a [u8],
    pub root: Node<'a>,
    pub target: Target<'a>,
    pub data: &'a Data,
}

impl<'a> Context<'a> {
    pub fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    /// Visit every node in document order.
    pub fn walk(&self, mut f: impl FnMut(Node<'a>)) {
        let mut cursor = self.root.walk();
        loop {
            f(cursor.node());
            if cursor.goto_first_child() {
                continue;
            }
            loop {
                if cursor.goto_next_sibling() {
                    break;
                }
                if !cursor.goto_parent() {
                    return;
                }
            }
        }
    }
}

/// A check that reports problems in the source.
pub trait LintRule: Send + Sync {
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn needs_board(&self) -> bool {
        false
    }
    fn check(&self, cx: &Context, out: &mut Vec<RawFinding>);
}

/// A check that marks regions where language-server diagnostics don't apply.
pub trait DiagnosticFilter: Send + Sync {
    fn id(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn regions(&self, cx: &Context, out: &mut Vec<RawRegion>);
}

pub struct Analyzer {
    rules: Vec<Box<dyn LintRule>>,
    filters: Vec<Box<dyn DiagnosticFilter>>,
    data: Data,
}

impl Analyzer {
    pub fn with_defaults() -> Result<Self> {
        let mut a = Self { rules: Vec::new(), filters: Vec::new(), data: Data::builtin()? };
        a.register_rule(Box::new(rules::CompatImports));
        a.register_rule(Box::new(rules::CompatSyntax));
        a.register_rule(Box::new(rules::PinCheck));
        a.register_filter(Box::new(filters::InlineCode));
        Ok(a)
    }

    pub fn register_rule(&mut self, r: Box<dyn LintRule>) {
        self.rules.push(r);
    }

    pub fn register_filter(&mut self, f: Box<dyn DiagnosticFilter>) {
        self.filters.push(f);
    }

    pub fn checks(&self) -> Vec<CheckInfo> {
        let rules = self.rules.iter().map(|r| (r.id(), r.description(), r.needs_board()));
        let filters = self.filters.iter().map(|f| (f.id(), f.description(), false));
        rules
            .chain(filters)
            .map(|(id, d, b)| CheckInfo { id: id.into(), description: d.into(), needs_board: b })
            .collect()
    }

    pub fn analyze(&self, source: &str, target: Target, disabled: &[String]) -> Result<AnalysisResult> {
        let tree = parse(source)?;
        let cx = Context { src: source.as_bytes(), root: tree.root_node(), target, data: &self.data };
        let on = |id: &str| !disabled.iter().any(|d| d == id);
        let pos = Utf16::new(source);

        let mut result = AnalysisResult::default();
        for rule in self.rules.iter().filter(|r| on(r.id())) {
            let mut raw = Vec::new();
            rule.check(&cx, &mut raw);
            result.findings.extend(raw.into_iter().map(|f| Finding {
                rule: rule.id().into(),
                severity: f.severity,
                message: f.message,
                from: pos.at(f.start),
                to: pos.at(f.end),
            }));
        }
        for filter in self.filters.iter().filter(|f| on(f.id())) {
            let mut raw = Vec::new();
            filter.regions(&cx, &mut raw);
            result.suppressions.extend(raw.into_iter().map(|r| Suppression {
                filter: filter.id().into(),
                from: pos.at(r.start),
                to: pos.at(r.end),
                names: r.names,
            }));
        }
        result.findings.sort_by_key(|f| (f.from, f.to));
        Ok(result)
    }
}

/// Parse Python source with tree-sitter (error-tolerant).
pub fn parse(source: &str) -> Result<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| Error::other(format!("tree-sitter: {e}")))?;
    parser.parse(source, None).ok_or_else(|| Error::other("could not parse the file"))
}

/// Byte offset → UTF-16 offset.
struct Utf16(Option<Vec<u32>>);

impl Utf16 {
    fn new(s: &str) -> Self {
        if s.is_ascii() {
            return Self(None);
        }
        let mut table = vec![0u32; s.len() + 1];
        let mut u = 0u32;
        for (i, c) in s.char_indices() {
            for b in i..i + c.len_utf8() {
                table[b] = u;
            }
            u += c.len_utf16() as u32;
        }
        table[s.len()] = u;
        Self(Some(table))
    }

    fn at(&self, byte: usize) -> u32 {
        match &self.0 {
            None => byte as u32,
            Some(t) => t[byte.min(t.len() - 1)],
        }
    }
}

#[tauri::command]
pub fn analysis_checks(state: State<'_, AppState>) -> Vec<CheckInfo> {
    state.analyzer.checks()
}

#[tauri::command]
pub async fn analysis_run(state: State<'_, AppState>, request: AnalysisRequest) -> Result<AnalysisResult> {
    run(&state, &request)
}

pub fn run(state: &AppState, request: &AnalysisRequest) -> Result<AnalysisResult> {
    let platforms = state.platforms();
    let target = Target {
        profile: request.platform.as_deref().and_then(|p| platforms.get(p)),
        board_id: request.board_id.as_deref(),
        chip: request.machine.as_deref().and_then(chip_of),
    };
    state.analyzer.analyze(&request.source, target, &request.disabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PlatformRegistry;

    fn analyze(src: &str, platform: Option<&str>, board: Option<&str>, machine: Option<&str>) -> AnalysisResult {
        let reg = PlatformRegistry::load(None).unwrap();
        let target = Target { profile: platform.and_then(|p| reg.get(p)), board_id: board, chip: machine.and_then(chip_of) };
        Analyzer::with_defaults().unwrap().analyze(src, target, &[]).unwrap()
    }

    fn messages(r: &AnalysisResult) -> Vec<String> {
        r.findings.iter().map(|f| format!("{}:{}", f.rule, f.message)).collect()
    }

    #[test]
    fn cpython_only_imports_and_attributes() {
        let src = "import threading\nimport RPi.GPIO as GPIO\nfrom time import perf_counter\nimport time as t\nimport os\nx = t.monotonic()\ny = t.ticks_ms()\nimport machine\n";
        let r = analyze(src, None, None, None);
        let m = messages(&r);
        assert_eq!(m.len(), 4, "{m:?}");
        assert!(m[0].contains("`threading`") && m[0].contains("_thread"));
        assert!(m[1].contains("`RPi.GPIO`") && m[1].contains("machine.Pin"));
        assert!(m[2].contains("`time.perf_counter`"));
        assert!(m[3].contains("`time.monotonic`"));
        // Ranges cover the offending name.
        assert_eq!(&src[r.findings[0].from as usize..r.findings[0].to as usize], "threading");
        assert_eq!(&src[r.findings[3].from as usize..r.findings[3].to as usize], "t.monotonic");
    }

    #[test]
    fn platform_exceptions() {
        let src = "import os\nos.getenv('HOME')\n";
        assert_eq!(analyze(src, Some("rp2"), None, None).findings.len(), 1);
        assert!(analyze(src, Some("unix"), None, None).findings.is_empty());
    }

    #[test]
    fn unsupported_syntax() {
        let src = "match x:\n    case 1:\n        pass\ntry:\n    pass\nexcept* ValueError:\n    pass\ntry:\n    pass\nexcept ValueError:\n    pass\n";
        let r = analyze(src, None, None, None);
        let m = messages(&r);
        assert_eq!(m.len(), 2, "{m:?}");
        assert!(m[0].contains("match") && r.findings[0].severity == Severity::Error);
        assert!(m[1].contains("except*"));
        // Only the first line of a block is underlined.
        assert_eq!(&src[r.findings[0].from as usize..r.findings[0].to as usize], "match x:");
    }

    #[test]
    fn pins_follow_the_board() {
        let src = "from machine import Pin\nimport machine\na = Pin(30)\nb = machine.Pin(25, machine.Pin.OUT)\nc = Pin(24)\n";
        // Pico: 30 doesn't exist.
        let r = analyze(src, Some("rp2"), Some("RPI_PICO"), Some("Raspberry Pi Pico with RP2040"));
        assert_eq!(messages(&r), vec!["pins:GPIO 30 doesn't exist on the RP2040 (valid: 0-29)".to_string()]);
        // Pico W: 24 and 25 belong to the wireless chip.
        let r = analyze(src, Some("rp2"), Some("RPI_PICO_W"), Some("Raspberry Pi Pico W with RP2040"));
        assert_eq!(r.findings.len(), 3);
        assert!(r.findings[1].message.starts_with("GPIO 25 is wired to the CYW43"));
        assert_eq!(r.findings[1].severity, Severity::Warning);
        // Unknown board: nothing to check against.
        assert!(analyze(src, None, None, None).findings.is_empty());
    }

    #[test]
    fn esp32_input_only_and_flash_pins() {
        let src = "from machine import Pin\nPin(34, Pin.OUT)\nPin(35, Pin.IN)\nPin(mode=Pin.OUT, id=36)\nPin(7)\nPin(0x10)\n";
        let r = analyze(src, Some("esp32"), Some("ESP32_GENERIC"), Some("Generic ESP32 module with ESP32"));
        let m = messages(&r);
        assert_eq!(m.len(), 3, "{m:?}");
        assert!(m[0].contains("GPIO 34 is input-only"));
        assert!(m[1].contains("GPIO 36 is input-only"));
        assert!(m[2].contains("GPIO 7 is connected to the SPI flash"));
    }

    #[test]
    fn inline_code_regions() {
        let src = "import rp2\n@rp2.asm_pio(set_init=rp2.PIO.OUT_LOW)\ndef blink():\n    wrap_target()\n    set(pins, 1) [31]\n    wrap()\n\nx = 1\n@micropython.asm_thumb\ndef f(r0):\n    mov(r0, 1)\n";
        let r = analyze(src, None, None, None);
        assert_eq!(r.suppressions.len(), 2);
        let pio = &r.suppressions[0];
        assert!(pio.names.contains(&"wrap_target".to_string()));
        assert!(src[pio.from as usize..pio.to as usize].contains("set(pins, 1)"));
        assert!(!src[pio.from as usize..pio.to as usize].contains("x = 1"));
        assert!(r.suppressions[1].names.is_empty());
    }

    #[test]
    fn utf16_offsets() {
        let src = "s = \"héllo 😀\"\nimport threading\n";
        let r = analyze(src, None, None, None);
        let utf16: Vec<u16> = src.encode_utf16().collect();
        let f = &r.findings[0];
        assert_eq!(String::from_utf16(&utf16[f.from as usize..f.to as usize]).unwrap(), "threading");
    }

    #[test]
    fn disabled_checks_are_skipped() {
        let a = Analyzer::with_defaults().unwrap();
        let r = a.analyze("import threading\n", Target::default(), &["compat-imports".into()]).unwrap();
        assert!(r.findings.is_empty());
        assert!(a.checks().iter().any(|c| c.id == "pins" && c.needs_board));
    }
}
