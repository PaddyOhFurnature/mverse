//! Metaverse Server
//!
//! Combined P2P relay + world state authority.
//!
//! Roles:
//!   • Relay: circuits for CGNAT clients (same as metaverse_relay)
//!   • World: serves chunk state, merges CRDT voxel ops, persists world data
//!
//! TUI dashboard by default when run in a terminal.
//! Falls back to plain log when piped/redirected or --headless is set.
//! Web dashboard always running on web_port (default 8080).
//!
//! Config: ./server.json  or  ~/.metaverse/server.json  (local wins)
//! CLI args override config file values.
//!
//! Keybindings: [m] Main  [p] Peers  [w] World  [l] Log  [c] Config  [h] Help  [q] Quit

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use libp2p::{
    gossipsub, identify, identity, kad, mdns, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, PeerId, SwarmBuilder,
};
use libp2p::kad::store::MemoryStore;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use sysinfo::System;
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    io::{self, IsTerminal},
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use clap::Parser;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use rusqlite::{params, Connection};
use sha2::{Sha256, Digest};
use rand::RngCore;
#[cfg(unix)]
use libc;

// ─── Config ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ServerConfig {
    // ── Network / Relay ──────────────────────────────────────────────────
    /// TCP relay port
    pub port: u16,
    /// WebSocket port (default port + 5000)
    pub ws_port: Option<u16>,
    /// Advertised external address (e.g. /ip4/1.2.3.4/tcp/4001)
    pub external_addr: Option<String>,
    /// Human-readable name shown in TUI header and DHT
    pub node_name: Option<String>,
    /// Node type advertised to peers ("server" or "relay")
    pub node_type: String,
    /// Priority score advertised in DHT (higher = preferred by clients)
    pub priority_score: u32,

    // ── Relay limits ─────────────────────────────────────────────────────
    pub max_circuits: usize,
    pub max_circuit_duration_secs: u64,
    pub max_circuit_bytes: u64,

    // ── Peer access control ──────────────────────────────────────────────
    /// Known peer relay multiaddrs to dial at startup
    pub peers: Vec<String>,
    /// Blocked peer IDs (rejected at connection)
    pub blacklist: Vec<String>,
    /// If non-empty, ONLY these peer IDs may connect
    pub whitelist: Vec<String>,
    /// Relay slot priority for these peer IDs
    pub priority_peers: Vec<String>,

    // ── Bandwidth / load limits ──────────────────────────────────────────
    /// Maximum bandwidth in MB/s (0 = unlimited)
    pub max_bandwidth_mbps: u32,
    /// Maximum simultaneous peers (0 = unlimited)
    pub max_peers: u32,
    /// Drop connections with RTT above this ms (0 = no limit)
    pub max_ping_ms: u32,
    /// Retry attempts for failed dials
    pub max_retries: u32,
    /// Whether this server is expected to be online 24/7 (advertised in NodeCapabilities).
    pub always_on: bool,

    // ── Load shedding ────────────────────────────────────────────────────
    /// Drop Low-priority relay circuits when CPU exceeds this % (0 = disabled)
    pub cpu_shed_threshold_pct: u8,
    /// Stop loading new chunks when RAM exceeds this % (0 = disabled)
    pub ram_shed_threshold_pct: u8,

    // ── World state ──────────────────────────────────────────────────────
    pub world_enabled: bool,
    pub world_dir: Option<String>,
    /// Maximum world data folder size in GB (0 = unlimited)
    pub max_world_data_gb: u32,
    pub max_loaded_chunks: usize,
    pub chunk_load_radius_m: f64,
    pub chunk_unload_radius_m: f64,
    pub world_save_interval_secs: u64,

    // ── Identity ─────────────────────────────────────────────────────────
    pub identity_file: Option<String>,
    pub temp_identity: bool,

    // ── UI ───────────────────────────────────────────────────────────────
    /// Force plain-log (auto-detected from terminal)
    pub headless: bool,
    pub ui: UiConfig,

    // ── Web dashboard ────────────────────────────────────────────────────
    pub web_enabled: bool,
    pub web_port: u16,
    pub web_bind: String,
    pub web_auth: bool,
    pub web_username: String,
    pub web_password: String,

    // ── Logging ──────────────────────────────────────────────────────────
    pub log_level: String,

    // ── Server sync ──────────────────────────────────────────────────────
    /// HTTP base URLs of peer servers to sync key records with.
    /// Format: "http://192.168.1.100:8080" (no trailing slash).
    /// On startup and every 10 minutes the server will query
    /// `GET /api/v1/sync/keys?since=<last_sync_ms>&limit=1000` on each.
    pub known_servers: Vec<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 4001,
            ws_port: None,
            external_addr: None,
            node_name: None,
            node_type: "server".to_string(),
            priority_score: 100,
            max_circuits: 100,
            max_circuit_duration_secs: 3600,
            max_circuit_bytes: 1_073_741_824,
            peers: vec![],
            blacklist: vec![],
            whitelist: vec![],
            priority_peers: vec![],
            max_bandwidth_mbps: 0,
            max_peers: 0,
            max_ping_ms: 0,
            max_retries: 5,
            always_on: true,
            cpu_shed_threshold_pct: 90,
            ram_shed_threshold_pct: 85,
            world_enabled: true,
            world_dir: None,
            max_world_data_gb: 10,
            max_loaded_chunks: 1000,
            chunk_load_radius_m: 500.0,
            chunk_unload_radius_m: 600.0,
            world_save_interval_secs: 300,
            identity_file: None,
            temp_identity: false,
            headless: false,
            ui: UiConfig::default(),
            web_enabled: true,
            web_port: 8080,
            web_bind: "0.0.0.0".to_string(),
            web_auth: false,
            web_username: "admin".to_string(),
            web_password: String::new(),
            log_level: "info".to_string(),
            known_servers: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UiConfig {
    pub show_cpu: bool,
    pub show_ram: bool,
    pub refresh_ms: u64,
    pub max_log_entries: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { show_cpu: true, show_ram: true, refresh_ms: 500, max_log_entries: 1000 }
    }
}

fn config_paths() -> [PathBuf; 2] {
    [
        PathBuf::from("server.json"),
        dirs::home_dir().unwrap_or_default().join(".metaverse").join("server.json"),
    ]
}

fn load_config() -> ServerConfig {
    for path in &config_paths() {
        if path.exists() {
            if let Ok(text) = std::fs::read_to_string(path) {
                match serde_json::from_str::<ServerConfig>(&text) {
                    Ok(cfg) => return cfg,
                    Err(e) => eprintln!("⚠️  Config parse error in {}: {}", path.display(), e),
                }
            }
        }
    }
    ServerConfig::default()
}

fn write_default_config_if_missing() {
    let path = dirs::home_dir().unwrap_or_default().join(".metaverse").join("server.json");
    if !path.exists() {
        if let Some(p) = path.parent() { std::fs::create_dir_all(p).ok(); }
        if let Ok(json) = serde_json::to_string_pretty(&ServerConfig::default()) {
            let _ = std::fs::write(&path, json);
            eprintln!("📝 Created default config at {}", path.display());
        }
    }
}

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "metaverse-server")]
#[command(about = "Metaverse P2P server — relay + world authority + TUI + web dashboard")]
#[command(version)]
struct Args {
    /// Config file path (default: ./server.json or ~/.metaverse/server.json)
    #[arg(long, value_name = "PATH")]
    config: Option<String>,
    /// TCP relay port
    #[arg(short, long)]
    port: Option<u16>,
    /// Web dashboard port (default: 8080)
    #[arg(long)]
    web_port: Option<u16>,
    /// Advertised external address
    #[arg(long)]
    external_addr: Option<String>,
    /// Node display name
    #[arg(long)]
    name: Option<String>,
    /// Maximum relay circuits
    #[arg(long)]
    max_circuits: Option<usize>,
    /// Maximum relay circuit bytes
    #[arg(long)]
    max_circuit_bytes: Option<u64>,
    /// World data directory
    #[arg(long, value_name = "PATH")]
    world_dir: Option<String>,
    /// Identity key file path
    #[arg(long, value_name = "PATH")]
    identity: Option<String>,
    /// Use a temporary (non-persistent) identity
    #[arg(long)]
    temp_identity: bool,
    /// Disable world state (relay-only mode)
    #[arg(long)]
    no_world: bool,
    /// Disable web dashboard
    #[arg(long)]
    no_web: bool,
    /// Plain log output (no TUI), auto-detected from terminal
    #[arg(long)]
    headless: bool,
    /// Peer relay to dial at startup (repeatable)
    #[arg(long, value_name = "MULTIADDR")]
    peer: Vec<String>,
}

fn apply_cli_overrides(config: &mut ServerConfig, args: &Args) {
    if let Some(v) = args.port             { config.port = v; }
    if let Some(v) = args.web_port         { config.web_port = v; }
    if let Some(ref v) = args.external_addr { config.external_addr = Some(v.clone()); }
    if let Some(ref v) = args.name          { config.node_name = Some(v.clone()); }
    if let Some(v) = args.max_circuits     { config.max_circuits = v; }
    if let Some(v) = args.max_circuit_bytes { config.max_circuit_bytes = v; }
    if let Some(ref v) = args.world_dir    { config.world_dir = Some(v.clone()); }
    if let Some(ref v) = args.identity     { config.identity_file = Some(v.clone()); }
    if args.temp_identity                  { config.temp_identity = true; }
    if args.no_world                       { config.world_enabled = false; }
    if args.no_web                         { config.web_enabled = false; }
    if args.headless                       { config.headless = true; }
    if !args.peer.is_empty()               { config.peers.extend(args.peer.clone()); }
}

// ─── Network behaviour ───────────────────────────────────────────────────────

#[derive(NetworkBehaviour)]
struct ServerBehaviour {
    relay: relay::Behaviour,
    ping: libp2p::ping::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour,
    mdns: mdns::tokio::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

// ─── State ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub addr: String,
    pub connected_secs: u64,
    pub peer_type: String, // "client" | "relay" | "server" | "unknown"
}

#[derive(Clone, Debug)]
struct CircuitInfo { src: PeerId, dst: PeerId, started_at: Instant }

#[derive(Clone, Debug, Default, Serialize)]
pub struct WorldStats {
    pub chunks_loaded: usize,
    pub chunks_queued: usize,
    pub chunks_loading: usize,
    pub world_data_mb: f64,
    pub voxel_ops_total: u64,
    pub ops_merged_total: u64,
    pub last_save_secs_ago: u64,
    pub shedding_chunks: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct NetStats {
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub gossip_msgs_in: u64,
    pub gossip_msgs_out: u64,
    pub state_requests_in: u64,
    pub state_responses_out: u64,
}

#[derive(PartialEq, Clone)]
enum Tab { Main }  // single-screen TUI — kept for headless/log path compatibility

/// Shared state readable by web server and TUI
#[derive(Clone, Serialize)]
pub struct SharedState {
    pub local_peer_id: String,
    pub public_ip: String,
    pub uptime_secs: u64,
    pub node_name: String,
    pub node_type: String,
    pub peers: Vec<PeerInfo>,
    pub circuit_count: usize,
    pub total_connections: u64,
    pub total_reservations: u64,
    pub dht_peer_count: usize,
    pub key_count: usize,
    pub world: WorldStats,
    pub net: NetStats,
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_pct: f32,
    pub shedding_relay: bool,
    pub relay_port: u16,
    pub version: String,
    /// Key registry database handle (not serialised — accessed via REST).
    #[serde(skip)]
    pub key_db: Option<KeyDatabase>,
    /// First 32 bytes of the server's signing key — used to generate stateless auth tokens.
    /// Not serialised (never sent over the wire).
    #[serde(skip)]
    pub server_secret: [u8; 32],
    /// In-flight auth challenges: nonce_hex → (expires_at_ms, requesting_peer_id).
    /// Shared between web handlers and the expiry cleaner.
    #[serde(skip)]
    pub pending_challenges: Arc<Mutex<HashMap<String, (u64, String)>>>,
    /// Channel to publish gossipsub messages from web handlers into the event loop.
    #[serde(skip)]
    pub gossip_tx: Option<tokio::sync::mpsc::Sender<GossipCommand>>,
    /// Channel to send a hot-reload `ServerConfig` from web handlers into the event loop.
    #[serde(skip)]
    pub config_reload_tx: Option<tokio::sync::mpsc::Sender<ServerConfig>>,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            local_peer_id: String::new(), public_ip: String::new(), uptime_secs: 0,
            node_name: String::new(), node_type: "server".to_string(),
            peers: vec![], circuit_count: 0, total_connections: 0,
            total_reservations: 0, dht_peer_count: 0, key_count: 0,
            world: WorldStats::default(), net: NetStats::default(),
            cpu_pct: 0.0, ram_used_mb: 0, ram_total_mb: 0, ram_pct: 0.0,
            shedding_relay: false,
            relay_port: 4001,
            version: env!("CARGO_PKG_VERSION").to_string(),
            key_db: None,
            server_secret: [0u8; 32],
            pending_challenges: Arc::new(Mutex::new(HashMap::new())),
            gossip_tx: None,
            config_reload_tx: None,
        }
    }
}

/// Full app state (only in event loop; not shared)
struct AppState {
    config: ServerConfig,
    shared: Arc<RwLock<SharedState>>,
    local_peer_id: String,
    public_ip: String,
    start_time: Instant,
    connected_peers: HashMap<PeerId, (Instant, String, String)>, // (connected_at, addr, peer_type)
    active_circuits: Vec<CircuitInfo>,
    total_connections: u64,
    total_reservations: u64,
    dht_peer_count: usize,
    log: VecDeque<String>,
    tab: Tab,
    should_quit: bool,
    sys: System,
    cpu_pct: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    last_sys_refresh: Instant,
    last_shared_sync: Instant,
    net: NetStats,
    shedding_relay: bool,
    /// Gossip commands from web handlers waiting to be published into the swarm.
    gossip_rx: tokio::sync::mpsc::Receiver<GossipCommand>,
    /// Config reload commands from web handlers / SIGHUP.
    config_reload_rx: tokio::sync::mpsc::Receiver<ServerConfig>,
    /// Peers to disconnect on next world_tick (populated by load-shedding logic).
    pending_shed: Vec<PeerId>,
    /// Incoming chunk state requests queued for processing in world_tick.
    pending_chunk_requests: Vec<metaverse_core::messages::ChunkStateRequest>,
    /// Incoming voxel ops from peers queued for persistence in world_tick.
    pending_voxel_ops: Vec<metaverse_core::messages::SignedOperation>,
    /// DHT provider keys to announce on next tick (populated at startup).
    pending_dht_provide: Vec<Vec<u8>>,
}

impl AppState {
    fn new(config: ServerConfig, shared: Arc<RwLock<SharedState>>,
           local_peer_id: String, public_ip: String,
           gossip_rx: tokio::sync::mpsc::Receiver<GossipCommand>,
           config_reload_rx: tokio::sync::mpsc::Receiver<ServerConfig>) -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        let cpu_pct = sys.global_cpu_usage();
        let ram_used_mb = sys.used_memory() / 1_048_576;
        let ram_total_mb = sys.total_memory() / 1_048_576;
        Self {
            config, shared, local_peer_id: local_peer_id.clone(), public_ip: public_ip.clone(),
            start_time: Instant::now(),
            connected_peers: HashMap::new(),
            active_circuits: vec![],
            total_connections: 0, total_reservations: 0, dht_peer_count: 0,
            log: VecDeque::new(), tab: Tab::Main, should_quit: false,
            sys, cpu_pct, ram_used_mb, ram_total_mb,
            last_sys_refresh: Instant::now(),
            last_shared_sync: Instant::now(),
            net: NetStats::default(),
            shedding_relay: false,
            gossip_rx,
            config_reload_rx,
            pending_shed: Vec::new(),
            pending_chunk_requests: Vec::new(),
            pending_voxel_ops: Vec::new(),
            pending_dht_provide: Vec::new(),
        }
    }

    fn log(&mut self, msg: impl Into<String>) {
        let e = self.start_time.elapsed().as_secs();
        let entry = format!("{:02}:{:02}:{:02}  {}", e/3600, (e%3600)/60, e%60, msg.into());
        self.log.push_back(entry.clone());
        while self.log.len() > self.config.ui.max_log_entries { self.log.pop_front(); }
        if self.config.headless { println!("{}", entry); }
    }

    fn refresh_sys(&mut self) {
        if self.last_sys_refresh.elapsed() < Duration::from_secs(2) { return; }
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.cpu_pct = self.sys.global_cpu_usage();
        self.ram_used_mb = self.sys.used_memory() / 1_048_576;
        self.ram_total_mb = self.sys.total_memory() / 1_048_576;
        self.last_sys_refresh = Instant::now();

        let cpu_thresh = self.config.cpu_shed_threshold_pct;
        let ram_thresh = self.config.ram_shed_threshold_pct;
        let ram_pct = if self.ram_total_mb > 0 {
            (self.ram_used_mb as f32 / self.ram_total_mb as f32) * 100.0
        } else { 0.0 };

        let over_cpu = cpu_thresh > 0 && self.cpu_pct > cpu_thresh as f32;
        let over_ram = ram_thresh > 0 && ram_pct > ram_thresh as f32;

        if over_cpu || over_ram {
            if !self.shedding_relay {
                self.shedding_relay = true;
                self.log(format!(
                    "⚠️  Load shedding: CPU={:.0}%/{cpu_thresh}% RAM={:.0}%/{ram_thresh}% — dropping oldest relay circuit",
                    self.cpu_pct, ram_pct,
                    cpu_thresh = cpu_thresh, ram_thresh = ram_thresh,
                ));
            }
            // Evict the oldest relay circuit (1 per refresh cycle to avoid mass-disconnect)
            if let Some(oldest) = self.active_circuits.iter()
                .min_by_key(|c| c.started_at)
            {
                // Disconnect the source peer of the oldest circuit
                self.pending_shed.push(oldest.src);
            }
        } else {
            if self.shedding_relay {
                self.log(format!(
                    "✅ Load below threshold — CPU={:.0}%/{cpu_thresh}% RAM={:.0}%/{ram_thresh}%",
                    self.cpu_pct, ram_pct,
                    cpu_thresh = cpu_thresh, ram_thresh = ram_thresh,
                ));
            }
            self.shedding_relay = false;
        }
    }

    /// Drain the list of peers to disconnect (populated by load-shedding).
    /// The event loop calls this and issues `SwarmAction::DisconnectPeer` for each.
    fn drain_pending_shed(&mut self) -> Vec<PeerId> {
        std::mem::take(&mut self.pending_shed)
    }

    fn sync_shared(&mut self, world: &WorldStats) {
        if self.last_shared_sync.elapsed() < Duration::from_millis(500) { return; }
        let ram_pct = if self.ram_total_mb > 0 { (self.ram_used_mb as f32 / self.ram_total_mb as f32) * 100.0 } else { 0.0 };
        let uptime = self.start_time.elapsed().as_secs();
        let peers: Vec<PeerInfo> = self.connected_peers.iter().map(|(pid, (at, addr, ptype))| {
            PeerInfo {
                peer_id: short_id(&pid.to_string()),
                addr: addr.clone(),
                connected_secs: at.elapsed().as_secs(),
                peer_type: ptype.clone(),
            }
        }).collect();
        let name = self.config.node_name.clone().unwrap_or_else(|| "server".to_string());
        let circuit_count = self.active_circuits.len();
        if let Ok(mut s) = self.shared.write() {
            s.uptime_secs = uptime;
            s.node_name = name;
            s.node_type = self.config.node_type.clone();
            s.peers = peers;
            s.circuit_count = circuit_count;
            s.total_connections = self.total_connections;
            s.total_reservations = self.total_reservations;
            s.dht_peer_count = self.dht_peer_count;
            s.world = world.clone();
            s.net = self.net.clone();
            s.cpu_pct = self.cpu_pct;
            s.ram_used_mb = self.ram_used_mb;
            s.ram_total_mb = self.ram_total_mb;
            s.ram_pct = ram_pct;
            s.shedding_relay = self.shedding_relay;
            if let Some(ref db) = s.key_db {
                s.key_count = db.count();
            }
        }
        self.last_shared_sync = Instant::now();
    }

    fn short(id: &str) -> String { short_id(id) }
}

fn short_id(id: &str) -> String {
    if id.len() > 12 { format!("…{}", &id[id.len()-10..]) } else { id.to_string() }
}

// ─── World stats helper ───────────────────────────────────────────────────────

fn world_data_size_mb(world_dir: &std::path::Path) -> f64 {
    let mut total: u64 = 0;
    if let Ok(entries) = std::fs::read_dir(world_dir) {
        for e in entries.flatten() {
            if let Ok(m) = e.metadata() { total += m.len(); }
        }
    }
    total as f64 / 1_048_576.0
}

// ─── Key Database ─────────────────────────────────────────────────────────────

/// SQLite-backed persistent key registry for the server.
///
/// Receives `KeyRecord` updates via the key-registry gossipsub topic and persists
/// them so clients can query the full registry via REST. Uses WAL mode for
/// concurrent reads while the event loop is writing.
///
/// `Clone` is cheap — the internal connection is behind `Arc<Mutex<>>`.
#[derive(Clone)]
struct KeyDatabase {
    conn: Arc<std::sync::Mutex<Connection>>,
}

impl KeyDatabase {
    /// Open (or create) the key registry database at `path`.
    fn open(path: &std::path::Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS key_records (
                peer_id      TEXT    PRIMARY KEY,
                record_bytes BLOB    NOT NULL,
                key_type     TEXT    NOT NULL,
                display_name TEXT,
                created_at   INTEGER NOT NULL DEFAULT 0,
                updated_at   INTEGER NOT NULL DEFAULT 0,
                revoked      INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_key_type   ON key_records (key_type);
            CREATE INDEX IF NOT EXISTS idx_updated_at ON key_records (updated_at);

            -- Key upgrade / relay issuance requests
            CREATE TABLE IF NOT EXISTS key_requests (
                id            TEXT    PRIMARY KEY,   -- UUID
                peer_id       TEXT    NOT NULL,
                requested_type TEXT   NOT NULL,      -- 'Relay', 'Server', 'Admin' etc.
                display_name  TEXT,
                justification TEXT,
                contact_info  TEXT,
                status        TEXT    NOT NULL DEFAULT 'pending', -- pending|approved|denied
                created_at    INTEGER NOT NULL DEFAULT 0,
                reviewed_at   INTEGER,
                reviewer_note TEXT,
                result_bytes  BLOB                   -- signed KeyRecord bytes when approved
            );
            CREATE INDEX IF NOT EXISTS idx_req_status ON key_requests (status);
            CREATE INDEX IF NOT EXISTS idx_req_peer   ON key_requests (peer_id);

            -- Server-to-server sync tracking
            CREATE TABLE IF NOT EXISTS server_sync (
                server_url         TEXT    PRIMARY KEY,
                last_synced_at     INTEGER NOT NULL DEFAULT 0,
                records_received   INTEGER NOT NULL DEFAULT 0
            );
        ")?;

        // Schema migrations: add columns that may not exist in older databases.
        // SQLite doesn't support IF NOT EXISTS on ALTER TABLE; ignore errors for
        // duplicate columns (they're harmless).
        for migration in &[
            "ALTER TABLE key_records ADD COLUMN revoked_at INTEGER",
            "ALTER TABLE key_records ADD COLUMN revoked_by TEXT",
            "ALTER TABLE key_records ADD COLUMN revocation_reason TEXT",
        ] {
            let _ = conn.execute(migration, []);
        }
        Ok(Self { conn: Arc::new(std::sync::Mutex::new(conn)) })
    }

    /// Insert or update a key record.
    ///
    /// Only replaces an existing record if `updated_at` is strictly newer,
    /// preventing replay of stale records over the wire.
    fn upsert(
        &self,
        peer_id: &str, record_bytes: &[u8],
        key_type: &str, display_name: Option<&str>,
        created_at: i64, updated_at: i64, revoked: bool,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO key_records
                (peer_id, record_bytes, key_type, display_name, created_at, updated_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(peer_id) DO UPDATE SET
                record_bytes = excluded.record_bytes,
                key_type     = excluded.key_type,
                display_name = excluded.display_name,
                updated_at   = excluded.updated_at,
                revoked      = excluded.revoked
             WHERE excluded.updated_at > key_records.updated_at",
            params![peer_id, record_bytes, key_type, display_name,
                    created_at, updated_at, revoked as i32],
        );
    }

    /// Count stored (non-revoked) records.
    fn count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM key_records WHERE revoked = 0", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    /// List records as JSON objects, optionally filtered by `key_type`.
    fn list(&self, key_type_filter: Option<&str>) -> Vec<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let mut rows = vec![];
        let result: Result<(), rusqlite::Error> = (|| {
            if let Some(kt) = key_type_filter {
                let mut stmt = conn.prepare(
                    "SELECT peer_id, key_type, display_name, created_at, updated_at, revoked
                     FROM key_records WHERE key_type = ?1 ORDER BY updated_at DESC"
                )?;
                for row in stmt.query_map(params![kt], key_record_to_json)?.flatten() {
                    rows.push(row);
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT peer_id, key_type, display_name, created_at, updated_at, revoked
                     FROM key_records ORDER BY updated_at DESC"
                )?;
                for row in stmt.query_map([], key_record_to_json)?.flatten() {
                    rows.push(row);
                }
            }
            Ok(())
        })();
        let _ = result;
        rows
    }

    /// Get the raw serialised `KeyRecord` bytes for a specific peer.
    fn get_bytes(&self, peer_id: &str) -> Option<Vec<u8>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT record_bytes FROM key_records WHERE peer_id = ?1",
            params![peer_id],
            |r| r.get(0),
        ).ok()
    }

    /// Get the key_type string for a specific peer (returns None if not found).
    fn get_key_type(&self, peer_id: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT key_type FROM key_records WHERE peer_id = ?1",
            params![peer_id],
            |r| r.get(0),
        ).ok()
    }

    /// Mark a key as revoked.
    ///
    /// Returns `true` if the record existed and was not already revoked.
    fn revoke_key_in_db(
        &self,
        peer_id: &str,
        revoked_by: &str,
        reason: Option<&str>,
        revoked_at_ms: i64,
    ) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE key_records
             SET revoked=1, revoked_at=?1, revoked_by=?2, revocation_reason=?3
             WHERE peer_id=?4 AND revoked=0",
            params![revoked_at_ms, revoked_by, reason, peer_id],
        ).unwrap_or(0) > 0
    }

    /// Return key records with `updated_at > since_ms`, ordered by `updated_at` ascending,
    /// up to `limit` rows.
    ///
    /// Used by the `GET /api/v1/sync/keys` endpoint so peer servers can pull incremental updates.
    fn list_since(&self, since_ms: i64, limit: usize) -> Vec<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let mut rows = vec![];
        let result: Result<(), rusqlite::Error> = (|| {
            let mut stmt = conn.prepare(
                "SELECT peer_id, key_type, display_name, created_at, updated_at, revoked, record_bytes
                 FROM key_records WHERE updated_at > ?1 ORDER BY updated_at ASC LIMIT ?2",
            )?;
            for row in stmt.query_map(params![since_ms, limit as i64], |r| {
                let record_bytes: Option<Vec<u8>> = r.get(6)?;
                Ok(serde_json::json!({
                    "peer_id":      r.get::<_, String>(0)?,
                    "key_type":     r.get::<_, String>(1)?,
                    "display_name": r.get::<_, Option<String>>(2)?,
                    "created_at":   r.get::<_, i64>(3)?,
                    "updated_at":   r.get::<_, i64>(4)?,
                    "revoked":      r.get::<_, i32>(5)? != 0,
                    "record_b64":   record_bytes.as_deref().map(|b| {
                                        use base64::{Engine as _, engine::general_purpose::STANDARD};
                                        STANDARD.encode(b)
                                    }),
                }))
            })?.flatten() {
                rows.push(row);
            }
            Ok(())
        })();
        let _ = result;
        rows
    }

    /// Update or set the last-synced timestamp for a peer server URL.
    fn update_server_sync(&self, server_url: &str, last_synced_at: i64, records_received: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO server_sync (server_url, last_synced_at, records_received)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(server_url) DO UPDATE SET
               last_synced_at = excluded.last_synced_at,
               records_received = server_sync.records_received + excluded.records_received",
            params![server_url, last_synced_at, records_received],
        );
    }

    /// Get the last-synced timestamp for a peer server URL (0 if never synced).
    fn get_last_synced_at(&self, server_url: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT last_synced_at FROM server_sync WHERE server_url = ?1",
            params![server_url],
            |r| r.get(0),
        ).unwrap_or(0)
    }

    // ── Key request methods ─────────────────────────────────────────────────

    /// Insert a new key upgrade request.
    fn insert_key_request(
        &self, id: &str, peer_id: &str,
        requested_type: &str, display_name: Option<&str>,
        justification: Option<&str>, contact_info: Option<&str>,
        created_at: i64,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO key_requests
             (id, peer_id, requested_type, display_name, justification, contact_info, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, peer_id, requested_type, display_name, justification, contact_info, created_at],
        );
    }

    /// List key requests, optionally filtered by `status` ("pending", "approved", "denied").
    fn list_key_requests(&self, status_filter: Option<&str>) -> Vec<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        let mut rows = vec![];
        let result: Result<(), rusqlite::Error> = (|| {
            let sql = "SELECT id, peer_id, requested_type, display_name, justification,
                              contact_info, status, created_at, reviewed_at, reviewer_note
                       FROM key_requests ORDER BY created_at DESC";
            let sql_filtered = "SELECT id, peer_id, requested_type, display_name, justification,
                                       contact_info, status, created_at, reviewed_at, reviewer_note
                                FROM key_requests WHERE status = ?1 ORDER BY created_at DESC";
            if let Some(st) = status_filter {
                let mut stmt = conn.prepare(sql_filtered)?;
                for row in stmt.query_map(params![st], key_request_to_json)?.flatten() { rows.push(row); }
            } else {
                let mut stmt = conn.prepare(sql)?;
                for row in stmt.query_map([], key_request_to_json)?.flatten() { rows.push(row); }
            }
            Ok(())
        })();
        let _ = result;
        rows
    }

    /// Get a single key request by ID.
    fn get_key_request(&self, id: &str) -> Option<serde_json::Value> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, peer_id, requested_type, display_name, justification,
                    contact_info, status, created_at, reviewed_at, reviewer_note
             FROM key_requests WHERE id = ?1",
            params![id],
            key_request_to_json,
        ).ok()
    }

    /// Update a key request to approved/denied with a countersigned record (or None).
    fn update_key_request_status(
        &self, id: &str, status: &str,
        reviewer_note: Option<&str>,
        reviewed_at: i64,
        result_bytes: Option<&[u8]>,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE key_requests SET status=?1, reviewer_note=?2, reviewed_at=?3, result_bytes=?4
             WHERE id = ?5",
            params![status, reviewer_note, reviewed_at, result_bytes, id],
        );
    }

    /// Get the signed result bytes for an approved request.
    fn get_key_request_result(&self, id: &str) -> Option<Vec<u8>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT result_bytes FROM key_requests WHERE id = ?1 AND status = 'approved'",
            params![id],
            |r| r.get(0),
        ).ok().flatten()
    }
}

fn key_request_to_json(row: &rusqlite::Row<'_>) -> Result<serde_json::Value, rusqlite::Error> {
    Ok(serde_json::json!({
        "id":             row.get::<_, String>(0)?,
        "peer_id":        row.get::<_, String>(1)?,
        "requested_type": row.get::<_, String>(2)?,
        "display_name":   row.get::<_, Option<String>>(3)?,
        "justification":  row.get::<_, Option<String>>(4)?,
        "contact_info":   row.get::<_, Option<String>>(5)?,
        "status":         row.get::<_, String>(6)?,
        "created_at":     row.get::<_, i64>(7)?,
        "reviewed_at":    row.get::<_, Option<i64>>(8)?,
        "reviewer_note":  row.get::<_, Option<String>>(9)?,
    }))
}

fn key_record_to_json(row: &rusqlite::Row<'_>) -> Result<serde_json::Value, rusqlite::Error> {
    Ok(serde_json::json!({
        "peer_id":      row.get::<_, String>(0)?,
        "key_type":     row.get::<_, String>(1)?,
        "display_name": row.get::<_, Option<String>>(2)?,
        "created_at":   row.get::<_, i64>(3)?,
        "updated_at":   row.get::<_, i64>(4)?,
        "revoked":      row.get::<_, i32>(5)? != 0,
    }))
}



/// Commands sent from web handlers → event loop to publish via gossipsub.
enum GossipCommand {
    Publish { topic: String, data: Vec<u8> },
}

enum SwarmAction {
    AddKadAddress(PeerId, Multiaddr),
    RefreshDhtCount,
    DialPeer(PeerId, Multiaddr),
    SubscribeTopic(String),
    PublishGossip { topic: String, data: Vec<u8> },
    StartProviding(Vec<u8>),
    PutDhtRecord { key: Vec<u8>, value: Vec<u8> },
    /// Disconnect a peer — used by load-shedding to drop the oldest relay circuit.
    DisconnectPeer(PeerId),
}

fn handle_swarm_event(
    event: SwarmEvent<ServerBehaviourEvent>,
    state: &mut AppState,
) -> Vec<SwarmAction> {
    let mut actions = vec![];
    match event {
        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
            state.total_connections += 1;
            let addr = endpoint.get_remote_address().to_string();
            // Check blacklist
            let pid_str = peer_id.to_string();
            if state.config.blacklist.contains(&pid_str) {
                state.log(format!("🚫 Blocked blacklisted peer {}", AppState::short(&pid_str)));
                return actions; // swarm will still connect; kick needed via dial close
            }
            // Check whitelist
            if !state.config.whitelist.is_empty() && !state.config.whitelist.contains(&pid_str) {
                state.log(format!("🚫 Rejected non-whitelisted peer {}", AppState::short(&pid_str)));
                return actions;
            }
            state.connected_peers.insert(peer_id, (Instant::now(), addr.clone(), "unknown".to_string()));
            state.log(format!("🔗 Connected  {} via {}", AppState::short(&pid_str), short_addr(&addr)));
        }
        SwarmEvent::ConnectionClosed { peer_id, num_established, cause, .. } => {
            if num_established == 0 {
                state.connected_peers.remove(&peer_id);
                let reason = cause.map(|e| format!(" ({})", e)).unwrap_or_default();
                state.log(format!("❌ Disconnected {}{}", AppState::short(&peer_id.to_string()), reason));
            }
        }
        SwarmEvent::NewListenAddr { address, .. } => {
            state.log(format!("👂 Listening  {}", address));
        }
        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            if let Some(pid) = peer_id {
                state.log(format!("✗  Dial failed  {} — {}", AppState::short(&pid.to_string()), error));
            }
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::Relay(ev)) => match ev {
            relay::Event::ReservationReqAccepted { src_peer_id, .. } => {
                state.total_reservations += 1;
                state.log(format!("✅ Reservation  {}", AppState::short(&src_peer_id.to_string())));
            }
            relay::Event::ReservationTimedOut { src_peer_id } => {
                state.log(format!("⏱  Reservation expired  {}", AppState::short(&src_peer_id.to_string())));
            }
            relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id } => {
                // Load shed: if CPU too high, log but still accept (libp2p handles it)
                if state.shedding_relay {
                    state.log(format!("⚠️  Circuit accepted under load  {} → {}",
                        AppState::short(&src_peer_id.to_string()),
                        AppState::short(&dst_peer_id.to_string())));
                } else {
                    state.log(format!("🔄 Circuit  {} → {}",
                        AppState::short(&src_peer_id.to_string()),
                        AppState::short(&dst_peer_id.to_string())));
                }
                state.active_circuits.push(CircuitInfo {
                    src: src_peer_id, dst: dst_peer_id, started_at: Instant::now(),
                });
            }
            relay::Event::CircuitClosed { src_peer_id, dst_peer_id, .. } => {
                state.active_circuits.retain(|c| !(c.src == src_peer_id && c.dst == dst_peer_id));
                state.log(format!("🔚 Circuit closed  {} → {}",
                    AppState::short(&src_peer_id.to_string()),
                    AppState::short(&dst_peer_id.to_string())));
            }
            _ => {}
        },
        SwarmEvent::Behaviour(ServerBehaviourEvent::Identify(
            identify::Event::Received { peer_id, info, .. }
        )) => {
            // Detect peer type from protocol strings
            let peer_type = if info.protocols.iter().any(|p| p.as_ref().contains("relay")) {
                "relay"
            } else if info.protocols.iter().any(|p| p.as_ref().contains("metaverse-server")) {
                "server"
            } else {
                "client"
            };
            if let Some(entry) = state.connected_peers.get_mut(&peer_id) {
                entry.2 = peer_type.to_string();
            }
            for addr in info.listen_addrs {
                actions.push(SwarmAction::AddKadAddress(peer_id, addr));
            }
            actions.push(SwarmAction::RefreshDhtCount);
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
            for (peer_id, addr) in peers {
                state.log(format!("🔍 mDNS  {}", AppState::short(&peer_id.to_string())));
                actions.push(SwarmAction::AddKadAddress(peer_id, addr.clone()));
                actions.push(SwarmAction::DialPeer(peer_id, addr));
            }
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::Gossipsub(gossipsub::Event::Message {
            propagation_source: _src,
            message,
            ..
        })) => {
            state.net.gossip_msgs_in += 1;
            state.net.bytes_in += message.data.len() as u64;
            let topic = message.topic.as_str();
            if topic.contains("state-request") {
                state.net.state_requests_in += 1;
                if let Ok(req) = metaverse_core::messages::ChunkStateRequest::from_bytes(&message.data) {
                    state.pending_chunk_requests.push(req);
                }
            } else if topic == "voxel-ops" || topic.starts_with("voxel-ops-") {
                if let Ok(op) = bincode::deserialize::<metaverse_core::messages::SignedOperation>(&message.data) {
                    state.pending_voxel_ops.push(op);
                }
            } else if topic == "key-registry" {
                // Deserialise and store incoming key records into SQLite.
                // We check self_sig on every record before persisting.
                use metaverse_core::key_registry::KeyRegistryMessage;
                use metaverse_core::identity::KeyRecord;
                if let Ok(msg) = bincode::deserialize::<KeyRegistryMessage>(&message.data) {
                    match msg {
                        KeyRegistryMessage::Publish(r)  => {
                            if let Ok(s) = state.shared.read() {
                                if let Some(ref db) = s.key_db {
                                    if r.verify_self_sig() {
                                        let peer_id   = r.peer_id.to_string();
                                        let key_type  = format!("{}", r.key_type);
                                        let disp_name = r.display_name.as_deref();
                                        let bytes     = bincode::serialize(&r).unwrap_or_default();
                                        db.upsert(&peer_id, &bytes, &key_type, disp_name,
                                                  r.created_at as i64, r.updated_at as i64,
                                                  r.revoked);
                                    }
                                }
                            }
                        }
                        KeyRegistryMessage::Batch(rs) => {
                            let mut stored = 0usize;
                            if let Ok(s) = state.shared.read() {
                                if let Some(ref db) = s.key_db {
                                    for rec in &rs {
                                        if rec.verify_self_sig() {
                                            let peer_id   = rec.peer_id.to_string();
                                            let key_type  = format!("{}", rec.key_type);
                                            let disp_name = rec.display_name.as_deref();
                                            let bytes     = bincode::serialize(rec).unwrap_or_default();
                                            db.upsert(&peer_id, &bytes, &key_type, disp_name,
                                                      rec.created_at as i64, rec.updated_at as i64,
                                                      rec.revoked);
                                            stored += 1;
                                        }
                                    }
                                }
                            }
                            if stored > 0 {
                                state.log(format!("🔑 Key registry: stored {} record(s) from batch", stored));
                            }
                        }
                        KeyRegistryMessage::Revocation { target_peer_id_bytes, revoker_peer_id_bytes, reason, revoked_at_ms, .. } => {
                            // Apply server-side: update SQLite revocation columns.
                            if let (Ok(target_pid), Ok(revoker_pid)) = (
                                libp2p::PeerId::from_bytes(&target_peer_id_bytes),
                                libp2p::PeerId::from_bytes(&revoker_peer_id_bytes),
                            ) {
                                let updated = state.shared.read().ok()
                                    .and_then(|s| s.key_db.as_ref().map(|db| {
                                        db.revoke_key_in_db(
                                            &target_pid.to_string(),
                                            &revoker_pid.to_string(),
                                            reason.as_deref(),
                                            revoked_at_ms as i64,
                                        )
                                    }))
                                    .unwrap_or(false);
                                if updated {
                                    state.log(format!("🔑 [Revocation] Revoked {} (by {})",
                                        short_id(&target_pid.to_string()),
                                        short_id(&revoker_pid.to_string())));
                                }
                            }
                        }
                    }
                }
            }
        }
        // A peer subscribed to a topic — mirror that subscription so the server stays in
        // the gossipsub mesh and can relay messages for dynamic chunk/region topics.
        SwarmEvent::Behaviour(ServerBehaviourEvent::Gossipsub(gossipsub::Event::Subscribed {
            peer_id: _,
            topic,
        })) => {
            actions.push(SwarmAction::SubscribeTopic(topic.as_str().to_string()));
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::Kademlia(
            kad::Event::RoutingUpdated { peer, .. }
        )) => {
            state.dht_peer_count = 0; // will be refreshed via RefreshDhtCount
            actions.push(SwarmAction::AddKadAddress(peer, Multiaddr::empty()));
            actions.push(SwarmAction::RefreshDhtCount);
        }
        _ => {}
    }
    actions
}

fn apply_swarm_actions(
    actions: Vec<SwarmAction>,
    state: &mut AppState,
    swarm: &mut libp2p::Swarm<ServerBehaviour>,
) {
    for action in actions {
        match action {
            SwarmAction::AddKadAddress(peer_id, addr) => {
                if addr != Multiaddr::empty() {
                    swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                }
            }
            SwarmAction::RefreshDhtCount => {
                state.dht_peer_count = swarm.behaviour_mut()
                    .kademlia.kbuckets().map(|b| b.num_entries()).sum();
            }
            SwarmAction::DialPeer(peer_id, addr) => {
                if !swarm.is_connected(&peer_id) {
                    let _ = swarm.dial(addr);
                }
            }
            SwarmAction::SubscribeTopic(topic_str) => {
                let topic = gossipsub::IdentTopic::new(&topic_str);
                // subscribe() is idempotent — no-op if already subscribed
                let _ = swarm.behaviour_mut().gossipsub.subscribe(&topic);
            }
            SwarmAction::PublishGossip { topic, data } => {
                let t = gossipsub::IdentTopic::new(&topic);
                if let Err(e) = swarm.behaviour_mut().gossipsub.publish(t, data) {
                    eprintln!("⚠️  [gossip] Failed to publish on '{}': {:?}", topic, e);
                }
            }
            SwarmAction::StartProviding(key) => {
                use libp2p::kad::RecordKey;
                if let Err(e) = swarm.behaviour_mut().kademlia.start_providing(RecordKey::new(&key)) {
                    eprintln!("⚠️  [DHT] start_providing failed: {:?}", e);
                }
            }
            SwarmAction::PutDhtRecord { key, value } => {
                use libp2p::kad::{Record, RecordKey, Quorum};
                let record = Record {
                    key: RecordKey::new(&key),
                    value,
                    publisher: None,
                    expires: None,
                };
                if let Err(e) = swarm.behaviour_mut().kademlia.put_record(record, Quorum::One) {
                    eprintln!("⚠️  [DHT] put_record failed: {:?}", e);
                }
            }
            SwarmAction::DisconnectPeer(peer_id) => {
                let _ = swarm.disconnect_peer_id(peer_id);
                state.log(format!("🔌 [LoadShed] Disconnected {} to relieve load", peer_id));
            }
        }
    }
}

fn short_addr(addr: &str) -> String {
    if addr.len() > 40 { format!("…{}", &addr[addr.len()-38..]) } else { addr.to_string() }
}

// ─── TUI: single htop-style dashboard ────────────────────────────────────────

/// Entry point for all TUI rendering — one screen, no tabs.
fn draw(frame: &mut Frame, state: &AppState, world: &WorldStats) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Min(6),     // body
            Constraint::Length(1),  // footer hint
        ])
        .split(area);

    draw_header(frame, state, world, rows[0]);

    // Body: left (Network) | right (World)
    let body_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    let left_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body_cols[0]);

    let right_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(body_cols[1]);

    draw_system(frame, state, left_rows[0]);
    draw_network(frame, state, left_rows[1]);
    draw_world(frame, world, state, right_rows[0]);
    draw_node_info(frame, state, right_rows[1]);

    // Footer
    let footer = format!(
        " [q] Quit   Web: http://{}:{}   Log: server.log ",
        state.public_ip, state.config.web_port,
    );
    frame.render_widget(
        Paragraph::new(footer)
            .style(Style::default().fg(Color::DarkGray)),
        rows[2],
    );
}

fn draw_header(frame: &mut Frame, state: &AppState, world: &WorldStats, area: Rect) {
    let e = state.start_time.elapsed().as_secs();
    let name = state.config.node_name.as_deref().unwrap_or("server");
    let short_id = &state.local_peer_id[state.local_peer_id.len().saturating_sub(12)..];
    let shedding = if state.shedding_relay { "  ⚠️ SHEDDING" } else { "" };
    let text = format!(
        " 🌍 {}  │  {}  │  peers:{}  circuits:{}  chunks:{}  │  ⏱ {:02}h{:02}m{:02}s{}",
        name, short_id,
        state.connected_peers.len(), state.active_circuits.len(), world.chunks_loaded,
        e / 3600, (e % 3600) / 60, e % 60, shedding,
    );
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn bar(filled: u8, width: u8) -> String {
    let n = (filled as usize * width as usize / 100).min(width as usize);
    format!("[{}{}]", "█".repeat(n), "░".repeat(width as usize - n))
}

fn draw_system(frame: &mut Frame, state: &AppState, area: Rect) {
    let cpu_pct = state.cpu_pct as u8;
    let ram_pct = if state.ram_total_mb > 0 { (state.ram_used_mb * 100 / state.ram_total_mb) as u8 } else { 0 };

    let cpu_style = if cpu_pct > 80 { Style::default().fg(Color::Red) }
        else if cpu_pct > 50 { Style::default().fg(Color::Yellow) }
        else { Style::default().fg(Color::Green) };
    let ram_style = if ram_pct > 85 { Style::default().fg(Color::Red) }
        else if ram_pct > 70 { Style::default().fg(Color::Yellow) }
        else { Style::default().fg(Color::Green) };

    let disk_pct: u8 = if state.config.max_world_data_gb > 0 {
        let used_gb = state.shared.read().map(|s| s.world.world_data_mb).unwrap_or(0.0) / 1024.0;
        ((used_gb / state.config.max_world_data_gb as f64) * 100.0).min(100.0) as u8
    } else { 0 };
    let disk_style = if disk_pct > 90 { Style::default().fg(Color::Red) }
        else if disk_pct > 75 { Style::default().fg(Color::Yellow) }
        else { Style::default().fg(Color::Cyan) };

    let lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(format!("CPU  {} ", bar(cpu_pct, 20)), cpu_style),
            Span::styled(format!("{:3}%", cpu_pct), cpu_style.add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled(format!("RAM  {} ", bar(ram_pct, 20)), ram_style),
            Span::styled(format!("{:3}%  {}/{} MB", ram_pct, state.ram_used_mb, state.ram_total_mb), ram_style),
        ]),
        Line::from(vec![
            Span::styled(format!("DSK  {} ", bar(disk_pct, 20)), disk_style),
            Span::styled(
                if state.config.max_world_data_gb > 0 {
                    format!("{:3}%  {} GB limit", disk_pct, state.config.max_world_data_gb)
                } else {
                    "  no limit".to_string()
                },
                disk_style,
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" System ").title_style(Style::default().fg(Color::Cyan)).borders(Borders::ALL)),
        area,
    );
}

fn draw_network(frame: &mut Frame, state: &AppState, area: Rect) {
    let ws_port = state.config.ws_port.unwrap_or(state.config.port + 5000);
    let items: Vec<Line> = vec![
        stat_line("Peers:      ", format!("{}", state.connected_peers.len())),
        stat_line("Circuits:   ", format!("{}", state.active_circuits.len())),
        stat_line("Total conn: ", format!("{}", state.total_connections)),
        stat_line("DHT peers:  ", format!("{}", state.dht_peer_count)),
        stat_line("Traffic in: ", fmt_bytes(state.net.bytes_in)),
        stat_line("Traffic out:", fmt_bytes(state.net.bytes_out)),
        stat_line("TCP port:   ", format!("{}", state.config.port)),
        stat_line("WS port:    ", format!("{}", ws_port)),
    ];
    frame.render_widget(
        Paragraph::new(items)
            .block(Block::default().title(" Network ").title_style(Style::default().fg(Color::Cyan)).borders(Borders::ALL)),
        area,
    );
}

fn draw_world(frame: &mut Frame, world: &WorldStats, state: &AppState, area: Rect) {
    let save_str = if world.last_save_secs_ago < 3600 {
        format!("{}s ago", world.last_save_secs_ago)
    } else {
        "not yet".to_string()
    };
    let items: Vec<Line> = vec![
        stat_line("Chunks loaded: ", format!("{}", world.chunks_loaded)),
        stat_line("Chunks queued: ", format!("{}", world.chunks_queued)),
        stat_line("Voxel ops:     ", format!("{}", world.voxel_ops_total)),
        stat_line("Ops merged:    ", format!("{}", world.ops_merged_total)),
        stat_line("Data size:     ", format!("{:.1} MB", world.world_data_mb)),
        stat_line("Last save:     ", save_str),
        stat_line("Shedding:      ",
            if world.shedding_chunks { "YES ⚠️".to_string() } else { "No".to_string() }),
        stat_line("World dir:     ",
            state.config.world_dir.as_deref().unwrap_or("~/.metaverse/world_data").to_string()),
    ];
    frame.render_widget(
        Paragraph::new(items)
            .block(Block::default().title(" World ").title_style(Style::default().fg(Color::Green)).borders(Borders::ALL)),
        area,
    );
}

fn draw_node_info(frame: &mut Frame, state: &AppState, area: Rect) {
    let items: Vec<Line> = vec![
        stat_line("Node type:  ", state.config.node_type.clone()),
        stat_line("Priority:   ", format!("{}", state.config.priority_score)),
        stat_line("Max circuits:", format!("{}", state.config.max_circuits)),
        stat_line("Max peers:  ", if state.config.max_peers == 0 { "∞".to_string() } else { state.config.max_peers.to_string() }),
        stat_line("Bandwidth:  ", if state.config.max_bandwidth_mbps == 0 { "∞".to_string() } else { format!("{} MB/s", state.config.max_bandwidth_mbps) }),
        stat_line("Web:        ", format!("http://{}:{}", state.public_ip, state.config.web_port)),
        stat_line("Peer ID:    ", state.local_peer_id[state.local_peer_id.len().saturating_sub(20)..].to_string()),
        stat_line("Version:    ", env!("CARGO_PKG_VERSION").to_string()),
    ];
    frame.render_widget(
        Paragraph::new(items)
            .block(Block::default().title(" Node ").title_style(Style::default().fg(Color::Green)).borders(Borders::ALL)),
        area,
    );
}

fn stat_line(label: impl Into<String>, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(label.into(), Style::default().fg(Color::DarkGray)),
        Span::styled(value.into(), Style::default().fg(Color::White)),
    ])
}

fn cpu_color(pct: f32) -> Style {
    if pct > 80.0 { Style::default().fg(Color::Red) }
    else if pct > 50.0 { Style::default().fg(Color::Yellow) }
    else { Style::default().fg(Color::Green) }
}

fn fmt_bytes(b: u64) -> String {
    if b < 1024 { format!("{} B", b) }
    else if b < 1_048_576 { format!("{:.1} KB", b as f64 / 1024.0) }
    else { format!("{:.1} MB", b as f64 / 1_048_576.0) }
}

// ─── API v1 ──────────────────────────────────────────────────────────────────
//
// REST API backbone used by game clients, CLI tools, other servers, and the
// meshsite.  All v1 endpoints are under /api/v1/.
//
// Auth: operator-only endpoints require an X-Auth-Token header with a token
// obtained via POST /api/v1/auth/verify.

/// Token lifetime for operator auth sessions (1 hour).
const AUTH_TOKEN_TTL_MS: u64 = 60 * 60 * 1_000;

/// Challenge nonce lifetime (5 minutes).
const CHALLENGE_TTL_MS: u64 = 5 * 60 * 1_000;

/// Generate a stateless auth token for `peer_id` that expires at `expires_at_ms`.
///
/// Token = hex(SHA-256(server_secret || "auth:" || peer_id || "|" || expires_at_as_string))
/// The server can re-derive this to verify without storing state.
fn make_auth_token(server_secret: &[u8; 32], peer_id: &str, expires_at_ms: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(server_secret);
    hasher.update(b"auth:");
    hasher.update(peer_id.as_bytes());
    hasher.update(b"|");
    hasher.update(expires_at_ms.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

/// Verify a token from an X-Auth-Token header.
///
/// Returns the authenticated peer_id on success, or an error HTTP response.
fn verify_auth_token(
    s: &SharedState,
    headers: &HeaderMap,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    // Format: "Bearer <peer_id>:<expires_at_ms>:<token_hex>"
    let raw = headers.get("X-Auth-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let bearer = raw.strip_prefix("Bearer ").unwrap_or(raw);
    let parts: Vec<&str> = bearer.splitn(3, ':').collect();
    if parts.len() != 3 {
        return Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "missing or malformed X-Auth-Token"
        }))));
    }
    let (peer_id, expires_str, provided_token) = (parts[0], parts[1], parts[2]);
    let expires_at_ms: u64 = expires_str.parse().unwrap_or(0);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if now_ms > expires_at_ms {
        return Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "token expired"
        }))));
    }
    let expected = make_auth_token(&s.server_secret, peer_id, expires_at_ms);
    if expected != provided_token {
        return Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "invalid token"
        }))));
    }
    Ok(peer_id.to_string())
}

// ── POST /api/v1/keys ─────────────────────────────────────────────────────────

/// Body for POST /api/v1/keys (JSON variant).
#[derive(Deserialize)]
struct PostKeyBody {
    /// Base64-encoded bincode `KeyRecord` bytes.
    record_b64: String,
}

/// POST /api/v1/keys — submit a `KeyRecord` for storage and propagation.
///
/// Accepts `Content-Type: application/octet-stream` (raw bincode bytes) or
/// `Content-Type: application/json` with a `{ "record_b64": "<base64>" }` body.
///
/// Validates the self-signature on the record before accepting it.
/// On success stores in SQLite, broadcasts on the key-registry gossipsub topic,
/// and returns `201 Created` with the accepted record as JSON.
async fn api_v1_post_keys(
    State(s): State<WebState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    use metaverse_core::identity::KeyRecord;
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

    // Parse body — raw bincode or JSON wrapper
    let record_bytes: Vec<u8> = {
        let ct = headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("");
        if ct.contains("application/json") {
            match serde_json::from_slice::<PostKeyBody>(&body) {
                Ok(j) => match BASE64.decode(&j.record_b64) {
                    Ok(b) => b,
                    Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                        "error": format!("invalid base64: {}", e)
                    }))).into_response(),
                },
                Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                    "error": format!("invalid JSON: {}", e)
                }))).into_response(),
            }
        } else {
            body.to_vec()
        }
    };

    // Deserialise and validate
    let record = match KeyRecord::from_bytes(&record_bytes) {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": format!("failed to deserialize KeyRecord: {}", e)
        }))).into_response(),
    };

    if !record.verify_self_sig() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "self-signature verification failed"
        }))).into_response();
    }

    let peer_id_str  = record.peer_id.to_string();
    let key_type_str = format!("{:?}", record.key_type);
    let display_name = record.display_name.clone();
    let created_at   = record.created_at as i64;
    let updated_at   = record.updated_at as i64;
    let revoked      = record.revoked;

    // Store in SQLite
    {
        let st = s.read().unwrap();
        if let Some(ref db) = st.key_db {
            db.upsert(
                &peer_id_str, &record_bytes,
                &key_type_str, display_name.as_deref(),
                created_at, updated_at, revoked,
            );
        }
        // Broadcast via gossipsub (best-effort — may fail if no peers yet)
        if let Some(ref tx) = st.gossip_tx {
            let _ = tx.try_send(GossipCommand::Publish {
                topic: "key-registry".to_string(),
                data: record_bytes.clone(),
            });
        }
    }

    (StatusCode::CREATED, Json(serde_json::json!({
        "accepted": true,
        "peer_id":      peer_id_str,
        "key_type":     key_type_str,
        "display_name": display_name,
        "created_at":   created_at,
        "updated_at":   updated_at,
        "revoked":      revoked,
    }))).into_response()
}

// ── POST /api/v1/auth/challenge ───────────────────────────────────────────────

#[derive(Deserialize)]
struct ChallengeRequest {
    /// The peer_id of the client requesting an auth token.
    peer_id: String,
}

/// POST /api/v1/auth/challenge — issue a one-time nonce for the client to sign.
///
/// The caller must have a `KeyRecord` already registered with this server.
/// Returns `{ nonce: hex, server_peer_id: str, expires_at: u64 }`.
/// The client must POST /api/v1/auth/verify within 5 minutes.
async fn api_v1_auth_challenge(
    State(s): State<WebState>,
    Json(req): Json<ChallengeRequest>,
) -> impl IntoResponse {
    // Verify the peer has a registered key
    let exists = {
        let st = s.read().unwrap();
        st.key_db.as_ref().map(|db| db.get_bytes(&req.peer_id).is_some()).unwrap_or(false)
    };
    if !exists {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "no key record found for this peer_id — register a key first"
        }))).into_response();
    }

    // Generate 32-byte nonce
    let mut nonce = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let nonce_hex = hex::encode(nonce);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let expires_at = now_ms + CHALLENGE_TTL_MS;

    // Store challenge
    {
        let st = s.read().unwrap();
        let mut map = st.pending_challenges.lock().unwrap();
        // Prune stale challenges to avoid unbounded growth
        map.retain(|_, (exp, _)| *exp > now_ms);
        map.insert(nonce_hex.clone(), (expires_at, req.peer_id.clone()));
    }

    let server_peer_id = s.read().unwrap().local_peer_id.clone();
    (StatusCode::OK, Json(serde_json::json!({
        "nonce":          nonce_hex,
        "server_peer_id": server_peer_id,
        "expires_at":     expires_at,
        "instructions":   "Sign the nonce bytes with your Ed25519 key and POST to /api/v1/auth/verify",
    }))).into_response()
}

// ── POST /api/v1/auth/verify ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct VerifyRequest {
    /// The nonce from /api/v1/auth/challenge (hex).
    nonce: String,
    /// The peer_id that is authenticating.
    peer_id: String,
    /// Ed25519 signature over the raw nonce bytes (hex).
    signature: String,
}

/// POST /api/v1/auth/verify — verify the signed challenge and return a bearer token.
///
/// The token format is: `<peer_id>:<expires_at_ms>:<hex_token>`
/// Include as `Authorization: Bearer <token>` on operator endpoints.
async fn api_v1_auth_verify(
    State(s): State<WebState>,
    Json(req): Json<VerifyRequest>,
) -> impl IntoResponse {
    use ed25519_dalek::{VerifyingKey, Signature};
    use ed25519_dalek::Verifier;
    use metaverse_core::identity::KeyRecord;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Look up and consume the challenge
    let (challenge_expires, stored_peer_id) = {
        let st = s.read().unwrap();
        let mut map = st.pending_challenges.lock().unwrap();
        match map.remove(&req.nonce) {
            Some(v) => v,
            None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "nonce not found or already used"
            }))).into_response(),
        }
    };
    if now_ms > challenge_expires {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "challenge expired"
        }))).into_response();
    }
    if stored_peer_id != req.peer_id {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "peer_id mismatch"
        }))).into_response();
    }

    // Fetch the client's KeyRecord to get their public key
    let key_bytes: Vec<u8> = {
        let st = s.read().unwrap();
        match st.key_db.as_ref().and_then(|db| db.get_bytes(&req.peer_id)) {
            Some(b) => b,
            None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "no key record found for this peer_id"
            }))).into_response(),
        }
    };
    let record = match KeyRecord::from_bytes(&key_bytes) {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("failed to deserialize stored KeyRecord: {}", e)
        }))).into_response(),
    };

    // Decode the nonce and signature
    let nonce_bytes = match hex::decode(&req.nonce) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": format!("invalid nonce hex: {}", e)
        }))).into_response(),
    };
    let sig_bytes = match hex::decode(&req.signature) {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": format!("invalid signature hex: {}", e)
        }))).into_response(),
    };

    // Verify Ed25519 signature
    let vk = match VerifyingKey::from_bytes(&record.public_key) {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "invalid public key in key record"
        }))).into_response(),
    };
    let sig = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "invalid signature bytes (must be 64 bytes)"
        }))).into_response(),
    };
    if vk.verify(&nonce_bytes, &sig).is_err() {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "signature verification failed"
        }))).into_response();
    }

    // Generate token
    let token_expires_at = now_ms + AUTH_TOKEN_TTL_MS;
    let server_secret = s.read().unwrap().server_secret;
    let token_hex = make_auth_token(&server_secret, &req.peer_id, token_expires_at);
    let bearer = format!("{}:{}:{}", req.peer_id, token_expires_at, token_hex);

    (StatusCode::OK, Json(serde_json::json!({
        "token":      bearer,
        "peer_id":    req.peer_id,
        "expires_at": token_expires_at,
        "ttl_seconds": AUTH_TOKEN_TTL_MS / 1000,
    }))).into_response()
}

// ─── API v1: key requests ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct KeyRequestBody {
    /// peer_id of the applicant.
    peer_id: String,
    /// Key type being requested: "Relay", "Server", "Admin", "Business".
    requested_type: String,
    /// Display name to apply to the issued key.
    display_name: Option<String>,
    /// Why this key type is needed.
    justification: Option<String>,
    /// Contact email/matrix/etc. for follow-up.
    contact_info: Option<String>,
}

/// POST /api/v1/key-requests — submit a key upgrade or relay issuance request.
///
/// The applicant must have an existing Guest or Personal key registered.
/// A server operator reviews via GET /api/v1/key-requests and then
/// POST /api/v1/key-requests/{id}/approve or /deny.
async fn api_v1_post_key_request(
    State(s): State<WebState>,
    Json(req): Json<KeyRequestBody>,
) -> impl IntoResponse {
    // Peer must have an existing key record
    let exists = {
        let st = s.read().unwrap();
        st.key_db.as_ref().map(|db| db.get_bytes(&req.peer_id).is_some()).unwrap_or(false)
    };
    if !exists {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "peer_id has no registered key — submit a Guest/Personal key first via POST /api/v1/keys"
        }))).into_response();
    }

    let valid_types = ["Relay", "Server", "Admin", "Business"];
    if !valid_types.contains(&req.requested_type.as_str()) {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": "requested_type must be one of: Relay, Server, Admin, Business"
        }))).into_response();
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Generate a UUID-like ID from random bytes
    let mut id_bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut id_bytes);
    let id = format!("{}", hex::encode(id_bytes));

    {
        let st = s.read().unwrap();
        if let Some(ref db) = st.key_db {
            db.insert_key_request(
                &id, &req.peer_id, &req.requested_type,
                req.display_name.as_deref(),
                req.justification.as_deref(),
                req.contact_info.as_deref(),
                now_ms as i64,
            );
        }
    }

    (StatusCode::CREATED, Json(serde_json::json!({
        "id":             id,
        "peer_id":        req.peer_id,
        "requested_type": req.requested_type,
        "status":         "pending",
        "created_at":     now_ms,
        "message":        "Request submitted. A server operator will review it.",
    }))).into_response()
}

/// GET /api/v1/key-requests — list pending (or all) key requests.
///
/// Requires operator auth (`X-Auth-Token: Bearer <token>`).
/// Optional query param: `?status=pending|approved|denied` (default: pending).
async fn api_v1_list_key_requests(
    State(s): State<WebState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let st = s.read().unwrap();
    if let Err(e) = verify_auth_token(&st, &headers) { return e.into_response(); }
    let status_filter = params.get("status").map(|s| s.as_str()).unwrap_or("pending");
    let rows = st.key_db.as_ref().map(|db| db.list_key_requests(Some(status_filter))).unwrap_or_default();
    Json(serde_json::json!({ "requests": rows })).into_response()
}

// ── Approve / Deny ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ReviewBody {
    /// Optional note to attach (visible on download too).
    reviewer_note: Option<String>,
}

/// POST /api/v1/key-requests/{id}/approve — countersign and broadcast the approved key.
///
/// Requires operator auth.  The server countersigns the applicant's existing
/// `KeyRecord`, upgrades the `key_type` to the requested type, stores the new
/// signed record in the DB, broadcasts it on gossipsub, and returns the `.keyrec`
/// bytes as `application/octet-stream` for the operator to forward to the applicant.
async fn api_v1_approve_key_request(
    State(s): State<WebState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ReviewBody>,
) -> impl IntoResponse {
    use metaverse_core::identity::{KeyRecord, KeyType};
    use libp2p::identity;

    // Auth check
    {
        let st = s.read().unwrap();
        if let Err(e) = verify_auth_token(&st, &headers) { return e.into_response(); }
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Fetch the request and the applicant's current KeyRecord
    let (req_peer_id, req_type, req_display_name) = {
        let st = s.read().unwrap();
        let req = match st.key_db.as_ref().and_then(|db| db.get_key_request(&id)) {
            Some(r) => r,
            None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "key request not found"
            }))).into_response(),
        };
        if req["status"] != "pending" {
            return (StatusCode::CONFLICT, Json(serde_json::json!({
                "error": format!("request is already {}", req["status"])
            }))).into_response();
        }
        (
            req["peer_id"].as_str().unwrap_or("").to_string(),
            req["requested_type"].as_str().unwrap_or("").to_string(),
            req["display_name"].as_str().map(|s| s.to_string()),
        )
    };

    let key_bytes = {
        let st = s.read().unwrap();
        st.key_db.as_ref().and_then(|db| db.get_bytes(&req_peer_id))
    };
    let key_bytes = match key_bytes {
        Some(b) => b,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "applicant's KeyRecord not found"
        }))).into_response(),
    };

    let mut record = match KeyRecord::from_bytes(&key_bytes) {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("failed to deserialize applicant KeyRecord: {}", e)
        }))).into_response(),
    };

    // Map requested_type string → KeyType
    let new_key_type = match req_type.as_str() {
        "Relay"    => KeyType::Relay,
        "Server"   => KeyType::Server,
        "Admin"    => KeyType::Admin,
        "Business" => KeyType::Business,
        other => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
            "error": format!("unknown key type '{}'", other)
        }))).into_response(),
    };

    // Upgrade the key type, apply display name if set, update timestamps
    record.key_type = new_key_type;
    if let Some(ref dn) = req_display_name { record.display_name = Some(dn.clone()); }
    record.updated_at = now_ms;
    // Set expiry: 1 year from now for infrastructure keys
    record.expires_at = Some(now_ms + 365 * 24 * 60 * 60 * 1_000);

    let server_peer_id_str = s.read().unwrap().local_peer_id.clone();
    let server_secret = s.read().unwrap().server_secret;

    // Set issuer PeerId on the record
    if let Ok(server_pid) = server_peer_id_str.parse::<libp2p::PeerId>() {
        record.issued_by = Some(server_pid);
    }
    // Sign issuer bytes with the server's ed25519 key
    {
        use ed25519_dalek::{SigningKey, Signer};
        let sk = SigningKey::from_bytes(&server_secret);
        let issuer_bytes = record.canonical_bytes_for_issuer_sig();
        let sig = sk.sign(&issuer_bytes);
        record.issuer_sig = Some(sig.to_bytes());
    }

    let result_bytes = match record.to_bytes() {
        Ok(b) => b,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": format!("failed to serialize approved KeyRecord: {}", e)
        }))).into_response(),
    };

    // Persist: update request status + upsert upgraded key record + broadcast
    {
        let st = s.read().unwrap();
        if let Some(ref db) = st.key_db {
            db.update_key_request_status(
                &id, "approved",
                body.reviewer_note.as_deref(),
                now_ms as i64,
                Some(&result_bytes),
            );
            db.upsert(
                &req_peer_id, &result_bytes,
                &format!("{:?}", record.key_type),
                record.display_name.as_deref(),
                record.created_at as i64,
                record.updated_at as i64,
                record.revoked,
            );
        }
        if let Some(ref tx) = st.gossip_tx {
            let _ = tx.try_send(GossipCommand::Publish {
                topic: "key-registry".to_string(),
                data: result_bytes.clone(),
            });
        }
    }

    // Return the signed .keyrec bytes as octet-stream (operator downloads and gives to relay)
    (
        StatusCode::OK,
        [("content-type", "application/octet-stream"),
         ("content-disposition", &format!("attachment; filename=\"{}.keyrec\"", &req_peer_id[..8]))],
        result_bytes,
    ).into_response()
}

/// POST /api/v1/key-requests/{id}/deny — reject a key request.
///
/// Requires operator auth.
async fn api_v1_deny_key_request(
    State(s): State<WebState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ReviewBody>,
) -> impl IntoResponse {
    {
        let st = s.read().unwrap();
        if let Err(e) = verify_auth_token(&st, &headers) { return e.into_response(); }
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let exists = {
        let st = s.read().unwrap();
        st.key_db.as_ref().map(|db| db.get_key_request(&id).is_some()).unwrap_or(false)
    };
    if !exists {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "key request not found" }))).into_response();
    }
    {
        let st = s.read().unwrap();
        if let Some(ref db) = st.key_db {
            db.update_key_request_status(&id, "denied", body.reviewer_note.as_deref(), now_ms as i64, None);
        }
    }
    Json(serde_json::json!({ "id": id, "status": "denied" })).into_response()
}

/// GET /api/v1/key-requests/{id}/download — download the signed .keyrec for an approved request.
///
/// No auth required — the request ID serves as the one-time download token
/// (IDs are random 32-hex-char strings, not guessable).
async fn api_v1_download_key_request(
    State(s): State<WebState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let result = {
        let st = s.read().unwrap();
        st.key_db.as_ref().and_then(|db| db.get_key_request_result(&id))
    };
    match result {
        Some(bytes) => (
            StatusCode::OK,
            [("content-type", "application/octet-stream"),
             ("content-disposition", &format!("attachment; filename=\"{}.keyrec\"", &id[..8.min(id.len())]))],
            bytes,
        ).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "no approved result found for this request ID"
        }))).into_response(),
    }
}

// ── POST /api/v1/keys/{peer_id}/revoke ────────────────────────────────────────

/// Body for `POST /api/v1/keys/{peer_id}/revoke`.
#[derive(Deserialize, Default)]
struct RevokeBody {
    reason: Option<String>,
}

/// POST /api/v1/keys/{peer_id}/revoke — revoke a key.
///
/// Auth rules:
/// - **Self-revoke**: the caller's bearer token peer_id matches `peer_id` in the path.
/// - **Operator-revoke**: the caller's key type is Admin, Server, or Genesis.
///
/// On success:
/// 1. Marks `revoked=1` in SQLite.
/// 2. Signs a [`KeyRegistryMessage::Revocation`] with the server's ed25519 key.
/// 3. Broadcasts on the `"key-revocations"` gossipsub topic so all peers update their registry.
async fn api_v1_revoke_key(
    State(s): State<WebState>,
    headers: HeaderMap,
    Path(peer_id_str): Path<String>,
    Json(body): Json<RevokeBody>,
) -> impl IntoResponse {
    use metaverse_core::key_registry::{KeyRegistryMessage, revocation_signable_bytes};
    use ed25519_dalek::{SigningKey, Signer};

    let st = s.read().unwrap();
    let auth_peer_id = match verify_auth_token(&st, &headers) {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    let db = match &st.key_db {
        Some(db) => db,
        None => return (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"key registry unavailable"}))).into_response(),
    };

    // Target key must exist
    if db.get_bytes(&peer_id_str).is_none() {
        return (StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error":"key not found"}))).into_response();
    }

    // Check authority
    let is_self = auth_peer_id == peer_id_str;
    let is_operator = if !is_self {
        matches!(db.get_key_type(&auth_peer_id).unwrap_or_default().as_str(),
            "Admin" | "Server" | "Genesis")
    } else { false };

    if !is_self && !is_operator {
        return (StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error":"insufficient privileges: must be self or Admin/Server/Genesis"}))).into_response();
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;

    if !db.revoke_key_in_db(&peer_id_str, &auth_peer_id, body.reason.as_deref(), now_ms) {
        return (StatusCode::CONFLICT,
            Json(serde_json::json!({"error":"already revoked or key not found"}))).into_response();
    }

    // Build and sign the revocation notice using the server's ed25519 key
    let server_peer_id_str = st.local_peer_id.clone();
    let server_secret = st.server_secret;
    let gossip_tx = st.gossip_tx.clone();
    drop(st);

    let Ok(target_pid) = peer_id_str.parse::<PeerId>() else {
        return (StatusCode::OK, Json(serde_json::json!({"revoked":true,"gossip":false}))).into_response();
    };
    let Ok(revoker_pid) = server_peer_id_str.parse::<PeerId>() else {
        return (StatusCode::OK, Json(serde_json::json!({"revoked":true,"gossip":false}))).into_response();
    };

    let target_bytes = target_pid.to_bytes();
    let revoker_bytes = revoker_pid.to_bytes();
    let signable = revocation_signable_bytes(
        &target_bytes, &revoker_bytes,
        body.reason.as_deref(),
        now_ms as u64,
    );
    let signing_key = SigningKey::from_bytes(&server_secret);
    let sig: [u8; 64] = signing_key.sign(&signable).to_bytes();
    let revoker_public_key: [u8; 32] = signing_key.verifying_key().to_bytes();

    let notice = KeyRegistryMessage::Revocation {
        target_peer_id_bytes: target_bytes,
        revoker_peer_id_bytes: revoker_bytes,
        reason: body.reason.clone(),
        revoked_at_ms: now_ms as u64,
        sig,
        revoker_public_key,
    };
    let gossip_sent = if let (Some(tx), Ok(data)) = (gossip_tx, bincode::serialize(&notice)) {
        tx.try_send(GossipCommand::Publish {
            topic: "key-revocations".to_string(),
            data,
        }).is_ok()
    } else { false };

    (StatusCode::OK, Json(serde_json::json!({
        "revoked": true,
        "peer_id": peer_id_str,
        "gossip_broadcast": gossip_sent,
    }))).into_response()
}

// ── GET /api/v1/sync/keys ─────────────────────────────────────────────────────

/// GET /api/v1/sync/keys?since=<unix_ms>&limit=<n>
///
/// Returns up to `limit` key records with `updated_at > since`, ordered by
/// `updated_at` ascending.  Used by peer servers for incremental sync.
/// Each record includes a `record_b64` field (base64 bincode) so the receiver
/// can import the full [`KeyRecord`] without a separate download.
async fn api_v1_sync_keys(
    State(s): State<WebState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let since: i64 = params.get("since").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: usize = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(1000).min(5000);

    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => Json(db.list_since(since, limit)).into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"key registry unavailable"}))).into_response(),
    }
}

// ── POST /api/config (hot-reload) ─────────────────────────────────────────────

/// POST /api/config — hot-reload the server configuration.
///
/// Accepts a full or partial [`ServerConfig`] JSON body.
/// **Live fields applied immediately** (no restart needed):
/// - `max_circuits`, `max_peers`, `max_ping_ms`
/// - `cpu_shed_threshold_pct`, `ram_shed_threshold_pct`
/// - `blacklist`, `whitelist`, `priority_peers`
/// - `known_servers`
/// - `log_level`, `node_name`
///
/// **Fields that require restart** (warn only, not applied):
/// - `port`, `ws_port`, `external_addr`, `web_port`, `identity_file`
///
/// Requires operator auth (`X-Auth-Token: Bearer <token>`).
async fn api_post_config(
    State(s): State<WebState>,
    headers: HeaderMap,
    Json(new_cfg): Json<ServerConfig>,
) -> impl IntoResponse {
    let st = s.read().unwrap();
    if let Err(e) = verify_auth_token(&st, &headers) { return e.into_response(); }
    let reload_tx = st.config_reload_tx.clone();
    drop(st);

    let Some(tx) = reload_tx else {
        return (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"config reload channel unavailable"}))).into_response();
    };
    match tx.try_send(new_cfg) {
        Ok(_) => (StatusCode::ACCEPTED, Json(serde_json::json!({
            "status": "accepted",
            "note": "live fields applied; port/identity changes require restart"
        }))).into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"reload channel full or closed"}))).into_response(),
    }
}

// ─── Config hot-reload helper ─────────────────────────────────────────────────

/// Spawn a background task that listens for SIGHUP and sends reloaded config into the
/// config_reload channel. On non-Unix platforms, does nothing.
fn spawn_sighup_handler(state: &AppState) {
    let reload_tx = state.shared.read().ok()
        .and_then(|s| s.config_reload_tx.clone());
    if let Some(tx) = reload_tx {
        #[cfg(unix)]
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sig = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => { eprintln!("⚠️  SIGHUP handler failed: {e}"); return; }
            };
            while sig.recv().await.is_some() {
                eprintln!("🔄 SIGHUP — reloading config from disk");
                let new_cfg = load_config();
                let _ = tx.send(new_cfg).await;
            }
        });
    }
}

/// Apply live-reloadable fields from `new_cfg` to the running server.
///
/// Called from the event loop on both SIGHUP and `POST /api/config`.
/// Port/identity changes are logged as warnings but not applied.
fn apply_config_hot_reload(state: &mut AppState, new_cfg: ServerConfig, world_config: &mut ServerConfig) {
    let mut changed = vec![];

    macro_rules! apply {
        ($field:ident) => {
            if state.config.$field != new_cfg.$field {
                changed.push(stringify!($field));
                world_config.$field = new_cfg.$field.clone();
                state.config.$field = new_cfg.$field.clone();
            }
        };
    }

    apply!(max_circuits);
    apply!(max_peers);
    apply!(max_ping_ms);
    apply!(cpu_shed_threshold_pct);
    apply!(ram_shed_threshold_pct);
    apply!(blacklist);
    apply!(whitelist);
    apply!(priority_peers);
    apply!(known_servers);
    apply!(log_level);
    apply!(node_name);
    apply!(max_bandwidth_mbps);
    apply!(max_retries);
    apply!(world_save_interval_secs);
    apply!(max_loaded_chunks);

    // Warn about fields that require restart
    if state.config.port != new_cfg.port {
        state.log(format!("⚠️  [Config] port change ({} → {}) requires restart — ignored",
            state.config.port, new_cfg.port));
    }
    if state.config.ws_port != new_cfg.ws_port {
        state.log("⚠️  [Config] ws_port change requires restart — ignored".to_string());
    }
    if state.config.web_port != new_cfg.web_port {
        state.log("⚠️  [Config] web_port change requires restart — ignored".to_string());
    }
    if state.config.identity_file != new_cfg.identity_file {
        state.log("⚠️  [Config] identity_file change requires restart — ignored".to_string());
    }

    if changed.is_empty() {
        state.log("🔄 [Config] Hot-reload: no live fields changed".to_string());
    } else {
        state.log(format!("🔄 [Config] Hot-reload applied: {}", changed.join(", ")));
    }
}

// ─── Server-to-server key sync ────────────────────────────────────────────────

/// Pull incremental key records from all `known_servers` in the config.
///
/// For each server, queries `GET /api/v1/sync/keys?since=<last_synced_at>&limit=1000`,
/// imports any new or updated records into the local SQLite DB, then updates
/// `server_sync.last_synced_at`.
///
/// This is called at startup and every 10 minutes from the event loop.
async fn sync_keys_from_servers(state: &AppState) {
    use metaverse_core::identity::KeyRecord;
    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

    let known_servers = state.config.known_servers.clone();
    if known_servers.is_empty() { return; }

    let db = match state.shared.read().ok().and_then(|s| s.key_db.clone()) {
        Some(db) => db,
        None => return,
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => { eprintln!("[ServerSync] Failed to create HTTP client: {}", e); return; }
    };

    for server_url in &known_servers {
        let last_synced = db.get_last_synced_at(server_url);
        let url = format!("{}/api/v1/sync/keys?since={}&limit=1000", server_url, last_synced);

        let response = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => { eprintln!("[ServerSync] {} returned {}", server_url, r.status()); continue; }
            Err(e) => { eprintln!("[ServerSync] {} unreachable: {}", server_url, e); continue; }
        };

        let records: Vec<serde_json::Value> = match response.json().await {
            Ok(v) => v,
            Err(e) => { eprintln!("[ServerSync] {} bad JSON: {}", server_url, e); continue; }
        };

        let count = records.len();
        let mut imported = 0i64;
        let mut newest_at: i64 = last_synced;

        for rec in &records {
            let Some(b64) = rec.get("record_b64").and_then(|v| v.as_str()) else { continue };
            let Ok(bytes) = BASE64.decode(b64) else { continue };
            let Ok(kr) = KeyRecord::from_bytes(&bytes) else { continue };
            if !kr.verify_self_sig() { continue; }
            let updated_at = rec.get("updated_at").and_then(|v| v.as_i64()).unwrap_or(0);
            if updated_at > newest_at { newest_at = updated_at; }
            let pid = kr.peer_id.to_base58();
            let ktype = format!("{:?}", kr.key_type);
            db.upsert(&pid, &bytes, &ktype, kr.display_name.as_deref(),
                kr.created_at as i64, kr.updated_at as i64, kr.revoked);
            imported += 1;
        }

        if count > 0 {
            db.update_server_sync(server_url, newest_at, imported);
            eprintln!("[ServerSync] {} — imported {}/{} records (newest_at={})",
                server_url, imported, count, newest_at);
        }
    }
}

type WebState = Arc<RwLock<SharedState>>;

async fn web_root(State(s): State<WebState>) -> Html<String> {
    let st = s.read().unwrap().clone();
    Html(render_dashboard_html(&st))
}

async fn web_api_status(State(s): State<WebState>) -> impl IntoResponse {
    let st = s.read().unwrap().clone();
    Json(st)
}

async fn web_api_peers(State(s): State<WebState>) -> impl IntoResponse {
    let peers = s.read().unwrap().peers.clone();
    Json(peers)
}

async fn web_api_config(State(s): State<WebState>) -> impl IntoResponse {
    // Return config as JSON — for now read-only (hot reload TODO)
    let st = s.read().unwrap().clone();
    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    (StatusCode::OK, headers, format!("{{\"node_name\":\"{}\",\"node_type\":\"{}\",\"priority_score\":{},\"version\":\"{}\"}}",
        st.node_name, st.node_type, 0u32, st.version))
}

async fn web_api_health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// GET /api/keys[?type=<key_type>]  — list all key records (or filter by type)
async fn web_api_keys(
    State(s): State<WebState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => {
            let filter = params.get("type").map(|s| s.as_str());
            Json(db.list(filter)).into_response()
        }
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error":"key registry not available"}))).into_response(),
    }
}

/// GET /api/keys/relays  — shortcut for relay keys
async fn web_api_keys_relays(State(s): State<WebState>) -> impl IntoResponse {
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => Json(db.list(Some("Relay"))).into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error":"key registry not available"}))).into_response(),
    }
}

/// GET /api/keys/servers  — shortcut for server keys
async fn web_api_keys_servers(State(s): State<WebState>) -> impl IntoResponse {
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => Json(db.list(Some("Server"))).into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error":"key registry not available"}))).into_response(),
    }
}

/// GET /api/keys/:peer_id  — get full key record for a specific peer
async fn web_api_key_by_id(
    State(s): State<WebState>,
    Path(peer_id): Path<String>,
) -> impl IntoResponse {
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => {
            match db.get_bytes(&peer_id) {
                Some(bytes) => {
                    // Deserialise to full KeyRecord and return as JSON
                    match bincode::deserialize::<metaverse_core::identity::KeyRecord>(&bytes) {
                        Ok(rec) => {
                            let json = serde_json::json!({
                                "peer_id":      rec.peer_id.to_string(),
                                "key_type":     format!("{}", rec.key_type),
                                "display_name": rec.display_name,
                                "bio":          rec.bio,
                                "created_at":   rec.created_at,
                                "updated_at":   rec.updated_at,
                                "expires_at":   rec.expires_at,
                                "revoked":      rec.revoked,
                                "issued_by":    rec.issued_by.map(|p| p.to_string()),
                            });
                            Json(json).into_response()
                        }
                        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR,
                                   Json(serde_json::json!({"error":"failed to deserialise record"}))).into_response(),
                    }
                }
                None => (StatusCode::NOT_FOUND,
                         Json(serde_json::json!({"error":"peer not found"}))).into_response(),
            }
        }
        None => (StatusCode::SERVICE_UNAVAILABLE,
                 Json(serde_json::json!({"error":"key registry not available"}))).into_response(),
    }
}

fn render_dashboard_html(st: &SharedState) -> String {
    let uptime_h = st.uptime_secs / 3600;
    let uptime_m = (st.uptime_secs % 3600) / 60;
    let uptime_s = st.uptime_secs % 60;
    let peer_rows = st.peers.iter().map(|p| format!(
        "<tr><td>{}</td><td><span class=\"badge badge-{}\">{}</span></td><td>{}</td><td>{}s</td></tr>",
        p.peer_id, p.peer_type, p.peer_type, p.addr, p.connected_secs
    )).collect::<Vec<_>>().join("\n");
    let cpu_bar_width = (st.cpu_pct as u32).min(100);
    let cpu_color = if st.cpu_pct > 80.0 { "#e74c3c" } else if st.cpu_pct > 50.0 { "#f39c12" } else { "#2ecc71" };
    let ram_bar_width = st.ram_pct as u32;
    let ram_color = if st.ram_pct > 85.0 { "#e74c3c" } else if st.ram_pct > 70.0 { "#f39c12" } else { "#2ecc71" };
    let shed_class = if st.shedding_relay { "warn" } else { "ok" };
    let shed_txt = if st.shedding_relay { "⚠️ Shedding" } else { "✅ Normal" };
    format!(r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="refresh" content="10">
<title>Metaverse Server — {name}</title>
<style>
  :root {{ --bg:#1a1a2e;--surface:#16213e;--accent:#0f3460;--cyan:#00d4ff;--green:#2ecc71;--red:#e74c3c;--yellow:#f39c12;--text:#eee;--dim:#888; }}
  *{{box-sizing:border-box;margin:0;padding:0}}
  body{{background:var(--bg);color:var(--text);font:14px/1.5 'Courier New',monospace;padding:16px}}
  h1{{color:var(--cyan);font-size:1.2em;margin-bottom:12px}}
  h2{{color:var(--cyan);font-size:0.95em;margin:16px 0 8px;text-transform:uppercase;letter-spacing:.05em}}
  .grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:12px;margin-bottom:16px}}
  .card{{background:var(--surface);border:1px solid var(--accent);border-radius:6px;padding:12px}}
  .stat{{display:flex;justify-content:space-between;padding:3px 0;border-bottom:1px solid var(--accent)}}
  .stat:last-child{{border:none}}
  .val{{color:var(--cyan)}}
  .ok{{color:var(--green)}}.warn{{color:var(--yellow)}}.err{{color:var(--red)}}
  .bar-wrap{{background:var(--accent);border-radius:4px;height:8px;margin-top:4px}}
  .bar{{height:8px;border-radius:4px;transition:width .5s}}
  table{{width:100%;border-collapse:collapse}}
  th{{text-align:left;color:var(--dim);padding:4px 8px;border-bottom:1px solid var(--accent)}}
  td{{padding:4px 8px;border-bottom:1px solid #1e1e3a}}
  .badge{{padding:2px 8px;border-radius:4px;font-size:.8em}}
  .badge-server{{background:#0f3460;color:var(--cyan)}}
  .badge-relay{{background:#1a3a5c;color:#74b9ff}}
  .badge-client{{background:#1a3a1a;color:var(--green)}}
  .badge-unknown{{background:#333;color:var(--dim)}}
  footer{{margin-top:20px;color:var(--dim);font-size:.8em}}
</style>
</head>
<body>
<h1>🌍 Metaverse Server — {name} <span style="color:var(--dim);font-size:.7em">v{version}</span></h1>
<div style="color:var(--dim);margin-bottom:16px">
  PeerID: {peer_id} &nbsp;|&nbsp; {ip}:{port} &nbsp;|&nbsp; Uptime: {uh:02}h{um:02}m{us:02}s &nbsp;|&nbsp; Type: {ntype}
</div>

<div class="grid">
  <div class="card">
    <h2>Network</h2>
    <div class="stat"><span>Connected peers</span><span class="val">{peers}</span></div>
    <div class="stat"><span>Active circuits</span><span class="val">{circuits}</span></div>
    <div class="stat"><span>Total connections</span><span class="val">{total_conns}</span></div>
    <div class="stat"><span>DHT peers</span><span class="val">{dht}</span></div>
    <div class="stat"><span>Registered keys</span><span class="val">{key_count}</span></div>
    <div class="stat"><span>Gossip msgs in</span><span class="val">{gmsg}</span></div>
    <div class="stat"><span>State requests</span><span class="val">{sreqs}</span></div>
    <div class="stat"><span>Relay status</span><span class="{shed_class}">{shed_txt}</span></div>
  </div>

  <div class="card">
    <h2>World</h2>
    <div class="stat"><span>Chunks loaded</span><span class="val">{chunks_loaded}</span></div>
    <div class="stat"><span>Chunks queued</span><span class="val">{chunks_queued}</span></div>
    <div class="stat"><span>Voxel ops</span><span class="val">{vops}</span></div>
    <div class="stat"><span>Ops merged</span><span class="val">{vops_merged}</span></div>
    <div class="stat"><span>Data size</span><span class="val">{data_mb:.1} MB</span></div>
    <div class="stat"><span>Last save</span><span class="val">{last_save}</span></div>
  </div>

  <div class="card">
    <h2>System</h2>
    <div class="stat"><span>CPU</span><span class="val">{cpu:.1}%</span></div>
    <div class="bar-wrap"><div class="bar" style="width:{cpu_bar}%;background:{cpu_color}"></div></div>
    <div class="stat" style="margin-top:8px"><span>RAM</span><span class="val">{ram_used}/{ram_total} MB</span></div>
    <div class="bar-wrap"><div class="bar" style="width:{ram_bar}%;background:{ram_color}"></div></div>
  </div>
</div>

<h2>Connected Peers ({peers})</h2>
<div class="card">
<table>
<tr><th>Peer ID</th><th>Type</th><th>Address</th><th>Connected</th></tr>
{peer_rows}
</table>
</div>

<footer>
  Auto-refreshes every 10s &nbsp;|&nbsp; API: <a href="/api/status" style="color:var(--cyan)">/api/status</a>
  &nbsp;|&nbsp; <a href="/api/peers" style="color:var(--cyan)">/api/peers</a>
  &nbsp;|&nbsp; <a href="/api/keys" style="color:var(--cyan)">/api/keys</a>
  &nbsp;|&nbsp; <a href="/health" style="color:var(--cyan)">/health</a>
</footer>
</body>
</html>"#,
        name = st.node_name,
        version = st.version,
        peer_id = &st.local_peer_id[st.local_peer_id.len().saturating_sub(16)..],
        ip = st.public_ip,
        port = st.relay_port,
        ntype = st.node_type,
        peers = st.peers.len(),
        circuits = st.circuit_count,
        total_conns = st.total_connections,
        dht = st.dht_peer_count,
        key_count = st.key_count,
        gmsg = st.net.gossip_msgs_in,
        sreqs = st.net.state_requests_in,
        shed_class = shed_class,
        shed_txt = shed_txt,
        chunks_loaded = st.world.chunks_loaded,
        chunks_queued = st.world.chunks_queued,
        vops = st.world.voxel_ops_total,
        vops_merged = st.world.ops_merged_total,
        data_mb = st.world.world_data_mb,
        last_save = if st.world.last_save_secs_ago < 3600 { format!("{}s ago", st.world.last_save_secs_ago) } else { "never".to_string() },
        cpu = st.cpu_pct,
        cpu_bar = cpu_bar_width,
        cpu_color = cpu_color,
        ram_used = st.ram_used_mb,
        ram_total = st.ram_total_mb,
        ram_bar = ram_bar_width,
        ram_color = ram_color,
        uh = uptime_h,
        um = uptime_m,
        us = uptime_s,
        peer_rows = peer_rows,
    )
}

// ─── Public IP detection ─────────────────────────────────────────────────────

async fn detect_public_ip() -> String {
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(_) => return "unknown".to_string(),
    };
    for url in &["https://api.ipify.org", "https://ipv4.icanhazip.com", "https://checkip.amazonaws.com"] {
        if let Ok(resp) = client.get(*url).send().await {
            if let Ok(text) = resp.text().await {
                let ip = text.trim().to_string();
                if ip.split('.').count() == 4 && ip.chars().all(|c| c.is_ascii_digit() || c == '.') {
                    return ip;
                }
            }
        }
    }
    "unknown".to_string()
}

// ─── World state integration ─────────────────────────────────────────────────
// The server holds world state for serving to clients on-demand.
// It does NOT proactively load terrain — chunks are loaded when clients
// request them. The ChunkStreamer (player-centric eager loader) belongs
// in the game client, not the server.

struct WorldSystems {
    chunk_manager: metaverse_core::chunk_manager::ChunkManager,
    user_content: std::sync::Arc<std::sync::Mutex<metaverse_core::user_content::UserContentLayer>>,
    world_dir: PathBuf,
    last_save: Instant,
    stats: WorldStats,
    ops_merged: u64,
}

impl WorldSystems {
    fn new(config: &ServerConfig) -> Option<Self> {
        use metaverse_core::{
            chunk_manager::ChunkManager,
            coordinates::GPS,
            elevation::ElevationPipeline,
            terrain::TerrainGenerator,
            user_content::UserContentLayer,
            voxel::VoxelCoord,
        };

        let world_dir = config.world_dir.as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".metaverse").join("world_data"));
        std::fs::create_dir_all(&world_dir).ok()?;

        // Load persisted user ops from disk
        let user_content = std::sync::Arc::new(std::sync::Mutex::new(UserContentLayer::new()));
        {
            let mut uc = user_content.lock().unwrap();
            let chunks_dir = world_dir.join("chunks");
            if chunks_dir.exists() {
                let mut ids = vec![];
                if let Ok(entries) = std::fs::read_dir(&chunks_dir) {
                    for e in entries.flatten() {
                        let name = e.file_name();
                        let s = name.to_string_lossy();
                        let parts: Vec<&str> = s.split('_').collect();
                        if parts.len() == 4 && parts[0] == "chunk" {
                            if let (Ok(x), Ok(y), Ok(z)) = (
                                parts[1].parse::<i64>(),
                                parts[2].parse::<i64>(),
                                parts[3].parse::<i64>(),
                            ) {
                                ids.push(metaverse_core::chunk::ChunkId { x, y, z });
                            }
                        }
                    }
                }
                if !ids.is_empty() {
                    let _ = uc.load_chunks(&world_dir, &ids);
                }
            }
        }

        // TerrainGenerator needed by ChunkManager for on-demand generation when a
        // client requests a chunk that hasn't been persisted yet.
        let origin_gps = GPS::new(-27.3996, 153.1871, 2.0);
        let origin_ecef = origin_gps.to_ecef();
        let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
        let elevation = ElevationPipeline::new(); // no API key on server by default
        let terrain_gen = TerrainGenerator::new(elevation, origin_gps, origin_voxel);

        let chunk_manager = ChunkManager::new(terrain_gen, user_content.lock().unwrap().clone());

        Some(WorldSystems {
            chunk_manager,
            user_content,
            world_dir,
            last_save: Instant::now(),
            stats: WorldStats::default(),
            ops_merged: 0,
        })
    }

    fn tick(&mut self, config: &ServerConfig) {
        // No proactive chunk loading — server only loads chunks on client request.
        let data_mb = world_data_size_mb(&self.world_dir);
        let max_mb = if config.max_world_data_gb == 0 { f64::INFINITY } else { config.max_world_data_gb as f64 * 1024.0 };
        let shedding = data_mb > max_mb * 0.95;
        let total_ops = self.user_content.lock().unwrap().op_count() as u64;

        self.stats = WorldStats {
            chunks_loaded: self.chunk_manager.loaded_count(),
            chunks_queued: 0,
            chunks_loading: 0,
            world_data_mb: data_mb,
            voxel_ops_total: total_ops,
            ops_merged_total: self.ops_merged,
            last_save_secs_ago: self.last_save.elapsed().as_secs(),
            shedding_chunks: shedding,
        };

        // Periodic save
        if self.last_save.elapsed().as_secs() >= config.world_save_interval_secs {
            if let Err(e) = self.user_content.lock().unwrap().save_chunks(&self.world_dir) {
                eprintln!("⚠️  World save failed: {}", e);
            }
            self.last_save = Instant::now();
        }
    }

    fn shutdown(&mut self) {
        let _ = self.user_content.lock().unwrap().save_chunks(&self.world_dir);
    }
}

// ─── NodeCapabilities advertisement ──────────────────────────────────────────

fn publish_node_capabilities(
    peer_id_str: &str,
    config: &ServerConfig,
    swarm: &mut libp2p::Swarm<ServerBehaviour>,
) {
    use metaverse_core::node_capabilities::NodeCapabilities;
    use libp2p::kad::{Record, RecordKey, Quorum};

    let caps = NodeCapabilities::for_server(config.max_world_data_gb as u64, config.always_on);
    let key = NodeCapabilities::dht_key(peer_id_str);
    let value = caps.to_bytes();
    let record = Record {
        key: RecordKey::new(&key),
        value,
        publisher: None,
        expires: None,
    };
    if let Err(e) = swarm.behaviour_mut().kademlia.put_record(record, Quorum::One) {
        eprintln!("⚠️  [DHT] NodeCapabilities publish failed: {:?}", e);
    }
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Load config (custom path if specified)
    let mut config = if let Some(ref path) = args.config {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str::<ServerConfig>(&text)?
    } else {
        load_config()
    };
    apply_cli_overrides(&mut config, &args);
    write_default_config_if_missing();

    // Headless if flag/config or not a terminal
    let headless = config.headless || !io::stdout().is_terminal();
    config.headless = headless;

    // Identity
    let identity_path = config.identity_file.as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".metaverse").join("server.key"));
    std::fs::create_dir_all(identity_path.parent().unwrap())?;

    let local_key = if config.temp_identity {
        println!("🔑 Using temporary identity");
        identity::Keypair::generate_ed25519()
    } else if identity_path.exists() {
        identity::Keypair::from_protobuf_encoding(&std::fs::read(&identity_path)?)?
    } else {
        let kp = identity::Keypair::generate_ed25519();
        std::fs::write(&identity_path, kp.to_protobuf_encoding()?)?;
        println!("🔑 Generated new identity at {}", identity_path.display());
        kp
    };
    let local_peer_id = local_key.public().to_peer_id();
    println!("🔑 Peer ID: {}", local_peer_id);

    // Public IP
    let public_ip = if let Some(ref addr) = config.external_addr {
        addr.split('/').find(|s| s.parse::<std::net::Ipv4Addr>().is_ok())
            .unwrap_or("?").to_string()
    } else {
        print!("🌐 Detecting public IP... ");
        let ip = detect_public_ip().await;
        println!("{}", ip);
        ip
    };

    // Build swarm — relay + gossipsub
    let max_circuits = config.max_circuits;
    let max_circuit_duration = Duration::from_secs(config.max_circuit_duration_secs);
    let max_circuit_bytes = config.max_circuit_bytes;

    let mut swarm = SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_tcp(libp2p::tcp::Config::default(), libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_dns()?
        .with_websocket(
            (libp2p::tls::Config::new, libp2p::noise::Config::new),
            libp2p::yamux::Config::default,
        ).await?
        .with_behaviour(|key: &identity::Keypair| {
            let peer_id = key.public().to_peer_id();

            // Kademlia
            let mut kad_config = kad::Config::default();
            kad_config.set_query_timeout(Duration::from_secs(60));
            let mut kademlia = kad::Behaviour::with_config(
                peer_id, MemoryStore::new(peer_id), kad_config,
            );
            kademlia.set_mode(Some(kad::Mode::Server));

            // Identify — advertise "metaverse-server" protocol so peers can detect node type
            let identify = identify::Behaviour::new(
                identify::Config::new("/metaverse-server/1.0.0".to_string(), key.public())
                    .with_push_listen_addr_updates(true),
            );

            // Gossipsub — for world data sync topics
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(1))
                .validation_mode(gossipsub::ValidationMode::Permissive)
                .max_transmit_size(2 * 1024 * 1024) // 2 MB
                .build()
                .expect("valid gossipsub config");
            let mut gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config,
            ).expect("valid gossipsub");

            // Subscribe to world data topics
            for topic in &[
                "player-state", "voxel-ops", "chat",
                "state-request", "state-response",
                "chunk-terrain", "chunk-manifest",
            ] {
                let t = gossipsub::IdentTopic::new(*topic);
                let _ = gossipsub.subscribe(&t);
            }

            Ok(ServerBehaviour {
                relay: relay::Behaviour::new(peer_id, relay::Config {
                    max_reservations: max_circuits,
                    max_circuits,
                    max_circuit_duration,
                    max_circuit_bytes,
                    ..Default::default()
                }),
                ping: libp2p::ping::Behaviour::new(libp2p::ping::Config::new()),
                kademlia,
                identify,
                mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)
                    .expect("mDNS init failed"),
                gossipsub,
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let ws_port = config.ws_port.unwrap_or(config.port + 5000);
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}", config.port).parse()?)?;
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}/ws", ws_port).parse()?)?;

    // Add external addresses
    let ext_tcp = config.external_addr.clone()
        .unwrap_or_else(|| format!("/ip4/{}/tcp/{}", public_ip, config.port));
    let ext_ws = format!("/ip4/{}/tcp/{}/ws", public_ip, ws_port);
    if let Ok(a) = ext_tcp.parse::<Multiaddr>() { swarm.add_external_address(a); }
    if let Ok(a) = ext_ws.parse::<Multiaddr>()  { swarm.add_external_address(a); }

    // Dial peer relays
    for peer_addr in config.peers.clone() {
        if let Ok(addr) = peer_addr.parse::<Multiaddr>() {
            if let Some(libp2p::multiaddr::Protocol::P2p(pid)) = addr.iter().last() {
                swarm.behaviour_mut().kademlia.add_address(&pid, addr.clone());
            }
            match swarm.dial(addr) {
                Ok(()) => println!("🔗 Dialing peer: {}", peer_addr),
                Err(e) => eprintln!("✗  {}: {}", peer_addr, e),
            }
        }
    }
    if !config.peers.is_empty() { swarm.behaviour_mut().kademlia.bootstrap().ok(); }

    // Bootstrap from metaverse bootstrap file
    if let Ok(data) = std::fs::read_to_string("bootstrap.json") {
        #[derive(serde::Deserialize)]
        struct BootstrapFile { bootstrap_nodes: Vec<BootstrapNodeFile> }
        #[derive(serde::Deserialize)]
        struct BootstrapNodeFile { multiaddr: String }
        if let Ok(bf) = serde_json::from_str::<BootstrapFile>(&data) {
            for n in bf.bootstrap_nodes {
                if let Ok(addr) = n.multiaddr.parse::<Multiaddr>() {
                    let _ = swarm.dial(addr);
                }
            }
        }
    }

    // Key registry database
    let key_db = {
        let db_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".metaverse")
            .join("key_registry.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).ok();
        match KeyDatabase::open(&db_path) {
            Ok(db) => {
                println!("🗄️  Key registry DB: {} ({} records)", db_path.display(), db.count());
                Some(db)
            }
            Err(e) => {
                eprintln!("⚠️  Key registry DB failed to open: {}", e);
                None
            }
        }
    };

    // Extract server secret key bytes for stateless auth token generation.
    // We use the first 32 bytes of the ed25519 signing key (the secret scalar).
    let server_secret: [u8; 32] = {
        if let Ok(ed) = local_key.clone().try_into_ed25519() {
            let sk = ed.secret();
            let bytes: &[u8] = sk.as_ref();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes[..32]);
            arr
        } else {
            // Fallback: hash of the protobuf encoding
            let mut h = Sha256::new();
            h.update(&local_key.to_protobuf_encoding().unwrap_or_default());
            h.finalize().into()
        }
    };

    // Channel for web handlers to push gossipsub publishes into the event loop.
    let (gossip_tx, gossip_rx) = tokio::sync::mpsc::channel::<GossipCommand>(256);
    // Channel for web handlers / SIGHUP to trigger config hot-reloads.
    let (config_reload_tx, config_reload_rx) = tokio::sync::mpsc::channel::<ServerConfig>(8);

    // Shared state for web server
    let shared = Arc::new(RwLock::new(SharedState {
        local_peer_id: local_peer_id.to_string(),
        public_ip: public_ip.clone(),
        node_name: config.node_name.clone().unwrap_or_else(|| "server".to_string()),
        node_type: config.node_type.clone(),
        relay_port: config.port,
        key_db,
        server_secret,
        gossip_tx: Some(gossip_tx),
        config_reload_tx: Some(config_reload_tx),
        ..Default::default()
    }));

    // World systems
    let world_enabled = config.world_enabled;
    let mut world = if world_enabled {
        println!("🌍 Initialising world state...");
        WorldSystems::new(&config)
    } else {
        println!("ℹ️  World disabled — relay-only mode");
        None
    };

    // Web server
    let web_enabled = config.web_enabled;
    let web_port = config.web_port;
    let web_bind = config.web_bind.clone();
    if web_enabled {
        let web_shared = Arc::clone(&shared);
        tokio::spawn(async move {
            let app = Router::new()
                .route("/", get(web_root))
                .route("/health", get(web_api_health))
                .route("/api/status", get(web_api_status))
                .route("/api/peers", get(web_api_peers))
                .route("/api/keys", get(web_api_keys))
                .route("/api/keys/relays", get(web_api_keys_relays))
                .route("/api/keys/servers", get(web_api_keys_servers))
                .route("/api/keys/:peer_id", get(web_api_key_by_id))
                // API v1 endpoints
                .route("/api/v1/keys", post(api_v1_post_keys))
                .route("/api/v1/auth/challenge", post(api_v1_auth_challenge))
                .route("/api/v1/auth/verify", post(api_v1_auth_verify))
                .route("/api/v1/key-requests", post(api_v1_post_key_request).get(api_v1_list_key_requests))
                .route("/api/v1/key-requests/:id/approve", post(api_v1_approve_key_request))
                .route("/api/v1/key-requests/:id/deny", post(api_v1_deny_key_request))
                .route("/api/v1/key-requests/:id/download", get(api_v1_download_key_request))
                .route("/api/v1/keys/:peer_id/revoke", post(api_v1_revoke_key))
                .route("/api/v1/sync/keys", get(api_v1_sync_keys))
                // Config (GET = read, POST = hot-reload)
                .route("/api/config", get(web_api_config).post(api_post_config))
                .with_state(web_shared);
            let bind_addr = format!("{}:{}", web_bind, web_port);
            println!("🌐 Web dashboard: http://{}/", bind_addr);
            if let Ok(listener) = tokio::net::TcpListener::bind(&bind_addr).await {
                let _ = axum::serve(listener, app).await;
            } else {
                eprintln!("⚠️  Web server failed to bind on {}", bind_addr);
            }
        });
    }

    let mut app_state = AppState::new(
        config, Arc::clone(&shared), local_peer_id.to_string(), public_ip, gossip_rx, config_reload_rx,
    );
    app_state.log("✅ Metaverse server started");
    if world_enabled && world.is_some() {
        app_state.log(format!("🌍 World state: {}", app_state.config.world_dir.as_deref().unwrap_or("~/.metaverse/world_data")));
    }
    // Queue DHT provider announcements for all pre-loaded chunks
    if let Some(ref w) = world {
        let uc = w.user_content.lock().unwrap();
        let chunk_ids: Vec<_> = uc.get_chunks_with_ops().into_keys().collect();
        let count = chunk_ids.len();
        for cid in chunk_ids {
            app_state.pending_dht_provide.push(cid.dht_key());
        }
        if count > 0 {
            app_state.log(format!("📡 Queued DHT announcements for {} chunk(s)", count));
        }
    }
    // Publish NodeCapabilities to DHT immediately (will re-announce every 30 min)
    publish_node_capabilities(&local_peer_id.to_string(), &app_state.config, &mut swarm);
    app_state.log(format!("📡 NodeCapabilities published (tier=server, always_on={})", app_state.config.always_on));

    if headless {
        run_headless(swarm, app_state, world).await
    } else {
        run_tui(swarm, app_state, world).await
    }
}

// ─── Headless loop ───────────────────────────────────────────────────────────

async fn run_headless(
    mut swarm: libp2p::Swarm<ServerBehaviour>,
    mut state: AppState,
    mut world: Option<WorldSystems>,
) -> Result<(), Box<dyn Error>> {
    let mut world_config = state.config.clone();
    let mut world_tick = tokio::time::interval(Duration::from_millis(50));
    let mut stats_tick = tokio::time::interval(Duration::from_secs(30));
    let mut sync_tick = tokio::time::interval(Duration::from_secs(600)); // 10-min server sync
    let mut caps_tick = tokio::time::interval(Duration::from_secs(1800)); // 30-min caps refresh
    let dummy_world = WorldStats::default();

    // Spawn a SIGHUP handler that sends the reloaded config to config_reload_rx (Unix only).
    spawn_sighup_handler(&state);

    loop {
        tokio::select! {
            _ = world_tick.tick() => {
                if let Some(ref mut w) = world {
                    w.tick(&world_config);
                    state.refresh_sys();
                    state.sync_shared(&w.stats);
                } else {
                    state.refresh_sys();
                    state.sync_shared(&dummy_world);
                }
                // Drain load-shedding disconnects
                for peer in state.drain_pending_shed() {
                    apply_swarm_actions(vec![SwarmAction::DisconnectPeer(peer)], &mut state, &mut swarm);
                }
                // Persist incoming voxel ops from peers
                {
                    let ops: Vec<_> = std::mem::take(&mut state.pending_voxel_ops);
                    if !ops.is_empty() {
                        if let Some(ref w) = world {
                            let mut uc = w.user_content.lock().unwrap();
                            let empty_local = std::collections::HashMap::new();
                            let mut stored = 0usize;
                            for op in ops {
                                if uc.apply_operation(op, &empty_local).unwrap_or(false) {
                                    stored += 1;
                                }
                            }
                            if stored > 0 {
                                state.log(format!("💾 Stored {} voxel op(s) from peers", stored));
                            }
                        }
                    }
                }
                // Respond to chunk state requests from clients
                {
                    let reqs: Vec<_> = std::mem::take(&mut state.pending_chunk_requests);
                    for req in reqs {
                        if let Some(ref w) = world {
                            let ops_map: std::collections::HashMap<_, Vec<_>> = {
                                let uc = w.user_content.lock().unwrap();
                                req.chunk_ids.iter().filter_map(|cid| {
                                    let ops: Vec<_> = uc.operations_for_chunk(cid).into_iter().cloned().collect();
                                    if ops.is_empty() { None } else { Some((*cid, ops)) }
                                }).collect()
                            };
                            if !ops_map.is_empty() {
                                let response = metaverse_core::messages::ChunkStateResponse::new(
                                    ops_map, metaverse_core::vector_clock::VectorClock::new(),
                                );
                                if let Ok(bytes) = response.to_bytes() {
                                    state.net.state_responses_out += 1;
                                    apply_swarm_actions(
                                        vec![SwarmAction::PublishGossip { topic: "state-response".to_string(), data: bytes }],
                                        &mut state, &mut swarm,
                                    );
                                }
                            }
                        }
                    }
                }
                // DHT provider announcements (startup + newly written chunks)
                {
                    let keys: Vec<_> = std::mem::take(&mut state.pending_dht_provide);
                    for key in keys {
                        apply_swarm_actions(vec![SwarmAction::StartProviding(key)], &mut state, &mut swarm);
                    }
                }
            }
            _ = stats_tick.tick() => {
                let ws = world.as_ref().map(|w| &w.stats).unwrap_or(&dummy_world);
                state.log(format!(
                    "📊 peers={} circuits={} chunks={} ops={} cpu={:.0}% ram={}MB{}",
                    state.connected_peers.len(), state.active_circuits.len(),
                    ws.chunks_loaded, ws.voxel_ops_total,
                    state.cpu_pct, state.ram_used_mb,
                    if state.shedding_relay { " ⚠️SHEDDING" } else { "" },
                ));
            }
            _ = sync_tick.tick() => {
                // Periodic server-to-server key sync
                sync_keys_from_servers(&state).await;
            }
            _ = caps_tick.tick() => {
                // Re-announce NodeCapabilities to DHT (keeps record alive, updates storage availability)
                publish_node_capabilities(&state.local_peer_id, &state.config, &mut swarm);
            }
            swarm_event = swarm.select_next_some() => {
                let actions = handle_swarm_event(swarm_event, &mut state);
                apply_swarm_actions(actions, &mut state, &mut swarm);
            }
            // Gossip commands from web handlers (best-effort, non-blocking)
            Some(cmd) = state.gossip_rx.recv() => {
                match cmd {
                    GossipCommand::Publish { topic, data } => {
                        let t = gossipsub::IdentTopic::new(&topic);
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(t, data) {
                            eprintln!("⚠️  [gossip] publish '{}': {:?}", topic, e);
                        }
                    }
                }
            }
            // Config hot-reload from web handler or SIGHUP task
            Some(new_cfg) = state.config_reload_rx.recv() => {
                apply_config_hot_reload(&mut state, new_cfg, &mut world_config);
            }
        }
    }
}

// ─── TUI loop ────────────────────────────────────────────────────────────────

async fn run_tui(
    mut swarm: libp2p::Swarm<ServerBehaviour>,
    mut state: AppState,
    mut world: Option<WorldSystems>,
) -> Result<(), Box<dyn Error>> {
    // Redirect stdout → server.log so worker println! doesn't corrupt the terminal.
    // Ratatui uses stderr, which stays clean.
    let log_path = state.config.world_dir.as_deref()
        .map(|d| format!("{}/server.log", d))
        .unwrap_or_else(|| "server.log".to_string());
    {
        let log_file = std::fs::OpenOptions::new()
            .create(true).append(true).open(&log_path)
            .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap());
        #[cfg(unix)]
        unsafe {
            use std::os::unix::io::AsRawFd;
            libc::dup2(log_file.as_raw_fd(), libc::STDOUT_FILENO);
        }
    }

    // Use stderr for ratatui — stdout is now the log file, so worker output never touches the screen.
    enable_raw_mode()?;
    execute!(io::stderr(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stderr()))?;
    let mut events = EventStream::new();
    let refresh_ms = state.config.ui.refresh_ms.max(500); // don't thrash faster than 2Hz
    let mut tui_tick = tokio::time::interval(Duration::from_millis(refresh_ms));
    let mut world_tick = tokio::time::interval(Duration::from_millis(50));
    let mut sync_tick = tokio::time::interval(Duration::from_secs(600)); // 10-min server sync
    let mut caps_tick = tokio::time::interval(Duration::from_secs(1800)); // 30-min caps refresh
    let mut world_config = state.config.clone();
    let dummy_world = WorldStats::default();

    // Spawn SIGHUP handler — sends reloaded config into config_reload_rx (Unix only).
    spawn_sighup_handler(&state);

    let result: Result<(), Box<dyn Error>> = async {
        loop {
            tokio::select! {
                _ = tui_tick.tick() => {
                    state.refresh_sys();
                    let ws = world.as_ref().map(|w| &w.stats).unwrap_or(&dummy_world);
                    terminal.draw(|f| draw(f, &state, ws))?;
                    state.sync_shared(ws);
                    // Drain load-shedding disconnects
                    for peer in state.drain_pending_shed() {
                        apply_swarm_actions(vec![SwarmAction::DisconnectPeer(peer)], &mut state, &mut swarm);
                    }
                }
                _ = world_tick.tick() => {
                    if let Some(ref mut w) = world {
                        w.tick(&world_config);
                    }
                    // Persist incoming voxel ops from peers
                    {
                        let ops: Vec<_> = std::mem::take(&mut state.pending_voxel_ops);
                        if !ops.is_empty() {
                            if let Some(ref w) = world {
                                let mut uc = w.user_content.lock().unwrap();
                                let empty_local = std::collections::HashMap::new();
                                for op in ops {
                                    let _ = uc.apply_operation(op, &empty_local);
                                }
                            }
                        }
                    }
                    // Respond to chunk state requests from clients
                    {
                        let reqs: Vec<_> = std::mem::take(&mut state.pending_chunk_requests);
                        for req in reqs {
                            if let Some(ref w) = world {
                                let ops_map: std::collections::HashMap<_, Vec<_>> = {
                                    let uc = w.user_content.lock().unwrap();
                                    req.chunk_ids.iter().filter_map(|cid| {
                                        let ops: Vec<_> = uc.operations_for_chunk(cid).into_iter().cloned().collect();
                                        if ops.is_empty() { None } else { Some((*cid, ops)) }
                                    }).collect()
                                };
                                if !ops_map.is_empty() {
                                    let response = metaverse_core::messages::ChunkStateResponse::new(
                                        ops_map, metaverse_core::vector_clock::VectorClock::new(),
                                    );
                                    if let Ok(bytes) = response.to_bytes() {
                                        state.net.state_responses_out += 1;
                                        apply_swarm_actions(
                                            vec![SwarmAction::PublishGossip { topic: "state-response".to_string(), data: bytes }],
                                            &mut state, &mut swarm,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    // DHT provider announcements
                    {
                        let keys: Vec<_> = std::mem::take(&mut state.pending_dht_provide);
                        for key in keys {
                            apply_swarm_actions(vec![SwarmAction::StartProviding(key)], &mut state, &mut swarm);
                        }
                    }
                }
                _ = sync_tick.tick() => {
                    sync_keys_from_servers(&state).await;
                }
                _ = caps_tick.tick() => {
                    publish_node_capabilities(&state.local_peer_id, &state.config, &mut swarm);
                }
                maybe_ev = events.next() => {
                    if let Some(Ok(Event::Key(k))) = maybe_ev {
                        match (k.code, k.modifiers) {
                            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                                state.should_quit = true;
                            }
                            _ => {}
                        }
                    }
                }
                swarm_event = swarm.select_next_some() => {
                    let actions = handle_swarm_event(swarm_event, &mut state);
                    apply_swarm_actions(actions, &mut state, &mut swarm);
                }
                // Gossip commands from web handlers (best-effort, non-blocking)
                Some(cmd) = state.gossip_rx.recv() => {
                    match cmd {
                        GossipCommand::Publish { topic, data } => {
                            let t = gossipsub::IdentTopic::new(&topic);
                            if let Err(e) = swarm.behaviour_mut().gossipsub.publish(t, data) {
                                eprintln!("⚠️  [gossip] publish '{}': {:?}", topic, e);
                            }
                        }
                    }
                }
                // Config hot-reload from web handler or SIGHUP task
                Some(new_cfg) = state.config_reload_rx.recv() => {
                    apply_config_hot_reload(&mut state, new_cfg, &mut world_config);
                }
            }
            if state.should_quit { break; }
        }
        Ok(())
    }.await;

    disable_raw_mode()?;
    execute!(io::stderr(), LeaveAlternateScreen)?;

    // Graceful shutdown — save world (these now go to server.log since stdout is redirected)
    if let Some(ref mut w) = world {
        eprintln!("💾 Saving world state...");
        w.shutdown();
        eprintln!("✅ World saved.");
    }
    eprintln!("👋 Metaverse server stopped. Log: {}", log_path);
    result
}
