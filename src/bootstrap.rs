//! Dynamic bootstrap node discovery
//!
//! Fetches a JSON file from a remote URL (GitHub Gist, archive.org, etc.)
//! containing the current list of bootstrap/relay nodes.
//!
//! Flow:
//!   1. Try local cache (`./bootstrap_cache.json`) - instant startup
//!   2. Fetch remote URL in background - update cache if newer
//!   3. Fall back to hardcoded nodes if both fail
//!
//! The JSON schema is defined in docs/BOOTSTRAP_SCHEMA.md

use serde::{Deserialize, Serialize};
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
    pub capabilities: Vec<String>,
    pub priority: u8,
    pub verified: bool,
    /// Geographic region tag (e.g. "AU", "EU", "US-WEST"). Optional — used for relay selection.
    #[serde(default)]
    pub region: Option<String>,
    /// Last measured round-trip time in milliseconds (populated at runtime, not in JSON).
    #[serde(skip)]
    pub measured_rtt_ms: Option<f64>,
}

// ─── Config ──────────────────────────────────────────────────────────────────

/// Where to fetch bootstrap nodes from. Listed in priority order - first success wins.
/// - Repo raw file: updated immediately on every push, GitHub raw CDN flushes fast
/// - Gist: kept as fallback, but can have CDN lag of several minutes after update
pub const BOOTSTRAP_URLS: &[&str] = &[
    "https://raw.githubusercontent.com/PaddyOhFurnature/mverse/main/bootstrap.json",
    "https://gist.githubusercontent.com/PaddyOhFurnature/e5b7fc9c077016682d8eb27abd7cca17/raw/bootstrap.json",
];

/// Hardcoded fallback — used only if ALL remote URLs fail AND no local cache exists.
/// Keep this empty: we don't know what IP the server will have in future, and a stale
/// hardcoded address is worse than gracefully failing (client just won't connect until
/// bootstrap is reachable again).
pub const HARDCODED_FALLBACK: &[&str] = &[];

// ─── Cache path ──────────────────────────────────────────────────────────────

fn cache_path() -> PathBuf {
    PathBuf::from("bootstrap_cache.json")
}

// ─── Main API ────────────────────────────────────────────────────────────────

/// Resolve bootstrap multiaddrs.
///
/// Returns a list of multiaddr strings ready to pass to `network.dial()`.
/// Sorted by measured latency (nearest relay first). Falls back to `priority` field when
/// latency measurement is unavailable (offline start, relay unreachable, etc.).
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

    // 3. Measure latency to each relay and sort nearest-first.
    let mut nodes = source.bootstrap_nodes;
    measure_relay_latencies(&mut nodes).await;

    // Sort: measured RTT first (ascending), then fall back to priority (descending).
    nodes.sort_by(|a, b| {
        match (a.measured_rtt_ms, b.measured_rtt_ms) {
            (Some(ra), Some(rb)) => ra.partial_cmp(&rb).unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,    // measured beats unmeasured
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.priority.cmp(&a.priority),    // higher priority first
        }
    });

    for n in &nodes {
        match n.measured_rtt_ms {
            Some(rtt) => println!("[bootstrap] {} ({}) — {:.0}ms", n.name, n.region.as_deref().unwrap_or("?"), rtt),
            None => println!("[bootstrap] {} ({}) — latency unknown", n.name, n.region.as_deref().unwrap_or("?")),
        }
    }

    nodes.into_iter().map(|n| n.multiaddr).collect()
}

// ─── Remote fetch ────────────────────────────────────────────────────────────

/// Measure TCP connect latency to each relay node.
///
/// Extracts the host:port from the multiaddr string and attempts a TCP connection.
/// The connection time is used as a proxy for network latency (close to RTT/2).
/// Timeout per relay: 3 seconds. Relays that time out or fail get no RTT recorded.
async fn measure_relay_latencies(nodes: &mut Vec<BootstrapNode>) {
    use tokio::net::TcpStream;
    use tokio::time::timeout;
    use std::time::Instant;

    for node in nodes.iter_mut() {
        // Parse multiaddr to extract host and port.
        // Format: /ip4/<host>/tcp/<port>/p2p/<peer_id>
        if let Some(addr) = extract_tcp_addr(&node.multiaddr) {
            let start = Instant::now();
            match timeout(Duration::from_secs(3), TcpStream::connect(&addr)).await {
                Ok(Ok(_)) => {
                    let rtt_ms = start.elapsed().as_secs_f64() * 1000.0;
                    node.measured_rtt_ms = Some(rtt_ms);
                }
                _ => {
                    // Connection failed or timed out — relay may be unreachable or wrong transport
                }
            }
        }
    }
}

/// Extract "host:port" from a libp2p multiaddr string.
/// Handles `/ip4/<host>/tcp/<port>/...` and `/dns4/<host>/tcp/<port>/...`.
fn extract_tcp_addr(multiaddr: &str) -> Option<String> {
    let parts: Vec<&str> = multiaddr.split('/').collect();
    // parts[0] is empty (leading slash), parts[1] is "ip4"/"dns4", parts[2] is host,
    // parts[3] is "tcp", parts[4] is port.
    if parts.len() >= 5 && (parts[1] == "ip4" || parts[1] == "dns4") && parts[3] == "tcp" {
        Some(format!("{}:{}", parts[2], parts[4]))
    } else {
        None
    }
}

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
            eprintln!("[bootstrap] Cache parse error: {} — deleting corrupt cache", e);
            let _ = std::fs::remove_file(&path);
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
