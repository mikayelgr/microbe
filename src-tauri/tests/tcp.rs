//! Network transports end to end: a local TCP server bridges to the
//! MicroPython unix port, playing raw socket, Telnet server (with login) or
//! RFC 2217 server. Skipped when no `micropython` executable is installed.
#![cfg(unix)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use microbe_lib::platform::PlatformRegistry;
use microbe_lib::repl::session::{Session, TermEvent};
use microbe_lib::transport::tcp::{Kind, TcpProvider, CONTROL_DTR_OFF, CONTROL_RTS_OFF};
use microbe_lib::transport::{local, ConnectOptions, Endpoint, TransportProvider};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const IAC: u8 = 255;

/// Bytes the server received as Telnet subnegotiations.
type Subs = Arc<Mutex<Vec<Vec<u8>>>>;

/// Serve one connection, bridged to a fresh unix-port PTY.
async fn bridge(kind: Kind) -> Option<(String, Subs, tempfile::TempDir)> {
    let exe = local::find_micropython()?;
    let dir = tempfile::tempdir().unwrap();
    let conn = local::spawn_pty(&exe, dir.path()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let subs: Subs = Default::default();
    let subs2 = subs.clone();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (mut rd, mut wr) = sock.into_split();
        let pty = conn.transport.clone();
        let mut incoming = conn.incoming;
        if kind == Kind::Telnet {
            // Negotiate like a real server, then ask for a login.
            wr.write_all(&[IAC, 251, 1, IAC, 253, 0]).await.unwrap();
            wr.write_all(b"Login as: ").await.unwrap();
            let mut buf = [0u8; 256];
            let mut seen = Vec::new();
            while !String::from_utf8_lossy(&seen).contains("micro\r\n") {
                let n = rd.read(&mut buf).await.unwrap();
                seen.extend_from_slice(&buf[..n]);
            }
            wr.write_all(b"Password: ").await.unwrap();
            seen.clear();
            while !String::from_utf8_lossy(&seen).contains("secret\r\n") {
                let n = rd.read(&mut buf).await.unwrap();
                seen.extend_from_slice(&buf[..n]);
            }
            wr.write_all(b"Login succeeded!\r\nType \"help()\" for more information.\r\n").await.unwrap();
        }
        let telnet = kind != Kind::Raw;
        tokio::spawn(async move {
            while let Some(d) = incoming.recv().await {
                let out = if telnet { microbe_lib::transport::tcp::escape(&d) } else { d };
                if wr.write_all(&out).await.is_err() {
                    break;
                }
            }
        });
        // Socket → PTY, stripping Telnet commands and recording subnegotiations.
        let mut parser = microbe_lib::transport::tcp::TelnetParser::new(false);
        let mut buf = vec![0u8; 4096];
        loop {
            let n = match rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let data = if telnet {
                let (d, _) = parser.feed(&buf[..n]);
                subs2.lock().unwrap().extend(parser.subnegotiations.drain(..));
                d
            } else {
                buf[..n].to_vec()
            };
            if !data.is_empty() {
                pty.write(&data).await.unwrap();
            }
        }
    });
    Some((addr, subs, dir))
}

async fn session(kind: Kind, addr: &str, password: Option<&str>) -> (Arc<Session>, Arc<Mutex<Vec<u8>>>) {
    let id = match kind {
        Kind::Raw => "tcp",
        Kind::Telnet => "telnet",
        Kind::Rfc2217 => "rfc2217",
    };
    let endpoint = Endpoint { transport: id.into(), address: addr.into(), label: addr.into(), detail: None, usb: None, platform_hint: None, role: None };
    let opts = ConnectOptions { baud: Some(460800), password: password.map(String::from) };
    let conn = TcpProvider(kind).connect(&endpoint, &opts).await.unwrap();
    let profile = PlatformRegistry::load(None).unwrap().get("unix").unwrap().clone();
    let term = Arc::new(Mutex::new(Vec::new()));
    let t = term.clone();
    let s = Session::start(
        "n1".into(),
        endpoint,
        conn,
        profile,
        Arc::new(move |ev| {
            if let TermEvent::Data { data } | TermEvent::Run { data, .. } = ev {
                t.lock().unwrap().extend_from_slice(&data)
            }
        }),
        Arc::new(|_| {}),
    );
    (s, term)
}

async fn exec(s: &Session, code: &str) -> String {
    let mut raw = s.raw().await.unwrap();
    let out = raw.exec_str(code).await;
    raw.finish_with(out).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn raw_tcp_runs_code_and_moves_binary_files() {
    let Some((addr, _, dir)) = bridge(Kind::Raw).await else { return };
    let (s, _) = session(Kind::Raw, &addr, None).await;
    assert_eq!(exec(&s, "print(6 * 7)").await.trim(), "42");
    let data: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    microbe_lib::fs::write(&s, "./blob.bin", &data, &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(dir.path().join("blob.bin")).unwrap(), data);
}

#[tokio::test(flavor = "multi_thread")]
async fn telnet_logs_in_and_keeps_0xff_bytes() {
    let Some((addr, _, dir)) = bridge(Kind::Telnet).await else { return };
    let (s, _) = session(Kind::Telnet, &addr, Some("micro:secret")).await;
    assert_eq!(exec(&s, "print('hi')").await.trim(), "hi");
    // 0xFF must survive escaping in both directions.
    let data: Vec<u8> = vec![0xFF, 0x00, 0xFF, 0xFF, b'\r', 0, 0x41];
    microbe_lib::fs::write(&s, "./ff.bin", &data, &|_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(dir.path().join("ff.bin")).unwrap(), data);
    assert_eq!(microbe_lib::fs::read(&s, "./ff.bin", &|_, _| {}).await.unwrap(), data);
    assert!(exec(&s, "print(bytes([255, 254]))").await.contains("\\xff\\xfe"));
}

#[tokio::test(flavor = "multi_thread")]
async fn rfc2217_sets_baud_and_drives_reset_lines() {
    let Some((addr, subs, _dir)) = bridge(Kind::Rfc2217).await else { return };
    let (s, _) = session(Kind::Rfc2217, &addr, None).await;
    assert_eq!(exec(&s, "print('ok')").await.trim(), "ok");
    // SET-BAUDRATE 460800, big-endian.
    assert!(subs.lock().unwrap().iter().any(|x| x == &[44, 1, 0, 7, 8, 0]), "{:?}", subs.lock().unwrap());
    assert!(s.transport().hard_reset(Some("esp_classic")).await.unwrap());
    tokio::time::sleep(Duration::from_millis(300)).await;
    let got = subs.lock().unwrap().clone();
    assert!(got.contains(&vec![44, 5, CONTROL_DTR_OFF]) && got.contains(&vec![44, 5, CONTROL_RTS_OFF]), "{got:?}");
}
