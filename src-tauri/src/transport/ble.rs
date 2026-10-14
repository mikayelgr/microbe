//! Bluetooth LE REPL over the Nordic UART Service.
//!
//! MicroPython exposes this with `examples/bluetooth/ble_uart_repl.py` (the
//! IDE can install it over USB). RX = writes to the device, TX = notifications.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use btleplug::api::{Central, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures_util::StreamExt;
use tokio::sync::{mpsc, OnceCell};
use uuid::Uuid;

use super::{ConnectOptions, Connection, Endpoint, Transport, TransportInfo, TransportProvider};
use crate::error::{Error, Result};
use crate::platform::PlatformRegistry;

pub const ID: &str = "ble";
pub const NUS_SERVICE: Uuid = Uuid::from_u128(0x6E400001_B5A3_F393_E0A9_E50E24DCCA9E);
pub const NUS_RX: Uuid = Uuid::from_u128(0x6E400002_B5A3_F393_E0A9_E50E24DCCA9E);
pub const NUS_TX: Uuid = Uuid::from_u128(0x6E400003_B5A3_F393_E0A9_E50E24DCCA9E);
const SCAN_TIME: Duration = Duration::from_secs(4);
/// Conservative payload size that fits the default ATT MTU (23 - 3).
const CHUNK: usize = 20;

fn ble_err(e: btleplug::Error) -> Error {
    Error::other(format!("Bluetooth: {e}"))
}

#[derive(Default)]
pub struct BleProvider {
    adapter: OnceCell<Adapter>,
}

impl BleProvider {
    async fn adapter(&self) -> Result<&Adapter> {
        self.adapter
            .get_or_try_init(|| async {
                let manager = Manager::new().await.map_err(ble_err)?;
                manager
                    .adapters()
                    .await
                    .map_err(ble_err)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::NotFound("no Bluetooth adapter".into()))
            })
            .await
    }

    async fn find(&self, id: &str) -> Result<Peripheral> {
        let adapter = self.adapter().await?;
        for attempt in 0..2 {
            for p in adapter.peripherals().await.map_err(ble_err)? {
                if p.id().to_string() == id {
                    return Ok(p);
                }
            }
            if attempt == 0 {
                adapter.start_scan(ScanFilter { services: vec![NUS_SERVICE] }).await.map_err(ble_err)?;
                tokio::time::sleep(SCAN_TIME).await;
                let _ = adapter.stop_scan().await;
            }
        }
        Err(Error::NotFound(format!("BLE device {id}")))
    }
}

#[async_trait]
impl TransportProvider for BleProvider {
    fn info(&self) -> TransportInfo {
        TransportInfo {
            id: ID.into(),
            label: "Bluetooth LE".into(),
            manual_address: None,
            needs_password: false,
            supports_baud: false,
        }
    }

    async fn discover(&self, _: &PlatformRegistry) -> Result<Vec<Endpoint>> {
        let adapter = self.adapter().await?;
        adapter.start_scan(ScanFilter { services: vec![NUS_SERVICE] }).await.map_err(ble_err)?;
        tokio::time::sleep(SCAN_TIME).await;
        let _ = adapter.stop_scan().await;
        let mut out = Vec::new();
        for p in adapter.peripherals().await.map_err(ble_err)? {
            let Ok(Some(props)) = p.properties().await else { continue };
            if !props.services.contains(&NUS_SERVICE) {
                continue;
            }
            out.push(Endpoint {
                transport: ID.into(),
                address: p.id().to_string(),
                label: props.local_name.unwrap_or_else(|| "MicroPython BLE".into()),
                detail: props.rssi.map(|r| format!("RSSI {r} dBm")),
                usb: None,
                platform_hint: None,
                role: None,
            });
        }
        Ok(out)
    }

    async fn connect(&self, endpoint: &Endpoint, _: &ConnectOptions) -> Result<Connection> {
        let p = self.find(&endpoint.address).await?;
        if !p.is_connected().await.map_err(ble_err)? {
            p.connect().await.map_err(ble_err)?;
        }
        p.discover_services().await.map_err(ble_err)?;
        let chars = p.characteristics();
        let find = |u: Uuid| chars.iter().find(|c| c.uuid == u).cloned();
        let rx = find(NUS_RX).ok_or_else(|| Error::NotFound("Nordic UART RX characteristic".into()))?;
        let tx = find(NUS_TX).ok_or_else(|| Error::NotFound("Nordic UART TX characteristic".into()))?;
        p.subscribe(&tx).await.map_err(ble_err)?;

        let mut notes = p.notifications().await.map_err(ble_err)?;
        let (itx, irx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(n) = notes.next().await {
                if n.uuid == NUS_TX && itx.send(n.value).is_err() {
                    break;
                }
            }
        });
        let write_type = if rx.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
            WriteType::WithoutResponse
        } else {
            WriteType::WithResponse
        };
        Ok(Connection { transport: Arc::new(BleTransport { p, rx, write_type }), incoming: irx })
    }
}

pub struct BleTransport {
    p: Peripheral,
    rx: Characteristic,
    write_type: WriteType,
}

#[async_trait]
impl Transport for BleTransport {
    async fn write(&self, data: &[u8]) -> Result<()> {
        for chunk in data.chunks(CHUNK) {
            self.p.write(&self.rx, chunk, self.write_type).await.map_err(ble_err)?;
        }
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        self.p.disconnect().await.map_err(ble_err)
    }

    fn max_write(&self) -> usize {
        CHUNK
    }
}
