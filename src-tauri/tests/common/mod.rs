//! Shared harness: a session on the MicroPython unix port in a PTY.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use microbe_lib::platform::PlatformRegistry;
use microbe_lib::repl::session::{Session, SessionEvent, TermEvent};
use microbe_lib::transport::{local, Endpoint};

pub struct Harness {
    pub session: Arc<Session>,
    pub term: Arc<Mutex<Vec<u8>>>,
    pub events: Arc<Mutex<Vec<SessionEvent>>>,
    pub records: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    pub dir: tempfile::TempDir,
}

pub fn start() -> Option<Harness> {
    let exe = local::find_micropython()?;
    let dir = tempfile::tempdir().unwrap();
    let conn = local::spawn_pty(&exe, dir.path()).unwrap();
    let platforms = PlatformRegistry::load(None).unwrap();
    let profile = platforms.get("unix").unwrap().clone();
    let term = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let records = Arc::new(Mutex::new(Vec::new()));
    let (t, e, r) = (term.clone(), events.clone(), records.clone());
    let endpoint = Endpoint {
        transport: local::ID.into(),
        address: exe,
        label: "test".into(),
        detail: None,
        usb: None,
        platform_hint: Some("unix".into()),
        role: None,
    };
    let session = Session::start(
        "t1".into(),
        endpoint,
        conn,
        profile,
        Arc::new(move |ev| match ev {
            TermEvent::Data { data } | TermEvent::Run { data, .. } => t.lock().unwrap().extend_from_slice(&data),
            TermEvent::Structured { tag, payload } => r.lock().unwrap().push((tag, payload)),
        }),
        Arc::new(move |ev| e.lock().unwrap().push(ev)),
    );
    Some(Harness { session, term, events, records, dir })
}

pub async fn wait_for(h: &Harness, cond: impl Fn() -> bool) {
    for _ in 0..100 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!(
        "condition not met in time; terminal: {:?}; events: {:?}",
        String::from_utf8_lossy(&h.term.lock().unwrap()),
        h.events.lock().unwrap()
    );
}


/// Wait until the current run finishes; returns its error, if any.
pub async fn run_finished(h: &Harness) -> Option<String> {
    wait_for(h, || h.events.lock().unwrap().iter().any(|e| matches!(e, SessionEvent::RunFinished { .. }))).await;
    let events = std::mem::take(&mut *h.events.lock().unwrap());
    events.into_iter().rev().find_map(|e| match e {
        SessionEvent::RunFinished { error } => Some(error),
        _ => None,
    }).flatten()
}

/// Structured records with this tag received so far.
pub fn records(h: &Harness, tag: &str) -> Vec<serde_json::Value> {
    h.records.lock().unwrap().iter().filter(|(t, _)| t == tag).map(|(_, p)| p.clone()).collect()
}
