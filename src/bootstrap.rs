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
    nodes.into_iter().map(|n| n.multiaddr).collect()
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
