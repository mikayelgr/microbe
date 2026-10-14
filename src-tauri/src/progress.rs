//! Progress reporting for long-running operations.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ProgressEvent {
    pub id: String,
    pub label: String,
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
    pub finished: bool,
}

/// A progress callback that ignores updates.
pub fn none(_: u64, _: u64) {}

pub type ProgressSink = std::sync::Arc<dyn Fn(ProgressEvent) + Send + Sync>;

/// Emits throttled progress events (to the frontend, or any sink).
pub struct Reporter {
    sink: ProgressSink,
    id: String,
    label: Mutex<String>,
    last: Mutex<Instant>,
}

impl Reporter {
    /// Report as `progress` events to the frontend.
    pub fn new(app: &AppHandle, id: impl Into<String>, label: impl Into<String>) -> Self {
        let app = app.clone();
        Self::with_sink(std::sync::Arc::new(move |ev| { let _ = app.emit("progress", ev); }), id, label)
    }

    pub fn with_sink(sink: ProgressSink, id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            sink,
            id: id.into(),
            label: Mutex::new(label.into()),
            last: Mutex::new(Instant::now() - Duration::from_secs(1)),
        }
    }

    pub fn set_label(&self, label: impl Into<String>) {
        *self.label.lock().unwrap() = label.into();
        self.emit(0, 0, false);
    }

    pub fn update(&self, done: u64, total: u64) {
        let mut last = self.last.lock().unwrap();
        if last.elapsed() < Duration::from_millis(80) && done < total {
            return;
        }
        *last = Instant::now();
        drop(last);
        self.emit(done, total, false);
    }

    pub fn finish(&self) {
        self.emit(1, 1, true);
    }

    fn emit(&self, done: u64, total: u64, finished: bool) {
        let ev = ProgressEvent {
            id: self.id.clone(),
            label: self.label.lock().unwrap().clone(),
            done,
            total,
            finished,
        };
        (self.sink)(ev);
    }
}

impl Drop for Reporter {
    fn drop(&mut self) {
        self.finish();
    }
}
