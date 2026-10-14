//! A live connection to one device.
//!
//! Device output is routed either to the terminal (friendly REPL) or to the
//! [`RawRepl`] owned by whichever operation currently holds the session. Ops
//! are serialized; a running program holds the session until it finishes.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Notify};
use ts_rs::TS;

use super::raw::{RawRepl, Stream};
use super::structured::{Piece, Splitter};
use crate::device_info::DeviceInfo;
use crate::error::{Error, Result};
use crate::platform::{PlatformProfile, ResolvedTarget};
use crate::transport::{Connection, Endpoint, Transport};

/// Bytes for the terminal view.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum TermEvent {
    /// Friendly-REPL output.
    Data { data: Vec<u8> },
    /// Output of a program started with Run.
    Run { data: Vec<u8>, stderr: bool },
    /// A `\x1e<TAG> <json>` record from a device-side helper.
    Structured {
        tag: String,
        #[ts(type = "unknown")]
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SessionState {
    Idle,
    Busy,
    Running,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum SessionEvent {
    State { state: SessionState },
    RunFinished { error: Option<String> },
    Disconnected { reason: Option<String> },
    /// The board didn't answer the raw REPL, even after escalating interrupts.
    Unresponsive { reason: String },
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export)]
pub struct RescueOptions {
    /// Reset the board first (control lines). Boards without reset lines need
    /// the user to press RESET or replug during the interrupt window.
    pub reset: bool,
    /// Rename main.py → main.py.disabled once the REPL is reached.
    pub disable_main: bool,
    /// How long to keep sending Ctrl-C, in milliseconds.
    #[serde(default = "default_window")]
    #[ts(type = "number")]
    pub window_ms: u64,
}

fn default_window() -> u64 {
    8000
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export)]
pub struct RescueReport {
    pub reached_repl: bool,
    /// A hardware reset was performed by the IDE.
    pub reset_performed: bool,
    /// The transport can't reset the board; the user must press RESET.
    pub needs_manual_reset: bool,
    /// Files renamed to stop them running at boot.
    pub disabled: Vec<String>,
}

/// Output of a snippet run with [`Session::exec_capture`].
#[derive(Debug, Clone, Default)]
pub struct ExecOutput {
    pub stdout: String,
    /// The traceback, if the code raised.
    pub stderr: String,
    /// The code was still running at the deadline and was interrupted.
    pub timed_out: bool,
}

/// The most recent terminal output (REPL and program), for tools that read it
/// after the fact. `total` counts every byte ever recorded, so readers can ask
/// for what arrived after an earlier position.
#[derive(Default)]
pub struct OutputLog {
    buf: std::collections::VecDeque<u8>,
    total: u64,
}

impl OutputLog {
    const CAP: usize = 64 * 1024;

    fn record(&mut self, data: &[u8]) {
        self.total += data.len() as u64;
        self.buf.extend(data);
        let excess = self.buf.len().saturating_sub(Self::CAP);
        self.buf.drain(..excess);
    }

    /// Output after byte position `after` (clamped to what is still kept),
    /// and the position to continue from.
    pub fn since(&self, after: u64) -> (String, u64) {
        let start = self.total - self.buf.len() as u64;
        let skip = after.clamp(start, self.total) - start;
        let bytes: Vec<u8> = self.buf.iter().skip(skip as usize).copied().collect();
        (String::from_utf8_lossy(&bytes).into_owned(), self.total)
    }
}

pub type TermSink = Arc<dyn Fn(TermEvent) + Send + Sync>;
pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

const ROUTE_TERMINAL: u8 = 0;
const ROUTE_CAPTURE: u8 = 1;

pub struct Session {
    pub id: String,
    pub endpoint: Endpoint,
    transport: Arc<dyn Transport>,
    route: Arc<AtomicU8>,
    repl: Arc<tokio::sync::Mutex<RawRepl>>,
    state: Arc<Mutex<SessionState>>,
    term: TermSink,
    events: EventSink,
    cancel_run: Arc<Notify>,
    pub output: Arc<Mutex<OutputLog>>,
    pub profile: Mutex<PlatformProfile>,
    pub target: Mutex<Option<ResolvedTarget>>,
    pub info: Mutex<Option<DeviceInfo>>,
}

impl Session {
    pub fn start(
        id: String,
        endpoint: Endpoint,
        conn: Connection,
        profile: PlatformProfile,
        term: TermSink,
        events: EventSink,
    ) -> Arc<Self> {
        let Connection { transport, mut incoming } = conn;
        let (cap_tx, cap_rx) = mpsc::unbounded_channel();
        let mut repl = RawRepl::new(transport.clone(), cap_rx);
        repl.chunk = profile.transfer_chunk.unwrap_or(1024) as usize;
        let route = Arc::new(AtomicU8::new(ROUTE_TERMINAL));
        let state = Arc::new(Mutex::new(SessionState::Idle));
        let output = Arc::new(Mutex::new(OutputLog::default()));
        let term: TermSink = {
            let output = output.clone();
            Arc::new(move |ev: TermEvent| {
                if let TermEvent::Data { data } | TermEvent::Run { data, .. } = &ev {
                    output.lock().unwrap().record(data);
                }
                term(ev)
            })
        };

        // Pump: route each chunk to the terminal or to the raw-REPL owner.
        {
            let route = route.clone();
            let term = term.clone();
            let events = events.clone();
            let state = state.clone();
            tokio::spawn(async move {
                while let Some(data) = incoming.recv().await {
                    if route.load(Ordering::Acquire) == ROUTE_CAPTURE {
                        let _ = cap_tx.send(data);
                    } else {
                        term(TermEvent::Data { data });
                    }
                }
                *state.lock().unwrap() = SessionState::Disconnected;
                events(SessionEvent::State { state: SessionState::Disconnected });
                events(SessionEvent::Disconnected { reason: Some("device disconnected".into()) });
            });
        }

        Arc::new(Self {
            id,
            endpoint,
            transport,
            route,
            repl: Arc::new(tokio::sync::Mutex::new(repl)),
            state,
            term,
            events,
            cancel_run: Arc::new(Notify::new()),
            output,
            profile: Mutex::new(profile),
            target: Mutex::new(None),
            info: Mutex::new(None),
        })
    }

    pub fn state(&self) -> SessionState {
        *self.state.lock().unwrap()
    }

    fn set_state(&self, s: SessionState) {
        set_state(&self.state, &self.events, s);
    }

    pub fn profile(&self) -> PlatformProfile {
        self.profile.lock().unwrap().clone()
    }

    fn ensure_alive(&self) -> Result<()> {
        match self.state() {
            SessionState::Disconnected => Err(Error::NotConnected),
            SessionState::Running => Err(Error::Busy("a program is running — stop it first")),
            _ => Ok(()),
        }
    }

    /// Keystrokes from the terminal (or stdin for a running program).
    pub async fn write(&self, data: &[u8]) -> Result<()> {
        match self.state() {
            SessionState::Disconnected => Err(Error::NotConnected),
            SessionState::Busy => Ok(()), // dropped: an internal op owns the REPL
            SessionState::Running if data.contains(&0x03) => self.transport.interrupt().await,
            _ => self.transport.write(data).await,
        }
    }

    /// Enter raw REPL and hold the session until the guard is finished or dropped.
    pub async fn raw(&self) -> Result<RawSession> {
        self.ensure_alive()?;
        let repl = self.repl.clone().lock_owned().await;
        self.ensure_alive()?;
        self.route.store(ROUTE_CAPTURE, Ordering::Release);
        self.set_state(SessionState::Busy);
        let mut guard = RawSession {
            repl: Some(repl),
            route: self.route.clone(),
            state: self.state.clone(),
            events: self.events.clone(),
        };
        if let Err(e) = guard.enter(false).await {
            guard.finish().await.ok();
            self.report_unresponsive(&e);
            return Err(e);
        }
        Ok(guard)
    }

    fn report_unresponsive(&self, e: &Error) {
        if matches!(e, Error::Unresponsive) {
            (self.events)(SessionEvent::Unresponsive { reason: e.to_string() });
        }
    }

    /// Apply version quirks (after the board was identified).
    pub async fn apply_quirks(&self, q: &super::quirks::Effective) {
        let mut repl = self.repl.lock().await;
        if q.raw_paste.is_some() {
            repl.set_raw_paste(q.raw_paste);
        }
        if let Some(c) = q.transfer_chunk {
            repl.chunk = repl.chunk.min(c);
        }
    }

    /// Regain control of a board whose program (often main.py) never returns:
    /// optionally reset it, flood Ctrl-C through boot, and optionally disable
    /// main.py so it doesn't start again.
    pub async fn rescue(&self, opts: &RescueOptions) -> Result<RescueReport> {
        match self.state() {
            SessionState::Disconnected => return Err(Error::NotConnected),
            SessionState::Running => {
                let _ = self.stop(true).await;
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            _ => {}
        }
        let profile = self.profile();
        let mut repl = self.repl.lock().await;
        self.route.store(ROUTE_CAPTURE, Ordering::Release);
        self.set_state(SessionState::Busy);
        let res = async {
            let mut report = RescueReport::default();
            if opts.reset {
                report.reset_performed = self.transport.hard_reset(profile.serial.hard_reset.as_deref()).await?;
                report.needs_manual_reset = !report.reset_performed;
            }
            let window = Duration::from_millis(opts.window_ms.max(500));
            report.reached_repl = repl.interrupt_flood(window, Duration::from_millis(20)).await?;
            if report.reached_repl && opts.disable_main {
                repl.enter(false).await?;
                let main = crate::fs::join(&profile.fs_root, "main.py");
                let renamed = async {
                    if crate::fs::stat_in(&mut repl, &profile, &main).await?.is_some() {
                        let mut target = format!("{main}.disabled");
                        let mut n = 1;
                        while crate::fs::stat_in(&mut repl, &profile, &target).await?.is_some() {
                            n += 1;
                            target = format!("{main}.disabled{n}");
                        }
                        crate::fs::rename_in(&mut repl, &profile, &main, &target).await?;
                        return Ok::<_, Error>(Some(target));
                    }
                    Ok(None)
                }
                .await;
                let _ = repl.exit().await;
                report.disabled.extend(renamed?);
            }
            Ok(report)
        }
        .await;
        self.route.store(ROUTE_TERMINAL, Ordering::Release);
        if self.state() != SessionState::Disconnected {
            self.set_state(SessionState::Idle);
        }
        res
    }

    /// Start running `code`; output streams to the terminal as `TermEvent::Run`
    /// and structured records as `TermEvent::Structured`. `preludes` run first
    /// in the same namespace (raw REPL keeps globals between executions).
    pub async fn run(self: &Arc<Self>, code: String, preludes: Vec<String>) -> Result<()> {
        self.run_with(code, preludes, Vec::new()).await
    }

    /// Like [`run`](Self::run), plus `cleanups` executed after the program ends
    /// (normally, with an error, or interrupted), e.g. to stop a sampling timer
    /// a prelude started. Their output is discarded.
    pub async fn run_with(self: &Arc<Self>, code: String, preludes: Vec<String>, cleanups: Vec<String>) -> Result<()> {
        self.ensure_alive()?;
        let mut repl = self.repl.clone().lock_owned().await;
        self.ensure_alive()?;
        self.route.store(ROUTE_CAPTURE, Ordering::Release);
        self.set_state(SessionState::Running);

        let started = async {
            repl.enter(false).await?;
            for p in &preludes {
                repl.exec(p).await?;
            }
            repl.exec_start(code.as_bytes()).await
        }
        .await;
        if let Err(e) = started {
            self.report_unresponsive(&e);
            let _ = repl.exit().await;
            self.route.store(ROUTE_TERMINAL, Ordering::Release);
            self.set_state(SessionState::Idle);
            return Err(e);
        }

        let this = self.clone();
        tokio::spawn(async move {
            let term = this.term.clone();
            let mut last = b'\n';
            let mut stderr_text = Vec::new();
            let mut splitter = Splitter::default();
            let res = tokio::select! {
                r = repl.follow(None, |s, d| {
                    if s == Stream::Stderr {
                        last = *d.last().unwrap_or(&last);
                        stderr_text.extend_from_slice(d);
                        term(TermEvent::Run { data: d.to_vec(), stderr: true });
                        return;
                    }
                    for piece in splitter.feed(d) {
                        match piece {
                            Piece::Text(t) => {
                                last = *t.last().unwrap_or(&last);
                                term(TermEvent::Run { data: t, stderr: false });
                            }
                            Piece::Record { tag, payload } => term(TermEvent::Structured { tag, payload }),
                        }
                    }
                }) => r,
                _ = this.cancel_run.notified() => Err(Error::other("stopped")),
            };
            // A program abandoned by a forced stop may still own the board.
            if res.is_ok() {
                for c in &cleanups {
                    let _ = repl.exec(c).await;
                }
            }
            let _ = repl.exit().await;
            this.route.store(ROUTE_TERMINAL, Ordering::Release);
            drop(repl);
            let prompt: &[u8] = if last == b'\n' { b">>> " } else { b"\r\n>>> " };
            (this.term)(TermEvent::Data { data: prompt.to_vec() });
            let error = match res {
                Err(e) => Some(e.to_string()),
                Ok(()) if !stderr_text.is_empty() => {
                    Some(super::raw::last_error_line(&stderr_text))
                }
                Ok(()) => None,
            };
            if this.state() != SessionState::Disconnected {
                this.set_state(SessionState::Idle);
            }
            (this.events)(SessionEvent::RunFinished { error });
        });
        Ok(())
    }

    /// Run a snippet in raw REPL and capture its output, interrupting it if it
    /// is still running after `limit`. A traceback is returned, not an error.
    pub async fn exec_capture(&self, code: &str, limit: Duration) -> Result<ExecOutput> {
        let mut raw = self.raw().await?;
        let res = async {
            raw.exec_start(code.as_bytes()).await?;
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let mut collect = |s: Stream, d: &[u8]| match s {
                Stream::Stdout => out.extend_from_slice(d),
                Stream::Stderr => err.extend_from_slice(d),
            };
            let timed_out = match tokio::time::timeout(limit, raw.follow(None, &mut collect)).await {
                Ok(r) => {
                    r?;
                    false
                }
                Err(_) => {
                    self.transport.interrupt().await?;
                    raw.follow(Some(super::raw::DEFAULT_TIMEOUT), &mut collect).await?;
                    true
                }
            };
            Ok(ExecOutput {
                stdout: String::from_utf8_lossy(&out).into_owned(),
                stderr: String::from_utf8_lossy(&err).into_owned(),
                timed_out,
            })
        }
        .await;
        raw.finish_with(res).await
    }

    /// Interrupt the running program. `force` abandons it if it doesn't respond.
    pub async fn stop(&self, force: bool) -> Result<()> {
        self.transport.interrupt().await?;
        if force {
            tokio::time::sleep(Duration::from_millis(150)).await;
            self.transport.interrupt().await?;
            self.cancel_run.notify_one();
        }
        Ok(())
    }

    pub async fn interrupt(&self) -> Result<()> {
        self.transport.write(b"\x03").await
    }

    /// Soft reboot from the friendly REPL (Ctrl-D).
    pub async fn soft_reset(&self) -> Result<()> {
        if self.state() == SessionState::Running {
            self.stop(true).await?;
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        // Leave raw REPL first in case a tool left the board there; Ctrl-D in
        // raw REPL would just run an empty program instead of rebooting.
        self.transport.write(b"\r\x02").await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
        self.transport.write(b"\x03\x03\x04").await
    }

    /// Hardware reset via control lines, or `machine.reset()` as a fallback.
    pub async fn hard_reset(&self) -> Result<()> {
        let seq = self.profile().serial.hard_reset.clone();
        if self.transport.hard_reset(seq.as_deref()).await? {
            return Ok(());
        }
        self.exec_detached("import machine\nmachine.reset()\n").await
    }

    /// Start code that won't return (reset, bootloader entry) without waiting
    /// for its output.
    pub async fn exec_detached(&self, code: &str) -> Result<()> {
        self.ensure_alive()?;
        let mut repl = self.repl.lock().await;
        self.route.store(ROUTE_CAPTURE, Ordering::Release);
        let res = async {
            repl.enter(false).await?;
            repl.exec_start(code.as_bytes()).await
        }
        .await;
        self.route.store(ROUTE_TERMINAL, Ordering::Release);
        res
    }

    pub async fn close(&self) -> Result<()> {
        self.cancel_run.notify_one();
        self.transport.close().await
    }

    pub fn transport(&self) -> &Arc<dyn Transport> {
        &self.transport
    }
}

/// Exclusive raw-REPL access. Leaves raw REPL on [`finish`](Self::finish), or in
/// the background if dropped early (e.g. on error).
pub struct RawSession {
    repl: Option<tokio::sync::OwnedMutexGuard<RawRepl>>,
    route: Arc<AtomicU8>,
    state: Arc<Mutex<SessionState>>,
    events: EventSink,
}

impl RawSession {
    pub async fn finish(mut self) -> Result<()> {
        let mut repl = self.repl.take().expect("repl present until finished");
        let res = repl.exit().await;
        restore(&self.route, &self.state, &self.events);
        res
    }

    /// Leave raw REPL and return `result`. An error from the operation itself
    /// takes precedence over one from leaving raw REPL.
    pub async fn finish_with<T>(self, result: Result<T>) -> Result<T> {
        let exited = self.finish().await;
        let value = result?;
        exited.map(|_| value)
    }
}

impl std::ops::Deref for RawSession {
    type Target = RawRepl;
    fn deref(&self) -> &RawRepl {
        self.repl.as_ref().expect("repl present until finished")
    }
}

impl std::ops::DerefMut for RawSession {
    fn deref_mut(&mut self) -> &mut RawRepl {
        self.repl.as_mut().expect("repl present until finished")
    }
}

impl Drop for RawSession {
    fn drop(&mut self) {
        if let Some(mut repl) = self.repl.take() {
            let (route, state, events) = (self.route.clone(), self.state.clone(), self.events.clone());
            tokio::spawn(async move {
                let _ = repl.exit().await;
                restore(&route, &state, &events);
            });
        }
    }
}

fn restore(route: &AtomicU8, state: &Arc<Mutex<SessionState>>, events: &EventSink) {
    route.store(ROUTE_TERMINAL, Ordering::Release);
    set_state(state, events, SessionState::Idle);
}

fn set_state(state: &Mutex<SessionState>, events: &EventSink, s: SessionState) {
    let mut g = state.lock().unwrap();
    if *g != s && *g != SessionState::Disconnected {
        *g = s;
        drop(g);
        events(SessionEvent::State { state: s });
    }
}
