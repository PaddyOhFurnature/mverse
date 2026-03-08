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
//! Config: ./server.json  (relative to working directory — portable)
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
use libp2p::kad::store::{MemoryStore, MemoryStoreConfig};
use libp2p::request_response::{self, ProtocolSupport};
use metaverse_core::tile_protocol::{TileCodec, TileRequest, TileResponse, TILE_PROTOCOL};
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
use metaverse_core::web_ui::{NodeStatus, PeerSummary, DASHBOARD_HTML};
use metaverse_core::node_config::NodeConfig as ServerConfig;
use rusqlite::{params, Connection};
use sha2::{Sha256, Digest};
use rand::RngCore;
#[cfg(unix)]
use libc;

// ─── Config ──────────────────────────────────────────────────────────────────
// ServerConfig is now NodeConfig from metaverse_core::node_config (type alias above).

fn config_paths() -> [PathBuf; 1] {
    [PathBuf::from("server.json")]
}

fn load_config() -> (ServerConfig, Option<String>) {
    for path in &config_paths() {
        if path.exists() {
            if let Ok(text) = std::fs::read_to_string(path) {
                match serde_json::from_str::<ServerConfig>(&text) {
                    Ok(cfg) => return (cfg, None),
                    Err(e) => {
                        let msg = format!("❌ Config parse error in {} — using defaults: {}", path.display(), e);
                        eprintln!("{}", msg);
                        return (metaverse_core::node_config::NodeConfig::server_defaults(), Some(msg));
                    }
                }
            }
        }
    }
    (metaverse_core::node_config::NodeConfig::server_defaults(), None)
}

fn write_default_config_if_missing() {
    let path = PathBuf::from("server.json");
    if !path.exists() {
        if let Ok(json) = serde_json::to_string_pretty(&metaverse_core::node_config::NodeConfig::server_defaults()) {
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
    /// Config file path (default: ./server.json)
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
    connection_limits: libp2p::connection_limits::Behaviour,
    relay: relay::Behaviour,
    ping: libp2p::ping::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    identify: identify::Behaviour,
    mdns: mdns::tokio::Behaviour,
    gossipsub: gossipsub::Behaviour,
    tile_rr: request_response::Behaviour<TileCodec>,
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
    pub swap_used_mb: u64,
    pub swap_total_mb: u64,
    pub proc_rss_mb: u64,
    pub shedding_relay: bool,
    pub relay_port: u16,
    pub web_port: u16,
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
    /// Channel to send SwarmActions (e.g. DHT puts) from web handlers into the event loop.
    #[serde(skip)]
    pub swarm_tx: Option<tokio::sync::mpsc::Sender<SwarmAction>>,
    /// Channel to send a hot-reload `ServerConfig` from web handlers into the event loop.
    #[serde(skip)]
    pub config_reload_tx: Option<tokio::sync::mpsc::Sender<ServerConfig>>,
    /// Set when an auto-update is available; cleared after applying the update.
    #[serde(skip)]
    pub update_available: Option<String>,
    /// World data directory (for tile cache access from web handlers).
    #[serde(skip)]
    pub world_dir: String,
    /// Recent log entries (last 200), synced from AppState.log — shown in web dashboard.
    pub recent_logs: Vec<String>,
    /// Log buffer written by background tasks (SRTM download etc.) — drained into AppState.log on sync.
    #[serde(skip)]
    pub task_log: Arc<std::sync::Mutex<Vec<String>>>,
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
            swap_used_mb: 0, swap_total_mb: 0, proc_rss_mb: 0,
            shedding_relay: false,
            relay_port: 4001,
            web_port: 8080,
            version: env!("CARGO_PKG_VERSION").to_string(),
            key_db: None,
            server_secret: [0u8; 32],
            pending_challenges: Arc::new(Mutex::new(HashMap::new())),
            gossip_tx: None,
            swarm_tx: None,
            config_reload_tx: None,
            update_available: None,
            world_dir: "world_data".to_string(),
            recent_logs: Vec::new(),
            task_log: Arc::new(std::sync::Mutex::new(Vec::new())),
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
    should_quit: bool,
    sys: System,
    cpu_pct: f32,
    ram_used_mb: u64,
    ram_total_mb: u64,
    swap_used_mb: u64,
    swap_total_mb: u64,
    proc_rss_mb: u64,
    last_sys_refresh: Instant,
    last_debug_log: Instant,
    last_shared_sync: Instant,
    net: NetStats,
    shedding_relay: bool,
    /// Gossip commands from web handlers waiting to be published into the swarm.
    gossip_rx: tokio::sync::mpsc::Receiver<GossipCommand>,
    /// SwarmActions from web handlers (e.g. DHT puts from content submissions).
    swarm_web_rx: tokio::sync::mpsc::Receiver<SwarmAction>,
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
    /// Per-peer shed cooldown — tracks when each peer was last disconnected by load-shedding.
    /// A peer that was shed will not be shed again for SHED_COOLDOWN_SECS seconds.
    shed_cooldown: HashMap<PeerId, Instant>,
    /// Minimum time between any shed actions (prevents shed storms).
    last_shed_at: Instant,
    /// In-flight outbound tile requests awaiting peer response.
    pending_tile_requests: std::collections::HashMap<
        request_response::OutboundRequestId,
        tokio::sync::oneshot::Sender<TileResponse>,
    >,
    /// Channel to the on-demand SRTM downloader — send (lat,lon) to prioritise a tile.
    srtm_priority_tx: Option<tokio::sync::mpsc::Sender<(i32, i32)>>,
}

impl AppState {
    fn new(config: ServerConfig, shared: Arc<RwLock<SharedState>>,
           local_peer_id: String, public_ip: String,
           gossip_rx: tokio::sync::mpsc::Receiver<GossipCommand>,
           swarm_web_rx: tokio::sync::mpsc::Receiver<SwarmAction>,
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
            log: VecDeque::new(), should_quit: false,
            sys, cpu_pct, ram_used_mb, ram_total_mb,
            swap_used_mb: 0, swap_total_mb: 0, proc_rss_mb: 0,
            last_sys_refresh: Instant::now(),
            last_debug_log: Instant::now() - Duration::from_secs(3600),
            last_shared_sync: Instant::now(),
            net: NetStats::default(),
            shedding_relay: false,
            gossip_rx,
            swarm_web_rx,
            config_reload_rx,
            pending_shed: Vec::new(),
            pending_chunk_requests: Vec::new(),
            pending_voxel_ops: Vec::new(),
            pending_dht_provide: Vec::new(),
            pending_tile_requests: std::collections::HashMap::new(),
            srtm_priority_tx: None,
            shed_cooldown: HashMap::new(),
            last_shed_at: Instant::now() - Duration::from_secs(3600),
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
        self.swap_used_mb = self.sys.used_swap() / 1_048_576;
        self.swap_total_mb = self.sys.total_swap() / 1_048_576;

        // Read this process's RSS from /proc/self/status (Linux)
        #[cfg(target_os = "linux")]
        {
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if line.starts_with("VmRSS:") {
                        if let Some(kb) = line.split_whitespace().nth(1) {
                            self.proc_rss_mb = kb.parse::<u64>().unwrap_or(0) / 1024;
                        }
                        break;
                    }
                }
            }
        }
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
            // Only shed at most once every 30 seconds to avoid immediate reconnect loops
            if self.last_shed_at.elapsed() < Duration::from_secs(30) {
                return;
            }
            // Evict the oldest relay circuit whose source peer is not in cooldown
            let now = Instant::now();
            if let Some(oldest) = self.active_circuits.iter()
                .filter(|c| self.shed_cooldown.get(&c.src)
                    .map(|t| now.duration_since(*t) > Duration::from_secs(300))
                    .unwrap_or(true))
                .min_by_key(|c| c.started_at)
            {
                let peer = oldest.src;
                self.shed_cooldown.insert(peer, now);
                self.last_shed_at = now;
                self.pending_shed.push(peer);
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

        // Periodic debug stats (every 60s when log_level = "debug")
        if (self.config.log_level == "debug" || self.config.log_level == "trace")
            && self.last_debug_log.elapsed() >= Duration::from_secs(60)
        {
            self.last_debug_log = Instant::now();
            let ram_pct = if self.ram_total_mb > 0 {
                self.ram_used_mb * 100 / self.ram_total_mb
            } else { 0 };
            let swap_pct = if self.swap_total_mb > 0 {
                self.swap_used_mb * 100 / self.swap_total_mb
            } else { 0 };
            let msg = format!(
                "🔍 [DEBUG] peers={} circuits={} | \
                 proc_rss={}MB | \
                 sys_ram={}/{}MB ({}%) | \
                 swap={}/{}MB ({}%) | \
                 cpu={:.1}%",
                self.connected_peers.len(),
                self.active_circuits.len(),
                self.proc_rss_mb,
                self.ram_used_mb, self.ram_total_mb, ram_pct,
                self.swap_used_mb, self.swap_total_mb, swap_pct,
                self.cpu_pct,
            );
            self.log(msg);
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
        // Drain pending task-log messages before acquiring write lock (avoids double borrow)
        let task_log_arc = self.shared.read().ok()
            .map(|s| Arc::clone(&s.task_log));
        let pending: Vec<String> = task_log_arc
            .as_ref()
            .and_then(|tl| tl.lock().ok().map(|mut v| v.drain(..).collect()))
            .unwrap_or_default();
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
            s.swap_used_mb = self.swap_used_mb;
            s.swap_total_mb = self.swap_total_mb;
            s.proc_rss_mb = self.proc_rss_mb;
            s.shedding_relay = self.shedding_relay;
            if let Some(ref db) = s.key_db {
                s.key_count = db.count();
            }
            // Sync last 200 log entries for web dashboard (updated after draining pending below)
            s.recent_logs = self.log.iter().rev().take(200).cloned().collect::<Vec<_>>();
            s.recent_logs.reverse();
        }
        // Now safe to mutably borrow self.log — shared write guard already dropped
        for msg in pending {
            self.log(msg);
        }
        self.last_shared_sync = Instant::now();
    }

    fn short(id: &str) -> String { short_id(id) }
}

fn short_id(id: &str) -> String {
    if id.len() > 12 { format!("…{}", &id[id.len()-10..]) } else { id.to_string() }
}

/// Returns true if the given multiaddr is worth adding to Kademlia / dialling.
/// Filters out loopback, link-local, Docker/VMware bridge ranges and any QUIC
/// addresses (the server transport only speaks TCP + WebSocket).
fn is_kad_routable(addr: &Multiaddr) -> bool {
    use libp2p::multiaddr::Protocol;
    for proto in addr.iter() {
        match proto {
            Protocol::Ip4(ip) => {
                if ip.is_loopback()    { return false; } // 127.x
                if ip.is_link_local()  { return false; } // 169.254.x
                let o = ip.octets();
                if o[0] == 172 && o[1] >= 16 && o[1] <= 31 { return false; } // docker / lxc
                if o[0] == 192 && o[1] == 168 && o[2] == 122 { return false; } // libvirt
                if o[0] == 10  && o[1] == 0   && o[2] == 2   { return false; } // VirtualBox NAT
            }
            Protocol::Ip6(ip) => {
                if ip.is_loopback() { return false; }
            }
            // Server has no QUIC transport — skip QUIC addresses to prevent "unsupported" errors
            Protocol::Udp(_) | Protocol::QuicV1 | Protocol::Quic => { return false; }
            // Circuit relay addresses are ephemeral — not useful for routing table
            Protocol::P2pCircuit => { return false; }
            _ => {}
        }
    }
    true
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
pub struct KeyDatabase {
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

            -- Meshsite content (forums, wiki, marketplace, post office)
            CREATE TABLE IF NOT EXISTS mesh_content (
                id          TEXT    PRIMARY KEY,       -- SHA-256 hex
                section     TEXT    NOT NULL,          -- forums|wiki|marketplace|post
                title       TEXT    NOT NULL,
                body        TEXT    NOT NULL,
                author      TEXT    NOT NULL,          -- peer_id
                signature   BLOB    NOT NULL,          -- ed25519 sig bytes
                created_at  INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_mc_section  ON mesh_content (section, created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_mc_author   ON mesh_content (author);

            -- World placed-object registry (modular placement)
            CREATE TABLE IF NOT EXISTS placed_objects (
                id            TEXT    PRIMARY KEY,       -- UUID
                object_type   TEXT    NOT NULL,          -- billboard|terminal|kiosk|portal|spawn_point|custom:…
                pos_x         REAL    NOT NULL DEFAULT 0,
                pos_y         REAL    NOT NULL DEFAULT 0,
                pos_z         REAL    NOT NULL DEFAULT 0,
                rotation_y    REAL    NOT NULL DEFAULT 0,
                scale         REAL    NOT NULL DEFAULT 1,
                content_key   TEXT    NOT NULL DEFAULT '',
                label         TEXT    NOT NULL DEFAULT '',
                placed_by     TEXT    NOT NULL DEFAULT '',
                placed_at     INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_po_chunk ON placed_objects (
                CAST(pos_x / 64.0 AS INTEGER),
                CAST(pos_z / 64.0 AS INTEGER)
            );
        ")?;

        // Schema migrations: add columns that may not exist in older databases.
        // SQLite doesn't support IF NOT EXISTS on ALTER TABLE; ignore errors for
        // duplicate columns (they're harmless).
        for migration in &[
            "ALTER TABLE key_records ADD COLUMN revoked_at INTEGER",
            "ALTER TABLE key_records ADD COLUMN revoked_by TEXT",
            "ALTER TABLE key_records ADD COLUMN revocation_reason TEXT",
            "ALTER TABLE server_sync ADD COLUMN content_last_synced_at INTEGER NOT NULL DEFAULT 0",
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

    // ── Meshsite content ──────────────────────────────────────────────────────

    /// Insert or ignore a content item (idempotent — content is immutable by id).
    fn insert_content(&self, item: &metaverse_core::meshsite::ContentItem) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR IGNORE INTO mesh_content (id, section, title, body, author, signature, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &item.id,
                item.section.as_str(),
                &item.title,
                &item.body,
                &item.author,
                &item.signature,
                item.created_at as i64,
            ],
        );
    }

    /// List content items for a section, newest first (max 100).
    fn list_content(&self, section: &str) -> Vec<metaverse_core::meshsite::ContentItem> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, section, title, body, author, signature, created_at
             FROM mesh_content WHERE section = ?1 ORDER BY created_at DESC LIMIT 100"
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![section], |row| {
            Ok(metaverse_core::meshsite::ContentItem {
                id:         row.get(0)?,
                section:    metaverse_core::meshsite::Section::from_str(
                                &row.get::<_, String>(1)?
                            ).unwrap_or(metaverse_core::meshsite::Section::Forums),
                title:      row.get(2)?,
                body:       row.get(3)?,
                author:     row.get(4)?,
                signature:  row.get(5)?,
                created_at: row.get::<_, i64>(6)? as u64,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Fetch a single content item by id.
    fn get_content(&self, id: &str) -> Option<metaverse_core::meshsite::ContentItem> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, section, title, body, author, signature, created_at
             FROM mesh_content WHERE id = ?1",
            params![id],
            |row| Ok(metaverse_core::meshsite::ContentItem {
                id:         row.get(0)?,
                section:    metaverse_core::meshsite::Section::from_str(
                                &row.get::<_, String>(1)?
                            ).unwrap_or(metaverse_core::meshsite::Section::Forums),
                title:      row.get(2)?,
                body:       row.get(3)?,
                author:     row.get(4)?,
                signature:  row.get(5)?,
                created_at: row.get::<_, i64>(6)? as u64,
            }),
        ).ok()
    }

    /// Return content items with `created_at > since_ms`, ordered ascending, up to `limit`.
    ///
    /// Used by `GET /api/v1/sync/content` so peer servers can pull incremental updates.
    fn list_content_since(&self, since_ms: i64, limit: usize) -> Vec<metaverse_core::meshsite::ContentItem> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id, section, title, body, author, signature, created_at
             FROM mesh_content WHERE created_at > ?1 ORDER BY created_at ASC LIMIT ?2"
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![since_ms, limit as i64], |row| {
            Ok(metaverse_core::meshsite::ContentItem {
                id:         row.get(0)?,
                section:    metaverse_core::meshsite::Section::from_str(
                                &row.get::<_, String>(1)?
                            ).unwrap_or(metaverse_core::meshsite::Section::Forums),
                title:      row.get(2)?,
                body:       row.get(3)?,
                author:     row.get(4)?,
                signature:  row.get(5)?,
                created_at: row.get::<_, i64>(6)? as u64,
            })
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Get the last-synced content timestamp for a peer server URL (0 if never).
    fn get_content_last_synced_at(&self, server_url: &str) -> i64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT content_last_synced_at FROM server_sync WHERE server_url = ?1",
            params![server_url],
            |r| r.get(0),
        ).unwrap_or(0)
    }

    /// Update the content-sync timestamp for a peer server.
    fn update_content_sync(&self, server_url: &str, content_last_synced_at: i64) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO server_sync (server_url, content_last_synced_at)
             VALUES (?1, ?2)
             ON CONFLICT(server_url) DO UPDATE SET
               content_last_synced_at = excluded.content_last_synced_at",
            params![server_url, content_last_synced_at],
        );
    }

    // ── World placed objects ──────────────────────────────────────────────────

    /// Insert or replace a placed object.
    fn insert_object(&self, obj: &metaverse_core::world_objects::PlacedObject) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO placed_objects
               (id, object_type, pos_x, pos_y, pos_z, rotation_y, scale, content_key, label, placed_by, placed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                obj.id,
                obj.object_type.as_str(),
                obj.position[0] as f64,
                obj.position[1] as f64,
                obj.position[2] as f64,
                obj.rotation_y as f64,
                obj.scale as f64,
                obj.content_key,
                obj.label,
                obj.placed_by,
                obj.placed_at as i64,
            ],
        );
    }

    /// Delete a placed object by id. Returns `true` if a row was deleted.
    fn delete_object(&self, id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM placed_objects WHERE id = ?1", params![id])
            .unwrap_or(0) > 0
    }

    /// List all placed objects in the given chunk (cx, cz) where chunk coords
    /// are `floor(pos_x / 64)` and `floor(pos_z / 64)`.
    fn list_objects_in_chunk(&self, cx: i32, cz: i32) -> Vec<metaverse_core::world_objects::PlacedObject> {
        use metaverse_core::world_objects::{PlacedObject, ObjectType, CHUNK_GRID_M};
        let x_min = cx as f64 * CHUNK_GRID_M as f64;
        let x_max = x_min + CHUNK_GRID_M as f64;
        let z_min = cz as f64 * CHUNK_GRID_M as f64;
        let z_max = z_min + CHUNK_GRID_M as f64;
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id,object_type,pos_x,pos_y,pos_z,rotation_y,scale,content_key,label,placed_by,placed_at
             FROM placed_objects
             WHERE pos_x >= ?1 AND pos_x < ?2 AND pos_z >= ?3 AND pos_z < ?4"
        ) { Ok(s) => s, Err(_) => return vec![] };
        stmt.query_map(params![x_min, x_max, z_min, z_max], |row| {
            Ok(PlacedObject {
                id:           row.get(0)?,
                object_type:  ObjectType::from_str(&row.get::<_, String>(1)?),
                position:     [row.get::<_, f64>(2)? as f32, row.get::<_, f64>(3)? as f32, row.get::<_, f64>(4)? as f32],
                rotation_y:   row.get::<_, f64>(5)? as f32,
                scale:        row.get::<_, f64>(6)? as f32,
                content_key:  row.get(7)?,
                label:        row.get(8)?,
                placed_by:    row.get(9)?,
                placed_at:    row.get::<_, i64>(10)? as u64,
            })
        }).ok().map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    /// Rebuild and return the `ChunkObjectList` for chunk (cx, cz).
    fn chunk_object_list(&self, cx: i32, cz: i32) -> metaverse_core::world_objects::ChunkObjectList {
        metaverse_core::world_objects::ChunkObjectList {
            cx, cz,
            objects: self.list_objects_in_chunk(cx, cz),
        }
    }

    /// List every placed object in the database (for admin view).
    fn list_all_objects(&self) -> Vec<metaverse_core::world_objects::PlacedObject> {
        use metaverse_core::world_objects::{PlacedObject, ObjectType};
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT id,object_type,pos_x,pos_y,pos_z,rotation_y,scale,content_key,label,placed_by,placed_at
             FROM placed_objects ORDER BY placed_at DESC"
        ) { Ok(s) => s, Err(_) => return vec![] };
        stmt.query_map([], |row| {
            Ok(PlacedObject {
                id:           row.get(0)?,
                object_type:  ObjectType::from_str(&row.get::<_, String>(1)?),
                position:     [row.get::<_, f64>(2)? as f32, row.get::<_, f64>(3)? as f32, row.get::<_, f64>(4)? as f32],
                rotation_y:   row.get::<_, f64>(5)? as f32,
                scale:        row.get::<_, f64>(6)? as f32,
                content_key:  row.get(7)?,
                label:        row.get(8)?,
                placed_by:    row.get(9)?,
                placed_at:    row.get::<_, i64>(10)? as u64,
            })
        }).ok().map(|rows| rows.flatten().collect()).unwrap_or_default()
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
pub enum GossipCommand {
    Publish { topic: String, data: Vec<u8> },
}

pub enum SwarmAction {
    AddKadAddress(PeerId, Multiaddr),
    RefreshDhtCount,
    DialPeer(PeerId, Multiaddr),
    SubscribeTopic(String),
    PublishGossip { topic: String, data: Vec<u8> },
    StartProviding(Vec<u8>),
    PutDhtRecord { key: Vec<u8>, value: Vec<u8> },
    /// Remove a peer from the Kademlia routing table (used when ephemeral clients disconnect).
    RemoveKadPeer(PeerId),
    /// Disconnect a peer — used by load-shedding to drop the oldest relay circuit.
    DisconnectPeer(PeerId),
    /// Request a tile from a specific peer
    TileRequest {
        peer_id: PeerId,
        request: TileRequest,
        response_tx: tokio::sync::oneshot::Sender<TileResponse>,
    },
    /// Respond to an inbound tile request from a peer
    RespondTile {
        channel: request_response::ResponseChannel<TileResponse>,
        response: TileResponse,
    },
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
                // Remove ephemeral (non-server, non-relay) peers from Kademlia routing table.
                // Without this, Kademlia spams dial attempts to their stale addresses after disconnect.
                let ptype = state.connected_peers.get(&peer_id)
                    .map(|e| e.2.as_str())
                    .unwrap_or("unknown");
                if ptype != "server" && ptype != "relay" {
                    actions.push(SwarmAction::RemoveKadPeer(peer_id));
                }
                state.connected_peers.remove(&peer_id);
                let reason = cause.map(|e| format!(" (Connection error: {})", e)).unwrap_or_default();
                state.log(format!("❌ Disconnected {}{}", AppState::short(&peer_id.to_string()), reason));
            }
        }
        SwarmEvent::NewListenAddr { address, .. } => {
            state.log(format!("👂 Listening  {}", address));
        }
        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            let err_str = error.to_string();
            if let Some(pid) = peer_id {
                state.log(format!("✗  Dial failed  {} — {}", AppState::short(&pid.to_string()), err_str));
                // If this peer is no longer connected, remove from Kademlia to stop infinite retry.
                // Kademlia learns peer addresses from connections and retries them after disconnect.
                if !state.connected_peers.contains_key(&pid) {
                    actions.push(SwarmAction::RemoveKadPeer(pid));
                }
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
            // Detect peer type from protocol strings — check metaverse-server first because
            // all nodes (clients included) also advertise the relay protocol, so checking
            // for "relay" first would misclassify game clients as relays.
            let peer_type = if info.protocols.iter().any(|p| p.as_ref().contains("metaverse-server")) {
                "server"
            } else if info.protocols.iter().any(|p| p.as_ref().contains("metaverse-relay")) {
                "relay"
            } else if !info.protocols.iter().any(|p| p.as_ref().contains("metaverse"))
                && info.protocols.iter().any(|p| p.as_ref().contains("/libp2p/circuit/relay/0.2.0/hop"))
            {
                // Peer has no metaverse protocol at all but IS a relay — must be a
                // standalone relay node (e.g. Protocol Labs bootstrap/relay nodes).
                "relay"
            } else {
                "client"
            };
            if let Some(entry) = state.connected_peers.get_mut(&peer_id) {
                entry.2 = peer_type.to_string();
            }
            for addr in info.listen_addrs {
                // Only add server/relay peers to Kademlia — clients are ephemeral
                // and adding them causes Kademlia to spam dial-back retries after disconnect.
                if (peer_type == "server" || peer_type == "relay") && is_kad_routable(&addr) {
                    actions.push(SwarmAction::AddKadAddress(peer_id, addr));
                }
            }
            actions.push(SwarmAction::RefreshDhtCount);
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
            for (peer_id, addr) in peers {
                // Only log mDNS for new peers not yet connected
                if !state.connected_peers.contains_key(&peer_id) {
                    state.log(format!("🔍 mDNS  {}", AppState::short(&peer_id.to_string())));
                }
                // Dial the peer; Identify will fire and add to Kademlia if they're a server/relay
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
            } else if topic.starts_with("meshsite/") {
                // Incoming meshsite content from the mesh — store locally and re-put to DHT.
                if let Some(item) = metaverse_core::meshsite::ContentItem::from_bytes(&message.data) {
                    if item.id_valid() {
                        let (is_new, dht_key, dht_val) = {
                            let s = state.shared.read().ok();
                            let db = s.as_ref().and_then(|s| s.key_db.as_ref());
                            if let Some(db) = db {
                                let is_new = db.get_content(&item.id).is_none();
                                if is_new { db.insert_content(&item); }
                                (is_new, item.dht_key(), message.data.clone())
                            } else {
                                (false, vec![], vec![])
                            }
                        };
                        if is_new {
                            state.log(format!("◈ [meshsite/{}] stored: {}…", item.section.as_str(), &item.id[..8]));
                            actions.push(SwarmAction::PutDhtRecord { key: dht_key, value: dht_val });
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
            // Only allow server/relay peers in the routing table.
            // Kademlia auto-adds peers it talks to; remove clients immediately
            // to prevent Kademlia from dialing their private/circuit addresses.
            let ptype = state.connected_peers.get(&peer).map(|e| e.2.as_str()).unwrap_or("unknown");
            if ptype == "server" || ptype == "relay" {
                state.dht_peer_count = 0;
                actions.push(SwarmAction::AddKadAddress(peer, Multiaddr::empty()));
                actions.push(SwarmAction::RefreshDhtCount);
            } else {
                actions.push(SwarmAction::RemoveKadPeer(peer));
            }
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::TileRr(
            request_response::Event::Message {
                peer,
                message: request_response::Message::Request { request, channel, .. },
                ..
            }
        )) => {
            let world_dir = state.config.world_dir.clone()
                .unwrap_or_else(|| "world_data".to_string());
            let tile_desc = match &request {
                TileRequest::OsmTile { s, w, n, e } => format!("OSM s={s:.2} w={w:.2} n={n:.2} e={e:.2}"),
                TileRequest::ElevationTile { lat, lon } => format!("SRTM lat={lat} lon={lon}"),
                TileRequest::TerrainChunk { cx, cz } => format!("chunk cx={cx} cz={cz}"),
            };
            let response = serve_tile_request(&request, &world_dir);
            let found = matches!(response, TileResponse::Found(_));
            state.log(format!("📡 [P2P] {} {} from {}",
                if found { "📤" } else { "❌" },
                tile_desc,
                AppState::short(&peer.to_string())));
            // If server doesn't have it, queue it (and neighbours) for on-demand download
            if !found {
                if let TileRequest::ElevationTile { lat, lon } = &request {
                    if let Some(ref tx) = state.srtm_priority_tx {
                        // Enqueue requested tile + 2-degree radius so surrounding terrain loads too
                        for dlat in -2i32..=2 {
                            for dlon in -2i32..=2 {
                                let _ = tx.try_send((*lat + dlat, *lon + dlon));
                            }
                        }
                    }
                }
            }
            actions.push(SwarmAction::RespondTile { channel, response });
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::TileRr(
            request_response::Event::Message {
                message: request_response::Message::Response { request_id, response },
                ..
            }
        )) => {
            if let Some(tx) = state.pending_tile_requests.remove(&request_id) {
                let _ = tx.send(response);
            }
        }
        SwarmEvent::Behaviour(ServerBehaviourEvent::TileRr(
            request_response::Event::OutboundFailure { request_id, .. }
        )) => {
            if let Some(tx) = state.pending_tile_requests.remove(&request_id) {
                let _ = tx.send(TileResponse::NotFound);
            }
        }
        _ => {}
    }

    actions
}

fn serve_tile_request(request: &TileRequest, world_dir: &str) -> TileResponse {
    use metaverse_core::tile_protocol::TileRequest::*;
    use std::path::PathBuf;
    match request {
        OsmTile { s, w, n, e } => {
            let cache_dir = PathBuf::from(world_dir).join("osm");
            let cache = metaverse_core::osm::OsmDiskCache::new(&cache_dir);
            match cache.load(*s, *w, *n, *e) {
                Some(data) => match bincode::serialize(&data) {
                    Ok(bytes) => TileResponse::Found(bytes),
                    Err(_) => TileResponse::NotFound,
                },
                None => TileResponse::NotFound,
            }
        }
        ElevationTile { lat, lon } => {
            let cache_dir = PathBuf::from(world_dir).join("elevation_cache");
            let lat_prefix = if *lat >= 0 { 'n' } else { 's' };
            let lon_prefix = if *lon >= 0 { 'e' } else { 'w' };
            let lat_dir = if *lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", lat.unsigned_abs()) };
            let lon_dir = if *lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lon.unsigned_abs()) };
            let tile_name = format!("srtm_{}{:02}_{}{:03}.tif",
                lat_prefix, lat.unsigned_abs(), lon_prefix, lon.unsigned_abs());
            let path = cache_dir.join(&lat_dir).join(&lon_dir).join(&tile_name);
            // Also check HGT format (Skadi downloads)
            let hgt_prefix_up: char = if *lat >= 0 { 'N' } else { 'S' };
            let hgt_lon_prefix_up: char = if *lon >= 0 { 'E' } else { 'W' };
            let hgt_name = format!("{}{:02}{}{:03}.hgt",
                hgt_prefix_up, lat.unsigned_abs(), hgt_lon_prefix_up, lon.unsigned_abs());
            let hgt_path = cache_dir.join(&lat_dir).join(&lon_dir).join(&hgt_name);
            if let Ok(bytes) = std::fs::read(&path) {
                TileResponse::Found(bytes)
            } else if let Ok(bytes) = std::fs::read(&hgt_path) {
                TileResponse::Found(bytes)
            } else {
                TileResponse::NotFound
            }
        }
        TerrainChunk { .. } => {
            // Terrain chunks are not yet served via P2P (future work)
            TileResponse::NotFound
        }
    }
}

fn apply_swarm_actions(
    actions: Vec<SwarmAction>,
    state: &mut AppState,
    swarm: &mut libp2p::Swarm<ServerBehaviour>,
) {
    for action in actions {
        match action {
            SwarmAction::RemoveKadPeer(peer_id) => {
                swarm.behaviour_mut().kademlia.remove_peer(&peer_id);
            }
            SwarmAction::AddKadAddress(peer_id, addr) => {
                if addr != Multiaddr::empty() && is_kad_routable(&addr) {
                    swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                }
            }
            SwarmAction::RefreshDhtCount => {
                state.dht_peer_count = swarm.behaviour_mut()
                    .kademlia.kbuckets().map(|b| b.num_entries()).sum();
            }
            SwarmAction::DialPeer(peer_id, addr) => {
                if is_kad_routable(&addr) && !swarm.is_connected(&peer_id) {
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
                    // Only log unexpected errors, not MaxProvidedKeys (handled by config)
                    let msg = format!("{:?}", e);
                    if !msg.contains("MaxProvidedKeys") {
                        eprintln!("⚠️  [DHT] start_providing failed: {}", msg);
                    }
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
            SwarmAction::TileRequest { peer_id, request, response_tx } => {
                let id = swarm.behaviour_mut().tile_rr.send_request(&peer_id, request);
                state.pending_tile_requests.insert(id, response_tx);
            }
            SwarmAction::RespondTile { channel, response } => {
                let _ = swarm.behaviour_mut().tile_rr.send_response(channel, response);
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
    draw_activity(frame, state, right_rows[1]);

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
    let swap_pct = if state.swap_total_mb > 0 { (state.swap_used_mb * 100 / state.swap_total_mb) as u8 } else { 0 };

    let cpu_style = if cpu_pct > 80 { Style::default().fg(Color::Red) }
        else if cpu_pct > 50 { Style::default().fg(Color::Yellow) }
        else { Style::default().fg(Color::Green) };
    let ram_style = if ram_pct > 85 { Style::default().fg(Color::Red) }
        else if ram_pct > 70 { Style::default().fg(Color::Yellow) }
        else { Style::default().fg(Color::Green) };
    let swap_style = if swap_pct > 50 { Style::default().fg(Color::Yellow) }
        else { Style::default().fg(Color::Cyan) };

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
            Span::styled(
                format!("{:3}%  {}/{} MB (proc {}MB)", ram_pct, state.ram_used_mb, state.ram_total_mb, state.proc_rss_mb),
                ram_style,
            ),
        ]),
        Line::from(vec![
            Span::styled(format!("SWP  {} ", bar(swap_pct, 20)), swap_style),
            Span::styled(format!("{:3}%  {}/{} MB", swap_pct, state.swap_used_mb, state.swap_total_mb), swap_style),
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
            state.config.world_dir.as_deref().unwrap_or("world_data").to_string()),
    ];
    frame.render_widget(
        Paragraph::new(items)
            .block(Block::default().title(" World ").title_style(Style::default().fg(Color::Green)).borders(Borders::ALL)),
        area,
    );
}

fn draw_activity(frame: &mut Frame, state: &AppState, area: Rect) {
    // Show last N log entries — most useful thing on the TUI
    let height = area.height.saturating_sub(2) as usize; // minus border
    let entries: Vec<Line> = state.log.iter().rev().take(height)
        .map(|msg| {
            let style = if msg.contains('✅') || msg.contains("complete") || msg.contains("saved") {
                Style::default().fg(Color::Green)
            } else if msg.contains('❌') || msg.contains("error") || msg.contains("Error") || msg.contains("failed") {
                Style::default().fg(Color::Red)
            } else if msg.contains('⚠') || msg.contains("warn") {
                Style::default().fg(Color::Yellow)
            } else if msg.contains("⬇") || msg.contains("📥") || msg.contains("download") || msg.contains("Download") {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(Span::styled(msg.clone(), style))
        })
        .collect::<Vec<_>>()
        .into_iter().rev().collect();

    frame.render_widget(
        Paragraph::new(entries)
            .block(Block::default().title(" Activity ").title_style(Style::default().fg(Color::Yellow)).borders(Borders::ALL))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        area,
    );
}

fn stat_line(label: impl Into<String>, value: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        Span::styled(label.into(), Style::default().fg(Color::DarkGray)),
        Span::styled(value.into(), Style::default().fg(Color::White)),
    ])
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

// ── GET /api/v1/sync/content ─────────────────────────────────────────────────

/// GET /api/v1/sync/content?since=<unix_ms>&limit=<n>
///
/// Returns up to `limit` content items with `created_at > since`, ordered by
/// `created_at` ascending.  Used by peer servers for incremental content sync.
async fn api_v1_sync_content(
    State(s): State<WebState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let since: i64 = params.get("since").and_then(|v| v.parse().ok()).unwrap_or(0);
    let limit: usize = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100).min(1000);

    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => Json(db.list_content_since(since, limit)).into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"key registry unavailable"}))).into_response(),
    }
}

// ── World placed objects API ──────────────────────────────────────────────────

/// GET /api/v1/world/objects?cx=X&cz=Z
///
/// Returns all placed objects in the given chunk as a [`ChunkObjectList`].
async fn api_v1_world_objects_get(
    State(s): State<WebState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => {
            if params.contains_key("cx") || params.contains_key("cz") {
                let cx: i32 = params.get("cx").and_then(|v| v.parse().ok()).unwrap_or(0);
                let cz: i32 = params.get("cz").and_then(|v| v.parse().ok()).unwrap_or(0);
                Json(db.chunk_object_list(cx, cz)).into_response()
            } else {
                // No chunk filter — return all objects (for admin dashboard)
                Json(db.list_all_objects()).into_response()
            }
        },
        None => (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"registry unavailable"}))).into_response(),
    }
}

/// POST /api/v1/world/objects — place a new object (or update an existing one).
///
/// Body: JSON [`PlacedObject`] (id optional — server generates UUID if missing).
/// Requires a `trust` or `admin` tier key (verified via `Authorization: Bearer <token>`).
/// On success: stores in DB and pushes the updated `ChunkObjectList` to DHT.
async fn api_v1_world_objects_post(
    State(s): State<WebState>,
    Json(mut obj): Json<metaverse_core::world_objects::PlacedObject>,
) -> impl IntoResponse {
    // Generate a UUID if the caller didn't supply one
    if obj.id.is_empty() {
        obj.id = format!("{:016x}{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64).unwrap_or(0),
            rand_u64(),
        );
    }
    if obj.placed_at == 0 {
        obj.placed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64).unwrap_or(0);
    }

    let (db, swarm_tx) = {
        let st = s.read().unwrap();
        (st.key_db.clone(), st.swarm_tx.clone())
    };

    let db = match db {
        Some(d) => d,
        None => return (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"registry unavailable"}))).into_response(),
    };

    db.insert_object(&obj);

    // Rebuild and publish the chunk record to DHT
    let (cx, cz) = obj.chunk_coords();
    let chunk_list = db.chunk_object_list(cx, cz);
    if let Some(tx) = swarm_tx {
        let _ = tx.try_send(SwarmAction::PutDhtRecord {
            key: metaverse_core::world_objects::chunk_dht_key(cx, cz),
            value: chunk_list.to_bytes(),
        });
    }

    (StatusCode::CREATED, Json(serde_json::json!({
        "ok": true, "id": obj.id, "cx": cx, "cz": cz
    }))).into_response()
}

/// DELETE /api/v1/world/objects/:id — remove a placed object.
async fn api_v1_world_objects_delete(
    State(s): State<WebState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let (db, swarm_tx) = {
        let st = s.read().unwrap();
        (st.key_db.clone(), st.swarm_tx.clone())
    };
    let db = match db {
        Some(d) => d,
        None => return (StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error":"registry unavailable"}))).into_response(),
    };

    // Need to know the chunk before deleting to update DHT record
    let obj = {
        let conn = db.conn.lock().unwrap();
        conn.query_row(
            "SELECT pos_x, pos_z FROM placed_objects WHERE id = ?1",
            params![id],
            |r| Ok((r.get::<_, f64>(0)? as f32, r.get::<_, f64>(1)? as f32)),
        ).ok()
    };

    if !db.delete_object(&id) {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response();
    }

    // Update DHT chunk record
    if let Some((px, pz)) = obj {
        let (cx, cz) = metaverse_core::world_objects::chunk_coords_for_pos(px, pz);
        let chunk_list = db.chunk_object_list(cx, cz);
        if let Some(tx) = swarm_tx {
            let _ = tx.try_send(SwarmAction::PutDhtRecord {
                key: metaverse_core::world_objects::chunk_dht_key(cx, cz),
                value: chunk_list.to_bytes(),
            });
        }
    }

    (StatusCode::OK, Json(serde_json::json!({"ok": true, "deleted": id}))).into_response()
}

/// Tiny non-crypto random u64 for ID generation (xorshift).
fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut x = SystemTime::now().duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64).unwrap_or(12345);
    x ^= x << 13; x ^= x >> 7; x ^= x << 17; x
}



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
                let (new_cfg, _) = load_config();
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

/// Sync content items from all `known_servers`, mirroring `sync_keys_from_servers`.
///
/// Queries `GET /api/v1/sync/content?since=<last>&limit=100` on each peer server,
/// imports new items via `insert_content` (idempotent), and updates
/// `server_sync.content_last_synced_at`.
async fn sync_content_from_servers(state: &AppState) {
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
        Err(e) => { eprintln!("[ContentSync] Failed to create HTTP client: {}", e); return; }
    };

    for server_url in &known_servers {
        let last_synced = db.get_content_last_synced_at(server_url);
        let url = format!("{}/api/v1/sync/content?since={}&limit=100", server_url, last_synced);

        let response = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => { eprintln!("[ContentSync] {} returned {}", server_url, r.status()); continue; }
            Err(e) => { eprintln!("[ContentSync] {} unreachable: {}", server_url, e); continue; }
        };

        let items: Vec<metaverse_core::meshsite::ContentItem> = match response.json().await {
            Ok(v) => v,
            Err(e) => { eprintln!("[ContentSync] {} bad JSON: {}", server_url, e); continue; }
        };

        let count = items.len();
        let mut newest_at: i64 = last_synced;

        for item in &items {
            if item.created_at as i64 > newest_at { newest_at = item.created_at as i64; }
            db.insert_content(item);
        }

        if count > 0 {
            db.update_content_sync(server_url, newest_at);
            eprintln!("[ContentSync] {} — imported {} items (newest_at={})",
                server_url, count, newest_at);
        }
    }
}

type WebState = Arc<RwLock<SharedState>>;

async fn web_root() -> Html<&'static str> {
    Html(DASHBOARD_HTML)
}

async fn web_api_status(State(s): State<WebState>) -> impl IntoResponse {
    let st = s.read().unwrap();
    let _ram_pct = if st.ram_total_mb > 0 { st.ram_used_mb as f32 / st.ram_total_mb as f32 * 100.0 } else { 0.0 };
    let status = NodeStatus {
        node_name:        st.node_name.clone(),
        node_type:        st.node_type.clone(),
        version:          st.version.clone(),
        peer_id:          st.local_peer_id.clone(),
        public_ip:        st.public_ip.clone(),
        p2p_port:         st.relay_port,
        web_port:         st.web_port,
        uptime_secs:      st.uptime_secs,
        peers:            st.peers.iter().map(|p| PeerSummary {
                              peer_id:        p.peer_id.clone(),
                              peer_type:      p.peer_type.clone(),
                              addr:           p.addr.clone(),
                              connected_secs: p.connected_secs,
                          }).collect(),
        circuit_count:    st.circuit_count,
        total_connections: st.total_connections,
        dht_peer_count:   st.dht_peer_count,
        gossip_msgs_in:   st.net.gossip_msgs_in,
        gossip_msgs_out:  st.net.gossip_msgs_out,
        bytes_in:         st.net.bytes_in,
        bytes_out:        st.net.bytes_out,
        cpu_pct:          st.cpu_pct,
        ram_used_mb:      st.ram_used_mb,
        ram_total_mb:     st.ram_total_mb,
        shedding:         st.shedding_relay,
        update_available: st.update_available.clone(),
        extra:            serde_json::json!({
                              "key_count": st.key_count,
                              "world":     serde_json::to_value(&st.world).unwrap_or_default(),
                              "recent_activity": st.recent_logs.iter().rev().take(12).cloned().collect::<Vec<_>>(),
                          }),
    };
    drop(st);
    Json(status)
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

async fn web_api_logs(State(s): State<WebState>) -> impl IntoResponse {
    let logs = s.read().unwrap().recent_logs.clone();
    Json(logs)
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

// ─── Meshsite API handlers ─────────────────────────────────────────────────────

/// POST /api/v1/content/post  — quick post without pre-computing a signature.
///
/// Takes `{"section":"forums","title":"...","body":"..."}`.
/// Server computes the id and publishes to gossipsub + DHT.
/// Content is received by all subscribed nodes and surfaces in-game (Construct module walls).
/// Confirm distribution with: GET /api/v1/content?section=forums
#[derive(serde::Deserialize)]
struct QuickPost {
    section: String,
    title:   String,
    body:    String,
    #[serde(default)]
    author:  String,
}
async fn api_v1_quick_post(
    State(s): State<WebState>,
    Json(payload): Json<QuickPost>,
) -> impl IntoResponse {
    use metaverse_core::meshsite::{ContentItem, Section, topic_for_section};

    let section = match Section::from_str(&payload.section) {
        Some(s) => s,
        None => return (StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error":"unknown section (forums|wiki|marketplace|post)"}))).into_response(),
    };
    if payload.title.is_empty() || payload.body.is_empty() {
        return (StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":"title and body required"}))).into_response();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    let author = if payload.author.is_empty() {
        s.read().unwrap().local_peer_id.clone()
    } else {
        payload.author
    };

    let mut item = ContentItem {
        id: String::new(),
        section,
        title: payload.title,
        body: payload.body,
        author,
        signature: vec![0u8; 64], // placeholder — sig verification not yet enforced
        created_at: now_ms,
    };
    item.id = item.compute_id();

    let topic = topic_for_section(&item.section).to_string();
    let data  = item.to_bytes();
    let id    = item.id.clone();
    let st = s.read().unwrap();

    if let Some(ref tx) = st.gossip_tx {
        let _ = tx.try_send(GossipCommand::Publish { topic, data: data.clone() });
    }
    if let Some(ref tx) = st.swarm_tx {
        let _ = tx.send(SwarmAction::PutDhtRecord { key: item.dht_key(), value: data });
    }

    (StatusCode::CREATED,
     Json(serde_json::json!({"id": id, "status": "published", "section": item.section.as_str(), "dht_key": item.dht_key()}))).into_response()
}

/// POST /api/v1/content  — inject a signed content item into the mesh.
///
/// The item is published to the gossipsub topic for its section.
/// Every subscribed node (including this server) will receive it,
/// verify it, store it locally, and put it in the DHT.
/// This endpoint is the local operator's injection point — not the distribution layer.
async fn api_v1_post_content(
    State(s): State<WebState>,
    Json(payload): Json<metaverse_core::meshsite::SubmitContent>,
) -> impl IntoResponse {
    use metaverse_core::meshsite::topic_for_section;

    let item = match payload.into_item() {
        Some(i) => i,
        None => return (StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error":"invalid section or signature hex"}))).into_response(),
    };

    if !item.id_valid() {
        return (StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error":"id mismatch — recompute sha256 of canonical fields"}))).into_response();
    }

    let st = s.read().unwrap();
    let topic = topic_for_section(&item.section).to_string();
    let data  = item.to_bytes();
    let id    = item.id.clone();

    // Publish to gossipsub — the mesh distributes it; our own handler will store it
    if let Some(ref tx) = st.gossip_tx {
        let _ = tx.try_send(GossipCommand::Publish { topic, data: data.clone() });
    }
    // Also put to DHT for offline/late-join persistence
    if let Some(ref tx) = st.swarm_tx {
        let _ = tx.send(SwarmAction::PutDhtRecord { key: item.dht_key(), value: data });
    }

    (StatusCode::ACCEPTED,
     Json(serde_json::json!({"id": id, "status": "published to mesh"}))).into_response()
}

/// GET /api/v1/content?section=forums  — list items in a section.
async fn api_v1_list_content(
    State(s): State<WebState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let section = params.get("section").map(|s| s.as_str()).unwrap_or("forums");
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => {
            let items = db.list_content(section);
            let json: Vec<_> = items.iter().map(|i| serde_json::json!({
                "id":         i.id,
                "section":    i.section.as_str(),
                "title":      i.title,
                "body":       i.body,
                "author":     i.author,
                "created_at": i.created_at,
            })).collect();
            Json(json).into_response()
        }
        None => (StatusCode::SERVICE_UNAVAILABLE,
                 Json(serde_json::json!({"error":"content store not available"}))).into_response(),
    }
}

/// GET /api/v1/content/:id  — fetch single content item by id.
async fn api_v1_get_content(
    State(s): State<WebState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let st = s.read().unwrap();
    match &st.key_db {
        Some(db) => match db.get_content(&id) {
            Some(item) => Json(serde_json::json!({
                "id":         item.id,
                "section":    item.section.as_str(),
                "title":      item.title,
                "body":       item.body,
                "author":     item.author,
                "created_at": item.created_at,
            })).into_response(),
            None => (StatusCode::NOT_FOUND,
                     Json(serde_json::json!({"error":"not found"}))).into_response(),
        },
        None => (StatusCode::SERVICE_UNAVAILABLE,
                 Json(serde_json::json!({"error":"content store not available"}))).into_response(),
    }
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
            .unwrap_or_else(|| PathBuf::from("world_data"));
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
    let (mut config, config_error) = if let Some(ref path) = args.config {
        let text = std::fs::read_to_string(path)?;
        (serde_json::from_str::<ServerConfig>(&text)?, None)
    } else {
        load_config()
    };
    apply_cli_overrides(&mut config, &args);
    write_default_config_if_missing();

    // ── Early update check (before TUI, clean terminal) ──────────────────────
    // 5-second timeout so startup isn't delayed when offline.
    if !config.github_repo.is_empty() {
        let repo = config.github_repo.clone();
        let current = env!("CARGO_PKG_VERSION");
        let check = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            metaverse_core::autoupdate::check_for_update(&repo, current),
        ).await;
        if let Ok(Some((tag, url, _notes))) = check {
            eprintln!("🔄 Update available: {} — downloading…", tag);
            match metaverse_core::autoupdate::apply_update(&tag, &url).await {
                Ok(()) => {} // apply_update does not return on success (exec-restart)
                Err(e) => eprintln!("⚠️  Auto-update failed: {} — continuing with current version", e),
            }
        }
    }

    // Headless if flag/config or not a terminal
    let headless = config.headless || !io::stdout().is_terminal();
    config.headless = headless;

    // ── OOM protection ──────────────────────────────────────────────────────
    // Lower this process's OOM priority so the kernel uses swap before killing us.
    // Requires CAP_SYS_RESOURCE (root or systemd AmbientCapabilities); silently ignored otherwise.
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        match std::fs::OpenOptions::new().write(true).open("/proc/self/oom_score_adj") {
            Ok(mut f) => {
                if f.write_all(b"-500\n").is_ok() {
                    println!("🛡️  OOM score set to -500 (swap preferred over kill)");
                }
            }
            Err(_) => {} // not root — ignore
        }
    }

    // Identity
    let identity_path = config.identity_file.as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("server.key"));
    if let Some(parent) = identity_path.parent() {
        if !parent.as_os_str().is_empty() { std::fs::create_dir_all(parent)?; }
    }

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
        .with_tcp(libp2p::tcp::Config::default().nodelay(true), libp2p::noise::Config::new, libp2p::yamux::Config::default)?
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
            let mut mem_store_cfg = MemoryStoreConfig::default();
            mem_store_cfg.max_provided_keys = 10_000_000; // enough for millions of tiles
            let mut kademlia = kad::Behaviour::with_config(
                peer_id, MemoryStore::with_config(peer_id, mem_store_cfg), kad_config,
            );
            kademlia.set_mode(Some(kad::Mode::Server));

            // Identify — advertise "metaverse-server" protocol so peers can detect node type.
            // push_listen_addr_updates enabled so clients learn the server's relay circuit
            // address after it is registered (without this they can't reach a server behind NAT).
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
                "key-registry", "key-revocations",
            ] {
                let t = gossipsub::IdentTopic::new(*topic);
                let _ = gossipsub.subscribe(&t);
            }
            // Subscribe to all meshsite content topics
            for topic in metaverse_core::meshsite::MESHSITE_TOPICS {
                let t = gossipsub::IdentTopic::new(*topic);
                let _ = gossipsub.subscribe(&t);
            }

            Ok(ServerBehaviour {
                connection_limits: libp2p::connection_limits::Behaviour::new(
                    libp2p::connection_limits::ConnectionLimits::default()
                        .with_max_established_per_peer(Some(3))
                        .with_max_pending_incoming(Some(30))
                        .with_max_pending_outgoing(Some(30)),
                ),
                relay: relay::Behaviour::new(peer_id, relay::Config {
                    max_reservations: max_circuits,
                    max_circuits,
                    max_circuit_duration,
                    max_circuit_bytes,
                    ..Default::default()
                }),
                ping: libp2p::ping::Behaviour::new(
                    libp2p::ping::Config::new()
                        .with_interval(Duration::from_secs(5))
                        .with_timeout(Duration::from_secs(20))
                ),
                kademlia,
                identify,
                mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)
                    .expect("mDNS init failed"),
                gossipsub,
                tile_rr: request_response::Behaviour::new(
                    [(
                        libp2p::StreamProtocol::new(TILE_PROTOCOL),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                ),
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(3600)))
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
    {
        #[derive(serde::Deserialize, Clone)]
        struct BootstrapFile {
            bootstrap_nodes: Vec<BootstrapNodeFile>,
            #[serde(default)]
            fallback_discovery: FallbackDiscovery,
        }
        #[derive(serde::Deserialize, Clone, Default)]
        struct FallbackDiscovery {
            #[serde(default)]
            http_rendezvous: Vec<String>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct BootstrapNodeFile { multiaddr: String }

        let dial_nodes = |nodes: &[BootstrapNodeFile], swarm: &mut libp2p::Swarm<ServerBehaviour>| {
            for n in nodes {
                if let Ok(addr) = n.multiaddr.parse::<Multiaddr>() {
                    let _ = swarm.dial(addr);
                }
            }
        };

        let mut http_rendezvous_urls: Vec<String> = Vec::new();

        // Load and apply local bootstrap.json
        if let Ok(data) = std::fs::read_to_string("bootstrap.json") {
            if let Ok(bf) = serde_json::from_str::<BootstrapFile>(&data) {
                dial_nodes(&bf.bootstrap_nodes, &mut swarm);
                http_rendezvous_urls = bf.fallback_discovery.http_rendezvous.clone();
            }
        }
        // Also apply bootstrap_cache.json (saved from previous remote fetch)
        if let Ok(data) = std::fs::read_to_string("bootstrap_cache.json") {
            if let Ok(bf) = serde_json::from_str::<BootstrapFile>(&data) {
                dial_nodes(&bf.bootstrap_nodes, &mut swarm);
            }
        }

        // Fetch http_rendezvous URLs in background; save to bootstrap_cache.json for next startup
        if !http_rendezvous_urls.is_empty() {
            tokio::spawn(async move {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build().unwrap_or_default();
                for url in &http_rendezvous_urls {
                    if let Ok(resp) = client.get(url).send().await {
                        if let Ok(text) = resp.text().await {
                            let _ = std::fs::write("bootstrap_cache.json", &text);
                            break; // First successful URL wins
                        }
                    }
                }
            });
        }
    }

    // Key registry database
    let key_db = {
        let db_path = PathBuf::from("key_registry.db");
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
    // Channel for web handlers to push SwarmActions (e.g. DHT puts) into the event loop.
    let (swarm_web_tx, swarm_web_rx) = tokio::sync::mpsc::channel::<SwarmAction>(256);
    // Channel for web handlers / SIGHUP to trigger config hot-reloads.
    let (config_reload_tx, config_reload_rx) = tokio::sync::mpsc::channel::<ServerConfig>(8);

    // Shared state for web server
    let world_dir_str = config.world_dir.clone().unwrap_or_else(|| "world_data".to_string());
    let shared = Arc::new(RwLock::new(SharedState {
        local_peer_id: local_peer_id.to_string(),
        public_ip: public_ip.clone(),
        node_name: config.node_name.clone().unwrap_or_else(|| "server".to_string()),
        node_type: config.node_type.clone(),
        relay_port: config.port,
        web_port: config.web_port,
        key_db,
        server_secret,
        gossip_tx: Some(gossip_tx),
        swarm_tx: Some(swarm_web_tx),
        config_reload_tx: Some(config_reload_tx),
        world_dir: world_dir_str,
        ..Default::default()
    }));

    // World systems
    let world_enabled = config.world_enabled;
    let world = if world_enabled {
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
                .route("/api/logs", get(web_api_logs))
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
                .route("/api/v1/sync/content", get(api_v1_sync_content))
                // ── Meshsite content API ───────────────────────────────────
                .route("/api/v1/content", post(api_v1_post_content).get(api_v1_list_content))
                .route("/api/v1/content/post", post(api_v1_quick_post))
                .route("/api/v1/content/:id", get(api_v1_get_content))
                // ── World placed objects (modular placement) ───────────────
                .route("/api/v1/world/objects",
                    get(api_v1_world_objects_get).post(api_v1_world_objects_post))
                .route("/api/v1/world/objects/:id", axum::routing::delete(api_v1_world_objects_delete))
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
        config, Arc::clone(&shared), local_peer_id.to_string(), public_ip, gossip_rx, swarm_web_rx, config_reload_rx,
    );

    // Channel for terrain workers (or future callers) to send tile requests through the swarm.
    let (tile_req_tx, mut tile_req_rx) = tokio::sync::mpsc::channel::<(
        PeerId,
        TileRequest,
        tokio::sync::oneshot::Sender<TileResponse>,
    )>(64);
    // Keep tx alive for future terrain-worker integration.
    let _ = &tile_req_tx;
    app_state.log("✅ Metaverse server started");
    if let Some(ref err) = config_error {
        app_state.log(err.clone());
        app_state.log("⚠️  Check server.json — strings must be quoted, e.g. \"node_name\": \"MyServer\"".to_string());
    }
    // Log effective config values so operator can confirm config is being read
    app_state.log(format!("📋 Config: name={}, world_dir={}, srtm={}, api_key={}",
        app_state.config.node_name.as_deref().unwrap_or("(none)"),
        app_state.config.world_dir.as_deref().unwrap_or("world_data"),
        if app_state.config.download_all_srtm { "enabled" } else { "disabled" },
        if app_state.config.data.opentopography_api_key.is_empty() { "NOT SET" } else { "set" },
    ));
    // Create world data directories up front so operators can see where data goes
    let world_data_root = std::path::PathBuf::from(
        app_state.config.world_dir.as_deref().unwrap_or("world_data")
    );
    std::fs::create_dir_all(world_data_root.join("elevation_cache")).ok();
    std::fs::create_dir_all(world_data_root.join("osm")).ok();
    std::fs::create_dir_all(world_data_root.join("terrain")).ok();
    app_state.log(format!("📁 World data dir: {}", world_data_root.display()));
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
    // Queue DHT provider announcements for cached OSM tiles — announce at 1°×1° granularity
    // (not per-tile: millions of 0.01° keys would overflow any DHT provider table)
    {
        let osm_dir = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        ).join("osm");
        let mut region_keys: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
        if let Ok(entries) = std::fs::read_dir(&osm_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) {
                    if name.starts_with("osm_") && name.ends_with(".bin") {
                        let parts: Vec<&str> = name[4..name.len()-4].split('_').collect();
                        if parts.len() == 4 {
                            if let (Ok(s), Ok(w)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                                // Announce at 1°×1° granularity — same as SRTM
                                region_keys.insert((s.floor() as i32, w.floor() as i32));
                            }
                        }
                    }
                }
            }
        }
        let tile_count = region_keys.len();
        for (lat, lon) in region_keys {
            app_state.pending_dht_provide.push(metaverse_core::osm::osm_dht_key(
                lat as f64, lon as f64, lat as f64 + 1.0, lon as f64 + 1.0,
            ));
        }
        if tile_count > 0 {
            app_state.log(format!("📡 Queued DHT announcements for {} OSM region(s)", tile_count));
        }
    }
    // Queue DHT provider announcements for all cached elevation tiles
    {
        let elev_dir = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        ).join("elevation_cache");
        let mut elev_count = 0usize;
        // Structure: elevation_cache/N{lat}/E{lon}/srtm_n{lat}_e{lon}.tif
        if let Ok(lat_dirs) = std::fs::read_dir(&elev_dir) {
            for lat_entry in lat_dirs.flatten() {
                let lat_name = lat_entry.file_name().to_string_lossy().to_string();
                let lat: i32 = if let Some(n) = lat_name.strip_prefix('N') {
                    n.parse().unwrap_or(i32::MAX)
                } else if let Some(s) = lat_name.strip_prefix('S') {
                    -(s.parse::<i32>().unwrap_or(i32::MAX))
                } else { continue };
                if lat == i32::MAX { continue; }
                if let Ok(lon_dirs) = std::fs::read_dir(lat_entry.path()) {
                    for lon_entry in lon_dirs.flatten() {
                        let lon_name = lon_entry.file_name().to_string_lossy().to_string();
                        let lon: i32 = if let Some(e) = lon_name.strip_prefix('E') {
                            e.parse().unwrap_or(i32::MAX)
                        } else if let Some(w) = lon_name.strip_prefix('W') {
                            -(w.parse::<i32>().unwrap_or(i32::MAX))
                        } else { continue };
                        if lon == i32::MAX { continue; }
                        app_state.pending_dht_provide.push(metaverse_core::elevation::elevation_dht_key(lat, lon));
                        elev_count += 1;
                    }
                }
            }
        }
        if elev_count > 0 {
            app_state.log(format!("📡 Queued DHT announcements for {} elevation tile(s)", elev_count));
        }
    }
    // Bulk download: download_on_start bboxes
    if !app_state.config.download_on_start.is_empty() {
        let bboxes = app_state.config.download_on_start.clone();
        let world_dir_pb = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        );
        let endpoints = app_state.config.data.overpass_endpoints.clone();
        let elev_api_key = app_state.config.data.opentopography_api_key.clone();
        let prefetch_swarm_tx = shared.read().unwrap().swarm_tx.clone();
        tokio::spawn(async move {
            bulk_download_task(bboxes, world_dir_pb, endpoints, elev_api_key, prefetch_swarm_tx).await;
        });
        app_state.log(format!("📥 Bulk download: {} bbox(es) queued", app_state.config.download_on_start.len()));
    }

    // On-demand SRTM priority downloader — always active so clients can get tiles
    // even when global download hasn't reached that region yet.
    {
        let world_dir_srtm = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        );
        let api_key = app_state.config.data.opentopography_api_key.clone();
        let swarm_tx_srtm = shared.read().unwrap().swarm_tx.clone();
        let task_log_srtm = Arc::clone(&shared.read().unwrap().task_log);
        let (srtm_prio_tx, srtm_prio_rx) = tokio::sync::mpsc::channel::<(i32, i32)>(512);
        app_state.srtm_priority_tx = Some(srtm_prio_tx);
        tokio::spawn(async move {
            download_srtm_on_demand_task(world_dir_srtm, api_key, srtm_prio_rx, swarm_tx_srtm, task_log_srtm).await;
        });
    }

    // Global SRTM download
    if app_state.config.download_all_srtm && !app_state.config.data.opentopography_api_key.is_empty() {
        let world_dir_srtm = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        );
        let api_key = app_state.config.data.opentopography_api_key.clone();
        let swarm_tx_srtm = shared.read().unwrap().swarm_tx.clone();
        let task_log_srtm = Arc::clone(&shared.read().unwrap().task_log);
        tokio::spawn(async move {
            download_all_srtm_task(world_dir_srtm, api_key, swarm_tx_srtm, task_log_srtm).await;
        });
        app_state.log("📥 Global SRTM download started in background".to_string());
    } else if app_state.config.download_all_srtm {
        app_state.log("⚠️  download_all_srtm=true but opentopography_api_key is not set — skipping".to_string());
    }

    // PBF import (explicit path in config)
    if let Some(ref pbf_path) = app_state.config.data.osm_pbf_path.clone() {
        let pbf = std::path::PathBuf::from(pbf_path);
        let world_dir_pbf = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        );
        let task_log_pbf = Arc::clone(&shared.read().unwrap().task_log);
        app_state.log(format!("📥 PBF import: {}", pbf.display()));
        tokio::spawn(async move {
            import_pbf_task(pbf, world_dir_pbf, task_log_pbf).await;
        });
    }

    // Auto-convert any existing PBF files in osm/ that have not yet been tiled.
    // Conversions run one-at-a-time (smallest file first) to avoid exhausting RAM.
    // The same serialisation lock is passed to the download task so no two conversions overlap.
    let conv_lock: Arc<std::sync::Mutex<()>> = Arc::new(std::sync::Mutex::new(()));
    {
        let osm_dir = std::path::PathBuf::from(
            app_state.config.world_dir.as_deref().unwrap_or("world_data")
        ).join("osm");

        // Count already-ready v7 tiles before triggering any conversion.
        let ready_tiles = std::fs::read_dir(&osm_dir).ok()
            .map(|entries| entries.flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("bin"))
                .count())
            .unwrap_or(0);
        app_state.log(format!("🗺  [OSM] {} tile(s) ready in cache", ready_tiles));

        let mut pbfs_to_convert: Vec<(u64, std::path::PathBuf)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&osm_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("pbf") {
                    if let Ok(meta) = std::fs::metadata(&path) {
                        if meta.len() > 1_000_000 {
                            pbfs_to_convert.push((meta.len(), path));
                        }
                    }
                }
            }
        }
        if !pbfs_to_convert.is_empty() {
            // Sort smallest-first so quick ones finish and release memory before big ones start.
            pbfs_to_convert.sort_by_key(|(sz, _)| *sz);
            let task_log_c = Arc::clone(&shared.read().unwrap().task_log);
            let lock_c = Arc::clone(&conv_lock);
            let n_pbfs = pbfs_to_convert.len();
            app_state.log(format!("🔄 [OSM] {} PBF file(s) queued for tile conversion (runs in background)", n_pbfs));
            for (_, path) in pbfs_to_convert {
                let task_log_cc = Arc::clone(&task_log_c);
                let osm_dir_c = osm_dir.clone();
                let lock_cc = Arc::clone(&lock_c);
                let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                tokio::task::spawn_blocking(move || {
                    { if let Ok(mut b) = task_log_cc.lock() { b.push(format!("🔄 [OSM] {} — starting tile conversion…", fname)); } }
                    let _guard = lock_cc.lock().unwrap(); // serialise: only one conversion at a time
                    match metaverse_core::osm::import_pbf_with_log(&path, &osm_dir_c, Arc::clone(&task_log_cc)) {
                        Ok(n) => { if let Ok(mut buf) = task_log_cc.lock() { buf.push(format!("✅ [OSM] {} — {} new tile(s) written", fname, n)); } }
                        Err(e) => { if let Ok(mut buf) = task_log_cc.lock() { buf.push(format!("❌ [OSM] {} conversion failed: {}", fname, e)); } }
                    }
                });
            }
        } else {
            app_state.log("🗺  [OSM] No PBF files found — tiles served from cache only".to_string());
        }
    }

    // Geofabrik OSM download — all continents or specific regions
    {
        const ALL_CONTINENTS: &[&str] = &[
            "africa", "antarctica", "asia", "australia-oceania",
            "central-america", "europe", "north-america", "south-america",
        ];
        let mut regions = app_state.config.osm_download_regions.clone();
        if app_state.config.download_all_osm {
            for c in ALL_CONTINENTS {
                if !regions.iter().any(|r| r == c) {
                    regions.push(c.to_string());
                }
            }
        }
        if !regions.is_empty() {
            let world_dir_osm = std::path::PathBuf::from(
                app_state.config.world_dir.as_deref().unwrap_or("world_data")
            );
            let task_log_osm = Arc::clone(&shared.read().unwrap().task_log);
            let swarm_tx_osm = shared.read().unwrap().swarm_tx.clone();
            app_state.log(format!("📥 Geofabrik OSM download started for {} region(s)", regions.len()));
            let lock_osm = Arc::clone(&conv_lock);
            tokio::spawn(async move {
                download_osm_regions_task(regions, world_dir_osm, swarm_tx_osm, task_log_osm, lock_osm).await;
            });
        }
    }

    publish_node_capabilities(&local_peer_id.to_string(), &app_state.config, &mut swarm);
    app_state.log(format!("📡 NodeCapabilities published (tier=server, always_on={})", app_state.config.always_on));

    if headless {
        run_headless(swarm, app_state, world, tile_req_rx).await
    } else {
        run_tui(swarm, app_state, world, tile_req_rx).await
    }
}

// ─── Headless loop ───────────────────────────────────────────────────────────

async fn run_headless(
    mut swarm: libp2p::Swarm<ServerBehaviour>,
    mut state: AppState,
    mut world: Option<WorldSystems>,
    mut tile_req_rx: tokio::sync::mpsc::Receiver<(PeerId, TileRequest, tokio::sync::oneshot::Sender<TileResponse>)>,
) -> Result<(), Box<dyn Error>> {
    let mut world_config = state.config.clone();
    let mut world_tick = tokio::time::interval(Duration::from_millis(50));
    let mut stats_tick = tokio::time::interval(Duration::from_secs(30));
    let mut sync_tick = tokio::time::interval(Duration::from_secs(600)); // 10-min server sync
    let mut caps_tick = tokio::time::interval(Duration::from_secs(1800)); // 30-min caps refresh
    let update_interval = state.config.update_check_interval_secs.max(60);
    let mut update_tick = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(update_interval),
        Duration::from_secs(update_interval),
    );
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
                // DHT provider announcements (startup + newly written chunks) — max 20 per tick
                {
                    let count = state.pending_dht_provide.len().min(20);
                    let keys: Vec<_> = state.pending_dht_provide.drain(..count).collect();
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
                // Periodic server-to-server key + content sync
                sync_keys_from_servers(&state).await;
                sync_content_from_servers(&state).await;
            }
            _ = update_tick.tick() => {
                let repo = state.config.github_repo.clone();
                if !repo.is_empty() {
                    let current = env!("CARGO_PKG_VERSION");
                    if let Some((tag, _url, _notes)) = metaverse_core::autoupdate::check_for_update(&repo, current).await {
                        state.log(format!("🔄 Update available: {} — will apply on next restart", tag));
                        if let Ok(mut st) = state.shared.write() {
                            st.update_available = Some(tag);
                        }
                    }
                }
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
            // SwarmActions queued by web handlers (content DHT puts, etc.)
            Some(action) = state.swarm_web_rx.recv() => {
                apply_swarm_actions(vec![action], &mut state, &mut swarm);
            }
            // Config hot-reload from web handler or SIGHUP task
            Some(new_cfg) = state.config_reload_rx.recv() => {
                apply_config_hot_reload(&mut state, new_cfg, &mut world_config);
            }
            // P2P tile requests from terrain workers
            Some((peer_id, req, resp_tx)) = tile_req_rx.recv() => {
                apply_swarm_actions(
                    vec![SwarmAction::TileRequest { peer_id, request: req, response_tx: resp_tx }],
                    &mut state, &mut swarm,
                );
            }
        }
    }
}

// ─── TUI loop ────────────────────────────────────────────────────────────────

async fn run_tui(
    mut swarm: libp2p::Swarm<ServerBehaviour>,
    mut state: AppState,
    mut world: Option<WorldSystems>,
    mut tile_req_rx: tokio::sync::mpsc::Receiver<(PeerId, TileRequest, tokio::sync::oneshot::Sender<TileResponse>)>,
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
    let update_interval_tui = state.config.update_check_interval_secs.max(60);
    let mut update_tick_tui = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(update_interval_tui),
        Duration::from_secs(update_interval_tui),
    );
    let mut world_config = state.config.clone();
    let dummy_world = WorldStats::default();

    // Spawn SIGHUP handler — sends reloaded config into config_reload_rx (Unix only).
    spawn_sighup_handler(&state);

    // SIGTERM / SIGINT → clean shutdown via a shared channel.
    let (quit_tx, mut quit_rx) = tokio::sync::mpsc::channel::<()>(1);
    #[cfg(unix)]
    {
        let tx = quit_tx.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            if let Ok(mut s) = signal(SignalKind::terminate()) {
                if s.recv().await.is_some() { let _ = tx.send(()).await; }
            }
        });
        let tx = quit_tx.clone();
        tokio::spawn(async move {
            use tokio::signal::unix::{signal, SignalKind};
            if let Ok(mut s) = signal(SignalKind::interrupt()) {
                if s.recv().await.is_some() { let _ = tx.send(()).await; }
            }
        });
    }

    let result: Result<(), Box<dyn Error>> = async {
        loop {
            tokio::select! {
                Some(_) = quit_rx.recv() => { state.should_quit = true; }
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
                    // DHT provider announcements — max 20 per tick
                    {
                        let count = state.pending_dht_provide.len().min(20);
                        let keys: Vec<_> = state.pending_dht_provide.drain(..count).collect();
                        for key in keys {
                            apply_swarm_actions(vec![SwarmAction::StartProviding(key)], &mut state, &mut swarm);
                        }
                    }
                }
                _ = sync_tick.tick() => {
                    sync_keys_from_servers(&state).await;
                    sync_content_from_servers(&state).await;
                }
                _ = update_tick_tui.tick() => {
                    let repo = state.config.github_repo.clone();
                    if !repo.is_empty() {
                        let current = env!("CARGO_PKG_VERSION");
                        if let Some((tag, _url, _notes)) = metaverse_core::autoupdate::check_for_update(&repo, current).await {
                            state.log(format!("🔄 Update available: {} — will apply on next restart", tag));
                            if let Ok(mut st) = state.shared.write() {
                                st.update_available = Some(tag);
                            }
                        }
                    }
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
                // SwarmActions queued by web handlers (content DHT puts, etc.)
                Some(action) = state.swarm_web_rx.recv() => {
                    apply_swarm_actions(vec![action], &mut state, &mut swarm);
                }
                // Config hot-reload from web handler or SIGHUP task
                Some(new_cfg) = state.config_reload_rx.recv() => {
                    apply_config_hot_reload(&mut state, new_cfg, &mut world_config);
                }
                // P2P tile requests from terrain workers
                Some((peer_id, req, resp_tx)) = tile_req_rx.recv() => {
                    apply_swarm_actions(
                        vec![SwarmAction::TileRequest { peer_id, request: req, response_tx: resp_tx }],
                        &mut state, &mut swarm,
                    );
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

// ─── Bulk download task ───────────────────────────────────────────────────────

/// Download all OSM tiles and SRTM elevation tiles within the given bounding boxes.
/// OSM is fetched in 0.01° tiles from Overpass; SRTM in 1° tiles from OpenTopography.
/// Already-cached tiles are skipped. Results are announced to DHT.
/// Download all global SRTM 1°×1° tiles. SRTM covers lat -60..60, lon -180..180.
/// Skips already-cached tiles. Announces each downloaded tile to DHT.

/// On-demand SRTM downloader: listens for (lat, lon) requests via channel,
/// downloads those specific tiles immediately (deduplicates via seen set),
/// and announces to DHT. Tiles outside -60..60 are silently skipped (no SRTM data).
async fn download_srtm_on_demand_task(
    world_dir: std::path::PathBuf,
    api_key: String,
    mut rx: tokio::sync::mpsc::Receiver<(i32, i32)>,
    swarm_tx: Option<tokio::sync::mpsc::Sender<SwarmAction>>,
    task_log: Arc<std::sync::Mutex<Vec<String>>>,
) {
    macro_rules! tlog {
        ($($arg:tt)*) => {{ if let Ok(mut buf) = task_log.lock() { buf.push(format!($($arg)*)); } }};
    }
    let elev_dir = world_dir.join("elevation_cache");
    let mut seen = std::collections::HashSet::<(i32, i32)>::new();

    while let Some((lat, lon)) = rx.recv().await {
        // SRTM only covers lat -60..60
        if lat < -60 || lat >= 60 { continue; }
        if !seen.insert((lat, lon)) { continue; } // already downloaded or in-progress

        let lat_dir = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", lat.unsigned_abs()) };
        let lon_dir = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lon.unsigned_abs()) };
        let tile_name = format!("srtm_{}{:02}_{}{:03}.tif",
            if lat >= 0 { 'n' } else { 's' }, lat.unsigned_abs(),
            if lon >= 0 { 'e' } else { 'w' }, lon.unsigned_abs());
        let hgt_name = format!("{}{:02}{}{:03}.hgt",
            if lat >= 0 { 'N' } else { 'S' }, lat.unsigned_abs(),
            if lon >= 0 { 'E' } else { 'W' }, lon.unsigned_abs());
        let tile_path = elev_dir.join(&lat_dir).join(&lon_dir).join(&tile_name);
        let hgt_path  = elev_dir.join(&lat_dir).join(&lon_dir).join(&hgt_name);

        let cached_ok = |p: &std::path::Path| p.exists() && p.metadata().map(|m| m.len()).unwrap_or(0) >= 1024;
        if cached_ok(&tile_path) || cached_ok(&hgt_path) { continue; }

        let key_c = api_key.clone();
        let tp = tile_path.clone();
        let hp = hgt_path.clone();
        tlog!("📥 [SRTM] On-demand download: lat={} lon={}", lat, lon);

        let result = tokio::task::spawn_blocking(move || -> Result<SrtmSource, String> {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build().map_err(|e| e.to_string())?;
            // Source 1: OpenTopography
            if !key_c.is_empty() {
                let url = format!(
                    "https://portal.opentopography.org/API/globaldem?demtype=SRTMGL1&\
                     south={}&north={}&west={}&east={}&outputFormat=GTiff&API_Key={}",
                    lat, lat + 1, lon, lon + 1, key_c);
                if let Ok(resp) = client.get(&url).send() {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes() {
                            if bytes.len() >= 1024 {
                                std::fs::create_dir_all(tp.parent().unwrap()).ok();
                                if std::fs::write(&tp, &bytes).is_ok() { return Ok(SrtmSource::OpenTopography); }
                            }
                        }
                    }
                }
            }
            // Source 2: Copernicus DEM GLO-30
            {
                let ns = if lat >= 0 { "N" } else { "S" }; let ew = if lon >= 0 { "E" } else { "W" };
                let la = lat.unsigned_abs(); let lo = lon.unsigned_abs();
                let tile_id = format!("Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM", ns, la, ew, lo);
                let url = format!("https://copernicus-dem-30m.s3.amazonaws.com/{}/{}.tif", tile_id, tile_id);
                if let Ok(resp) = client.get(&url).send() {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes() {
                            if bytes.len() >= 1024 {
                                std::fs::create_dir_all(tp.parent().unwrap()).ok();
                                if std::fs::write(&tp, &bytes).is_ok() { return Ok(SrtmSource::Copernicus); }
                            }
                        }
                    }
                }
            }
            // Source 3: AWS Skadi (HGT.gz)
            {
                let ns = if lat >= 0 { "N" } else { "S" }; let ew = if lon >= 0 { "E" } else { "W" };
                let la = lat.unsigned_abs(); let lo = lon.unsigned_abs();
                let url = format!(
                    "https://s3.amazonaws.com/elevation-tiles-prod/skadi/{}{:02}/{}{:02}{}{:03}.hgt.gz",
                    ns, la, ns, la, ew, lo);
                if let Ok(resp) = client.get(&url).send() {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes() {
                            if bytes.len() >= 512 {
                                use std::io::Read;
                                let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
                                let mut raw = Vec::new();
                                if decoder.read_to_end(&mut raw).is_ok() && raw.len() >= 1024 {
                                    std::fs::create_dir_all(hp.parent().unwrap()).ok();
                                    if std::fs::write(&hp, &raw).is_ok() { return Ok(SrtmSource::Skadi); }
                                }
                            }
                        }
                    }
                }
            }
            Err("all_sources_failed".to_string())
        }).await;

        match result {
            Ok(Ok(src)) => {
                tlog!("✅ [SRTM] On-demand: lat={} lon={} via {}", lat, lon, src.name());
                if let Some(ref tx) = swarm_tx {
                    let key = metaverse_core::elevation::elevation_dht_key(lat, lon);
                    let _ = tx.send(SwarmAction::StartProviding(key)).await;
                }
            }
            Ok(Err(ref e)) if e == "all_sources_failed" => {
                // Ocean tile — mark as seen so we don't retry
            }
            Ok(Err(ref e)) => {
                tlog!("⚠️ [SRTM] On-demand failed lat={} lon={}: {}", lat, lon, e);
                seen.remove(&(lat, lon)); // allow retry later
            }
            Err(_) => {
                seen.remove(&(lat, lon));
            }
        }
    }
}

async fn download_all_srtm_task(
    world_dir: std::path::PathBuf,
    api_key: String,
    swarm_tx: Option<tokio::sync::mpsc::Sender<SwarmAction>>,
    task_log: Arc<std::sync::Mutex<Vec<String>>>,
) {
    macro_rules! tlog {
        ($($arg:tt)*) => {{
            if let Ok(mut buf) = task_log.lock() { buf.push(format!($($arg)*)); }
        }};
    }

    let elev_dir = world_dir.join("elevation_cache");
    std::fs::create_dir_all(&elev_dir).ok();
    let total_tiles: u32 = 120 * 360;
    let mut total = 0u32;
    let mut skipped = 0u32;
    let mut errors = 0u32;
    let mut processed = 0u32;

    tlog!("📥 [SRTM] Starting global download: {} tiles (lat -60..60, all lon)", total_tiles);
    tlog!("ℹ️  [SRTM] Sources: OpenTopography (key) → Copernicus DEM AWS → AWS Terrain Tiles (skadi)");

    let mut opentopo_rate_limited_until: Option<std::time::Instant> = None;

    for lat in -60i32..60 {
        for lon in -180i32..180 {
            let lat_dir = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", lat.unsigned_abs()) };
            let lon_dir = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lon.unsigned_abs()) };
            let tile_name = format!("srtm_{}{:02}_{}{:03}.tif",
                if lat >= 0 { 'n' } else { 's' }, lat.unsigned_abs(),
                if lon >= 0 { 'e' } else { 'w' }, lon.unsigned_abs());
            let tile_path = elev_dir.join(&lat_dir).join(&lon_dir).join(&tile_name);
            // HGT naming used by Skadi fallback downloads
            let hgt_name = format!("{}{:02}{}{:03}.hgt",
                if lat >= 0 { 'N' } else { 'S' }, lat.unsigned_abs(),
                if lon >= 0 { 'E' } else { 'W' }, lon.unsigned_abs());
            let hgt_path = elev_dir.join(&lat_dir).join(&lon_dir).join(&hgt_name);
            processed += 1;

            // Skip if any valid cached file exists
            let cached_ok = |p: &std::path::Path| p.exists() && p.metadata().map(|m| m.len()).unwrap_or(0) >= 1024;
            if cached_ok(&tile_path) || cached_ok(&hgt_path) {
                skipped += 1; continue;
            }
            // Remove corrupt 0-byte files
            if tile_path.exists() { let _ = std::fs::remove_file(&tile_path); }
            if hgt_path.exists() { let _ = std::fs::remove_file(&hgt_path); }

            let opentopo_ok = !opentopo_rate_limited_until.map(|u| std::time::Instant::now() < u).unwrap_or(false);
            let key_c = api_key.clone();
            let tp = tile_path.clone();
            let hp = hgt_path.clone();

            let result = tokio::task::spawn_blocking(move || -> Result<SrtmSource, String> {
                let client = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(60))
                    .build().map_err(|e| e.to_string())?;

                // --- Source 1: OpenTopography (GeoTIFF, requires API key) ---
                if opentopo_ok && !key_c.is_empty() {
                    let south = lat as f64; let north = (lat + 1) as f64;
                    let west = lon as f64; let east = (lon + 1) as f64;
                    let url = format!(
                        "https://portal.opentopography.org/API/globaldem?demtype=SRTMGL1&\
                         south={}&north={}&west={}&east={}&outputFormat=GTiff&API_Key={}",
                        south, north, west, east, key_c);
                    match client.get(&url).send() {
                        Ok(resp) => {
                            let status = resp.status();
                            if status.is_success() {
                                if let Ok(bytes) = resp.bytes() {
                                    if bytes.len() >= 1024 {
                                        std::fs::create_dir_all(tp.parent().unwrap()).ok();
                                        if std::fs::write(&tp, &bytes).is_ok() {
                                            return Ok(SrtmSource::OpenTopography);
                                        }
                                    }
                                }
                            } else {
                                let body = resp.text().unwrap_or_default();
                                if status.as_u16() == 401 && body.to_lowercase().contains("rate limit") {
                                    return Err(format!("RATE_LIMITED:{}", body));
                                }
                                // Fall through to next source
                            }
                        }
                        Err(_) => { /* network error, try next source */ }
                    }
                }

                // --- Source 2: Copernicus DEM GLO-30 (AWS S3, free, no key, GeoTIFF) ---
                {
                    let ns = if lat >= 0 { "N" } else { "S" };
                    let ew = if lon >= 0 { "E" } else { "W" };
                    let la = lat.unsigned_abs(); let lo = lon.unsigned_abs();
                    let tile_id = format!("Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM", ns, la, ew, lo);
                    let url = format!("https://copernicus-dem-30m.s3.amazonaws.com/{}/{}.tif", tile_id, tile_id);
                    match client.get(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(bytes) = resp.bytes() {
                                if bytes.len() >= 1024 {
                                    std::fs::create_dir_all(tp.parent().unwrap()).ok();
                                    if std::fs::write(&tp, &bytes).is_ok() {
                                        return Ok(SrtmSource::Copernicus);
                                    }
                                }
                            }
                        }
                        _ => {} // Fall through
                    }
                }

                // --- Source 3: AWS Terrain Tiles / Skadi (free, no key, HGT.gz) ---
                {
                    let ns = if lat >= 0 { "N" } else { "S" };
                    let ew = if lon >= 0 { "E" } else { "W" };
                    let la = lat.unsigned_abs(); let lo = lon.unsigned_abs();
                    let url = format!(
                        "https://s3.amazonaws.com/elevation-tiles-prod/skadi/{}{:02}/{}{:02}{}{:03}.hgt.gz",
                        ns, la, ns, la, ew, lo);
                    match client.get(&url).send() {
                        Ok(resp) if resp.status().is_success() => {
                            if let Ok(bytes) = resp.bytes() {
                                if bytes.len() >= 512 {
                                    // Decompress gzip
                                    use std::io::Read;
                                    let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
                                    let mut raw = Vec::new();
                                    if decoder.read_to_end(&mut raw).is_ok() && raw.len() >= 1024 {
                                        std::fs::create_dir_all(hp.parent().unwrap()).ok();
                                        if std::fs::write(&hp, &raw).is_ok() {
                                            return Ok(SrtmSource::Skadi);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                Err("all_sources_failed".to_string())
            }).await;

            match result {
                Ok(Ok(src)) => {
                    total += 1;
                    if let Some(ref tx) = swarm_tx {
                        let key = metaverse_core::elevation::elevation_dht_key(lat, lon);
                        let _ = tx.send(SwarmAction::StartProviding(key)).await;
                    }
                    if total <= 3 || total % 500 == 0 {
                        tlog!("📥 [SRTM] {}/{}: downloaded via {}", lat, lon, src.name());
                    }
                }
                Ok(Err(ref e)) if e.starts_with("RATE_LIMITED:") => {
                    let wait_secs = 6 * 3600u64;
                    tlog!("⏸ [SRTM] OpenTopography rate limited (200 calls/day). Pausing {} h, using free sources meanwhile",
                        wait_secs / 3600);
                    opentopo_rate_limited_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(wait_secs));
                    errors += 1;
                    // Don't skip tile — next iteration will try Copernicus/Skadi
                    // Re-enqueue this tile by decrementing to re-run: mark as error only
                }
                Ok(Err(ref e)) if e == "all_sources_failed" => {
                    // Likely ocean tile — all sources returned empty/404
                    errors += 1;
                }
                Ok(Err(ref e)) => {
                    errors += 1;
                    if errors <= 5 || errors % 200 == 0 {
                        tlog!("⚠️  [SRTM] {}/{}: {}", lat, lon, e);
                    }
                }
                Err(ref e) => {
                    errors += 1;
                    tlog!("⚠️  [SRTM] task error {}/{}: {}", lat, lon, e);
                }
            }

            // Resume OpenTopography after rate-limit window expires
            if let Some(until) = opentopo_rate_limited_until {
                let now = std::time::Instant::now();
                if now >= until {
                    opentopo_rate_limited_until = None;
                    tlog!("▶️  [SRTM] OpenTopography rate-limit window expired, resuming");
                }
            }

            // Progress every latitude row (~1%)
            if processed % 360 == 0 {
                let pct = processed * 100 / total_tiles;
                tlog!("📥 [SRTM] {}% ({}/{}) — {} downloaded, {} cached, {} ocean/void",
                    pct, processed, total_tiles, total, skipped, errors);
            }
            // Small delay between tiles (free sources are unlimited but be polite)
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        }
    }
    tlog!("✅ [SRTM] Complete: {} downloaded, {} already cached, {} ocean/void",
        total, skipped, errors);
}

#[derive(Debug)]
enum SrtmSource { OpenTopography, Copernicus, Skadi }
impl SrtmSource {
    fn name(&self) -> &'static str {
        match self {
            SrtmSource::OpenTopography => "OpenTopography",
            SrtmSource::Copernicus => "Copernicus DEM AWS",
            SrtmSource::Skadi => "AWS Terrain Tiles",
        }
    }
}

async fn import_pbf_task(
    pbf_path: std::path::PathBuf,
    world_dir: std::path::PathBuf,
    task_log: Arc<std::sync::Mutex<Vec<String>>>,
) {
    macro_rules! tlog {
        ($($arg:tt)*) => {{ if let Ok(mut buf) = task_log.lock() { buf.push(format!($($arg)*)); } }};
    }
    let osm_dir = world_dir.join("osm");
    std::fs::create_dir_all(&osm_dir).ok();
    tlog!("📥 [PBF] Starting import of {}", pbf_path.display());
    let log_c = Arc::clone(&task_log);
    let result = tokio::task::spawn_blocking(move || {
        metaverse_core::osm::import_pbf_with_log(&pbf_path, &osm_dir, log_c)
    }).await;
    match result {
        Ok(Ok(n)) => tlog!("✅ [PBF] Import complete: {} OSM tiles written", n),
        Ok(Err(e)) => tlog!("❌ [PBF] Import failed: {}", e),
        Err(e) => tlog!("❌ [PBF] Import task panicked: {}", e),
    }
}

/// Download OSM PBF files from Geofabrik for a list of region paths.
/// Region format: "europe/germany", "north-america/us/california", "australia-oceania", etc.
/// Files are saved to {world_dir}/osm/{slug}-latest.osm.pbf and skipped if already present.
/// After each successful download, automatically converts to bincode tiles in background.
async fn download_osm_regions_task(
    regions: Vec<String>,
    world_dir: std::path::PathBuf,
    swarm_tx: Option<tokio::sync::mpsc::Sender<SwarmAction>>,
    task_log: Arc<std::sync::Mutex<Vec<String>>>,
    conv_lock: Arc<std::sync::Mutex<()>>,
) {
    macro_rules! tlog {
        ($($arg:tt)*) => {{ if let Ok(mut buf) = task_log.lock() { buf.push(format!($($arg)*)); } }};
    }
    let osm_dir = world_dir.join("osm");
    std::fs::create_dir_all(&osm_dir).ok();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(7200)) // 2hr — planet files are large
        .user_agent("metaverse-server/geofabrik-downloader")
        .build()
    {
        Ok(c) => c,
        Err(e) => { tlog!("❌ [OSM] Failed to build HTTP client: {}", e); return; }
    };

    tlog!("📥 [OSM] Starting Geofabrik download for {} region(s)", regions.len());

    for region in &regions {
        // slug = last path component, e.g. "europe/germany" → "germany"
        let slug = region.trim_matches('/').split('/').last().unwrap_or(region.as_str()).to_string();
        let filename = format!("{}-latest.osm.pbf", slug);
        let dest = osm_dir.join(&filename);

        // Skip only if file exists and is >1MB (partial/empty files get re-downloaded)
        // Skip completed file
        if dest.exists() {
            if let Ok(meta) = std::fs::metadata(&dest) {
                if meta.len() > 1_000_000 {
                    tlog!("⏭ [OSM] {} already exists ({:.1} MB), skipping",
                        filename, meta.len() as f64 / 1_048_576.0);
                    continue;
                }
            }
            let _ = std::fs::remove_file(&dest);
        }

        let url = format!("https://download.geofabrik.de/{}-latest.osm.pbf", region.trim_matches('/'));

        // Check for a partial .tmp file to resume from
        let tmp = dest.with_extension("pbf.tmp");
        let resume_offset: u64 = tokio::fs::metadata(&tmp).await.map(|m| m.len()).unwrap_or(0);

        let req = if resume_offset > 0 {
            tlog!("⬇ [OSM] {} — resuming from {:.1} MB…", filename, resume_offset as f64 / 1_048_576.0);
            client.get(&url).header("Range", format!("bytes={}-", resume_offset))
        } else {
            client.get(&url)
        };

        match req.send().await {
            Err(e) => { tlog!("❌ [OSM] {} — request failed: {}", region, e); continue; }
            Ok(resp) if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
                // 416: range is beyond EOF — .tmp is already the full file
                tlog!("✅ [OSM] {} — download complete (already full), converting…", filename);
                if let Err(e) = tokio::fs::rename(&tmp, &dest).await {
                    tlog!("❌ [OSM] {} — rename failed: {}", filename, e);
                } else {
                    let dest_c = dest.clone(); let osm_dir_c = osm_dir.clone();
                    let task_log_c = Arc::clone(&task_log); let swarm_tx_c = swarm_tx.clone();
                    let lock_c = Arc::clone(&conv_lock);
                    tokio::task::spawn_blocking(move || {
                        let _guard = lock_c.lock().unwrap();
                        macro_rules! tlog2 { ($($a:tt)*) => {{ if let Ok(mut b) = task_log_c.lock() { b.push(format!($($a)*)); } }}; }
                        match metaverse_core::osm::import_pbf_with_log(&dest_c, &osm_dir_c, Arc::clone(&task_log_c)) {
                            Ok(n) => { tlog2!("✅ [OSM] {} → {} tiles", dest_c.file_name().unwrap_or_default().to_string_lossy(), n);
                                if let Some(tx) = swarm_tx_c { let _ = tx.blocking_send(SwarmAction::StartProviding(b"osm_tiles_updated".to_vec())); } }
                            Err(e) => tlog2!("❌ [OSM] conversion failed: {}", e),
                        }
                    });
                }
                continue;
            }
            Ok(resp) if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT => {
                tlog!("❌ [OSM] {} — HTTP {}", region, resp.status()); continue;
            }
            Ok(resp) => {
                let content_len = resp.content_length().unwrap_or(0);
                let total_est = if resume_offset > 0 { resume_offset + content_len } else { content_len };
                tlog!("⬇ [OSM] {} — streaming{}…", filename,
                    if total_est > 0 { format!(" ({:.1} MB total)", total_est as f64 / 1_048_576.0) } else { String::new() });

                // Open for append if resuming, otherwise create fresh
                let mut file = if resume_offset > 0 {
                    match tokio::fs::OpenOptions::new().append(true).open(&tmp).await {
                        Ok(f) => f,
                        Err(e) => { tlog!("❌ [OSM] {} — open for resume failed: {}", filename, e); continue; }
                    }
                } else {
                    match tokio::fs::File::create(&tmp).await {
                        Ok(f) => f,
                        Err(e) => { tlog!("❌ [OSM] {} — create tmp failed: {}", filename, e); continue; }
                    }
                };

                use tokio::io::AsyncWriteExt;
                use futures::StreamExt as _;
                let mut stream = resp.bytes_stream();
                let mut written: u64 = resume_offset; // count total bytes including resumed portion
                let mut last_log = resume_offset;
                let mut ok = true;
                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Err(e) => { tlog!("❌ [OSM] {} — stream error at {:.1} MB: {}", filename, written as f64 / 1_048_576.0, e); ok = false; break; }
                        Ok(b) => {
                            if let Err(e) = file.write_all(&b).await {
                                tlog!("❌ [OSM] {} — write error: {}", filename, e); ok = false; break;
                            }
                            written += b.len() as u64;
                            if written - last_log >= 100 * 1_048_576 {
                                last_log = written;
                                tlog!("⬇ [OSM] {} — {:.1} MB written…", filename, written as f64 / 1_048_576.0);
                            }
                        }
                    }
                }
                drop(file);
                if ok && written > 1024 {
                    if let Err(e) = tokio::fs::rename(&tmp, &dest).await {
                        tlog!("❌ [OSM] {} — rename failed: {}", filename, e);
                    } else {
                        tlog!("✅ [OSM] {} complete ({:.1} MB) — starting tile conversion…", filename, written as f64 / 1_048_576.0);
                        let dest_c = dest.clone(); let osm_dir_c = osm_dir.clone();
                        let task_log_c = Arc::clone(&task_log); let swarm_tx_c = swarm_tx.clone();
                        let lock_c = Arc::clone(&conv_lock);
                        tokio::task::spawn_blocking(move || {
                            let _guard = lock_c.lock().unwrap();
                            macro_rules! tlog2 { ($($a:tt)*) => {{ if let Ok(mut b) = task_log_c.lock() { b.push(format!($($a)*)); } }}; }
                            match metaverse_core::osm::import_pbf_with_log(&dest_c, &osm_dir_c, Arc::clone(&task_log_c)) {
                                Ok(n) => { tlog2!("✅ [OSM] {} → {} tiles", dest_c.file_name().unwrap_or_default().to_string_lossy(), n);
                                    if let Some(tx) = swarm_tx_c { let _ = tx.blocking_send(SwarmAction::StartProviding(b"osm_tiles_updated".to_vec())); } }
                                Err(e) => tlog2!("❌ [OSM] {} conversion failed: {}", dest_c.file_name().unwrap_or_default().to_string_lossy(), e),
                            }
                        });
                    }
                } else {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    if !ok { /* error already logged */ } else {
                        tlog!("❌ [OSM] {} — response too small, invalid region?", filename);
                    }
                }
            }
        }
    }
    tlog!("✅ [OSM] Geofabrik download complete");
}

async fn bulk_download_task(
    bboxes: Vec<metaverse_core::node_config::DownloadBbox>,
    world_dir: std::path::PathBuf,
    overpass_endpoints: Vec<String>,
    opentopo_api_key: String,
    swarm_tx: Option<tokio::sync::mpsc::Sender<SwarmAction>>,
) {
    use metaverse_core::elevation::ElevationSource as _;

    let osm_dir = world_dir.join("osm");
    let elev_dir = world_dir.join("elevation_cache");
    std::fs::create_dir_all(&osm_dir).ok();
    std::fs::create_dir_all(&elev_dir).ok();

    for bbox in &bboxes {
        // ── OSM: 0.01° tiles ──────────────────────────────────────────────
        let tile = 0.01_f64;
        let mut lat = (bbox.south / tile).floor() * tile;
        while lat < bbox.north {
            let mut lon = (bbox.west / tile).floor() * tile;
            while lon < bbox.east {
                let (s, w, n, e) = (lat, lon, lat + tile, lon + tile);
                let cache = metaverse_core::osm::OsmDiskCache::new(&osm_dir);
                if cache.load(s, w, n, e).is_none() {
                    let osm_dir2 = osm_dir.clone();
                    let ep = overpass_endpoints.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        metaverse_core::osm::fetch_osm_for_bounds(s, w, n, e, &osm_dir2, &ep)
                    }).await;
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                }
                if let Some(ref tx) = swarm_tx {
                    let key = metaverse_core::osm::osm_dht_key(s, w, n, e);
                    let _ = tx.send(SwarmAction::StartProviding(key)).await;
                }
                lon += tile;
            }
            lat += tile;
        }

        // ── SRTM: 1° tiles ───────────────────────────────────────────────
        if !opentopo_api_key.is_empty() {
            let lat_lo = bbox.south.floor() as i32;
            let lat_hi = bbox.north.ceil() as i32;
            let lon_lo = bbox.west.floor() as i32;
            let lon_hi = bbox.east.ceil() as i32;
            for lat_tile in lat_lo..lat_hi {
                for lon_tile in lon_lo..lon_hi {
                    let lat_dir = if lat_tile >= 0 { format!("N{:02}", lat_tile) } else { format!("S{:02}", lat_tile.unsigned_abs()) };
                    let lon_dir = if lon_tile >= 0 { format!("E{:03}", lon_tile) } else { format!("W{:03}", lon_tile.unsigned_abs()) };
                    let tile_path = elev_dir.join(&lat_dir).join(&lon_dir)
                        .join(format!("srtm_n{:02}_e{:03}.tif", lat_tile.unsigned_abs(), lon_tile.unsigned_abs()));
                    if !tile_path.exists() {
                        let key_c = opentopo_api_key.clone();
                        let cache_c = elev_dir.clone();
                        let lat_f = lat_tile as f64 + 0.5;
                        let lon_f = lon_tile as f64 + 0.5;
                        let _ = tokio::task::spawn_blocking(move || {
                            let src = metaverse_core::elevation::OpenTopographySource::new(key_c, cache_c);
                            let gps = metaverse_core::coordinates::GPS::new(lat_f, lon_f, 0.0);
                            src.query(&gps)
                        }).await;
                        if tile_path.exists() {
                            if let Some(ref tx) = swarm_tx {
                                let key = metaverse_core::elevation::elevation_dht_key(lat_tile, lon_tile);
                                let _ = tx.send(SwarmAction::StartProviding(key)).await;
                            }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }
    }
    println!("✅ [Download] Bulk download complete");
}
