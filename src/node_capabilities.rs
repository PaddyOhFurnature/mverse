//! Node capability advertisement via Kademlia DHT.
//!
//! Every node (relay, server, client) publishes a `NodeCapabilities` record to the DHT
//! so peers can make intelligent routing decisions:
//! - Prefer servers for large chunk fetches (high bandwidth, always-on, large storage)
//! - Prefer relays for circuit establishment (lightweight, purpose-built)
//! - Use client caches as a secondary CDN source

use serde::{Deserialize, Serialize};

/// Tier of a network node — determines its expected role and capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeTier {
    /// Pure NAT traversal + gossipsub forwarding. No world state.
    /// Target: 64 MB RAM, any CPU. Runs on phones, cheap VPS, future LoRa nodes.
    Relay,
    /// Always-on world state authority. Full archive, web dashboard.
    /// Target: 1–32 GB RAM, large disk, multi-core CPU.
    Server,
    /// Desktop game client. Optionally caches content within a user budget.
    /// Target: 8+ GB RAM, GPU required.
    Client,
    /// Minimal read-only observer. No storage contribution.
    Light,
}

impl std::fmt::Display for NodeTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeTier::Relay  => write!(f, "relay"),
            NodeTier::Server => write!(f, "server"),
            NodeTier::Client => write!(f, "client"),
            NodeTier::Light  => write!(f, "light"),
        }
    }
}

/// Capabilities advertised by a node in the DHT.
///
/// Stored at key `caps/{peer_id}` as bincode-serialised bytes.
/// Published on startup and refreshed every 30 minutes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    /// Role of this node.
    pub tier: NodeTier,

    /// Bytes of storage this node is willing to serve to the network.
    /// 0 = no contribution beyond own data.
    pub available_storage_bytes: u64,

    /// Approximate outbound bandwidth capacity in bytes/sec.
    /// 0 = unknown.
    pub bandwidth_out_bps: u32,

    /// Whether this node is expected to be online 24/7.
    pub always_on: bool,

    /// SpatialShard cell IDs this node covers. Empty = global (no geographic limit).
    pub regions: Vec<u32>,

    /// Binary version: [major, minor, patch].
    pub version: [u8; 3],
}

impl NodeCapabilities {
    /// DHT key for storing this node's capabilities.
    pub fn dht_key(peer_id_str: &str) -> Vec<u8> {
        format!("caps/{}", peer_id_str).into_bytes()
    }

    /// Serialise to DHT-storable bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialise from DHT bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }

    /// Build capabilities for a relay node.
    pub fn for_relay(always_on: bool) -> Self {
        let version = parse_version(env!("CARGO_PKG_VERSION"));
        Self {
            tier: NodeTier::Relay,
            available_storage_bytes: 0,
            bandwidth_out_bps: 0,
            always_on,
            regions: vec![],
            version,
        }
    }

    /// Build capabilities for a server node.
    pub fn for_server(storage_budget_gb: u64, always_on: bool) -> Self {
        let version = parse_version(env!("CARGO_PKG_VERSION"));
        Self {
            tier: NodeTier::Server,
            available_storage_bytes: storage_budget_gb * 1_073_741_824,
            bandwidth_out_bps: 0,
            always_on,
            regions: vec![],
            version,
        }
    }

    /// Build capabilities for a game client.
    pub fn for_client(storage_budget_gb: u64) -> Self {
        let version = parse_version(env!("CARGO_PKG_VERSION"));
        Self {
            tier: NodeTier::Client,
            available_storage_bytes: storage_budget_gb * 1_073_741_824,
            bandwidth_out_bps: 0,
            always_on: false,
            regions: vec![],
            version,
        }
    }
}

fn parse_version(s: &str) -> [u8; 3] {
    let parts: Vec<u8> = s.split('.').filter_map(|p| p.parse().ok()).collect();
    [
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    ]
}
