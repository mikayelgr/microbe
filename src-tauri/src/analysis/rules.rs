//! Built-in lint rules.

use std::collections::HashMap;

use tree_sitter::Node;

use super::{Context, LintRule, RawFinding, Severity};
use crate::platform::pins_contain;

/// Imports and module attributes that exist in CPython but not in MicroPython.
pub struct CompatImports;

impl LintRule for CompatImports {
    fn id(&self) -> &'static str {
        "compat-imports"
    }

    fn description(&self) -> &'static str {
        "CPython-only modules and functions, with the MicroPython replacement"
    }

    fn check(&self, cx: &Context, out: &mut Vec<RawFinding>) {
        let platform = cx.target.platform();
        // Local name → module, for `time.perf_counter()` after `import time as t`.
        let mut aliases: HashMap<&str, &str> = HashMap::new();
        let module_hit = |name: &str, node: Node, out: &mut Vec<RawFinding>| {
            if let Some(m) = cx.data.module(name, platform) {
                out.push(warning(node, format!("`{name}` isn't available in MicroPython; {}", m.hint)));
            }
        };
        cx.walk(|n| match n.kind() {
            "import_statement" => {
                let mut c = n.walk();
                for child in n.children_by_field_name("name", &mut c) {
                    let (dotted, alias) = match child.kind() {
                        "aliased_import" => (child.child_by_field_name("name"), child.child_by_field_name("alias")),
                        _ => (Some(child), None),
                    };
                    let Some(dotted) = dotted else { continue };
                    let module = cx.text(dotted);
                    module_hit(module, dotted, out);
                    match alias {
                        Some(a) => aliases.insert(cx.text(a), module),
                        None => aliases.insert(module.split('.').next().unwrap_or(module), module.split('.').next().unwrap_or(module)),
                    };
                }
            }
            "import_from_statement" => {
                let Some(module_node) = n.child_by_field_name("module_name").filter(|m| m.kind() == "dotted_name") else {
                    return;
                };
                let module = cx.text(module_node);
                module_hit(module, module_node, out);
                let mut c = n.walk();
                for child in n.children_by_field_name("name", &mut c) {
                    let name = match child.kind() {
                        "aliased_import" => child.child_by_field_name("name"),
                        _ => Some(child),
                    };
                    if let Some(name) = name {
                        attribute_hit(cx, module, cx.text(name), name, out);
                    }
                }
            }
            _ => {}
        });
        cx.walk(|n| {
            if n.kind() != "attribute" {
                return;
            }
            let (Some(obj), Some(attr)) = (n.child_by_field_name("object"), n.child_by_field_name("attribute")) else {
                return;
            };
            if obj.kind() == "identifier" {
                if let Some(module) = aliases.get(cx.text(obj)) {
                    attribute_hit(cx, module, cx.text(attr), n, out);
                }
            }
        });
    }
}

fn attribute_hit(cx: &Context, module: &str, name: &str, node: Node, out: &mut Vec<RawFinding>) {
    if let Some(a) = cx.data.attribute(module, name, cx.target.platform()) {
        out.push(warning(node, format!("`{module}.{name}` isn't available in MicroPython; {}", a.hint)));
    }
}

/// Syntax that MicroPython's compiler rejects.
pub struct CompatSyntax;

impl LintRule for CompatSyntax {
    fn id(&self) -> &'static str {
        "compat-syntax"
    }

    fn description(&self) -> &'static str {
        "Python syntax MicroPython doesn't support (match, except*, type aliases)"
    }

    fn check(&self, cx: &Context, out: &mut Vec<RawFinding>) {
        cx.walk(|n| {
            for s in cx.data.syntax.iter().filter(|s| s.node == n.kind()) {
                if let Some(tok) = &s.token {
                    let mut c = n.walk();
                    if !n.children(&mut c).any(|ch| !ch.is_named() && ch.kind() == tok) {
                        continue;
                    }
                }
                // Underline the first line only, not a whole block.
                let text = cx.text(n);
                let end = n.start_byte() + text.find('\n').unwrap_or(text.len());
                out.push(RawFinding {
                    severity: Severity::Error,
                    message: s.message.clone(),
                    start: n.start_byte(),
                    end,
                });
            }
        });
    }
}

/// `Pin(n)` with a GPIO number the target board doesn't have, can't drive, or
/// shouldn't touch. Uses the profile's `[[pins]]` maps.
pub struct PinCheck;

impl LintRule for PinCheck {
    fn id(&self) -> &'static str {
        "pins"
    }

    fn description(&self) -> &'static str {
        "Pin numbers checked against the connected board's GPIO map"
    }

    fn needs_board(&self) -> bool {
        true
    }

    fn check(&self, cx: &Context, out: &mut Vec<RawFinding>) {
        let t = cx.target;
        let Some(map) = t.profile.and_then(|p| p.pin_map(t.board_id, t.chip)) else { return };
        let label = match (t.chip, t.board_id, t.profile) {
            (Some(c), _, _) => format!("the {c}"),
            (None, Some(b), _) => b.to_string(),
            (None, None, Some(p)) => p.name.clone(),
            _ => "this board".into(),
        };
        cx.walk(|n| {
            if n.kind() != "call" || !calls_pin(cx, n) {
                return;
            }
            let Some(args) = n.child_by_field_name("arguments") else { return };
            let (id, mode) = pin_args(cx, args);
            let Some((pin, node)) = id.and_then(|a| int_literal(cx.text(a)).map(|p| (p, a))) else { return };
            if !pins_contain(&map.gpio, pin) {
                out.push(RawFinding {
                    severity: Severity::Error,
                    message: format!("GPIO {pin} doesn't exist on {label} (valid: {})", map.gpio),
                    start: node.start_byte(),
                    end: node.end_byte(),
                });
                return;
            }
            let output = mode.is_some_and(|m| {
                let m = cx.text(m);
                m.ends_with("OUT") || m.ends_with("OPEN_DRAIN")
            });
            if output && pins_contain(&map.input_only, pin) {
                out.push(RawFinding {
                    severity: Severity::Error,
                    message: format!("GPIO {pin} is input-only on {label}"),
                    start: node.start_byte(),
                    end: node.end_byte(),
                });
            }
            if let Some(r) = map.reserved.iter().find(|r| pins_contain(&r.pins, pin)) {
                out.push(warning(node, format!("GPIO {pin} {}", r.reason)));
            }
        });
    }
}

/// `Pin(...)` or `<module>.Pin(...)`.
fn calls_pin(cx: &Context, call: Node) -> bool {
    match call.child_by_field_name("function") {
        Some(f) if f.kind() == "identifier" => cx.text(f) == "Pin",
        Some(f) if f.kind() == "attribute" => f.child_by_field_name("attribute").is_some_and(|a| cx.text(a) == "Pin"),
        _ => false,
    }
}

/// The `id` and `mode` arguments of a `Pin(id, mode, ...)` call.
fn pin_args<'a>(cx: &Context<'a>, args: Node<'a>) -> (Option<Node<'a>>, Option<Node<'a>>) {
    let (mut id, mut mode) = (None, None);
    let mut positional = 0;
    let mut c = args.walk();
    for a in args.named_children(&mut c) {
        match a.kind() {
            "keyword_argument" => {
                let value = a.child_by_field_name("value");
                match a.child_by_field_name("name").map(|n| cx.text(n)) {
                    Some("id") => id = value,
                    Some("mode") => mode = value,
                    _ => {}
                }
            }
            "comment" => {}
            _ => {
                match positional {
                    0 => id = Some(a),
                    1 => mode = Some(a),
                    _ => {}
                }
                positional += 1;
            }
        }
    }
    (id, mode)
}

fn int_literal(text: &str) -> Option<u32> {
    let t = text.replace('_', "").to_ascii_lowercase();
    match t.get(..2) {
        Some("0x") => u32::from_str_radix(&t[2..], 16).ok(),
        Some("0o") => u32::from_str_radix(&t[2..], 8).ok(),
        Some("0b") => u32::from_str_radix(&t[2..], 2).ok(),
        _ => t.parse().ok(),
    }
}

fn warning(node: Node, message: String) -> RawFinding {
    RawFinding { severity: Severity::Warning, message, start: node.start_byte(), end: node.end_byte() }
}
