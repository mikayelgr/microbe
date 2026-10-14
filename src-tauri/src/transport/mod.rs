//! Byte transports to a MicroPython REPL.
//!
//! A [`TransportProvider`] discovers endpoints and opens a [`Connection`]. Each
//! connection pumps incoming bytes into a channel and exposes an async writer,
//! so everything above this layer (raw REPL, files, run) is transport-agnostic.
//! Transports may offer optional fast paths, e.g. [`FileTransferExt`].

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use ts_rs::TS;

use crate::error::Result;
use crate::platform::{PlatformRegistry, UsbRole};

pub mod ble;
#[cfg(unix)]
pub mod local;
pub mod serial;
pub mod tcp;
pub mod webrepl;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct UsbInfo {
    pub vid: u16,
    pub pid: u16,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Endpoint {
    /// Id of the provider that can open this endpoint.
    pub transport: String,
    /// Port path, URL, BLE peripheral id, executable path...
    pub address: String,
    pub label: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub usb: Option<UsbInfo>,
    /// Best-guess platform profile id.
    #[serde(default)]
    pub platform_hint: Option<String>,
    #[serde(default)]
    pub role: Option<UsbRole>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConnectOptions {
    #[serde(default)]
    pub baud: Option<u32>,
    #[serde(default)]
    pub password: Option<String>,
}

/// Static description of a provider, used to build the connect dialog.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TransportInfo {
    pub id: String,
    pub label: String,
    /// Placeholder for a manually typed address; `None` if discovery only.
    pub manual_address: Option<String>,
    pub needs_password: bool,
    pub supports_baud: bool,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn write(&self, data: &[u8]) -> Result<()>;
    async fn close(&self) -> Result<()>;
    /// Interrupt a running program (KeyboardInterrupt). Boards watch for Ctrl-C
    /// on the input stream; host processes need a signal instead.
    async fn interrupt(&self) -> Result<()> {
        self.write(b"\x03").await
    }
    /// Reset the board through control lines. Returns `false` if unsupported.
    async fn hard_reset(&self, _sequence: Option<&str>) -> Result<bool> {
        Ok(false)
    }
    /// Largest single write the link handles well (BLE MTU, WebSocket frame...).
    fn max_write(&self) -> usize {
        4096
    }
    fn file_transfer(&self) -> Option<&dyn FileTransferExt> {
        None
    }
}

pub type Progress<'a> = &'a (dyn Fn(u64, u64) + Send + Sync);

/// Optional out-of-band file transfer offered by some transports (WebREPL).
#[async_trait]
pub trait FileTransferExt: Send + Sync {
    async fn put(&self, path: &str, data: &[u8], progress: Progress<'_>) -> Result<()>;
    async fn get(&self, path: &str, progress: Progress<'_>) -> Result<Vec<u8>>;
}

/// Pumps a blocking reader (serial port, PTY) into the connection channel on a
/// thread. Read timeouts are retried; `Ok(0)` ends the stream only when
/// `eof_on_zero` (PTYs signal EOF that way; serial ports don't).
pub fn spawn_reader<R: std::io::Read + Send + 'static>(
    name: &str,
    mut reader: R,
    eof_on_zero: bool,
    stop: Arc<std::sync::atomic::AtomicBool>,
) -> Result<mpsc::UnboundedReceiver<Vec<u8>>> {
    use std::io::ErrorKind;
    use std::sync::atomic::Ordering;
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::Builder::new().name(name.into()).spawn(move || {
        let mut buf = [0u8; 4096];
        while !stop.load(Ordering::Relaxed) {
            match reader.read(&mut buf) {
                Ok(0) if eof_on_zero => break,
                Ok(0) => {}
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::Interrupted | ErrorKind::WouldBlock) => {}
                Err(e) => {
                    log::info!("reader stopped: {e}");
                    break;
                }
            }
        }
    })?;
    Ok(rx)
}

/// Run a blocking operation on a shared writer without stalling the runtime.
pub async fn blocking<W: Send + 'static, T: Send + 'static>(
    writer: &Arc<std::sync::Mutex<W>>,
    f: impl FnOnce(&mut W) -> Result<T> + Send + 'static,
) -> Result<T> {
    let w = writer.clone();
    tokio::task::spawn_blocking(move || {
        let mut guard = w.lock().map_err(|_| crate::error::Error::other("transport lock poisoned"))?;
        f(&mut guard)
    })
    .await
    .map_err(|e| crate::error::Error::other(e.to_string()))?
}

pub struct Connection {
    pub transport: Arc<dyn Transport>,
    /// Bytes from the device. Closes when the link drops.
    pub incoming: mpsc::UnboundedReceiver<Vec<u8>>,
}

#[async_trait]
pub trait TransportProvider: Send + Sync {
    fn info(&self) -> TransportInfo;
    async fn discover(&self, platforms: &PlatformRegistry) -> Result<Vec<Endpoint>>;
    async fn connect(&self, endpoint: &Endpoint, opts: &ConnectOptions) -> Result<Connection>;
}

#[derive(Default)]
pub struct TransportRegistry {
    providers: Vec<Arc<dyn TransportProvider>>,
}

impl TransportRegistry {
    /// Built-in transports. `data_dir` hosts the local simulator's filesystem.
    pub fn with_defaults(data_dir: &std::path::Path) -> Self {
        let mut r = Self::default();
        r.register(Arc::new(serial::SerialProvider));
        r.register(Arc::new(webrepl::WebReplProvider));
        r.register(Arc::new(ble::BleProvider::default()));
        r.register(Arc::new(tcp::TcpProvider(tcp::Kind::Raw)));
        r.register(Arc::new(tcp::TcpProvider(tcp::Kind::Telnet)));
        r.register(Arc::new(tcp::TcpProvider(tcp::Kind::Rfc2217)));
        #[cfg(unix)]
        r.register(Arc::new(local::LocalProvider { root: data_dir.join("simulator") }));
        r
    }

    pub fn register(&mut self, p: Arc<dyn TransportProvider>) {
        self.providers.push(p);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn TransportProvider>> {
        self.providers.iter().find(|p| p.info().id == id).cloned()
    }

    pub fn all(&self) -> &[Arc<dyn TransportProvider>] {
        &self.providers
    }
}
