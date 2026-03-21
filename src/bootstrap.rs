//! Dynamic bootstrap node discovery.
//!
//! Bootstrap node lists are fetched from external HTTPS URLs (GitHub Gist, raw files, etc.)
//! that can be edited without touching this codebase.
//!
//! ## How to add/update nodes
//! Edit the Gist (or other URL listed in `BOOTSTRAP_URLS`). No git push or code change needed.
//!
//! ## Node format — use host + ports, NOT multiaddr syntax
//!
//! ```json
//! {
//!   "id":         "au1",
//!   "name":       "AU Server 1",
//!   "host":       "meta.theworkpc.com",
//!   "tcp_port":   4001,
//!   "ws_port":    9001,
//!   "peer_id":    "12D3KooW...",
//!   "capabilities": ["relay", "bootstrap", "world-authority"],
//!   "region":     "AU",
//!   "priority":   100,
//!   "expires_at": null
//! }
//! ```
//!
//! `host`       — IPv4 address ("144.6.111.191") OR hostname ("meta.theworkpc.com").
//!                The correct /ip4/ or /dns4/ multiaddr is built automatically.
//! `tcp_port`   — Main libp2p TCP port.
//! `ws_port`    — WebSocket port (optional — omit for TCP-only nodes).
//! `peer_id`    — libp2p peer ID of the server (base58 string starting with "12D3Koo...").
//! `expires_at` — ISO 8601 timestamp after which this node is ignored, or null for permanent.
//!
//! ## Multiple bootstrap sources
//! All URLs in `BOOTSTRAP_URLS` are fetched and their node lists are merged.
//! Nodes with the same `peer_id` are de-duplicated (highest `priority` wins).
//! This lets you have multiple independent Gists or mirror URLs as fallbacks.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

// ─── Schema ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BootstrapFile {
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
    pub bootstrap_nodes: Vec<BootstrapNode>,
}

fn default_ttl() -> u64 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapNode {
    pub id: String,
    #[serde(default)]
    pub name: String,

    // ── Simple format (preferred) ─────────────────────────────────────────────
    /// IP address or hostname. System auto-selects /ip4/ or /dns4/ multiaddr.
    #[serde(default)]
    pub host: String,
    /// Main TCP port for libp2p.
    #[serde(default)]
    pub tcp_port: u16,
    /// WebSocket port (optional — omit or set to 0 for TCP-only).
    #[serde(default)]
    pub ws_port: Option<u16>,
    /// libp2p Peer ID (base58, "12D3KooW...").
    #[serde(default)]
    pub peer_id: String,

    // ── Legacy format (backwards compat) ─────────────────────────────────────
    /// Raw multiaddr string — still accepted but auto-corrected if malformed.
    /// Prefer host+tcp_port+peer_id for new entries.
    #[serde(default)]
    pub multiaddr: Option<String>,

    // ── Common metadata ───────────────────────────────────────────────────────
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Expiry timestamp (ISO 8601). null/absent = permanent node.
    /// Expired nodes are silently dropped on load.
    #[serde(default)]
    pub expires_at: Option<String>,

    /// Measured TCP round-trip time (ms). Set at runtime, never stored.
    #[serde(skip)]
    pub measured_rtt_ms: Option<f64>,
}

fn default_priority() -> u8 {
    50
}

impl BootstrapNode {
    /// Build the libp2p multiaddr strings for this node (TCP + optional WS).
    /// Uses host+ports if present; falls back to the raw `multiaddr` field.
    pub fn multiaddrs(&self) -> Vec<String> {
        if !self.host.is_empty() && !self.peer_id.is_empty() && self.tcp_port > 0 {
            // Auto-select protocol: ip4 for IPv4 literals, dns4 for hostnames.
            let proto = if self.host.parse::<std::net::Ipv4Addr>().is_ok() {
                "ip4"
            } else {
                "dns4"
            };
            let mut addrs = vec![format!(
                "/{}/{}/tcp/{}/p2p/{}",
                proto, self.host, self.tcp_port, self.peer_id
            )];
            if let Some(ws) = self.ws_port.filter(|&p| p > 0) {
                addrs.push(format!(
                    "/{}/{}/tcp/{}/ws/p2p/{}",
                    proto, self.host, ws, self.peer_id
                ));
            }
            return addrs;
        }

        // Legacy: use raw multiaddr with auto-correction.
        if let Some(ma) = &self.multiaddr {
            let fixed = autocorrect_multiaddr(ma);
            if fixed != *ma {
                eprintln!(
                    "[bootstrap] ⚠️  Auto-corrected '{}' multiaddr: {} → {}",
                    self.id, ma, fixed
                );
                eprintln!(
                    "[bootstrap]    (Update your bootstrap source to use host+tcp_port+peer_id instead)"
                );
            }
            return vec![fixed];
        }

        vec![]
    }

    /// Validate this node. Returns Err with a human-readable message on failure.
    pub fn validate(&self) -> Result<(), String> {
        let has_new_fmt = !self.host.is_empty() && !self.peer_id.is_empty() && self.tcp_port > 0;
        let has_legacy = self
            .multiaddr
            .as_deref()
            .map(|m| !m.is_empty())
            .unwrap_or(false);

        if !has_new_fmt && !has_legacy {
            return Err(format!(
                "Node '{}' ('{}'): missing required fields.\n\
                 Required: \"host\": \"<ip-or-hostname>\", \"tcp_port\": <port>, \"peer_id\": \"12D3KooW...\"\n\
                 Optional: \"ws_port\": <port>",
                self.id, self.name
            ));
        }

        // Catch the specific mistake of putting a hostname in the host field when
        // it was supposed to be an IP — that's fine, we handle it. But catch
        // someone accidentally putting a full multiaddr in the host field.
        if has_new_fmt && (self.host.starts_with('/') || self.host.contains("/tcp/")) {
            return Err(format!(
                "Node '{}': 'host' field contains a multiaddr string ('{}').\n\
                 Use plain hostname or IP in 'host', e.g.: \"host\": \"meta.theworkpc.com\"",
                self.id, self.host
            ));
        }

        Ok(())
    }

    /// True if the node has a non-null expires_at that is in the past.
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = &self.expires_at {
            // Basic ISO 8601 expiry check without chrono dependency.
            // Format: "2026-12-31T00:00:00Z" — compare as string against current UTC.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if let Some(exp_secs) = parse_iso8601_approx(exp) {
                return now > exp_secs;
            }
        }
        false
    }

    /// "host:tcp_port" for TCP latency measurement.
    fn tcp_addr(&self) -> Option<String> {
        if !self.host.is_empty() && self.tcp_port > 0 {
            return Some(format!("{}:{}", self.host, self.tcp_port));
        }
        // Fall back to parsing legacy multiaddr.
        if let Some(ma) = &self.multiaddr {
            return extract_host_port(ma);
        }
        None
    }
}

// ─── Config ──────────────────────────────────────────────────────────────────

/// Bootstrap source URLs. All are fetched; their node lists are merged.
/// Edit these (or add new ones) to add bootstrap sources — no other code change needed.
/// The Gist is the canonical editable source; add mirrors here for redundancy.
pub const BOOTSTRAP_URLS: &[&str] = &[
    "https://gist.githubusercontent.com/PaddyOhFurnature/e5b7fc9c077016682d8eb27abd7cca17/raw/bootstrap.json",
];

// ─── Main API ────────────────────────────────────────────────────────────────

/// Resolve bootstrap multiaddrs from all configured sources.
///
/// Returns a flat list of multiaddr strings (TCP + WS per node) sorted nearest-first.
/// Always fetches remote on startup — cache is only used when ALL remotes fail.
pub async fn resolve_bootstrap_nodes() -> Vec<String> {
    // 1. Always attempt remote fetch first (bootstrap list must be current).
    let remote = fetch_and_merge_all().await;

    let nodes = match remote {
        Ok(nodes) if !nodes.is_empty() => {
            println!("[bootstrap] ✅ {} node(s) from remote", nodes.len());
            let file = BootstrapFile {
                schema_version: "2.0".into(),
                network: "mainnet".into(),
                updated_at: String::new(),
                ttl_seconds: 3600,
                bootstrap_nodes: nodes.clone(),
            };
            save_cache(&file);
            nodes
        }
        Ok(_) => {
            eprintln!("[bootstrap] ⚠️  Remote returned 0 valid nodes — checking cache");
            load_and_validate_cache().unwrap_or_default()
        }
        Err(e) => {
            eprintln!("[bootstrap] ❌ Remote fetch failed: {} — checking cache", e);
            match load_and_validate_cache() {
                Some(cached) => {
                    println!("[bootstrap] Using {} cached node(s)", cached.len());
                    cached
                }
                None => {
                    eprintln!("[bootstrap] No valid cache — no bootstrap nodes available");
                    return vec![];
                }
            }
        }
    };

    // 2. Measure latency to each node, sort nearest-first.
    let mut nodes = nodes;
    measure_relay_latencies(&mut nodes).await;
    nodes.sort_by(|a, b| match (a.measured_rtt_ms, b.measured_rtt_ms) {
        (Some(ra), Some(rb)) => ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.priority.cmp(&a.priority),
    });

    for n in &nodes {
        let rtt = n
            .measured_rtt_ms
            .map(|r| format!("{:.0}ms", r))
            .unwrap_or_else(|| "unreachable".into());
        println!(
            "[bootstrap]   {} ({}) — {}",
            n.name,
            n.region.as_deref().unwrap_or("?"),
            rtt
        );
    }

    // Flatten: each node may produce TCP + WS multiaddrs.
    nodes.iter().flat_map(|n| n.multiaddrs()).collect()
}

// ─── Fetch + merge ───────────────────────────────────────────────────────────

/// Fetch all BOOTSTRAP_URLS, parse, validate, merge by peer_id (highest priority wins).
async fn fetch_and_merge_all() -> Result<Vec<BootstrapNode>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("metaverse-bootstrap/2.0")
        .build()
        .map_err(|e| e.to_string())?;

    // peer_id → node (keep highest priority)
    let mut merged: HashMap<String, BootstrapNode> = HashMap::new();
    let mut any_success = false;

    for url in BOOTSTRAP_URLS {
        println!("[bootstrap] Fetching {}", url);
        match fetch_one(&client, url).await {
            Ok(nodes) => {
                any_success = true;
                for node in nodes {
                    let key = if !node.peer_id.is_empty() {
                        node.peer_id.clone()
                    } else {
                        node.id.clone()
                    };
                    let keep = merged
                        .get(&key)
                        .map(|e| node.priority > e.priority)
                        .unwrap_or(true);
                    if keep {
                        merged.insert(key, node);
                    }
                }
            }
            Err(e) => eprintln!("[bootstrap] ❌ {} — {}", url, e),
        }
    }

    if !any_success {
        return Err("all bootstrap URLs failed".into());
    }

    let mut nodes: Vec<BootstrapNode> = merged.into_values().collect();
    nodes.sort_by(|a, b| b.priority.cmp(&a.priority));
    Ok(nodes)
}

/// Fetch a single URL, parse, and return validated non-expired nodes.
async fn fetch_one(client: &reqwest::Client, url: &str) -> Result<Vec<BootstrapNode>, String> {
    let text = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| format!("HTTP {}", e.status().map(|s| s.as_u16()).unwrap_or(0)))?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    // Accept either {"bootstrap_nodes":[...]} wrapper or a bare [...] array.
    let nodes: Vec<BootstrapNode> = if let Ok(file) = serde_json::from_str::<BootstrapFile>(&text) {
        file.bootstrap_nodes
    } else if let Ok(arr) = serde_json::from_str::<Vec<BootstrapNode>>(&text) {
        arr
    } else {
        return Err("could not parse as bootstrap file or node array".into());
    };

    let mut valid = Vec::new();
    for node in nodes {
        if node.is_expired() {
            println!("[bootstrap]   ⏰ Skipping expired node '{}'", node.id);
            continue;
        }
        match node.validate() {
            Ok(()) => valid.push(node),
            Err(e) => eprintln!("[bootstrap]   ❌ Invalid node — {}", e),
        }
    }

    if valid.is_empty() {
        return Err("no valid nodes after validation".into());
    }
    Ok(valid)
}

// ─── Latency measurement ─────────────────────────────────────────────────────

async fn measure_relay_latencies(nodes: &mut Vec<BootstrapNode>) {
    use std::time::Instant;
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    for node in nodes.iter_mut() {
        if let Some(addr) = node.tcp_addr() {
            let start = Instant::now();
            if let Ok(Ok(_)) = timeout(Duration::from_secs(3), TcpStream::connect(&addr)).await {
                node.measured_rtt_ms = Some(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
}

// ─── Cache ───────────────────────────────────────────────────────────────────

fn cache_path() -> PathBuf {
    if let Some(path) = std::env::var_os("METAVERSE_BOOTSTRAP_CACHE_FILE") {
        return PathBuf::from(path);
    }
    PathBuf::from("bootstrap_cache.json")
}

/// Load cache and re-validate every node. Expired/invalid nodes are dropped.
/// Returns None if cache is missing, corrupt, or contains zero valid nodes.
fn load_and_validate_cache() -> Option<Vec<BootstrapNode>> {
    let path = cache_path();
    let data = std::fs::read(&path).ok()?;

    let file: BootstrapFile = match serde_json::from_slice(&data) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[bootstrap] Cache corrupt ({}), deleting", e);
            let _ = std::fs::remove_file(&path);
            return None;
        }
    };

    let mut valid = Vec::new();
    for node in file.bootstrap_nodes {
        if node.is_expired() {
            continue;
        }
        match node.validate() {
            Ok(()) => valid.push(node),
            Err(e) => eprintln!("[bootstrap]   ❌ Dropping cached node — {}", e),
        }
    }

    if valid.is_empty() {
        eprintln!("[bootstrap] Cache has no valid nodes — discarding");
        let _ = std::fs::remove_file(&path);
        return None;
    }

    println!("[bootstrap] Cache: {} valid node(s)", valid.len());
    Some(valid)
}

fn save_cache(file: &BootstrapFile) {
    let path = cache_path();
    if let Ok(data) = serde_json::to_vec_pretty(file) {
        let _ = std::fs::write(&path, data);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Auto-correct common multiaddr mistakes:
/// - /ip4/<hostname>/...  →  /dns4/<hostname>/...   (hostname in ip4 slot)
fn autocorrect_multiaddr(ma: &str) -> String {
    let parts: Vec<&str> = ma.splitn(6, '/').collect();
    // /ip4/<host>/tcp/... — parts: ["", "ip4", host, "tcp", port, rest?]
    if parts.len() >= 5 && parts[1] == "ip4" {
        let host = parts[2];
        if host.parse::<std::net::Ipv4Addr>().is_err() {
            // It's a hostname, not an IP — replace ip4 with dns4.
            let rest = parts[3..].join("/");
            return format!("/dns4/{}/{}", host, rest);
        }
    }
    ma.to_string()
}

/// Extract "host:port" from a legacy multiaddr string.
/// Handles /ip4/<host>/tcp/<port>/... and /dns4/<host>/tcp/<port>/...
fn extract_host_port(ma: &str) -> Option<String> {
    let parts: Vec<&str> = ma.split('/').collect();
    if parts.len() >= 5 && (parts[1] == "ip4" || parts[1] == "dns4") && parts[3] == "tcp" {
        Some(format!("{}:{}", parts[2], parts[4]))
    } else {
        None
    }
}

/// Very basic ISO 8601 → Unix seconds, handles "YYYY-MM-DDTHH:MM:SSZ" format.
fn parse_iso8601_approx(s: &str) -> Option<u64> {
    // "2026-12-31T00:00:00Z" — split on T, then parse date.
    let s = s.trim_end_matches('Z');
    let (date, _time) = s.split_once('T').unwrap_or((s, "00:00:00"));
    let mut dp = date.split('-');
    let y: u64 = dp.next()?.parse().ok()?;
    let m: u64 = dp.next()?.parse().ok()?;
    let d: u64 = dp.next()?.parse().ok()?;
    // Approximate: days since epoch (good enough for expiry checking).
    let days = (y.saturating_sub(1970)) * 365
        + (y.saturating_sub(1970)) / 4
        + [0u64, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334]
            .get(m.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(0)
        + d.saturating_sub(1);
    Some(days * 86400)
}
