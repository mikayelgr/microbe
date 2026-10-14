//! Network transports over TCP:
//! * `tcp`: a raw socket (ser2net raw mode, ESP-Link, a socket REPL).
//! * `telnet`: Telnet with option negotiation and the WiPy-style login
//!   prompt ("Login as:" / "Password:").
//! * `rfc2217`: Telnet plus the COM-PORT-OPTION, so the serial port at the
//!   other end gets its baud rate and DTR/RTS lines set, and hard reset works.
//!
//! All three are manual-address only (`host[:port]`, default port 23).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;

use super::{ConnectOptions, Connection, Endpoint, Transport, TransportInfo, TransportProvider};
use crate::error::{Error, Result};
use crate::platform::PlatformRegistry;

pub const TCP: &str = "tcp";
pub const TELNET: &str = "telnet";
pub const RFC2217: &str = "rfc2217";

const DEFAULT_PORT: u16 = 23;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(3);

// Telnet bytes.
const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_BINARY: u8 = 0;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_COM_PORT: u8 = 44;

// RFC 2217 client commands.
const CPO_SET_BAUDRATE: u8 = 1;
const CPO_SET_DATASIZE: u8 = 2;
const CPO_SET_PARITY: u8 = 3;
const CPO_SET_STOPSIZE: u8 = 4;
const CPO_SET_CONTROL: u8 = 5;
pub const CONTROL_DTR_ON: u8 = 8;
pub const CONTROL_DTR_OFF: u8 = 9;
pub const CONTROL_RTS_ON: u8 = 11;
pub const CONTROL_RTS_OFF: u8 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Raw,
    Telnet,
    Rfc2217,
}

pub struct TcpProvider(pub Kind);

#[async_trait]
impl TransportProvider for TcpProvider {
    fn info(&self) -> TransportInfo {
        let (id, label, manual, password, baud) = match self.0 {
            Kind::Raw => (TCP, "TCP socket", "192.168.4.1:2000", false, false),
            Kind::Telnet => (TELNET, "Telnet", "192.168.4.1 or host:23", true, false),
            Kind::Rfc2217 => (RFC2217, "Network serial (RFC 2217)", "ser2net-host:2217", false, true),
        };
        TransportInfo { id: id.into(), label: label.into(), manual_address: Some(manual.into()), needs_password: password, supports_baud: baud }
    }

    async fn discover(&self, _: &PlatformRegistry) -> Result<Vec<Endpoint>> {
        Ok(Vec::new())
    }

    async fn connect(&self, endpoint: &Endpoint, opts: &ConnectOptions) -> Result<Connection> {
        let addr = with_default_port(&endpoint.address);
        let stream = timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::Timeout(format!("connecting to {addr}")))??;
        stream.set_nodelay(true)?;
        let (mut reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));
        let telnet = self.0 != Kind::Raw;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let pump = {
            let writer = writer.clone();
            let mut parser = TelnetParser::new(self.0 == Kind::Rfc2217);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    let n = match reader.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let data = if telnet {
                        let (data, replies) = parser.feed(&buf[..n]);
                        if !replies.is_empty() && writer.lock().await.write_all(&replies).await.is_err() {
                            break;
                        }
                        data
                    } else {
                        buf[..n].to_vec()
                    };
                    if !data.is_empty() && tx.send(data).is_err() {
                        break;
                    }
                }
            })
        };

        let t = TcpTransport { kind: self.0, writer, pump };
        if telnet {
            t.send_raw(&TelnetParser::greeting(self.0 == Kind::Rfc2217)).await?;
        }
        if self.0 == Kind::Rfc2217 {
            t.configure_port(opts.baud.unwrap_or(super::serial::DEFAULT_BAUD)).await?;
        }

        // Telnet servers may ask for a login first; anything else is REPL output.
        let mut early = Vec::new();
        if self.0 == Kind::Telnet {
            early = login(&t, &mut rx, opts.password.as_deref()).await?;
        }
        let incoming = if early.is_empty() {
            rx
        } else {
            let (ftx, frx) = mpsc::unbounded_channel();
            let _ = ftx.send(early);
            tokio::spawn(async move {
                while let Some(d) = rx.recv().await {
                    if ftx.send(d).is_err() {
                        break;
                    }
                }
            });
            frx
        };
        Ok(Connection { transport: Arc::new(t), incoming })
    }
}

/// `host` → `host:23`; keeps an explicit port (and bracketed IPv6).
pub fn with_default_port(addr: &str) -> String {
    let a = addr.trim().trim_start_matches("telnet://").trim_start_matches("rfc2217://").trim_start_matches("tcp://");
    let has_port = match a.rsplit_once(':') {
        Some((host, port)) => port.parse::<u16>().is_ok() && (!host.contains(':') || host.ends_with(']')),
        None => false,
    };
    if has_port {
        a.to_string()
    } else {
        format!("{a}:{DEFAULT_PORT}")
    }
}

/// Answer a "Login as:" / "Password:" prompt, if one comes. Returns output
/// received meanwhile that belongs to the REPL.
async fn login(t: &TcpTransport, rx: &mut mpsc::UnboundedReceiver<Vec<u8>>, password: Option<&str>) -> Result<Vec<u8>> {
    let (user, pass) = match password.and_then(|p| p.split_once(':')) {
        Some((u, p)) => (u.to_string(), p.to_string()),
        None => ("micro".to_string(), password.unwrap_or("python").to_string()),
    };
    let mut seen = Vec::new();
    let mut stage = 0;
    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Some(chunk)) = timeout(left, rx.recv()).await else { return Ok(seen) };
        seen.extend_from_slice(&chunk);
        let text = String::from_utf8_lossy(&seen).to_ascii_lowercase();
        if stage == 0 && text.contains("login as:") {
            t.write(format!("{user}\r\n").as_bytes()).await?;
            seen.clear();
            stage = 1;
        } else if stage == 1 && text.contains("password:") {
            t.write(format!("{pass}\r\n").as_bytes()).await?;
            seen.clear();
            stage = 2;
        } else if stage == 2 && text.contains("invalid") {
            return Err(Error::other("telnet login failed: wrong user or password (password field: user:password)"));
        } else if stage == 2 && text.contains("login succeeded") {
            let at = text.find("login succeeded").unwrap_or(0);
            let rest = seen[at..].iter().position(|&b| b == b'\n').map_or(Vec::new(), |nl| seen[at + nl + 1..].to_vec());
            return Ok(rest);
        } else if stage == 0 && text.contains(">>> ") {
            return Ok(seen); // no login required
        }
    }
}

pub struct TcpTransport {
    kind: Kind,
    writer: Arc<Mutex<OwnedWriteHalf>>,
    pump: tokio::task::JoinHandle<()>,
}

impl TcpTransport {
    async fn send_raw(&self, data: &[u8]) -> Result<()> {
        let mut w = self.writer.lock().await;
        w.write_all(data).await?;
        w.flush().await?;
        Ok(())
    }

    async fn configure_port(&self, baud: u32) -> Result<()> {
        let mut out = subnegotiation(CPO_SET_BAUDRATE, &baud.to_be_bytes());
        out.extend(subnegotiation(CPO_SET_DATASIZE, &[8]));
        out.extend(subnegotiation(CPO_SET_PARITY, &[1])); // none
        out.extend(subnegotiation(CPO_SET_STOPSIZE, &[1]));
        // As for local serial ports: both lines asserted.
        out.extend(subnegotiation(CPO_SET_CONTROL, &[CONTROL_DTR_ON]));
        out.extend(subnegotiation(CPO_SET_CONTROL, &[CONTROL_RTS_ON]));
        self.send_raw(&out).await
    }

    async fn control(&self, value: u8) -> Result<()> {
        self.send_raw(&subnegotiation(CPO_SET_CONTROL, &[value])).await
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn write(&self, data: &[u8]) -> Result<()> {
        if self.kind == Kind::Raw {
            self.send_raw(data).await
        } else {
            self.send_raw(&escape(data)).await
        }
    }

    async fn close(&self) -> Result<()> {
        self.pump.abort();
        let _ = self.writer.lock().await.shutdown().await;
        Ok(())
    }

    async fn hard_reset(&self, sequence: Option<&str>) -> Result<bool> {
        if self.kind != Kind::Rfc2217 {
            return Ok(false);
        }
        match sequence {
            // Same sequence as the local serial transport: RTS drives EN, DTR drives IO0.
            Some("esp_classic") => {
                self.control(CONTROL_DTR_OFF).await?;
                self.control(CONTROL_RTS_ON).await?;
                tokio::time::sleep(Duration::from_millis(120)).await;
                self.control(CONTROL_RTS_OFF).await?;
                tokio::time::sleep(Duration::from_millis(50)).await;
                self.control(CONTROL_DTR_ON).await?;
                self.control(CONTROL_RTS_ON).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

impl Drop for TcpTransport {
    fn drop(&mut self) {
        self.pump.abort();
    }
}

/// Double every IAC byte in outgoing data.
pub fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for &b in data {
        out.push(b);
        if b == IAC {
            out.push(IAC);
        }
    }
    out
}

/// `IAC SB COM-PORT-OPTION <cmd> <value…> IAC SE`, with IAC in the value doubled.
pub fn subnegotiation(cmd: u8, value: &[u8]) -> Vec<u8> {
    let mut out = vec![IAC, SB, OPT_COM_PORT, cmd];
    out.extend(escape(value));
    out.extend([IAC, SE]);
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Data,
    Iac,
    Option(u8),
    Sub,
    SubIac,
    /// Just saw CR; a following NUL is padding.
    Cr,
}

/// Client side of the Telnet protocol: strips commands from the data stream
/// and produces replies to the server's option requests.
pub struct TelnetParser {
    state: State,
    com_port: bool,
    /// The server agreed to send binary (WILL BINARY): CR NUL isn't padding.
    binary: bool,
    /// Options already answered, so negotiation can't loop.
    answered: std::collections::HashSet<(u8, u8)>,
    /// Completed subnegotiations (for RFC 2217 acknowledgements).
    pub subnegotiations: Vec<Vec<u8>>,
    sub: Vec<u8>,
}

impl TelnetParser {
    pub fn new(com_port: bool) -> Self {
        Self { state: State::Data, com_port, binary: false, answered: Default::default(), subnegotiations: Vec::new(), sub: Vec::new() }
    }

    /// What the client offers when the connection opens: an 8-bit clean,
    /// character-at-a-time session (and the COM port option for RFC 2217).
    pub fn greeting(com_port: bool) -> Vec<u8> {
        let mut out = vec![IAC, WILL, OPT_BINARY, IAC, DO, OPT_BINARY, IAC, DO, OPT_SGA, IAC, WILL, OPT_SGA];
        if com_port {
            out.extend([IAC, WILL, OPT_COM_PORT]);
        }
        out
    }

    fn reply(&mut self, cmd: u8, opt: u8, out: &mut Vec<u8>) {
        if opt == OPT_BINARY {
            self.binary = cmd == WILL;
        }
        let ours = matches!(opt, OPT_BINARY | OPT_SGA) || (self.com_port && opt == OPT_COM_PORT);
        let theirs = matches!(opt, OPT_BINARY | OPT_SGA | OPT_ECHO);
        let answer = match cmd {
            DO => Some(if ours { WILL } else { WONT }),
            WILL => Some(if theirs { DO } else { DONT }),
            // Acknowledge refusals once.
            DONT => Some(WONT),
            WONT => Some(DONT),
            _ => None,
        };
        if let Some(a) = answer {
            // Don't answer what we've already offered or agreed to.
            let offered = Self::greeting(self.com_port).chunks(3).any(|c| c == [IAC, a, opt]);
            if self.answered.insert((cmd, opt)) && !(offered && matches!(cmd, DO | WILL)) {
                out.extend([IAC, a, opt]);
            }
        }
    }

    /// Feed received bytes; returns (data for the REPL, bytes to send back).
    pub fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut data = Vec::with_capacity(bytes.len());
        let mut replies = Vec::new();
        for &b in bytes {
            self.state = match self.state {
                State::Data | State::Cr => match b {
                    IAC => State::Iac,
                    0 if self.state == State::Cr && !self.binary => State::Data,
                    b'\r' => {
                        data.push(b);
                        State::Cr
                    }
                    _ => {
                        data.push(b);
                        State::Data
                    }
                },
                State::Iac => match b {
                    IAC => {
                        data.push(IAC);
                        State::Data
                    }
                    DO | DONT | WILL | WONT => State::Option(b),
                    SB => {
                        self.sub.clear();
                        State::Sub
                    }
                    _ => State::Data, // NOP, GA, AYT…
                },
                State::Option(cmd) => {
                    self.reply(cmd, b, &mut replies);
                    State::Data
                }
                State::Sub => match b {
                    IAC => State::SubIac,
                    _ => {
                        self.sub.push(b);
                        State::Sub
                    }
                },
                State::SubIac => match b {
                    SE => {
                        self.subnegotiations.push(std::mem::take(&mut self.sub));
                        State::Data
                    }
                    _ => {
                        self.sub.push(b);
                        State::Sub
                    }
                },
            };
        }
        (data, replies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(with_default_port("192.168.4.1"), "192.168.4.1:23");
        assert_eq!(with_default_port("host:2217"), "host:2217");
        assert_eq!(with_default_port("telnet://board.local"), "board.local:23");
        assert_eq!(with_default_port("[::1]:4000"), "[::1]:4000");
    }

    #[test]
    fn strips_commands_and_answers_options() {
        let mut p = TelnetParser::new(false);
        // Server: WILL ECHO, DO TERMINAL-TYPE(24), data with an escaped 0xFF and CR NUL.
        let (data, replies) = p.feed(&[IAC, WILL, OPT_ECHO, IAC, DO, 24, b'>', IAC, IAC, b'\r', 0, b'\n']);
        assert_eq!(data, vec![b'>', 0xFF, b'\r', b'\n']);
        assert_eq!(replies, vec![IAC, DO, OPT_ECHO, IAC, WONT, 24]);
        // Once the server sends binary, CR NUL is data.
        p.feed(&[IAC, WILL, OPT_BINARY]);
        assert_eq!(p.feed(b"\r\0").0, b"\r\0");
        // Repeated requests aren't answered again.
        assert!(p.feed(&[IAC, WILL, OPT_ECHO]).1.is_empty());
        // What the greeting already offered isn't re-offered.
        assert!(p.feed(&[IAC, DO, OPT_BINARY]).1.is_empty());
    }

    #[test]
    fn split_sequences_and_subnegotiation() {
        let mut p = TelnetParser::new(true);
        let mut data = p.feed(&[b'a', IAC]).0;
        data.extend(p.feed(&[SB, OPT_COM_PORT, 101, 0, 1, IAC, IAC, IAC]).0);
        data.extend(p.feed(&[SE, b'b']).0);
        assert_eq!(data, b"ab");
        assert_eq!(p.subnegotiations, vec![vec![OPT_COM_PORT, 101, 0, 1, IAC]]);
    }

    #[test]
    fn encodes_com_port_commands() {
        assert_eq!(escape(&[1, IAC, 2]), vec![1, IAC, IAC, 2]);
        assert_eq!(subnegotiation(CPO_SET_BAUDRATE, &115200u32.to_be_bytes()), vec![IAC, SB, 44, 1, 0, 1, 0xC2, 0, IAC, SE]);
        assert_eq!(subnegotiation(CPO_SET_CONTROL, &[CONTROL_DTR_OFF]), vec![IAC, SB, 44, 5, 9, IAC, SE]);
    }
}
