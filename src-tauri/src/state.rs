//! Shared application state.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use crate::error::{Error, Result};
use crate::firmware::flashers::FlasherRegistry;
use crate::firmware::detect::DetectorRegistry;
use crate::lsp::LspManager;
use crate::platform::PlatformRegistry;
use crate::repl::session::Session;
use crate::transport::TransportRegistry;

pub struct Paths {
    /// Persistent data: user platform profiles, settings.
    pub data: PathBuf,
    /// Re-downloadable data: firmware, stubs, board index, tools.
    pub cache: PathBuf,
}

pub struct AppState {
    pub paths: Paths,
    pub platforms: RwLock<Arc<PlatformRegistry>>,
    pub transports: TransportRegistry,
    pub flashers: FlasherRegistry,
    pub detectors: DetectorRegistry,
    /// Connected boards by session id. Several can be open at once.
    pub sessions: Mutex<BTreeMap<String, Arc<Session>>>,
    next_session: AtomicU32,
    pub http: reqwest::Client,
    pub lsp: LspManager,
    pub quirks: Vec<crate::repl::quirks::Quirk>,
    pub analyzer: crate::analysis::Analyzer,
    pub mcp: crate::mcp::McpServer,
    pub builds: crate::firmware::builder::Builds,
}

impl AppState {
    pub fn new(paths: Paths) -> Result<Self> {
        std::fs::create_dir_all(&paths.data)?;
        std::fs::create_dir_all(&paths.cache)?;
        let platforms = PlatformRegistry::load(Some(&paths.data.join("platforms")))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("Microbe/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            platforms: RwLock::new(Arc::new(platforms)),
            transports: TransportRegistry::with_defaults(&paths.data),
            flashers: FlasherRegistry::with_defaults(),
            detectors: DetectorRegistry::with_defaults(),
            sessions: Mutex::new(BTreeMap::new()),
            next_session: AtomicU32::new(1),
            lsp: LspManager::new(),
            quirks: crate::repl::quirks::load()?,
            analyzer: crate::analysis::Analyzer::with_defaults()?,
            mcp: crate::mcp::McpServer::new(&paths.data),
            builds: Default::default(),
            http,
            paths,
        })
    }

    pub fn platforms(&self) -> Arc<PlatformRegistry> {
        self.platforms.read().unwrap().clone()
    }

    pub fn session(&self, id: &str) -> Result<Arc<Session>> {
        self.sessions.lock().unwrap().get(id).cloned().ok_or(Error::NotConnected)
    }

    pub fn all_sessions(&self) -> Vec<Arc<Session>> {
        self.sessions.lock().unwrap().values().cloned().collect()
    }

    pub fn new_session_id(&self) -> String {
        format!("s{}", self.next_session.fetch_add(1, Ordering::Relaxed))
    }

    pub fn insert_session(&self, s: Arc<Session>) {
        self.sessions.lock().unwrap().insert(s.id.clone(), s);
    }

    pub fn remove_session(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().remove(id)
    }

    /// The session using the given endpoint address, if any.
    pub fn session_on(&self, address: &str) -> Option<Arc<Session>> {
        self.sessions.lock().unwrap().values().find(|s| s.endpoint.address == address).cloned()
    }

    pub fn cache_dir(&self, sub: &str) -> Result<PathBuf> {
        let d = self.paths.cache.join(sub);
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }
}
