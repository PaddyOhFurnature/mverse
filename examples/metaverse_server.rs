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
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use clap::Parser;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
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
    pub world: WorldStats,
    pub net: NetStats,
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub ram_pct: f32,
    pub shedding_relay: bool,
    pub relay_port: u16,
    pub version: String,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            local_peer_id: String::new(), public_ip: String::new(), uptime_secs: 0,
            node_name: String::new(), node_type: "server".to_string(),
            peers: vec![], circuit_count: 0, total_connections: 0,
            total_reservations: 0, dht_peer_count: 0,
            world: WorldStats::default(), net: NetStats::default(),
            cpu_pct: 0.0, ram_used_mb: 0, ram_total_mb: 0, ram_pct: 0.0,
            shedding_relay: false,
            relay_port: 4001,
            version: env!("CARGO_PKG_VERSION").to_string(),
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
}

impl AppState {
    fn new(config: ServerConfig, shared: Arc<RwLock<SharedState>>,
           local_peer_id: String, public_ip: String) -> Self {
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

        // Load shedding: relay circuits
        let cpu_thresh = self.config.cpu_shed_threshold_pct;
        if cpu_thresh > 0 && self.cpu_pct > cpu_thresh as f32 {
            if !self.shedding_relay {
                self.shedding_relay = true;
                self.log(format!("⚠️  CPU {}% > {}% — shedding relay circuits", self.cpu_pct as u8, cpu_thresh));
            }
        } else {
            self.shedding_relay = false;
        }
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

// ─── Swarm event handler ─────────────────────────────────────────────────────

enum SwarmAction {
    AddKadAddress(PeerId, Multiaddr),
    RefreshDhtCount,
    DialPeer(PeerId, Multiaddr),
    SubscribeTopic(String),
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
                state.log(format!("📩 State request ({} bytes)", message.data.len()));
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

// ─── Web dashboard ───────────────────────────────────────────────────────────

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

    // Shared state for web server
    let shared = Arc::new(RwLock::new(SharedState {
        local_peer_id: local_peer_id.to_string(),
        public_ip: public_ip.clone(),
        node_name: config.node_name.clone().unwrap_or_else(|| "server".to_string()),
        node_type: config.node_type.clone(),
        relay_port: config.port,
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
                .route("/api/config", get(web_api_config))
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
        config, Arc::clone(&shared), local_peer_id.to_string(), public_ip,
    );
    app_state.log("✅ Metaverse server started");
    if world_enabled && world.is_some() {
        app_state.log(format!("🌍 World state: {}", app_state.config.world_dir.as_deref().unwrap_or("~/.metaverse/world_data")));
    }

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
    let world_config = state.config.clone();
    let mut world_tick = tokio::time::interval(Duration::from_millis(50));
    let mut stats_tick = tokio::time::interval(Duration::from_secs(30));
    let dummy_world = WorldStats::default();

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
            swarm_event = swarm.select_next_some() => {
                let actions = handle_swarm_event(swarm_event, &mut state);
                apply_swarm_actions(actions, &mut state, &mut swarm);
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
    let world_config = state.config.clone();
    let dummy_world = WorldStats::default();

    let result: Result<(), Box<dyn Error>> = async {
        loop {
            tokio::select! {
                _ = tui_tick.tick() => {
                    state.refresh_sys();
                    let ws = world.as_ref().map(|w| &w.stats).unwrap_or(&dummy_world);
                    terminal.draw(|f| draw(f, &state, ws))?;
                    state.sync_shared(ws);
                }
                _ = world_tick.tick() => {
                    if let Some(ref mut w) = world {
                        w.tick(&world_config);
                    }
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
