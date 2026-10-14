//! Device-side Python script templates.
//!
//! Scripts live in `src-tauri/py/*.py` and use `{{name}}` placeholders that are
//! replaced with Python literals. A platform profile can override any script
//! by name (`[scripts]` table) for ports that lack a module or behave oddly.

use include_dir::{include_dir, Dir};

use crate::error::{Error, Result};
use crate::platform::PlatformProfile;

static SCRIPTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/py");

/// A value substituted into a template as a Python literal.
pub enum Py<'a> {
    Str(&'a str),
    Bytes(&'a [u8]),
    Int(i64),
    Bool(bool),
    /// Floats, arrays, objects: substituted as a Python string of their JSON.
    Json(&'a serde_json::Value),
}

impl<'a> Py<'a> {
    /// A JSON value as a Python literal (strings, integers, booleans; anything
    /// else is passed as its JSON text in a string).
    pub fn from_json(v: &'a serde_json::Value) -> Py<'a> {
        match v {
            serde_json::Value::Bool(b) => Py::Bool(*b),
            serde_json::Value::String(s) => Py::Str(s),
            serde_json::Value::Number(n) if n.is_i64() => Py::Int(n.as_i64().unwrap_or_default()),
            _ => Py::Json(v),
        }
    }
}

/// Render a template with JSON parameters (e.g. from a frontend request).
pub fn render_json(
    profile: &PlatformProfile,
    name: &str,
    args: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<String> {
    let params: Vec<(&str, Py)> = args.iter().map(|(k, v)| (k.as_str(), Py::from_json(v))).collect();
    render(profile, name, &params)
}

pub fn render(profile: &PlatformProfile, name: &str, params: &[(&str, Py)]) -> Result<String> {
    let template = match profile.scripts.get(name) {
        Some(s) => s.as_str(),
        None => SCRIPTS
            .get_file(format!("{name}.py"))
            .and_then(|f| f.contents_utf8())
            .ok_or_else(|| Error::other(format!("unknown script {name}")))?,
    };
    let mut out = template.to_string();
    for (key, val) in params {
        let lit = match val {
            Py::Str(s) => py_str(s),
            Py::Bytes(b) => py_bytes(b),
            Py::Int(i) => i.to_string(),
            Py::Bool(b) => if *b { "True" } else { "False" }.to_string(),
            Py::Json(v) => match v {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Null => "None".into(),
                other => py_str(&other.to_string()),
            },
        };
        out = out.replace(&format!("{{{{{key}}}}}"), &lit);
    }
    Ok(out)
}

/// A bundled file from `py/` as-is (no substitution), e.g. a helper module.
pub fn bundled(name: &str) -> Option<&'static str> {
    SCRIPTS.get_file(name).and_then(|f| f.contents_utf8())
}

pub fn py_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

pub fn py_bytes(b: &[u8]) -> String {
    let mut out = String::with_capacity(b.len() + 3);
    out.push_str("b'");
    for &c in b {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            0x20..=0x7e => out.push(c as char),
            _ => out.push_str(&format!("\\x{c:02x}")),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::PlatformRegistry;

    #[test]
    fn literals() {
        assert_eq!(py_str("a'b\\c\n"), "'a\\'b\\\\c\\n'");
        assert_eq!(py_bytes(b"a\x00'"), "b'a\\x00\\''");
    }

    #[test]
    fn json_params() {
        let reg = PlatformRegistry::load(None).unwrap();
        let mut p = reg.get("rp2").unwrap().clone();
        p.scripts.insert("t".into(), "f({{s}}, {{i}}, {{b}}, {{x}}, {{n}})".into());
        let args = serde_json::from_str(r#"{"s":"a'b","i":3,"b":true,"x":1.5,"n":null}"#).unwrap();
        assert_eq!(render_json(&p, "t", &args).unwrap(), "f('a\\'b', 3, True, 1.5, None)");
    }

    #[test]
    fn render_and_override() {
        let reg = PlatformRegistry::load(None).unwrap();
        let mut p = reg.get("rp2").unwrap().clone();
        let s = render(&p, "ls", &[("path", Py::Str("/lib"))]).unwrap();
        assert!(s.contains("_f('/lib')"));
        p.scripts.insert("ls".into(), "print({{path}})".into());
        assert_eq!(render(&p, "ls", &[("path", Py::Str("/"))]).unwrap(), "print('/')");
    }
}
