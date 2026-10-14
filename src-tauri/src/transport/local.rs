//! Runs a host MicroPython executable (the unix port) in a PTY.
//!
//! The REPL behaves like a board's, raw REPL and raw-paste included. That makes
//! it a handy simulator for trying code without hardware, and the integration
//! tests use it to exercise the whole stack.

use std::fs::File;
use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::{ConnectOptions, Connection, Endpoint, Transport, TransportInfo, TransportProvider};
use crate::error::{Error, Result};
use crate::platform::PlatformRegistry;

pub const ID: &str = "local";

/// Runs the unix port with its working directory inside `root`, so the
/// simulated device has its own filesystem.
pub struct LocalProvider {
    pub root: std::path::PathBuf,
}

pub fn find_micropython() -> Option<String> {
    crate::tools::which("micropython").map(|p| p.display().to_string())
}

#[async_trait]
impl TransportProvider for LocalProvider {
    fn info(&self) -> TransportInfo {
        TransportInfo {
            id: ID.into(),
            label: "Local (unix port)".into(),
            manual_address: Some("/path/to/micropython".into()),
            needs_password: false,
            supports_baud: false,
        }
    }

    async fn discover(&self, _: &PlatformRegistry) -> Result<Vec<Endpoint>> {
        Ok(find_micropython()
            .map(|exe| Endpoint {
                transport: ID.into(),
                label: "MicroPython unix port (simulator)".into(),
                detail: Some(exe.clone()),
                address: exe,
                usb: None,
                platform_hint: Some("unix".into()),
                role: None,
            })
            .into_iter()
            .collect())
    }

    async fn connect(&self, endpoint: &Endpoint, _: &ConnectOptions) -> Result<Connection> {
        std::fs::create_dir_all(&self.root)?;
        spawn_pty(&endpoint.address, &self.root)
    }
}

pub struct LocalTransport {
    writer: Arc<Mutex<File>>,
    child: Mutex<Child>,
}

pub fn spawn_pty(exe: &str, cwd: &std::path::Path) -> Result<Connection> {
    let (master, slave) = open_raw_pty()?;
    let slave_file = File::from(slave);
    let child = Command::new(exe)
        .current_dir(cwd)
        .stdin(Stdio::from(slave_file.try_clone()?))
        .stdout(Stdio::from(slave_file.try_clone()?))
        .stderr(Stdio::from(slave_file))
        .spawn()
        .map_err(|e| Error::other(format!("cannot start {exe}: {e}")))?;

    let master = File::from(master);
    // EOF (read of 0 / EIO) once the child exits and the slave side closes.
    let incoming = super::spawn_reader("pty-reader", master.try_clone()?, true, Default::default())?;
    let t = LocalTransport { writer: Arc::new(Mutex::new(master)), child: Mutex::new(child) };
    Ok(Connection { transport: Arc::new(t), incoming })
}

fn open_raw_pty() -> Result<(OwnedFd, OwnedFd)> {
    let mut master: libc::c_int = 0;
    let mut slave: libc::c_int = 0;
    // SAFETY: openpty writes two fds into the provided out-params.
    let rc = unsafe {
        libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut())
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: both fds were just returned by openpty and are owned by us.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // Raw mode so the line discipline doesn't rewrite \n to \r\n or echo input.
    unsafe {
        use std::os::fd::AsRawFd;
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(slave.as_raw_fd(), &mut t) == 0 {
            libc::cfmakeraw(&mut t);
            libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &t);
        }
    }
    Ok((master, slave))
}

#[async_trait]
impl Transport for LocalTransport {
    async fn write(&self, data: &[u8]) -> Result<()> {
        let data = data.to_vec();
        super::blocking(&self.writer, move |f| {
            f.write_all(&data).map_err(|e| if e.raw_os_error() == Some(libc::EIO) { Error::Closed } else { e.into() })
        })
        .await
    }

    async fn interrupt(&self) -> Result<()> {
        let pid = self.child.lock().map_err(|_| Error::other("pty poisoned"))?.id();
        // SAFETY: sending a signal to our own child process.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGINT) };
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
        Ok(())
    }
}

impl Drop for LocalTransport {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
