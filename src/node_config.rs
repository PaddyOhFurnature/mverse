//! Unified node configuration — shared by client, server, and relay.
//!
//! All nodes run the same code base. The binary just sets different defaults.
//! Client = everything. Server = client - graphics. Relay = server - world data.
//!
//! Config file: ./node.json  (relative to working directory — portable)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ─── Sub-structs ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct UiConfig {
    pub show_cpu: bool,
    pub show_ram: bool,
    pub show_dht: bool,
    pub refresh_ms: u64,
    pub max_log_entries: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self { show_cpu: true, show_ram: true, show_dht: true, refresh_ms: 500, max_log_entries: 1000 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct DataSourceConfig {
    /// Overpass API endpoints tried in order. Empty = use built-in defaults.
    pub overpass_endpoints: Vec<String>,
    /// OpenTopography API key for SRTM download. Empty = disable API download.
    pub opentopography_api_key: String,
    /// Path to a directory of pre-downloaded SRTM 1°×1° .tif files.
    /// Files must be named srtm_n{lat}_e{lon}.tif (standard naming).
    pub srtm_source_dir: Option<String>,
    /// Path to a local OSM PBF extract (e.g. from Geofabrik).
    /// When set, OSM tile queries check this file before hitting Overpass.
    pub osm_pbf_path: Option<String>,
}

impl Default for DataSourceConfig {
    fn default() -> Self {
        Self {
            overpass_endpoints: vec![],
            opentopography_api_key: String::new(),
            srtm_source_dir: None,
            osm_pbf_path: None,
        }
    }
}

/// Bounding box for bulk data download on startup.
/// Both OSM tiles and SRTM elevation tiles are downloaded for this area.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DownloadBbox {
    pub south: f64,
    pub west:  f64,
    pub north: f64,
    pub east:  f64,
}

// ─── Main config ─────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct NodeConfig {
    // ── Identity ─────────────────────────────────────────────────────────
    pub node_name:              Option<String>,
    /// Path to persistent Ed25519 keypair file (default: ./node.key)
    pub identity_file:          Option<String>,
    pub temp_identity:          bool,

    // ── Network / relay ──────────────────────────────────────────────────
    pub port:                   u16,
    /// WebSocket port (default port + 5000)
    pub ws_port:                Option<u16>,
    pub external_addr:          Option<String>,
    pub node_type:              String,
    pub priority_score:         u32,

    pub max_circuits:           usize,
    pub max_circuit_duration_secs: u64,
    pub max_circuit_bytes:      u64,

    pub peers:                  Vec<String>,
    pub blacklist:              Vec<String>,
    pub whitelist:              Vec<String>,
    pub priority_peers:         Vec<String>,

    pub max_bandwidth_mbps:     u32,
    pub max_peers:              u32,
    pub max_ping_ms:            u32,
    pub max_retries:            u32,
    pub always_on:              bool,

    // ── Feature flags (client has all true; strip down per node type) ──
    pub graphics_enabled:       bool,   // wgpu renderer — false for server/relay
    pub world_enabled:          bool,   // terrain/OSM/elevation — false for relay (unless relay wants to cache)
    pub relay_enabled:          bool,   // libp2p circuit relay
    pub tui_enabled:            bool,   // terminal TUI dashboard
    pub web_enabled:            bool,   // REST API + web dashboard

    // ── World / tile data ─────────────────────────────────────────────────
    pub world_dir:              Option<String>,
    pub max_world_data_gb:      u32,
    pub max_loaded_chunks:      usize,
    pub chunk_load_radius_m:    f64,
    pub chunk_unload_radius_m:  f64,
    pub world_save_interval_secs: u64,

    // ── Storage budget ────────────────────────────────────────────────────
    /// Total disk budget for cached tiles/chunks (GB). 0 = unlimited.
    pub storage_budget_gb:      u32,
    /// Cache OSM/elevation/terrain tiles within this many chunks of visited area.
    pub cache_radius_chunks:    u32,

    // ── Data sources ──────────────────────────────────────────────────────
    pub data: DataSourceConfig,

    // ── Tile distribution ─────────────────────────────────────────────────
    /// Bounding boxes to bulk-download OSM + SRTM data for on startup.
    /// Any node (client or server) will download all missing tiles in these boxes.
    pub download_on_start: Vec<DownloadBbox>,

    /// Download all global SRTM 1°×1° elevation tiles on startup.
    /// Requires opentopography_api_key in [data] to be set.
    /// Tiles already on disk are skipped. Runs in background.
    pub download_all_srtm: bool,

    // ── Load shedding ─────────────────────────────────────────────────────
    pub cpu_shed_threshold_pct: u8,
    pub ram_shed_threshold_pct: u8,

    // ── Web dashboard ─────────────────────────────────────────────────────
    pub web_port:               u16,
    pub web_bind:               String,
    pub web_auth:               bool,
    pub web_username:           String,
    pub web_password:           String,

    // ── UI ────────────────────────────────────────────────────────────────
    pub headless:               bool,
    pub ui:                     UiConfig,

    // ── Logging ───────────────────────────────────────────────────────────
    pub log_level:              String,

    // ── Server sync ───────────────────────────────────────────────────────
    pub known_servers:          Vec<String>,

    // ── Auto-update ───────────────────────────────────────────────────────
    pub github_repo:            String,
    pub update_check_interval_secs: u64,
}

impl NodeConfig {
    /// Defaults for a full client (everything enabled).
    pub fn client_defaults() -> Self {
        Self {
            node_type: "client".to_string(),
            port: 0, // ephemeral
            always_on: false,
            graphics_enabled: true,
            world_enabled: true,
            relay_enabled: true,
            tui_enabled: false,
            web_enabled: false,
            storage_budget_gb: 10,
            cache_radius_chunks: 5,
            web_port: 8082,
            ..Self::default()
        }
    }

    /// Defaults for a server node (no graphics).
    pub fn server_defaults() -> Self {
        Self {
            node_name: Some("MyServer".to_string()),
            node_type: "server".to_string(),
            port: 4001,
            always_on: true,
            graphics_enabled: false,
            world_enabled: true,
            relay_enabled: true,
            tui_enabled: true,
            web_enabled: true,
            storage_budget_gb: 500,
            cache_radius_chunks: 0, // unlimited
            max_world_data_gb: 10,
            cpu_shed_threshold_pct: 90,
            ram_shed_threshold_pct: 85,
            priority_score: 100,
            web_port: 8080,
            web_bind: "0.0.0.0".to_string(),
            ..Self::default()
        }
    }

    /// Defaults for a relay node (no world data, no graphics).
    pub fn relay_defaults() -> Self {
        Self {
            node_type: "relay".to_string(),
            port: 4001,
            always_on: true,
            graphics_enabled: false,
            world_enabled: false,
            relay_enabled: true,
            tui_enabled: true,
            web_enabled: true,
            storage_budget_gb: 0,
            cache_radius_chunks: 0,
            web_port: 8081,
            web_bind: "0.0.0.0".to_string(),
            ..Self::default()
        }
    }

    /// Path to the identity key file (defaults to ./node.key, overridden by identity_file field).
    pub fn identity_path(&self) -> PathBuf {
        self.identity_file.as_ref().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("node.key"))
    }
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_name: None,
            identity_file: None,
            temp_identity: false,
            port: 4001,
            ws_port: None,
            external_addr: None,
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
            graphics_enabled: false,
            world_enabled: true,
            relay_enabled: true,
            tui_enabled: true,
            web_enabled: true,
            world_dir: None,
            max_world_data_gb: 10,
            max_loaded_chunks: 1000,
            chunk_load_radius_m: 500.0,
            chunk_unload_radius_m: 600.0,
            world_save_interval_secs: 300,
            storage_budget_gb: 10,
            cache_radius_chunks: 0,
            data: DataSourceConfig::default(),
            download_on_start: vec![],
            download_all_srtm: false,
            cpu_shed_threshold_pct: 90,
            ram_shed_threshold_pct: 85,
            web_port: 8080,
            web_bind: "0.0.0.0".to_string(),
            web_auth: false,
            web_username: "admin".to_string(),
            web_password: String::new(),
            headless: false,
            ui: UiConfig::default(),
            log_level: "info".to_string(),
            known_servers: vec![],
            github_repo: "PaddyOhFurnature/mverse".to_string(),
            update_check_interval_secs: 21600,
        }
    }
}
