//! Spatial sharding for planet-scale P2P mesh networking
//!
//! Implements "controlled chaos" redundancy architecture with proper bandwidth optimization:
//! - **Layer 1: Active Push** - Targeted sends to X closest peers (hierarchical fallback)
//! - **Layer 2: Lazy Gossip** - Passive propagation through random peer selection
//! - **Layer 3: Geographic Topics** - Dynamic subscription to region-based channels
//!
//! # Core Philosophy
//!
//! This is NOT client-server (linear, predictable, centralized).
//! This IS a mesh network (chaotic, multi-path, decentralized).
//!
//! **Design Principles:**
//! - **Redundancy:** Every operation exists on X nodes (never lose data)
//! - **Fallback:** If preferred option fails, try next tier (graceful degradation)
//! - **Eventual Consistency:** CRDTs + gossip = convergence
//! - **Self-Healing:** Multiple paths ensure delivery despite failures
//! - **No Single Point of Failure:** Every node can fail, system survives
//! - **Bandwidth Efficient:** Only send to peers who need it (region-based topics)
//!
//! # Geographic Grid System
//!
//! Earth divided into hierarchical grid cells for topic-based sharding:
//! - **L0 (Macro):** ~1000km cells (continental scale)
//! - **L1 (Meso):** ~100km cells (city scale)
//! - **L2 (Micro):** ~10km cells (neighborhood scale)
//! - **L3 (Nano):** ~1km cells (block scale)
//!
//! Each cell has a unique ID. Peers subscribe to topics for:
//! - Their current cell (nano/micro scale)
//! - Neighboring cells (for cross-boundary visibility)
//! - Parent cells (for regional awareness)
//!
//! # Distance Tiers
//!
//! - **Tier 1 (100m):** Visibility range, immediate rendering, low latency
//! - **Tier 2 (500m):** Nearby region, medium latency, backup storage
//! - **Tier 3 (1km):** Local area, acceptable latency, wider backup
//! - **Tier 4 (Global):** Any peer worldwide, high latency, guaranteed redundancy
//!
//! # Example Flow
//!
//! ```text
//! Player digs hole in New York:
//! 1. Identify region ID from GPS coords (e.g., "L3-US-NY-Manhattan-012")
//! 2. Active push to X closest peers in same region (targeted send)
//! 3. Publish to region topic "voxel-ops-L3-US-NY-Manhattan-012"
//! 4. Only peers subscribed to that topic receive it (bandwidth efficient)
//! 5. If < X peers in region, fallback to parent region "L2-US-NY-Metro"
//! 6. If still < X, fallback to global broadcast
//! 7. Lazy gossip: Every 10s, re-broadcast recent ops to 20% of peers
//! ```

use crate::coordinates::{ECEF, GPS};
use libp2p::PeerId;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::{Duration, Instant};

/// Geographic region identifier for topic-based sharding
///
/// Represents a cell in the hierarchical geographic grid.
/// Each level provides different granularity:
/// - L0: Continental scale (~1000km)
/// - L1: City scale (~100km)
/// - L2: Neighborhood scale (~10km)
/// - L3: Block scale (~1km)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RegionId {
    /// Grid level (0 = macro, 3 = nano)
    pub level: u8,

    /// X coordinate in grid at this level
    pub x: i32,

    /// Y coordinate in grid at this level
    pub y: i32,
}

impl RegionId {
    /// Create a region ID from GPS coordinates at specified level
    ///
    /// Uses a simple lat/lon grid system:
    /// - Level 0: 10° cells (~1000km at equator)
    /// - Level 1: 1° cells (~100km at equator)
    /// - Level 2: 0.1° cells (~10km at equator)
    /// - Level 3: 0.01° cells (~1km at equator)
    pub fn from_gps(gps: &GPS, level: u8) -> Self {
        let cell_size = match level {
            0 => 10.0, // ~1000km cells
            1 => 1.0,  // ~100km cells
            2 => 0.1,  // ~10km cells
            3 => 0.01, // ~1km cells
            _ => 0.01, // Default to finest granularity
        };

        let x = (gps.lon / cell_size).floor() as i32;
        let y = (gps.lat / cell_size).floor() as i32;

        Self { level, x, y }
    }

    /// Create from ECEF coordinates
    pub fn from_ecef(ecef: &ECEF, level: u8) -> Self {
        let gps = ecef.to_gps();
        Self::from_gps(&gps, level)
    }

    /// Get parent region (one level coarser)
    pub fn parent(&self) -> Option<Self> {
        if self.level == 0 {
            return None; // Already at coarsest level
        }

        let scale_factor = match self.level {
            1 => 10, // L1 → L0: 1° → 10°
            2 => 10, // L2 → L1: 0.1° → 1°
            3 => 10, // L3 → L2: 0.01° → 0.1°
            _ => 10,
        };

        Some(Self {
            level: self.level - 1,
            x: self.x / scale_factor,
            y: self.y / scale_factor,
        })
    }

    /// Get neighboring regions (8 surrounding cells + self = 9 total)
    pub fn neighbors(&self) -> Vec<Self> {
        let mut neighbors = Vec::with_capacity(9);

        for dx in -1..=1 {
            for dy in -1..=1 {
                neighbors.push(Self {
                    level: self.level,
                    x: self.x + dx,
                    y: self.y + dy,
                });
            }
        }

        neighbors
    }

    /// Convert to gossipsub topic name
    ///
    /// Format: "region-LX-xNNN-yMMM" (e.g., "region-L3-x042-y-015")
    pub fn to_topic(&self, prefix: &str) -> String {
        format!("{}-L{}-x{:04}-y{:04}", prefix, self.level, self.x, self.y)
    }
}

impl fmt::Display for RegionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}({},{})", self.level, self.x, self.y)
    }
}

/// Configuration for spatial sharding behavior
#[derive(Debug, Clone)]
pub struct SpatialConfig {
    /// Target number of redundant copies for each operation (default: 5)
    pub redundancy_target: usize,

    /// Tier 1 radius: visibility range (default: 100m)
    pub tier1_radius_m: f64,

    /// Tier 2 radius: nearby region (default: 500m)
    pub tier2_radius_m: f64,

    /// Tier 3 radius: local area (default: 1000m)
    pub tier3_radius_m: f64,

    /// Percentage of peers to gossip to (default: 20%)
    pub gossip_percentage: f64,

    /// Interval between lazy gossip rounds (default: 10s)
    pub gossip_interval: Duration,

    /// Maximum age of operations to gossip (default: 60s)
    pub recent_op_window: Duration,
}

impl Default for SpatialConfig {
    fn default() -> Self {
        Self {
            redundancy_target: 5,
            tier1_radius_m: 100.0,
            tier2_radius_m: 500.0,
            tier3_radius_m: 1000.0,
            gossip_percentage: 0.20,
            gossip_interval: Duration::from_secs(10),
            recent_op_window: Duration::from_secs(60),
        }
    }
}

/// Information about a peer's location and capabilities
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer identifier
    pub peer_id: PeerId,

    /// Last known position
    pub position: ECEF,

    /// Last position update time
    pub last_update: Instant,

    /// Is this a relay node (high uptime, long-term storage)?
    pub is_relay: bool,

    /// Distance from local player (cached for performance)
    pub distance_m: f64,
}

impl PeerInfo {
    pub fn new(peer_id: PeerId, position: ECEF, is_relay: bool) -> Self {
        Self {
            peer_id,
            position,
            last_update: Instant::now(),
            is_relay,
            distance_m: 0.0,
        }
    }

    /// Update position and recalculate distance from local player
    pub fn update_position(&mut self, position: ECEF, local_position: &ECEF) {
        self.position = position;
        self.last_update = Instant::now();
        self.distance_m = local_position.distance_to(&position);
    }
}

/// Tiered peer selection for hierarchical fallback
#[derive(Debug, Clone)]
pub struct TieredPeerSelection {
    /// Peers in visibility range (100m)
    pub tier1_visible: Vec<PeerId>,

    /// Peers in nearby region (500m)
    pub tier2_nearby: Vec<PeerId>,

    /// Peers in local area (1km)
    pub tier3_local: Vec<PeerId>,

    /// All other peers (sorted by distance)
    pub tier4_global: Vec<PeerId>,

    /// Total number of peers available
    pub total_peers: usize,
}

impl TieredPeerSelection {
    /// Select peers for active push with hierarchical fallback
    ///
    /// Returns up to `target` peers, preferring closer tiers.
    /// Guarantees at least `target` peers if available globally.
    pub fn select_for_active_push(&self, target: usize) -> Vec<PeerId> {
        let mut selected = Vec::new();

        // Tier 1: Visible range (100m) - highest priority
        let from_tier1 = std::cmp::min(target, self.tier1_visible.len());
        selected.extend_from_slice(&self.tier1_visible[..from_tier1]);

        if selected.len() >= target {
            return selected;
        }

        // Tier 2: Nearby region (500m)
        let remaining = target - selected.len();
        let from_tier2 = std::cmp::min(remaining, self.tier2_nearby.len());
        selected.extend_from_slice(&self.tier2_nearby[..from_tier2]);

        if selected.len() >= target {
            return selected;
        }

        // Tier 3: Local area (1km)
        let remaining = target - selected.len();
        let from_tier3 = std::cmp::min(remaining, self.tier3_local.len());
        selected.extend_from_slice(&self.tier3_local[..from_tier3]);

        if selected.len() >= target {
            return selected;
        }

        // Tier 4: Global (any distance)
        let remaining = target - selected.len();
        let from_tier4 = std::cmp::min(remaining, self.tier4_global.len());
        selected.extend_from_slice(&self.tier4_global[..from_tier4]);

        selected
    }

    /// Select random subset of peers for lazy gossip
    ///
    /// Returns approximately `percentage` of all peers, randomly selected.
    pub fn select_for_lazy_gossip(&self, percentage: f64) -> Vec<PeerId> {
        use rand::seq::SliceRandom;
        use rand::thread_rng;

        // Collect all peers into one vector
        let mut all_peers = Vec::new();
        all_peers.extend_from_slice(&self.tier1_visible);
        all_peers.extend_from_slice(&self.tier2_nearby);
        all_peers.extend_from_slice(&self.tier3_local);
        all_peers.extend_from_slice(&self.tier4_global);

        // Calculate target count
        let target_count = ((all_peers.len() as f64) * percentage).ceil() as usize;
        let target_count = std::cmp::max(1, target_count); // At least 1 if any peers exist

        // Random selection
        let mut rng = thread_rng();
        all_peers.shuffle(&mut rng);
        all_peers.truncate(target_count);

        all_peers
    }
}

/// Spatial sharding manager for intelligent peer selection
pub struct SpatialSharding {
    /// Configuration parameters
    config: SpatialConfig,

    /// Known peers and their positions
    peers: HashMap<PeerId, PeerInfo>,

    /// Local player position (for distance calculations)
    local_position: ECEF,

    /// Current region ID (nano level - 1km cells)
    current_region: RegionId,

    /// Subscribed region topics (for dynamic topic management)
    subscribed_regions: HashSet<RegionId>,

    /// Last time we ran lazy gossip
    last_gossip: Instant,

    /// Peers we've already gossiped to recently (deduplication)
    recent_gossip_targets: HashSet<PeerId>,
}

impl SpatialSharding {
    /// Create a new spatial sharding manager
    pub fn new(local_position: ECEF, config: SpatialConfig) -> Self {
        let current_region = RegionId::from_ecef(&local_position, 3); // Nano level (1km cells)

        Self {
            config,
            peers: HashMap::new(),
            local_position,
            current_region,
            subscribed_regions: HashSet::new(),
            last_gossip: Instant::now(),
            recent_gossip_targets: HashSet::new(),
        }
    }

    /// Create with default configuration
    pub fn new_default(local_position: ECEF) -> Self {
        Self::new(local_position, SpatialConfig::default())
    }

    /// Update local player position (recalculates all peer distances and checks for region changes)
    ///
    /// Returns true if the player moved to a new region (triggers topic re-subscription)
    pub fn update_local_position(&mut self, position: ECEF) -> bool {
        self.local_position = position;

        // Check if we moved to a new region
        let new_region = RegionId::from_ecef(&position, 3);
        let changed_region = new_region != self.current_region;

        if changed_region {
            self.current_region = new_region;
        }

        // Recalculate distances for all known peers
        for peer in self.peers.values_mut() {
            peer.distance_m = self.local_position.distance_to(&peer.position);
        }

        changed_region
    }

    /// Get current region ID
    pub fn current_region(&self) -> &RegionId {
        &self.current_region
    }

    /// Get topics to subscribe to based on current position
    ///
    /// Returns:
    /// - Current region (nano level - 1km)
    /// - 8 neighboring regions (for cross-boundary visibility)
    /// - Parent region (micro level - 10km, for regional awareness)
    /// - Parent's neighbors (for broader context)
    ///
    /// This ensures smooth transitions when moving between regions.
    pub fn get_subscribe_topics(&self, prefix: &str) -> Vec<String> {
        let mut topics = Vec::new();

        // Current region + neighbors (9 topics at nano level)
        for region in self.current_region.neighbors() {
            topics.push(region.to_topic(prefix));
        }

        // Parent region + neighbors (9 topics at micro level)
        if let Some(parent) = self.current_region.parent() {
            for region in parent.neighbors() {
                topics.push(region.to_topic(prefix));
            }
        }

        // Total: ~18 topics (with some overlap)
        topics.sort();
        topics.dedup();
        topics
    }

    /// Get topic to publish to for current position
    ///
    /// Returns the most specific (nano level) region topic
    pub fn get_publish_topic(&self, prefix: &str) -> String {
        self.current_region.to_topic(prefix)
    }

    /// Update subscribed regions (for tracking)
    pub fn set_subscribed_regions(&mut self, regions: HashSet<RegionId>) {
        self.subscribed_regions = regions;
    }

    /// Check if we should re-subscribe (moved to new region)
    pub fn needs_resubscribe(&self) -> bool {
        !self.subscribed_regions.contains(&self.current_region)
    }

    /// Register or update a peer's position
    pub fn update_peer(&mut self, peer_id: PeerId, position: ECEF, is_relay: bool) {
        if let Some(peer) = self.peers.get_mut(&peer_id) {
            peer.update_position(position, &self.local_position);
            peer.is_relay = is_relay;
        } else {
            let mut peer = PeerInfo::new(peer_id, position, is_relay);
            peer.distance_m = self.local_position.distance_to(&position);
            self.peers.insert(peer_id, peer);
        }
    }

    /// Remove a peer (disconnected)
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
        self.recent_gossip_targets.remove(peer_id);
    }

    /// Get tiered peer selection based on distance
    pub fn get_tiered_selection(&self) -> TieredPeerSelection {
        let mut tier1 = Vec::new();
        let mut tier2 = Vec::new();
        let mut tier3 = Vec::new();
        let mut tier4 = Vec::new();

        for peer in self.peers.values() {
            if peer.distance_m <= self.config.tier1_radius_m {
                tier1.push(peer.peer_id);
            } else if peer.distance_m <= self.config.tier2_radius_m {
                tier2.push(peer.peer_id);
            } else if peer.distance_m <= self.config.tier3_radius_m {
                tier3.push(peer.peer_id);
            } else {
                tier4.push(peer.peer_id);
            }
        }

        // Sort tier4 by distance (closest first)
        tier4.sort_by(|a, b| {
            let dist_a = self.peers.get(a).map(|p| p.distance_m).unwrap_or(f64::MAX);
            let dist_b = self.peers.get(b).map(|p| p.distance_m).unwrap_or(f64::MAX);
            dist_a
                .partial_cmp(&dist_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        TieredPeerSelection {
            tier1_visible: tier1,
            tier2_nearby: tier2,
            tier3_local: tier3,
            tier4_global: tier4,
            total_peers: self.peers.len(),
        }
    }

    /// Select peers for active push (guaranteed delivery)
    ///
    /// Returns up to `redundancy_target` peers using hierarchical fallback.
    pub fn select_for_broadcast(&self) -> Vec<PeerId> {
        let tiers = self.get_tiered_selection();
        tiers.select_for_active_push(self.config.redundancy_target)
    }

    /// Select peers for lazy gossip (eventual consistency)
    ///
    /// Returns a random subset of peers if gossip interval has elapsed.
    /// Returns empty vector if not time to gossip yet.
    pub fn select_for_gossip(&mut self) -> Vec<PeerId> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_gossip);

        if elapsed < self.config.gossip_interval {
            return Vec::new(); // Not time yet
        }

        // Reset gossip timer
        self.last_gossip = now;
        self.recent_gossip_targets.clear();

        // Select random peers
        let tiers = self.get_tiered_selection();
        let selected = tiers.select_for_lazy_gossip(self.config.gossip_percentage);

        // Track who we gossiped to
        self.recent_gossip_targets.extend(selected.iter().copied());

        selected
    }

    /// Get configuration
    pub fn config(&self) -> &SpatialConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: SpatialConfig) {
        self.config = config;
    }

    /// Get number of known peers
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get peer info by ID
    pub fn get_peer(&self, peer_id: &PeerId) -> Option<&PeerInfo> {
        self.peers.get(peer_id)
    }

    /// Get all known peers
    pub fn peers(&self) -> impl Iterator<Item = &PeerInfo> {
        self.peers.values()
    }

    /// Get relay nodes (high uptime, long-term storage)
    pub fn relay_nodes(&self) -> impl Iterator<Item = &PeerInfo> {
        self.peers.values().filter(|p| p.is_relay)
    }

    /// Get statistics for monitoring
    pub fn stats(&self) -> SpatialStats {
        let tiers = self.get_tiered_selection();
        SpatialStats {
            total_peers: self.peers.len(),
            tier1_visible: tiers.tier1_visible.len(),
            tier2_nearby: tiers.tier2_nearby.len(),
            tier3_local: tiers.tier3_local.len(),
            tier4_global: tiers.tier4_global.len(),
            relay_nodes: self.peers.values().filter(|p| p.is_relay).count(),
            last_gossip_age: Instant::now().duration_since(self.last_gossip),
        }
    }
}

/// Statistics for monitoring spatial sharding
#[derive(Debug, Clone)]
pub struct SpatialStats {
    pub total_peers: usize,
    pub tier1_visible: usize,
    pub tier2_nearby: usize,
    pub tier3_local: usize,
    pub tier4_global: usize,
    pub relay_nodes: usize,
    pub last_gossip_age: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer_id(n: u8) -> PeerId {
        // Create a valid peer ID for testing.
        // The old raw-byte constructor drifted when PeerId encoding changed.
        let _ = n;
        libp2p::identity::Keypair::generate_ed25519()
            .public()
            .to_peer_id()
    }

    #[test]
    fn test_tiered_selection() {
        let local = ECEF::new(0.0, 0.0, 0.0);
        let mut sharding = SpatialSharding::new_default(local);

        // Add peers at various distances
        sharding.update_peer(create_test_peer_id(1), ECEF::new(50.0, 0.0, 0.0), false); // 50m
        sharding.update_peer(create_test_peer_id(2), ECEF::new(200.0, 0.0, 0.0), false); // 200m
        sharding.update_peer(create_test_peer_id(3), ECEF::new(700.0, 0.0, 0.0), false); // 700m
        sharding.update_peer(create_test_peer_id(4), ECEF::new(2000.0, 0.0, 0.0), false); // 2000m

        let tiers = sharding.get_tiered_selection();

        assert_eq!(tiers.tier1_visible.len(), 1); // 50m peer
        assert_eq!(tiers.tier2_nearby.len(), 1); // 200m peer
        assert_eq!(tiers.tier3_local.len(), 1); // 700m peer
        assert_eq!(tiers.tier4_global.len(), 1); // 2000m peer
    }

    #[test]
    fn test_hierarchical_fallback() {
        let local = ECEF::new(0.0, 0.0, 0.0);
        let mut sharding = SpatialSharding::new_default(local);

        // Only distant peers available
        sharding.update_peer(create_test_peer_id(1), ECEF::new(2000.0, 0.0, 0.0), false);
        sharding.update_peer(create_test_peer_id(2), ECEF::new(3000.0, 0.0, 0.0), false);
        sharding.update_peer(create_test_peer_id(3), ECEF::new(4000.0, 0.0, 0.0), false);

        let selected = sharding.select_for_broadcast();

        // Should select 3 peers despite them being far away (tier 4 fallback)
        assert_eq!(selected.len(), 3);
    }

    #[test]
    fn test_prefer_nearby() {
        let local = ECEF::new(0.0, 0.0, 0.0);
        let mut sharding = SpatialSharding::new_default(local);

        // Mix of nearby and distant peers
        let nearby1 = create_test_peer_id(1);
        let nearby2 = create_test_peer_id(2);
        sharding.update_peer(nearby1, ECEF::new(50.0, 0.0, 0.0), false); // 50m
        sharding.update_peer(nearby2, ECEF::new(80.0, 0.0, 0.0), false); // 80m
        sharding.update_peer(create_test_peer_id(3), ECEF::new(2000.0, 0.0, 0.0), false); // 2000m

        sharding.config.redundancy_target = 2;
        let selected = sharding.select_for_broadcast();

        // Should prefer the 2 nearby peers over distant one
        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&nearby1));
        assert!(selected.contains(&nearby2));
    }
}
