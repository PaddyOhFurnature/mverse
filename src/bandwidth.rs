//! Bandwidth profiles and message priority management
//!
//! Controls what data gets sent under different network conditions, enabling graceful
//! degradation from gigabit LAN all the way down to LoRa radio (~250 bps).
//!
//! # Priority Tiers
//!
//! ```text
//! Critical  — Presence beacon, identity. ALWAYS sent.
//! High      — Chat, voxel ops. Queued if congested, never dropped.
//! Normal    — Player position. Dropped if queue full.
//! Low       — Chunk terrain transfers. Background only.
//! ```
//!
//! # Bandwidth Profiles
//!
//! | Profile     | Rate       | Position rate | Chunk terrain |
//! |-------------|------------|---------------|---------------|
//! | LoRa        | ~250 bps   | suppressed    | suppressed    |
//! | Constrained | ~10 KB/s   | 2 Hz          | suppressed    |
//! | Normal      | ~100 KB/s  | 10 Hz         | queued        |
//! | LAN         | unlimited  | 20 Hz         | immediate     |
//! | Auto        | measured   | adaptive      | adaptive      |

use std::time::{Duration, Instant};

// ─── Priority ────────────────────────────────────────────────────────────────

/// Message priority class. Controls ordering and drop policy under congestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// Presence beacon, peer identity. Never dropped.
    Critical = 0,
    /// Chat, block edits. Queued if congested, never dropped.
    High = 1,
    /// Player position. Dropped if queue exceeds budget.
    Normal = 2,
    /// Chunk terrain transfers. Suppressed when bandwidth is limited.
    Low = 3,
}

// ─── Bandwidth profiles ───────────────────────────────────────────────────────

/// Active bandwidth profile.
///
/// Choose `Auto` for self-adapting behaviour — the system measures actual
/// throughput to the relay every 30 seconds and reclassifies. Tier transitions
/// require 3 consecutive upgrades to prevent flapping but downgrade immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandwidthProfile {
    /// ~250 bps — LoRa / ham radio mesh. Critical messages only.
    LoRa,
    /// ~10 KB/s — dialup, satellite, degraded 4G. No terrain transfers.
    Constrained,
    /// ~100 KB/s — normal 4G / broadband. Full game, reduced position rate.
    Normal,
    /// Effectively unlimited — LAN / gigabit. All features at max rate.
    LAN,
    /// Automatic: measure relay RTT and throughput, classify, adapt.
    Auto,
}

impl Default for BandwidthProfile {
    fn default() -> Self {
        Self::Auto
    }
}

impl BandwidthProfile {
    /// Maximum position update rate (Hz) for this profile.
    pub fn max_position_hz(&self) -> f32 {
        match self {
            Self::LoRa => 0.0,        // suppressed
            Self::Constrained => 2.0,
            Self::Normal => 10.0,
            Self::LAN => 20.0,
            Self::Auto => 20.0,       // Auto starts optimistic, degrades on measurement
        }
    }

    /// Minimum interval between position updates.
    pub fn position_interval(&self) -> Duration {
        let hz = self.max_position_hz();
        if hz <= 0.0 {
            Duration::from_secs(u64::MAX / 2) // Suppress
        } else {
            Duration::from_millis((1000.0 / hz) as u64)
        }
    }

    /// Whether chunk terrain transfers are allowed.
    pub fn allow_terrain_transfer(&self) -> bool {
        matches!(self, Self::Normal | Self::LAN | Self::Auto)
    }

    /// Whether a given priority should be sent under this profile.
    pub fn allows_priority(&self, priority: MessagePriority) -> bool {
        match self {
            Self::LoRa => priority == MessagePriority::Critical,
            Self::Constrained => priority <= MessagePriority::High,
            Self::Normal | Self::LAN | Self::Auto => true,
        }
    }

    /// Human-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::LoRa => "LoRa (~250 bps)",
            Self::Constrained => "Constrained (~10 KB/s)",
            Self::Normal => "Normal (~100 KB/s)",
            Self::LAN => "LAN (unlimited)",
            Self::Auto => "Auto (measuring)",
        }
    }
}

// ─── Auto-detection ───────────────────────────────────────────────────────────

/// Auto-detects the effective bandwidth profile by measuring relay RTT.
///
/// Classifies based on observed round-trip time:
/// - RTT < 5ms   → LAN
/// - RTT < 50ms  → Normal
/// - RTT < 300ms → Constrained
/// - RTT ≥ 300ms → Constrained (LoRa requires manual opt-in)
///
/// Hysteresis: 3 consecutive measurements needed to *upgrade*, 1 to *downgrade*.
#[derive(Debug)]
pub struct BandwidthManager {
    /// Current active profile.
    pub profile: BandwidthProfile,

    /// User-configured override (None = Auto).
    user_override: Option<BandwidthProfile>,

    /// Recent RTT measurements (ms).
    rtt_samples: Vec<f64>,

    /// Last time we updated the classification.
    last_measured: Instant,

    /// Number of consecutive measurements suggesting a better (lower) tier.
    upgrade_streak: u32,
}

impl Default for BandwidthManager {
    fn default() -> Self {
        Self::new(None)
    }
}

impl BandwidthManager {
    /// Create a new manager. Pass `Some(profile)` to pin a specific profile.
    pub fn new(user_override: Option<BandwidthProfile>) -> Self {
        let profile = user_override.unwrap_or(BandwidthProfile::Auto);
        Self {
            profile,
            user_override,
            rtt_samples: Vec::new(),
            last_measured: Instant::now(),
            upgrade_streak: 0,
        }
    }

    /// Record an RTT measurement (in milliseconds) from a relay ping response.
    ///
    /// Call this whenever the network layer receives a pong.
    pub fn record_rtt(&mut self, rtt_ms: f64) {
        if self.user_override.is_some() {
            return; // User has pinned a profile; ignore measurements.
        }

        self.rtt_samples.push(rtt_ms);
        // Keep last 5 samples for median calculation
        if self.rtt_samples.len() > 5 {
            self.rtt_samples.remove(0);
        }

        self.last_measured = Instant::now();
        self.reclassify();
    }

    /// Check whether it's time to send a position update.
    pub fn should_send_position(&self) -> bool {
        self.profile.max_position_hz() > 0.0
    }

    /// Check whether terrain transfers are allowed right now.
    pub fn should_send_terrain(&self) -> bool {
        self.profile.allow_terrain_transfer()
    }

    /// Check whether a message priority is allowed under the current profile.
    pub fn allows(&self, priority: MessagePriority) -> bool {
        self.profile.allows_priority(priority)
    }

    /// Force-set the profile (user override). Pass `None` to return to Auto.
    pub fn set_override(&mut self, profile: Option<BandwidthProfile>) {
        self.user_override = profile;
        self.profile = profile.unwrap_or(BandwidthProfile::Auto);
    }

    // ─── Internal ────────────────────────────────────────────────────────────

    fn reclassify(&mut self) {
        if self.rtt_samples.is_empty() {
            return;
        }

        // Use median RTT sample for stability
        let mut sorted = self.rtt_samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median_rtt = sorted[sorted.len() / 2];

        let candidate = classify_rtt(median_rtt);
        let current_rank = profile_rank(&self.profile);
        let candidate_rank = profile_rank(&candidate);

        if candidate_rank < current_rank {
            // Better conditions — require 3 consecutive measurements before upgrading
            self.upgrade_streak += 1;
            if self.upgrade_streak >= 3 {
                println!("[bandwidth] Upgraded to {} (RTT {:.0}ms)", candidate.name(), median_rtt);
                self.profile = candidate;
                self.upgrade_streak = 0;
            }
        } else if candidate_rank > current_rank {
            // Worse conditions — downgrade immediately
            println!("[bandwidth] Downgraded to {} (RTT {:.0}ms)", candidate.name(), median_rtt);
            self.profile = candidate;
            self.upgrade_streak = 0;
        } else {
            self.upgrade_streak = 0;
        }
    }
}

/// Classify a relay RTT into a bandwidth profile.
fn classify_rtt(rtt_ms: f64) -> BandwidthProfile {
    if rtt_ms < 5.0 {
        BandwidthProfile::LAN
    } else if rtt_ms < 50.0 {
        BandwidthProfile::Normal
    } else {
        BandwidthProfile::Constrained
    }
}

/// Numeric rank for comparison (lower = better bandwidth).
fn profile_rank(p: &BandwidthProfile) -> u8 {
    match p {
        BandwidthProfile::LAN => 0,
        BandwidthProfile::Normal => 1,
        BandwidthProfile::Constrained => 2,
        BandwidthProfile::LoRa => 3,
        BandwidthProfile::Auto => 1, // Treat Auto as Normal for comparison
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lora_suppresses_position() {
        let profile = BandwidthProfile::LoRa;
        assert!(!profile.allows_priority(MessagePriority::Normal));
        assert!(profile.allows_priority(MessagePriority::Critical));
        assert!(!profile.allow_terrain_transfer());
    }

    #[test]
    fn test_lan_allows_all() {
        let profile = BandwidthProfile::LAN;
        assert!(profile.allows_priority(MessagePriority::Low));
        assert!(profile.allow_terrain_transfer());
        assert_eq!(profile.max_position_hz(), 20.0);
    }

    #[test]
    fn test_auto_downgrades_immediately() {
        let mut mgr = BandwidthManager::new(None);
        mgr.profile = BandwidthProfile::LAN;
        mgr.record_rtt(400.0); // Very high latency
        assert_eq!(mgr.profile, BandwidthProfile::Constrained);
    }

    #[test]
    fn test_auto_requires_streak_to_upgrade() {
        let mut mgr = BandwidthManager::new(None);
        mgr.profile = BandwidthProfile::Constrained;
        mgr.record_rtt(2.0); // Low latency — should not upgrade yet
        assert_eq!(mgr.profile, BandwidthProfile::Constrained);
        mgr.record_rtt(2.0);
        assert_eq!(mgr.profile, BandwidthProfile::Constrained);
        mgr.record_rtt(2.0); // Third consecutive — should now upgrade
        assert_eq!(mgr.profile, BandwidthProfile::LAN);
    }
}
