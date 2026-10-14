//! The MicroPython raw REPL protocol, including raw-paste mode.
//!
//! Behaviour mirrors `mpremote`'s `pyboard.py` so it works with every port:
//!
//! * `Ctrl-A` enters raw REPL (`raw REPL; CTRL-B to exit\r\n>`).
//! * Raw-paste (`Ctrl-E A Ctrl-A`) streams code with window-based flow control;
//!   devices that don't support it fall back to plain raw mode.
//! * Output follows as `OK <stdout> \x04 <stderr> \x04 >`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};

use crate::error::{Error, Result};
use crate::transport::Transport;

pub const RAW_BANNER: &[u8] = b"raw REPL; CTRL-B to exit\r\n";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Raw-REPL access to a device. Owns the receiving end of the byte stream for
/// as long as the session routes device output to it.
pub struct RawRepl {
    pub transport: Arc<dyn Transport>,
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    buf: Vec<u8>,
    /// `None` until probed; then whether the device supports raw-paste.
    raw_paste: Option<bool>,
    /// Max bytes per raw-REPL file-transfer chunk.
    pub chunk: usize,
}

impl RawRepl {
    pub fn new(transport: Arc<dyn Transport>, rx: mpsc::UnboundedReceiver<Vec<u8>>) -> Self {
        Self { transport, rx, buf: Vec::new(), raw_paste: None, chunk: 1024 }
    }

    async fn write(&self, data: &[u8]) -> Result<()> {
        for part in data.chunks(self.transport.max_write().max(1)) {
            self.transport.write(part).await?;
        }
        Ok(())
    }

    /// Wait for one more chunk of input, up to `wait`.
    async fn fill(&mut self, wait: Duration) -> Result<()> {
        match timeout(wait, self.rx.recv()).await {
            Ok(Some(data)) => {
                self.buf.extend_from_slice(&data);
                Ok(())
            }
            Ok(None) => Err(Error::Closed),
            Err(_) => Err(Error::Timeout("device".into())),
        }
    }

    /// Read up to and including `pat`. `wait` is an inactivity timeout.
    pub async fn read_until(&mut self, pat: &[u8], wait: Duration) -> Result<Vec<u8>> {
        loop {
            if let Some(i) = find(&self.buf, pat) {
                return Ok(self.buf.drain(..i + pat.len()).collect());
            }
            self.fill(wait).await.map_err(|e| match e {
                Error::Timeout(_) => Error::Timeout(format!(
                    "{:?} (got {:?})",
                    String::from_utf8_lossy(pat),
                    String::from_utf8_lossy(&self.buf[self.buf.len().saturating_sub(80)..])
                )),
                e => e,
            })?;
        }
    }

    pub async fn read_exact(&mut self, n: usize, wait: Duration) -> Result<Vec<u8>> {
        while self.buf.len() < n {
            self.fill(wait).await?;
        }
        Ok(self.buf.drain(..n).collect())
    }

    /// Discard input until the device has been quiet for `quiet` (max ~1s).
    pub async fn flush_input(&mut self, quiet: Duration) {
        self.buf.clear();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        while tokio::time::Instant::now() < deadline {
            match timeout(quiet, self.rx.recv()).await {
                Ok(Some(_)) => continue,
                _ => break,
            }
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.buf.is_empty()
    }

    /// Interrupt any running program and enter raw REPL, escalating if the
    /// board doesn't answer: leave a half-entered raw REPL, then flood Ctrl-C
    /// (a program may be swallowing single interrupts or busy in C code).
    pub async fn enter(&mut self, soft_reset: bool) -> Result<()> {
        self.write(b"\r\x03").await?;
        sleep(Duration::from_millis(100)).await;
        self.write(b"\x03").await?;
        self.flush_input(Duration::from_millis(60)).await;

        // Escalation steps after each failed attempt.
        const FLOODS: [Duration; 2] = [Duration::from_millis(1500), Duration::from_millis(3000)];
        let mut attempt = 0;
        loop {
            self.write(b"\r\x01").await?;
            let res = async {
                if soft_reset {
                    self.read_until(b"raw REPL; CTRL-B to exit\r\n>", DEFAULT_TIMEOUT).await?;
                    self.write(b"\x04").await?;
                    self.read_until(b"soft reboot\r\n", DEFAULT_TIMEOUT).await?;
                }
                self.read_until(RAW_BANNER, Duration::from_secs(2)).await
            }
            .await;
            match res {
                Ok(_) => return Ok(()),
                Err(Error::Timeout(_)) if attempt < FLOODS.len() => {
                    self.write(b"\r\x02").await?;
                    self.interrupt_flood(FLOODS[attempt], Duration::from_millis(20)).await?;
                    self.flush_input(Duration::from_millis(100)).await;
                    attempt += 1;
                }
                Err(Error::Timeout(_)) => return Err(Error::Unresponsive),
                Err(e) => return Err(e),
            }
        }
    }

    /// Send Ctrl-C every `interval` for up to `duration`, stopping early once
    /// the friendly REPL prompt appears. Returns whether a prompt was seen.
    pub async fn interrupt_flood(&mut self, duration: Duration, interval: Duration) -> Result<bool> {
        let deadline = tokio::time::Instant::now() + duration;
        self.buf.clear();
        while tokio::time::Instant::now() < deadline {
            self.write(b"\x03").await?;
            match self.fill(interval).await {
                Ok(()) | Err(Error::Timeout(_)) => {}
                Err(e) => return Err(e),
            }
            if find(&self.buf, b">>> ").is_some() || find(&self.buf, RAW_BANNER).is_some() {
                // A few more, in case boot.py/main.py starts right after.
                for _ in 0..5 {
                    self.write(b"\x03").await?;
                    sleep(interval).await;
                }
                return Ok(true);
            }
            if self.buf.len() > 64 * 1024 {
                let keep = self.buf.split_off(self.buf.len() - 1024);
                self.buf = keep;
            }
        }
        Ok(false)
    }

    /// Preset raw-paste support (e.g. from version quirks) to skip probing.
    pub fn set_raw_paste(&mut self, supported: Option<bool>) {
        self.raw_paste = supported;
    }

    /// Leave raw REPL and swallow the friendly-REPL banner that follows.
    pub async fn exit(&mut self) -> Result<()> {
        self.write(b"\r\x02").await?;
        let _ = self.read_until(b">>> ", Duration::from_millis(800)).await;
        self.buf.clear();
        Ok(())
    }

    /// Send code for execution without reading its output.
    pub async fn exec_start(&mut self, code: &[u8]) -> Result<()> {
        self.read_until(b">", DEFAULT_TIMEOUT).await?;

        if self.raw_paste != Some(false) {
            self.write(b"\x05A\x01").await?;
            let resp = self.read_exact(2, DEFAULT_TIMEOUT).await?;
            match resp.as_slice() {
                b"R\x01" => {
                    self.raw_paste = Some(true);
                    return self.raw_paste_write(code).await;
                }
                b"R\x00" => self.raw_paste = Some(false),
                _ => {
                    // Device didn't understand Ctrl-E; it re-printed the banner.
                    self.read_until(b"w REPL; CTRL-B to exit\r\n>", DEFAULT_TIMEOUT).await?;
                    self.raw_paste = Some(false);
                }
            }
        }

        for part in code.chunks(256) {
            self.write(part).await?;
            sleep(Duration::from_millis(10)).await;
        }
        self.write(b"\x04").await?;
        let ok = self.read_exact(2, DEFAULT_TIMEOUT).await?;
        if ok != b"OK" {
            return Err(Error::Protocol(format!("could not exec command (got {ok:?})")));
        }
        Ok(())
    }

    async fn raw_paste_write(&mut self, code: &[u8]) -> Result<()> {
        let hdr = self.read_exact(2, DEFAULT_TIMEOUT).await?;
        let window = u16::from_le_bytes([hdr[0], hdr[1]]) as usize;
        let mut remain = window;
        let mut i = 0;
        while i < code.len() {
            // Consume flow-control bytes; block while the window is exhausted.
            while remain == 0 || !self.buf.is_empty() || self.rx_ready() {
                if self.buf.is_empty() {
                    self.fill(DEFAULT_TIMEOUT).await?;
                }
                match self.buf.remove(0) {
                    0x01 => remain += window,
                    0x04 => {
                        // Device aborted (e.g. syntax error mid-stream).
                        self.write(b"\x04").await?;
                        return Ok(());
                    }
                    b => return Err(Error::Protocol(format!("unexpected raw-paste byte {b:#x}"))),
                }
            }
            let n = remain.min(code.len() - i);
            self.write(&code[i..i + n]).await?;
            remain -= n;
            i += n;
        }
        self.write(b"\x04").await?;
        self.read_until(b"\x04", DEFAULT_TIMEOUT).await?;
        Ok(())
    }

    fn rx_ready(&mut self) -> bool {
        match self.rx.try_recv() {
            Ok(data) => {
                self.buf.extend_from_slice(&data);
                true
            }
            Err(_) => false,
        }
    }

    /// Stream output of the running command until both `\x04` markers arrive.
    /// `wait` is an inactivity timeout (`None` waits forever).
    pub async fn follow(
        &mut self,
        wait: Option<Duration>,
        mut on_output: impl FnMut(Stream, &[u8]),
    ) -> Result<()> {
        let mut parser = FollowParser::default();
        loop {
            let pending = std::mem::take(&mut self.buf);
            let (done, rest) = parser.feed(&pending, &mut on_output);
            if done {
                self.buf = rest;
                return Ok(());
            }
            match wait {
                Some(w) => self.fill(w).await?,
                None => match self.rx.recv().await {
                    Some(d) => self.buf.extend_from_slice(&d),
                    None => return Err(Error::Closed),
                },
            }
        }
    }

    /// Run code and return its stdout. A non-empty stderr becomes an error.
    pub async fn exec(&mut self, code: &str) -> Result<Vec<u8>> {
        self.exec_with(code, |_| {}).await
    }

    /// Like [`exec`](Self::exec) but reports stdout chunks as they arrive.
    pub async fn exec_with(&mut self, code: &str, mut on_stdout: impl FnMut(&[u8])) -> Result<Vec<u8>> {
        self.exec_start(code.as_bytes()).await?;
        let mut out = Vec::new();
        let mut err = Vec::new();
        self.follow(Some(DEFAULT_TIMEOUT), |s, d| match s {
            Stream::Stdout => {
                on_stdout(d);
                out.extend_from_slice(d)
            }
            Stream::Stderr => err.extend_from_slice(d),
        })
        .await?;
        if !err.is_empty() {
            return Err(Error::Device(last_error_line(&err)));
        }
        Ok(out)
    }

    pub async fn exec_str(&mut self, code: &str) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.exec(code).await?).into_owned())
    }
}

/// Incremental parser for `<stdout>\x04<stderr>\x04`.
#[derive(Default)]
pub struct FollowParser {
    in_stderr: bool,
}

impl FollowParser {
    /// Feed bytes; returns `(finished, leftover_after_finish)`.
    pub fn feed(&mut self, data: &[u8], mut emit: impl FnMut(Stream, &[u8])) -> (bool, Vec<u8>) {
        let mut rest = data;
        loop {
            let stream = if self.in_stderr { Stream::Stderr } else { Stream::Stdout };
            match rest.iter().position(|&b| b == 0x04) {
                Some(i) => {
                    if i > 0 {
                        emit(stream, &rest[..i]);
                    }
                    rest = &rest[i + 1..];
                    if self.in_stderr {
                        return (true, rest.to_vec());
                    }
                    self.in_stderr = true;
                }
                None => {
                    if !rest.is_empty() {
                        emit(stream, rest);
                    }
                    return (false, Vec::new());
                }
            }
        }
    }
}

/// The most useful line of a traceback ("OSError: [Errno 2] ENOENT").
pub fn last_error_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("device error")
        .trim()
        .to_string()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    #[test]
    fn follow_parser_splits_streams_across_chunks() {
        let mut p = FollowParser::default();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut emit = |s: Stream, d: &[u8]| match s {
            Stream::Stdout => out.extend_from_slice(d),
            Stream::Stderr => err.extend_from_slice(d),
        };
        assert!(!p.feed(b"hel", &mut emit).0);
        assert!(!p.feed(b"lo\x04Trace", &mut emit).0);
        let (done, rest) = p.feed(b"back\x04>", &mut emit);
        assert!(done);
        assert_eq!(rest, b">");
        assert_eq!(out, b"hello");
        assert_eq!(err, b"Traceback");
    }

    #[test]
    fn error_line_extraction() {
        let tb = b"Traceback (most recent call last):\r\n  File \"<stdin>\", line 1\r\nOSError: [Errno 2] ENOENT\r\n";
        assert_eq!(last_error_line(tb), "OSError: [Errno 2] ENOENT");
    }

    /// A scripted device: replies to known requests with canned responses.
    struct Scripted {
        tx: mpsc::UnboundedSender<Vec<u8>>,
        written: Mutex<Vec<u8>>,
        raw_paste: bool,
    }

    #[async_trait]
    impl Transport for Scripted {
        async fn write(&self, data: &[u8]) -> Result<()> {
            let mut w = self.written.lock().unwrap();
            w.extend_from_slice(data);
            let reply: &[u8] = match data {
                b"\r\x01" => b"\r\n>>> \r\nraw REPL; CTRL-B to exit\r\n>",
                b"\x05A\x01" if self.raw_paste => b"R\x01\x80\x00\x01",
                b"\x05A\x01" => b"R\x00",
                b"\x04" if self.raw_paste => b"\x04hi\r\n\x04\x04>",
                b"\x04" => b"OKhi\r\n\x04\x04>",
                b"\r\x02" => b"\r\nMicroPython v1.29\r\n>>> ",
                _ => b"",
            };
            if !reply.is_empty() {
                self.tx.send(reply.to_vec()).unwrap();
            }
            Ok(())
        }
        async fn close(&self) -> Result<()> {
            Ok(())
        }
    }

    async fn run_scripted(raw_paste: bool) -> (Vec<u8>, Vec<u8>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let t = Arc::new(Scripted { tx, written: Mutex::new(Vec::new()), raw_paste });
        let mut r = RawRepl::new(t.clone(), rx);
        r.enter(false).await.unwrap();
        let out = r.exec("print('hi')").await.unwrap();
        r.exit().await.unwrap();
        let written = t.written.lock().unwrap().clone();
        (out, written)
    }

    #[tokio::test]
    async fn exec_with_raw_paste() {
        let (out, written) = run_scripted(true).await;
        assert_eq!(out, b"hi\r\n");
        assert!(find(&written, b"print('hi')").is_some());
    }

    /// A board stuck in a loop that only yields after several Ctrl-Cs.
    struct Stubborn {
        tx: mpsc::UnboundedSender<Vec<u8>>,
        interrupts: Mutex<usize>,
        needed: usize,
    }

    #[async_trait]
    impl Transport for Stubborn {
        async fn write(&self, data: &[u8]) -> Result<()> {
            let n = data.iter().filter(|&&b| b == 0x03).count();
            let total = {
                let mut i = self.interrupts.lock().unwrap();
                *i += n;
                *i
            };
            if total < self.needed {
                // Still busy: produce program noise, ignore everything else.
                self.tx.send(b"tick\r\n".to_vec()).unwrap();
            } else if n > 0 {
                self.tx.send(b"KeyboardInterrupt\r\n>>> ".to_vec()).unwrap();
            } else if data == b"\r\x01" {
                self.tx.send(b"\r\n>>> \r\nraw REPL; CTRL-B to exit\r\n>".to_vec()).unwrap();
            }
            Ok(())
        }
        async fn close(&self) -> Result<()> {
            Ok(())
        }
    }

    fn stubborn(needed: usize) -> RawRepl {
        let (tx, rx) = mpsc::unbounded_channel();
        RawRepl::new(Arc::new(Stubborn { tx, interrupts: Mutex::new(0), needed }), rx)
    }

    #[tokio::test]
    async fn interrupt_flood_stops_at_prompt() {
        let mut r = stubborn(10);
        assert!(r.interrupt_flood(Duration::from_secs(2), Duration::from_millis(5)).await.unwrap());
        let mut never = stubborn(usize::MAX);
        assert!(!never.interrupt_flood(Duration::from_millis(150), Duration::from_millis(5)).await.unwrap());
    }

    #[tokio::test]
    async fn enter_escalates_until_the_board_yields() {
        // Two interrupts at the start aren't enough; the escalation's flood is.
        let mut r = stubborn(8);
        r.enter(false).await.unwrap();
        let mut dead = stubborn(usize::MAX);
        assert!(matches!(dead.enter(false).await, Err(Error::Unresponsive)));
    }

    #[tokio::test]
    async fn exec_falls_back_without_raw_paste() {
        let (out, _) = run_scripted(false).await;
        assert_eq!(out, b"hi\r\n");
    }
}
