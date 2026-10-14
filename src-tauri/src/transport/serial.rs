//! USB / UART serial transport.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serialport::{SerialPort, SerialPortType};

use super::{ConnectOptions, Connection, Endpoint, Transport, TransportInfo, TransportProvider, UsbInfo};
use crate::error::{Error, Result};
use crate::platform::PlatformRegistry;

pub const ID: &str = "serial";
pub const DEFAULT_BAUD: u32 = 115_200;

pub struct SerialProvider;

#[async_trait]
impl TransportProvider for SerialProvider {
    fn info(&self) -> TransportInfo {
        TransportInfo {
            id: ID.into(),
            label: "USB / Serial".into(),
            manual_address: Some("/dev/ttyUSB0 or COM3".into()),
            needs_password: false,
            supports_baud: true,
        }
    }

    async fn discover(&self, platforms: &PlatformRegistry) -> Result<Vec<Endpoint>> {
        let ports = tokio::task::spawn_blocking(serialport::available_ports)
            .await
            .map_err(|e| Error::other(e.to_string()))??;
        let mut out = Vec::new();
        for p in ports {
            // macOS lists every port twice; the cu.* device is the one to open.
            if p.port_name.starts_with("/dev/tty.") {
                continue;
            }
            let (usb, label, hint, role) = match &p.port_type {
                SerialPortType::UsbPort(u) => {
                    let hits = platforms.by_usb(u.vid, u.pid);
                    let hint = hits.first().map(|(prof, _)| prof.id.clone());
                    let role = hits.first().map(|(_, m)| m.role);
                    let label = u
                        .product
                        .clone()
                        .or_else(|| hits.first().and_then(|(_, m)| m.note.clone()))
                        .unwrap_or_else(|| format!("USB {:04x}:{:04x}", u.vid, u.pid));
                    let usb = UsbInfo {
                        vid: u.vid,
                        pid: u.pid,
                        serial_number: u.serial_number.clone(),
                        manufacturer: u.manufacturer.clone(),
                        product: u.product.clone(),
                    };
                    (Some(usb), label, hint, role)
                }
                SerialPortType::BluetoothPort => continue,
                _ => (None, "Serial port".to_string(), None, None),
            };
            // Skip non-USB system ports (debug consoles, Bluetooth-Incoming-Port...).
            if usb.is_none() && cfg!(target_os = "macos") {
                continue;
            }
            out.push(Endpoint {
                transport: ID.into(),
                detail: Some(p.port_name.clone()),
                address: p.port_name,
                label,
                usb,
                platform_hint: hint,
                role,
            });
        }
        // Likely MicroPython devices first.
        out.sort_by_key(|e| e.platform_hint.is_none());
        Ok(out)
    }

    async fn connect(&self, endpoint: &Endpoint, opts: &ConnectOptions) -> Result<Connection> {
        let path = endpoint.address.clone();
        let baud = opts.baud.unwrap_or(DEFAULT_BAUD);
        let port = tokio::task::spawn_blocking(move || -> Result<Box<dyn SerialPort>> {
            let mut port = serialport::new(&path, baud)
                .timeout(Duration::from_millis(20))
                .open()?;
            // Assert both lines: USB-CDC boards only send data with DTR set, and
            // setting both keeps ESP auto-reset circuits idle.
            let _ = port.write_data_terminal_ready(true);
            let _ = port.write_request_to_send(true);
            Ok(port)
        })
        .await
        .map_err(|e| Error::other(e.to_string()))??;
        SerialTransport::start(port)
    }
}

pub struct SerialTransport {
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    stop: Arc<AtomicBool>,
}

impl SerialTransport {
    pub fn start(port: Box<dyn SerialPort>) -> Result<Connection> {
        let stop = Arc::new(AtomicBool::new(false));
        let incoming = super::spawn_reader("serial-reader", port.try_clone()?, false, stop.clone())?;
        let t = SerialTransport { port: Arc::new(Mutex::new(port)), stop };
        Ok(Connection { transport: Arc::new(t), incoming })
    }

    async fn with_port<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Box<dyn SerialPort>) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        super::blocking(&self.port, f).await
    }
}

#[async_trait]
impl Transport for SerialTransport {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        self.with_port(move |p| {
            p.write_all(&data)?;
            p.flush()?;
            Ok(())
        })
        .await
    }

    async fn close(&self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn hard_reset(&self, sequence: Option<&str>) -> Result<bool> {
        match sequence {
            // Classic ESP auto-reset circuit: RTS drives EN, DTR drives IO0.
            Some("esp_classic") => {
                self.with_port(|p| {
                    p.write_data_terminal_ready(false)?;
                    p.write_request_to_send(true)?;
                    std::thread::sleep(Duration::from_millis(120));
                    p.write_request_to_send(false)?;
                    std::thread::sleep(Duration::from_millis(50));
                    p.write_data_terminal_ready(true)?;
                    p.write_request_to_send(true)?;
                    Ok(())
                })
                .await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

impl Drop for SerialTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
