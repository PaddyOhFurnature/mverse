//! Dynamic bootstrap node discovery
//!
//! Fetches a JSON file from a remote URL (GitHub Gist, archive.org, etc.)
//! containing the current list of bootstrap/relay nodes.
//!
//! Flow:
//!   1. Try local cache (`~/.metaverse/bootstrap_cache.json`) - instant startup
//!   2. Fetch remote URL in background - update cache if newer
//!   3. Fall back to hardcoded nodes if both fail
//!
//! The JSON schema is defined in docs/BOOTSTRAP_SCHEMA.md

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::time::Duration;

// ─── Schema ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapFile {
    pub schema_version: String,
    pub network: String,
    pub updated_at: String,
    pub ttl_seconds: u64,
    pub bootstrap_nodes: Vec<BootstrapNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNode {
    pub id: String,
    pub name: String,
    pub multiaddr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub local_multiaddr: Option<String>,
    pub capabilities: Vec<String>,
    pub priority: u8,
    pub verified: bool,
}

// ─── Config ──────────────────────────────────────────────────────────────────

/// Where to fetch bootstrap nodes from. Listed in priority order - first success wins.
pub const BOOTSTRAP_URLS: &[&str] = &[
    "https://gist.githubusercontent.com/PaddyOhFurnature/e5b7fc9c077016682d8eb27abd7cca17/raw/bootstrap.json",
];

/// Hardcoded fallback - used only if all URLs fail and no cache exists.
pub const HARDCODED_FALLBACK: &[&str] = &[
    "/ip4/49.182.84.9/tcp/4001/p2p/12D3KooWH6ARmErmjFZPHaUPtHcpCHJ4rwUtWMU4kYnogo3tPymm",
];

// ─── Cache path ──────────────────────────────────────────────────────────────

fn cache_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".metaverse").join("bootstrap_cache.json")
}

// ─── Main API ────────────────────────────────────────────────────────────────

/// Resolve bootstrap multiaddrs.
///
/// Returns a list of multiaddr strings ready to pass to `network.dial()`.
/// Sorted by priority (highest first).
///
/// This is intentionally synchronous-blocking at startup — bootstrap resolution
/// must complete before the swarm can meaningfully dial.
pub async fn resolve_bootstrap_nodes() -> Vec<String> {
    // 1. Try to load cache (fast path - works offline)
    let cached = load_cache();

    // 2. Try to fetch remote (update in background if cache is fresh enough)
    let remote = fetch_remote().await;

    let source = match (remote, cached) {
        (Ok(fetched), _) => {
            println!("[bootstrap] Fetched {} nodes from remote", fetched.bootstrap_nodes.len());
            save_cache(&fetched);
            fetched
        }
        (Err(e), Some(cache)) => {
            eprintln!("[bootstrap] Remote fetch failed ({}), using cache ({} nodes)", e, cache.bootstrap_nodes.len());
            cache
        }
        (Err(e), None) => {
            eprintln!("[bootstrap] Remote fetch failed ({}), no cache - using hardcoded fallback", e);
            return HARDCODED_FALLBACK.iter().map(|s| s.to_string()).collect();
        }
    };

    // Sort by priority descending, return multiaddrs
    let mut nodes = source.bootstrap_nodes;
    nodes.sort_by(|a, b| b.priority.cmp(&a.priority));

    let local_subnets = get_local_subnets();
    let mut result: Vec<String> = Vec::new();

    for node in &nodes {
        if let Some(ref lan_ma) = node.local_multiaddr {
            if let Some(lan_ip) = extract_ip_from_multiaddr(lan_ma) {
                let on_same_lan = local_subnets
                    .iter()
                    .any(|&(lip, prefix)| is_same_subnet(lip, prefix, lan_ip));
                if on_same_lan {
                    result.push(lan_ma.clone());
                }
            }
        }
    }

    for node in nodes {
        result.push(node.multiaddr);
    }

    result
}

// ─── LAN detection ───────────────────────────────────────────────────────────

/// Returns (ip, prefix_len) pairs for all local IPv4 interfaces.
fn get_local_subnets() -> Vec<(Ipv4Addr, u8)> {
    let output = match std::process::Command::new("ip").args(["addr"]).output() {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("inet ") {
            continue;
        }
        // e.g. "inet 192.168.1.111/24 brd ..."
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let cidr = parts[1]; // "192.168.1.111/24"
        let mut split = cidr.splitn(2, '/');
        let ip_str = split.next().unwrap_or("");
        let prefix_str = split.next().unwrap_or("32");
        let ip: Ipv4Addr = match ip_str.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let prefix: u8 = prefix_str.parse().unwrap_or(32);
        result.push((ip, prefix));
    }
    result
}

/// Returns true if `remote_ip` falls within the subnet defined by `local_ip/prefix_len`.
fn is_same_subnet(local_ip: Ipv4Addr, prefix_len: u8, remote_ip: Ipv4Addr) -> bool {
    if prefix_len == 0 {
        return true;
    }
    if prefix_len > 32 {
        return false;
    }
    let mask = !((1u32 << (32 - prefix_len)) - 1);
    (u32::from(local_ip) & mask) == (u32::from(remote_ip) & mask)
}

/// Parses a multiaddr like `/ip4/192.168.1.182/tcp/4001/...` and returns the IPv4 address.
fn extract_ip_from_multiaddr(ma: &str) -> Option<Ipv4Addr> {
    let mut parts = ma.split('/');
    // skip leading empty string from leading '/'
    parts.next();
    while let Some(proto) = parts.next() {
        if proto == "ip4" {
            if let Some(addr_str) = parts.next() {
                return addr_str.parse().ok();
            }
        }
    }
    None
}

// ─── Remote fetch ────────────────────────────────────────────────────────────

async fn fetch_remote() -> Result<BootstrapFile, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("metaverse-core/0.1")
        .build()
        .map_err(|e| e.to_string())?;

    for url in BOOTSTRAP_URLS {
        println!("[bootstrap] Fetching {}", url);
        match client.get(*url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<BootstrapFile>().await {
                    Ok(file) => return Ok(file),
                    Err(e) => eprintln!("[bootstrap] Failed to parse {}: {}", url, e),
                }
            }
            Ok(resp) => eprintln!("[bootstrap] {} returned HTTP {}", url, resp.status()),
            Err(e) => eprintln!("[bootstrap] Failed to fetch {}: {}", url, e),
        }
    }

    Err("all bootstrap URLs failed".to_string())
}

// ─── Cache ───────────────────────────────────────────────────────────────────

fn load_cache() -> Option<BootstrapFile> {
    let path = cache_path();
    let data = std::fs::read(&path).ok()?;
    match serde_json::from_slice(&data) {
        Ok(file) => {
            println!("[bootstrap] Loaded cache from {:?}", path);
            Some(file)
        }
        Err(e) => {
            eprintln!("[bootstrap] Cache parse error: {}", e);
            None
        }
    }
}

fn save_cache(file: &BootstrapFile) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec_pretty(file) {
        Ok(data) => {
            if let Err(e) = std::fs::write(&path, data) {
                eprintln!("[bootstrap] Failed to save cache: {}", e);
            } else {
                println!("[bootstrap] Cache saved to {:?}", path);
            }
        }
        Err(e) => eprintln!("[bootstrap] Failed to serialize cache: {}", e),
    }
}
