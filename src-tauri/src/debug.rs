//! A debugger that works on stock firmware.
//!
//! Stock MicroPython builds have no `sys.settrace`, so the program is
//! instrumented on the host instead: every statement gets a hook that asks
//! the device-side debugger (`py/debugger.py`) whether to stop. Hooks are
//! inserted *within* existing lines (`hook; stmt`, `if (hook) or (cond):`), so
//! line numbers, and therefore tracebacks, stay correct.
//!
//! MicroPython's `locals()` returns the globals inside functions, so local
//! variables are passed as `("name", lambda: name)` pairs. The lambdas are only
//! created when the debugger decides to stop.
//!
//! Protocol: the debugger prints `\x1eDBG {...}` records (`stopped`, `eval`)
//! and reads one-line commands from stdin while stopped: `c` continue, `s` step
//! into, `n` step over, `o` step out, `b 3,7` set breakpoints, `e <expr>`
//! evaluate in the global scope.

use std::collections::BTreeSet;

use serde::Serialize;
use tree_sitter::Node;
use ts_rs::TS;

use crate::error::Result;

/// Name of the debugger object the hooks call (no leading `__`, which
/// CPython would mangle inside classes).
const DBG: &str = "_mbdbg";

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Instrumented {
    pub code: String,
    /// Function names by id; id 0 is the module.
    pub functions: Vec<String>,
    /// 1-based lines where the program can stop.
    pub lines: Vec<u32>,
}

struct Scope {
    id: usize,
    /// Local variable names (empty for the module, which uses globals).
    locals: Vec<String>,
    /// The function's entry hook is still to be placed.
    entry_pending: bool,
}

struct Rewriter<'a> {
    src: &'a [u8],
    edits: Vec<(usize, String)>,
    functions: Vec<String>,
    lines: BTreeSet<u32>,
    /// Start of a statement that shares its line with an unhooked clause
    /// header (`else: x = 1`) and may be hooked anyway.
    inline_ok: Option<usize>,
}

pub fn instrument(source: &str) -> Result<Instrumented> {
    let tree = crate::analysis::parse(source)?;
    let mut w = Rewriter { src: source.as_bytes(), edits: Vec::new(), functions: vec!["<module>".into()], lines: BTreeSet::new(), inline_ok: None };
    let mut module = Scope { id: 0, locals: Vec::new(), entry_pending: false };
    w.block(tree.root_node(), &mut module, "", false);

    let mut code = source.to_string();
    w.edits.sort_by(|a, b| b.0.cmp(&a.0));
    for (at, text) in w.edits {
        code.insert_str(at, &text);
    }
    Ok(Instrumented { code, functions: w.functions, lines: w.lines.into_iter().collect() })
}

impl<'a> Rewriter<'a> {
    fn text(&self, n: Node) -> &'a str {
        n.utf8_text(self.src).unwrap_or("")
    }

    /// Only the first statement on a line gets a hook (not `b` in `a; b`, nor
    /// the body of `if x: y`).
    fn starts_line(&self, n: Node) -> bool {
        let start = n.start_byte();
        if self.inline_ok == Some(start) {
            return true;
        }
        let line_start = self.src[..start].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        self.src[line_start..start].iter().all(|b| b.is_ascii_whitespace())
    }

    /// The hook expression for a statement at `node`'s line. Always falsy.
    fn hook(&mut self, node: Node, scope: &mut Scope) -> String {
        let line = node.start_position().row as u32 + 1;
        self.lines.insert(line);
        let vars: String = scope.locals.iter().map(|n| format!("(\"{n}\", lambda: {n}), ")).collect();
        let stop = format!("{DBG}.l({line}, {}) and {DBG}.s(({vars}))", scope.id);
        if std::mem::take(&mut scope.entry_pending) {
            format!("({DBG}.e({}) or {stop})", scope.id)
        } else {
            format!("({stop})")
        }
    }

    /// Hook the statements of a block (or the module). `class_body`: class-level
    /// statements run once at definition and can't see their names from a
    /// lambda, so only the methods inside are instrumented.
    fn block(&mut self, block: Node, scope: &mut Scope, qual: &str, class_body: bool) {
        let mut c = block.walk();
        let stmts: Vec<Node> = block.named_children(&mut c).collect();
        for stmt in stmts {
            self.statement(stmt, scope, qual, class_body);
        }
    }

    fn statement(&mut self, stmt: Node, scope: &mut Scope, qual: &str, class_body: bool) {
        let hookable = !class_body && self.starts_line(stmt);
        match stmt.kind() {
            "comment" | "global_statement" | "nonlocal_statement" | "future_import_statement" => {}
            "function_definition" => self.function(stmt, qual),
            "class_definition" => {
                let name = stmt.child_by_field_name("name").map(|n| self.text(n)).unwrap_or("?");
                let qual = join(qual, name);
                if let Some(body) = stmt.child_by_field_name("body") {
                    self.block(body, scope, &qual, true);
                }
            }
            "decorated_definition" => {
                if let Some(def) = stmt.child_by_field_name("definition") {
                    self.statement(def, scope, qual, class_body);
                }
            }
            "if_statement" | "while_statement" => {
                if hookable {
                    if let Some(cond) = stmt.child_by_field_name("condition") {
                        self.wrap(cond, stmt, scope);
                    }
                }
                self.suites(stmt, scope, qual, class_body);
            }
            "for_statement" => {
                if hookable {
                    if let Some(right) = stmt.child_by_field_name("right") {
                        self.wrap(right, stmt, scope);
                    }
                }
                self.suites(stmt, scope, qual, class_body);
            }
            "with_statement" => {
                if hookable {
                    if let Some(value) = first_with_value(stmt) {
                        self.wrap(value, stmt, scope);
                    }
                }
                self.suites(stmt, scope, qual, class_body);
            }
            "try_statement" | "match_statement" => self.suites(stmt, scope, qual, class_body),
            _ if stmt.kind().ends_with("_statement") => {
                if hookable {
                    let h = self.hook(stmt, scope);
                    self.edits.push((stmt.start_byte(), format!("{h}; ")));
                }
            }
            _ => {}
        }
    }

    /// `expr` → `(hook) or (expr)`, hooked at `stmt`'s line.
    fn wrap(&mut self, expr: Node, stmt: Node, scope: &mut Scope) {
        let h = self.hook(stmt, scope);
        self.edits.push((expr.start_byte(), format!("{h} or (")));
        self.edits.push((expr.end_byte(), ")".into()));
    }

    /// Recurse into the blocks of a compound statement and its clauses
    /// (`elif`, `else`, `except`, `finally`).
    fn suites(&mut self, stmt: Node, scope: &mut Scope, qual: &str, class_body: bool) {
        let mut c = stmt.walk();
        let children: Vec<Node> = stmt.named_children(&mut c).collect();
        let headless = matches!(stmt.kind(), "try_statement" | "else_clause" | "except_clause" | "except_group_clause" | "finally_clause");
        for child in children {
            match child.kind() {
                "block" => {
                    if headless && child.start_position().row == stmt.start_position().row {
                        self.inline_ok = Some(child.start_byte());
                    }
                    self.block(child, scope, qual, class_body)
                }
                "elif_clause" => {
                    if let Some(cond) = child.child_by_field_name("condition").filter(|_| self.starts_line(child)) {
                        self.wrap(cond, child, scope);
                    }
                    self.suites(child, scope, qual, class_body);
                }
                "else_clause" | "except_clause" | "except_group_clause" | "finally_clause" | "case_clause" => {
                    self.suites(child, scope, qual, class_body)
                }
                _ => {}
            }
        }
    }

    fn function(&mut self, def: Node, qual: &str) {
        let name = def.child_by_field_name("name").map(|n| self.text(n)).unwrap_or("?");
        let qual = join(qual, name);
        let id = self.functions.len();
        self.functions.push(qual.clone());
        let Some(body) = def.child_by_field_name("body") else { return };
        let mut names = BTreeSet::new();
        let mut declared = BTreeSet::new();
        if let Some(params) = def.child_by_field_name("parameters") {
            self.params(params, &mut names);
        }
        self.bindings(body, &mut names, &mut declared);
        let locals = names.into_iter().filter(|n| !declared.contains(n) && n != DBG).collect();
        let mut scope = Scope { id, locals, entry_pending: true };
        // A one-line body (`def f(): return 1`) isn't hooked.
        if body.start_position().row != def.start_position().row {
            self.block(body, &mut scope, &qual, false);
        }
    }

    fn params(&self, params: Node, out: &mut BTreeSet<String>) {
        let mut c = params.walk();
        for p in params.named_children(&mut c) {
            let ident = match p.kind() {
                "identifier" => Some(p),
                "default_parameter" | "typed_default_parameter" => p.child_by_field_name("name"),
                _ => first_identifier(p),
            };
            if let Some(i) = ident.filter(|i| i.kind() == "identifier") {
                out.insert(self.text(i).to_string());
            }
        }
    }

    /// Names a function body binds, without descending into nested scopes;
    /// `declared` collects `global` / `nonlocal` names.
    fn bindings(&self, node: Node, out: &mut BTreeSet<String>, declared: &mut BTreeSet<String>) {
        let mut c = node.walk();
        for child in node.named_children(&mut c) {
            match child.kind() {
                "function_definition" | "class_definition" => {
                    if let Some(n) = child.child_by_field_name("name") {
                        out.insert(self.text(n).to_string());
                    }
                    continue;
                }
                "lambda" | "list_comprehension" | "dictionary_comprehension" | "set_comprehension" | "generator_expression" => continue,
                "global_statement" | "nonlocal_statement" => {
                    let mut cc = child.walk();
                    for n in child.named_children(&mut cc).filter(|n| n.kind() == "identifier") {
                        declared.insert(self.text(n).to_string());
                    }
                    continue;
                }
                "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
                    if let Some(left) = child.child_by_field_name("left") {
                        self.targets(left, out);
                    }
                }
                "named_expression" => {
                    if let Some(n) = child.child_by_field_name("name") {
                        out.insert(self.text(n).to_string());
                    }
                }
                "as_pattern" => {
                    if let Some(alias) = child.child_by_field_name("alias") {
                        self.targets(alias, out);
                    }
                }
                "except_clause" => {
                    if let Some(alias) = child.child_by_field_name("alias") {
                        self.targets(alias, out);
                    }
                }
                "import_statement" | "import_from_statement" => {
                    let mut cc = child.walk();
                    for n in child.children_by_field_name("name", &mut cc) {
                        let bound = match n.kind() {
                            "aliased_import" => n.child_by_field_name("alias"),
                            _ => Some(n),
                        };
                        if let Some(b) = bound {
                            let t = self.text(b);
                            out.insert(t.split('.').next().unwrap_or(t).to_string());
                        }
                    }
                }
                _ => {}
            }
            self.bindings(child, out, declared);
        }
    }

    /// Identifiers bound by an assignment target (not attributes or subscripts).
    fn targets(&self, n: Node, out: &mut BTreeSet<String>) {
        match n.kind() {
            "identifier" => {
                out.insert(self.text(n).to_string());
            }
            "pattern_list" | "tuple_pattern" | "list_pattern" | "list_splat_pattern" | "expression_list" | "tuple"
            | "list" | "parenthesized_expression" | "as_pattern_target" | "list_splat" => {
                let mut c = n.walk();
                for child in n.named_children(&mut c) {
                    self.targets(child, out);
                }
            }
            _ => {}
        }
    }
}

fn join(qual: &str, name: &str) -> String {
    if qual.is_empty() {
        name.to_string()
    } else {
        format!("{qual}.{name}")
    }
}

fn first_identifier(n: Node) -> Option<Node> {
    if n.kind() == "identifier" {
        return Some(n);
    }
    let mut c = n.walk();
    let children: Vec<Node> = n.named_children(&mut c).collect();
    children.into_iter().find_map(first_identifier)
}

/// The expression of the first `with` item, without its `as` target.
fn first_with_value(stmt: Node) -> Option<Node> {
    let mut c = stmt.walk();
    let clause = stmt.named_children(&mut c).find(|n| n.kind() == "with_clause")?;
    let mut cc = clause.walk();
    let item = clause.named_children(&mut cc).find(|n| n.kind() == "with_item")?;
    let value = item.child_by_field_name("value")?;
    if value.kind() == "as_pattern" {
        value.named_child(0)
    } else {
        Some(value)
    }
}

#[tauri::command]
pub fn debug_instrument(source: String) -> Result<Instrumented> {
    instrument(&source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_keep_line_numbers() {
        let src = "import time\nx = 1\nif x > 0:\n    y = 2\nelif x < 0:\n    y = 3\nelse: y = 4\nfor i in range(3):\n    x += i; y = i\nwhile x > 100:\n    x -= 1\n";
        let out = instrument(src).unwrap();
        assert_eq!(out.code.lines().count(), src.lines().count());
        assert_eq!(out.lines, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        let lines: Vec<&str> = out.code.lines().collect();
        assert_eq!(lines[1], "(_mbdbg.l(2, 0) and _mbdbg.s(())); x = 1");
        assert_eq!(lines[2], "if (_mbdbg.l(3, 0) and _mbdbg.s(())) or (x > 0):");
        assert!(lines[4].starts_with("elif (_mbdbg.l(5, 0)") && lines[4].ends_with("or (x < 0):"));
        // `else` has no hook of its own, so its inline body gets one.
        assert!(lines[6].starts_with("else: (_mbdbg.l(7, 0)") && lines[6].ends_with("; y = 4"), "{}", lines[6]);
        assert!(lines[7].ends_with("or (range(3)):"));
        // Only the first statement on a line is hooked.
        assert!(lines[8].matches("_mbdbg.l").count() == 1 && lines[8].ends_with("x += i; y = i"));
    }

    #[test]
    fn functions_pass_locals_and_enter() {
        let src = "G = 1\n\nclass Led:\n    PIN = 25\n    def toggle(self, n=1):\n        global G\n        for k in range(n):\n            v = k\n        with open('f') as fh:\n            pass\n        return v\n\nasync def main():\n    await go()\n";
        let out = instrument(src).unwrap();
        assert_eq!(out.functions, vec!["<module>", "Led.toggle", "main"]);
        let lines: Vec<&str> = out.code.lines().collect();
        // Class-level statements aren't hooked; method bodies are.
        assert_eq!(lines[3], "    PIN = 25");
        assert_eq!(lines[5], "        global G");
        let first = lines[6];
        assert!(first.contains("_mbdbg.e(1) or _mbdbg.l(7, 1)"), "{first}");
        for v in ["fh", "k", "n", "self", "v"] {
            assert!(first.contains(&format!("(\"{v}\", lambda: {v})")), "{v} in {first}");
        }
        assert!(!first.contains("\"G\""));
        assert!(lines[8].contains("or (open('f')) as fh:"), "{}", lines[8]);
        assert!(lines[13].contains("_mbdbg.e(2)") && lines[13].ends_with("; await go()"));
    }

    #[test]
    fn instrumented_code_is_valid_python() {
        let src = "def f(a, *args, b=2, **kw):\n    \"\"\"doc\"\"\"\n    try:\n        x = [i for i in args]\n    except ValueError as e:\n        raise\n    finally:\n        pass\n    return lambda: a\n";
        let out = instrument(src).unwrap();
        let tree = crate::analysis::parse(&out.code).unwrap();
        assert!(!tree.root_node().has_error(), "{}", out.code);
        assert!(out.code.contains("(\"e\", lambda: e)") && out.code.contains("(\"kw\", lambda: kw)"));
        assert!(!out.code.contains("(\"i\", lambda: i)"), "comprehension variables are not locals");
    }
}
