//! Test discovery for the on-device test runner.
//!
//! Test files are `test_*.py` / `*_test.py` in the project. Tests are top-level
//! `test_*` functions (sync or async) and `test_*` methods of top-level classes
//! named `Test*` or deriving from `TestCase`, in source order. The runner
//! itself is the `py/testrunner.py` prelude; results come back as `TEST`
//! records on the structured channel.

use std::path::Path;

use serde::Serialize;
use tree_sitter::Node;
use ts_rs::TS;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TestCase {
    /// `test_x` or `Class.test_x`.
    pub name: String,
    /// 1-based line of the definition.
    pub line: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TestFile {
    pub path: String,
    /// Path relative to the project root, with forward slashes.
    pub rel: String,
    pub tests: Vec<TestCase>,
}

pub fn is_test_file(name: &str) -> bool {
    name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py"))
}

/// The tests defined in one source file.
pub fn find_tests(source: &str) -> Result<Vec<TestCase>> {
    let tree = crate::analysis::parse(source)?;
    let src = source.as_bytes();
    let text = |n: Node| n.utf8_text(src).unwrap_or("").to_string();
    let mut out = Vec::new();
    let mut c = tree.root_node().walk();
    for stmt in tree.root_node().named_children(&mut c) {
        let def = definition(stmt);
        match def.kind() {
            "function_definition" => {
                if let Some(name) = def.child_by_field_name("name").map(text).filter(|n| n.starts_with("test")) {
                    out.push(TestCase { name, line: def.start_position().row as u32 + 1 });
                }
            }
            "class_definition" => {
                let Some(class) = def.child_by_field_name("name").map(text) else { continue };
                let bases = def.child_by_field_name("superclasses").map(text).unwrap_or_default();
                if !(class.starts_with("Test") || bases.contains("TestCase")) {
                    continue;
                }
                let Some(body) = def.child_by_field_name("body") else { continue };
                let mut bc = body.walk();
                for m in body.named_children(&mut bc).map(definition) {
                    if m.kind() != "function_definition" {
                        continue;
                    }
                    if let Some(name) = m.child_by_field_name("name").map(text).filter(|n| n.starts_with("test")) {
                        out.push(TestCase { name: format!("{class}.{name}"), line: m.start_position().row as u32 + 1 });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The definition inside a `decorated_definition`, else the node itself.
fn definition(n: Node) -> Node {
    if n.kind() == "decorated_definition" {
        n.child_by_field_name("definition").unwrap_or(n)
    } else {
        n
    }
}

/// Every test file under `root` with its tests (files without tests are left out).
pub fn discover(root: &Path) -> Result<Vec<TestFile>> {
    let mut out = Vec::new();
    for (rel, path) in crate::sync::local_files(root, &[])? {
        let name = rel.rsplit('/').next().unwrap_or(&rel);
        if !is_test_file(name) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else { continue };
        let tests = find_tests(&source)?;
        if !tests.is_empty() {
            out.push(TestFile { path: path.display().to_string(), rel, tests });
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn tests_discover(root: String) -> Result<Vec<TestFile>> {
    tauri::async_runtime::spawn_blocking(move || discover(Path::new(&root)))
        .await
        .map_err(|e| crate::error::Error::other(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_functions_classes_and_async_tests() {
        let src = "import unittest\n\ndef helper():\n    pass\n\ndef test_add():\n    assert 1 + 1 == 2\n\nasync def test_sleep():\n    pass\n\n@skip\ndef test_decorated():\n    pass\n\nclass TestMath:\n    def setUp(self):\n        pass\n    def test_mul(self):\n        pass\n\nclass Other(unittest.TestCase):\n    def test_x(self):\n        pass\n\nclass NotATest:\n    def test_no(self):\n        pass\n";
        let names: Vec<(String, u32)> = find_tests(src).unwrap().into_iter().map(|t| (t.name, t.line)).collect();
        assert_eq!(
            names,
            vec![
                ("test_add".into(), 6),
                ("test_sleep".into(), 9),
                ("test_decorated".into(), 13),
                ("TestMath.test_mul".into(), 19),
                ("Other.test_x".into(), 23),
            ]
        );
    }

    #[test]
    fn discovers_test_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(dir.path().join("tests/test_a.py"), "def test_one():\n    pass\n").unwrap();
        std::fs::write(dir.path().join("b_test.py"), "def test_two():\n    pass\n").unwrap();
        std::fs::write(dir.path().join("test_empty.py"), "x = 1\n").unwrap();
        std::fs::write(dir.path().join("main.py"), "def test_not_here():\n    pass\n").unwrap();
        let files = discover(dir.path()).unwrap();
        let rels: Vec<_> = files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, vec!["b_test.py", "tests/test_a.py"]);
    }
}
