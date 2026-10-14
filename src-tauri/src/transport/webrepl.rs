//! WebREPL over WebSocket (ESP32 / ESP8266 / Pico W ... with `webrepl` running).
//!
//! REPL traffic uses text frames. Files use WebREPL's binary protocol
//! (`WA` requests, `WB` responses), exposed as a [`FileTransferExt`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::{ConnectOptions, Connection, Endpoint, FileTransferExt, Progress, Transport, TransportInfo, TransportProvider};
use crate::error::{Error, Result};
use crate::platform::PlatformRegistry;

pub const ID: &str = "webrepl";
const PUT_FILE: u8 = 1;
const GET_FILE: u8 = 2;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct WebReplProvider;

pub fn normalize_url(addr: &str) -> String {
    let a = addr.trim();
    let with_scheme = if a.starts_with("ws://") || a.starts_with("wss://") { a.to_string() } else { format!("ws://{a}") };
    let (scheme, rest) = with_scheme.split_once("://").unwrap();
    let (hostport, path) = match rest.split_once('/') {
        Some((h, p)) => (h.to_string(), format!("/{p}")),
        None => (rest.to_string(), "/".to_string()),
    };
    let hostport = if hostport.contains(':') { hostport } else { format!("{hostport}:8266") };
    format!("{scheme}://{hostport}{path}")
}

#[async_trait]
impl TransportProvider for WebReplProvider {
    fn info(&self) -> TransportInfo {
        TransportInfo {
            id: ID.into(),
            label: "WebREPL (Wi-Fi)".into(),
            manual_address: Some("192.168.4.1 or ws://host:8266".into()),
            needs_password: true,
            supports_baud: false,
        }
    }

    async fn discover(&self, _: &PlatformRegistry) -> Result<Vec<Endpoint>> {
        // WebREPL has no discovery protocol; addresses are entered manually.
        Ok(vec![])
    }

    async fn connect(&self, endpoint: &Endpoint, opts: &ConnectOptions) -> Result<Connection> {
        let url = normalize_url(&endpoint.address);
        let (ws, _) = timeout(IO_TIMEOUT, tokio_tungstenite::connect_async(&url))
            .await
            .map_err(|_| Error::Timeout(format!("connecting to {url}")))?
            .map_err(|e| Error::other(format!("WebREPL: {e}")))?;
        let (mut sink, mut stream) = ws.split();
        login(&mut sink, &mut stream, opts.password.as_deref().unwrap_or("")).await?;

        let (tx, rx) = mpsc::unbounded_channel();
        let (btx, brx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                match msg {
                    Message::Text(t) => {
                        if tx.send(t.as_bytes().to_vec()).is_err() {
                            break;
                        }
                    }
                    Message::Binary(b) => {
                        let _ = btx.send(b.to_vec());
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        let t = WebReplTransport {
            sink: Arc::new(Mutex::new(sink)),
            bin: Mutex::new(BinReader { rx: brx, buf: Vec::new() }),
        };
        Ok(Connection { transport: Arc::new(t), incoming: rx })
    }
}

async fn login(sink: &mut SplitSink<Ws, Message>, stream: &mut SplitStream<Ws>, password: &str) -> Result<()> {
    let mut seen = String::new();
    let mut sent = false;
    loop {
        let msg = timeout(IO_TIMEOUT, stream.next())
            .await
            .map_err(|_| Error::Timeout("WebREPL login".into()))?
            .ok_or(Error::Closed)?
            .map_err(|e| Error::other(format!("WebREPL: {e}")))?;
        if let Message::Text(t) = msg {
            seen.push_str(t.as_str());
            if !sent && seen.contains("Password:") {
                sink.send(Message::text(format!("{password}\r")))
                    .await
                    .map_err(|e| Error::other(e.to_string()))?;
                sent = true;
            }
            if seen.contains("WebREPL connected") {
                return Ok(());
            }
            if seen.contains("Access denied") {
                return Err(Error::other("WebREPL: wrong password"));
            }
        }
    }
}

struct BinReader {
    rx: mpsc::UnboundedReceiver<Vec<u8>>,
    buf: Vec<u8>,
}

impl BinReader {
    async fn read(&mut self, n: usize) -> Result<Vec<u8>> {
        while self.buf.len() < n {
            let d = timeout(IO_TIMEOUT, self.rx.recv())
                .await
                .map_err(|_| Error::Timeout("WebREPL file transfer".into()))?
                .ok_or(Error::Closed)?;
            self.buf.extend_from_slice(&d);
        }
        Ok(self.buf.drain(..n).collect())
    }

    async fn response(&mut self) -> Result<()> {
        let r = self.read(4).await?;
        if &r[..2] != b"WB" {
            return Err(Error::Protocol("bad WebREPL response".into()));
        }
        match u16::from_le_bytes([r[2], r[3]]) {
            0 => Ok(()),
            code => Err(Error::Device(format!("WebREPL file error {code}"))),
        }
    }
}

pub struct WebReplTransport {
    sink: Arc<Mutex<SplitSink<Ws, Message>>>,
    bin: Mutex<BinReader>,
}

/// `struct.pack("<2sBBQLH64s", b"WA", op, 0, 0, size, len(name), name)`
pub fn request(op: u8, size: u32, name: &str) -> Result<Vec<u8>> {
    let name = name.as_bytes();
    if name.len() > 64 {
        return Err(Error::Unsupported("WebREPL paths are limited to 64 bytes".into()));
    }
    let mut rec = Vec::with_capacity(82);
    rec.extend_from_slice(b"WA");
    rec.push(op);
    rec.push(0);
    rec.extend_from_slice(&0u64.to_le_bytes());
    rec.extend_from_slice(&size.to_le_bytes());
    rec.extend_from_slice(&(name.len() as u16).to_le_bytes());
    rec.extend_from_slice(name);
    rec.resize(82, 0);
    Ok(rec)
}

impl WebReplTransport {
    async fn send_bin(&self, data: Vec<u8>) -> Result<()> {
        self.sink
            .lock()
            .await
            .send(Message::binary(data))
            .await
            .map_err(|e| Error::other(format!("WebREPL: {e}")))
    }
}

#[async_trait]
impl Transport for WebReplTransport {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let text = String::from_utf8_lossy(data).into_owned();
        self.sink
            .lock()
            .await
            .send(Message::text(text))
            .await
            .map_err(|e| Error::other(format!("WebREPL: {e}")))
    }

    async fn close(&self) -> Result<()> {
        let _ = self.sink.lock().await.close().await;
        Ok(())
    }

    fn max_write(&self) -> usize {
        256
    }

    fn file_transfer(&self) -> Option<&dyn FileTransferExt> {
        Some(self)
    }
}

#[async_trait]
impl FileTransferExt for WebReplTransport {
    async fn put(&self, path: &str, data: &[u8], progress: Progress<'_>) -> Result<()> {
        let mut bin = self.bin.lock().await;
        let rec = request(PUT_FILE, data.len() as u32, path)?;
        self.send_bin(rec[..10].to_vec()).await?;
        self.send_bin(rec[10..].to_vec()).await?;
        bin.response().await?;
        let mut done = 0u64;
        for chunk in data.chunks(1024) {
            self.send_bin(chunk.to_vec()).await?;
            done += chunk.len() as u64;
            progress(done, data.len() as u64);
        }
        bin.response().await
    }

    async fn get(&self, path: &str, progress: Progress<'_>) -> Result<Vec<u8>> {
        let mut bin = self.bin.lock().await;
        self.send_bin(request(GET_FILE, 0, path)?).await?;
        bin.response().await?;
        let mut out = Vec::new();
        loop {
            self.send_bin(vec![0]).await?;
            let sz = bin.read(2).await?;
            let sz = u16::from_le_bytes([sz[0], sz[1]]) as usize;
            if sz == 0 {
                break;
            }
            out.extend_from_slice(&bin.read(sz).await?);
            progress(out.len() as u64, 0);
        }
        bin.response().await?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalize_url("192.168.4.1"), "ws://192.168.4.1:8266/");
        assert_eq!(normalize_url("ws://esp.local:8000"), "ws://esp.local:8000/");
        assert_eq!(normalize_url("ws://h:1/x"), "ws://h:1/x");
    }

    #[test]
    fn request_layout() {
        let r = request(PUT_FILE, 300, "main.py").unwrap();
        assert_eq!(r.len(), 82);
        assert_eq!(&r[..4], b"WA\x01\x00");
        assert_eq!(&r[12..16], &300u32.to_le_bytes());
        assert_eq!(&r[16..18], &7u16.to_le_bytes());
        assert_eq!(&r[18..25], b"main.py");
    }
}
