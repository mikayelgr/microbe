//! The structured run channel.
//!
//! Device-side helpers (debugger, asyncio probe, test runner, memory sampler,
//! plot markers) print records as `\x1e<TAG> <json>\n` on stdout. The splitter
//! separates those records from ordinary program output, across chunk
//! boundaries, so one protocol serves every runtime feature.

pub const RS: u8 = 0x1e;

#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(Vec<u8>),
    Record { tag: String, payload: serde_json::Value },
}

#[derive(Default)]
pub struct Splitter {
    record: Option<Vec<u8>>,
}

impl Splitter {
    pub fn feed(&mut self, data: &[u8]) -> Vec<Piece> {
        let mut out = Vec::new();
        let mut text = Vec::new();
        for &b in data {
            match &mut self.record {
                Some(rec) => {
                    if b == b'\n' {
                        let rec = self.record.take().unwrap();
                        out.extend(flush(&mut text));
                        out.push(parse(&rec));
                    } else if b != b'\r' {
                        rec.push(b);
                    }
                }
                None if b == RS => self.record = Some(Vec::new()),
                None => text.push(b),
            }
        }
        out.extend(flush(&mut text));
        out
    }
}

fn flush(text: &mut Vec<u8>) -> Option<Piece> {
    (!text.is_empty()).then(|| Piece::Text(std::mem::take(text)))
}

fn parse(rec: &[u8]) -> Piece {
    let s = String::from_utf8_lossy(rec);
    let (tag, rest) = s.split_once(' ').unwrap_or((&s, ""));
    let payload = if rest.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(rest).unwrap_or_else(|_| serde_json::Value::String(rest.to_string()))
    };
    Piece::Record { tag: tag.to_string(), payload }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn splits_records_across_chunks() {
        let mut s = Splitter::default();
        let mut all = s.feed(b"hello\n\x1eDBG {\"line\":");
        all.extend(s.feed(b" 3}\r\nmore\x1eMEM 1024\n"));
        assert_eq!(
            all,
            vec![
                Piece::Text(b"hello\n".to_vec()),
                Piece::Record { tag: "DBG".into(), payload: json!({"line": 3}) },
                Piece::Text(b"more".to_vec()),
                Piece::Record { tag: "MEM".into(), payload: json!(1024) },
            ]
        );
    }

    #[test]
    fn non_json_payload_is_a_string() {
        let mut s = Splitter::default();
        assert_eq!(
            s.feed(b"\x1eNOTE hi there\n"),
            vec![Piece::Record { tag: "NOTE".into(), payload: json!("hi there") }]
        );
    }
}
