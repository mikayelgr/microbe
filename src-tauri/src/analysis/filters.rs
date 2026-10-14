//! Built-in diagnostic filters.

use tree_sitter::Node;

use super::{Context, DiagnosticFilter, RawRegion};

/// Functions compiled by MicroPython itself (`@rp2.asm_pio`, inline assembler,
/// viper) use names that only exist inside them, so a type checker reports them
/// as undefined.
pub struct InlineCode;

impl DiagnosticFilter for InlineCode {
    fn id(&self) -> &'static str {
        "inline-code"
    }

    fn description(&self) -> &'static str {
        "Hide undefined-name errors inside @asm_pio, inline assembler and viper functions"
    }

    fn regions(&self, cx: &Context, out: &mut Vec<RawRegion>) {
        cx.walk(|n| {
            if n.kind() != "decorated_definition" {
                return;
            }
            let mut c = n.walk();
            for d in n.children(&mut c).filter(|d| d.kind() == "decorator") {
                let Some(name) = d.named_child(0).and_then(|e| decorator_name(cx, e)) else { continue };
                if let Some(entry) = cx.data.inline.iter().find(|e| e.decorators.iter().any(|x| x == name)) {
                    out.push(RawRegion {
                        start: n.start_byte(),
                        end: n.end_byte(),
                        names: if entry.any { Vec::new() } else { entry.names.clone() },
                    });
                    return;
                }
            }
        });
    }
}

/// `rp2.asm_pio(...)` → `asm_pio`.
fn decorator_name<'a>(cx: &Context<'a>, expr: Node<'a>) -> Option<&'a str> {
    match expr.kind() {
        "identifier" => Some(cx.text(expr)),
        "attribute" => expr.child_by_field_name("attribute").map(|a| cx.text(a)),
        "call" => expr.child_by_field_name("function").and_then(|f| decorator_name(cx, f)),
        _ => None,
    }
}
