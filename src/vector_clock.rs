//! Vector clock implementation for CRDT causality tracking
//!
//! Vector clocks provide proper causal ordering for distributed operations.
//! Unlike Lamport timestamps which only provide total ordering, vector clocks
//! can detect concurrent operations.
//!
//! # Why Vector Clocks?
//!
//! **Problem with Lamport timestamps:**
//! - Two operations with different timestamps might actually be concurrent
//! - Can't distinguish "A caused B" from "A and B happened simultaneously"
//!
//! **Vector clock solution:**
//! - Each peer maintains a counter for every other peer
//! - Can definitively determine: happens-before, happens-after, or concurrent
//!
//! # Example
//!
//! ```
//! use metaverse_core::vector_clock::VectorClock;
//! use libp2p::PeerId;
//!
//! let peer_a = PeerId::random();
//! let peer_b = PeerId::random();
//!
//! // Alice makes operation 1
//! let mut clock_a = VectorClock::new();
//! clock_a.increment(peer_a);
//! // clock_a = {A: 1}
//!
//! // Bob makes operation 1 (concurrent with Alice)
//! let mut clock_b = VectorClock::new();
//! clock_b.increment(peer_b);
//! // clock_b = {B: 1}
//!
//! // These operations are concurrent
//! assert!(clock_a.concurrent(&clock_b));
//!
//! // Alice receives Bob's operation and merges
//! clock_a.merge(&clock_b);
//! clock_a.increment(peer_a);
//! // clock_a = {A: 2, B: 1}
//!
//! // Now Alice's clock happens after Bob's
//! assert!(clock_a.happens_after(&clock_b));
//! ```

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Vector clock for tracking causality in distributed operations
///
/// Each peer maintains a logical clock value for every peer it knows about.
/// This allows us to determine causal relationships between operations:
/// - **happens-before**: All clock values ≤, at least one <
/// - **happens-after**: All clock values ≥, at least one >
/// - **concurrent**: Neither happens-before nor happens-after
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    /// Map of PeerId to logical clock value
    /// Using BTreeMap for deterministic ordering (important for serialization)
    clocks: BTreeMap<String, u64>,
}

impl VectorClock {
    /// Create a new empty vector clock
    pub fn new() -> Self {
        Self {
            clocks: BTreeMap::new(),
        }
    }

    /// Increment the clock for a specific peer
    ///
    /// Call this when a peer performs an operation:
    /// ```
    /// let mut clock = VectorClock::new();
    /// clock.increment(my_peer_id);
    /// ```
    pub fn increment(&mut self, peer: PeerId) {
        let key = peer.to_base58();
        let counter = self.clocks.entry(key).or_insert(0);
        *counter += 1;
    }

    /// Get the clock value for a specific peer
    pub fn get(&self, peer: &PeerId) -> u64 {
        let key = peer.to_base58();
        self.clocks.get(&key).copied().unwrap_or(0)
    }

    /// Set the clock value for a specific peer
    pub fn set(&mut self, peer: PeerId, value: u64) {
        let key = peer.to_base58();
        self.clocks.insert(key, value);
    }

    /// Merge another vector clock into this one (take maximum of each value)
    ///
    /// Call this when receiving an operation from another peer:
    /// ```
    /// clock.merge(&received_clock);
    /// clock.increment(my_peer_id);
    /// ```
    pub fn merge(&mut self, other: &VectorClock) {
        for (peer, &value) in &other.clocks {
            let current = self.clocks.entry(peer.clone()).or_insert(0);
            *current = (*current).max(value);
        }
    }

    /// Check if this clock happens before another (causal predecessor)
    ///
    /// Returns true if:
    /// - All values in self ≤ corresponding values in other
    /// - At least one value in self < corresponding value in other
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut has_less = false;

        // Check all peers we know about
        for (peer, &self_value) in &self.clocks {
            let other_value = other.clocks.get(peer).copied().unwrap_or(0);
            if self_value > other_value {
                return false; // Self has a larger value, can't be before
            }
            if self_value < other_value {
                has_less = true;
            }
        }

        // Check peers other knows about that we don't
        for (peer, &other_value) in &other.clocks {
            if !self.clocks.contains_key(peer) && other_value > 0 {
                has_less = true;
            }
        }

        has_less
    }

    /// Check if this clock happens after another (causal successor)
    pub fn happens_after(&self, other: &VectorClock) -> bool {
        other.happens_before(self)
    }

    /// Check if this clock is concurrent with another
    ///
    /// Two clocks are concurrent if neither happens-before the other.
    /// This means the operations occurred independently without knowledge of each other.
    pub fn concurrent(&self, other: &VectorClock) -> bool {
        self != other && !self.happens_before(other) && !other.happens_before(self)
    }

    /// Compare two vector clocks
    ///
    /// Returns:
    /// - `Ordering::Less` if self happens-before other
    /// - `Ordering::Greater` if self happens-after other
    /// - `Ordering::Equal` if clocks are identical OR concurrent
    ///
    /// Note: For proper concurrent detection, use `concurrent()` method.
    pub fn partial_cmp_clocks(&self, other: &VectorClock) -> Ordering {
        if self == other {
            Ordering::Equal
        } else if self.happens_before(other) {
            Ordering::Less
        } else if self.happens_after(other) {
            Ordering::Greater
        } else {
            // Concurrent - no causal relationship
            Ordering::Equal
        }
    }

    /// Get the number of peers tracked in this clock
    pub fn peer_count(&self) -> usize {
        self.clocks.len()
    }

    /// Check if this clock is empty (no operations tracked)
    pub fn is_empty(&self) -> bool {
        self.clocks.is_empty()
    }

    /// Get all peers tracked in this clock
    pub fn peers(&self) -> Vec<PeerId> {
        self.clocks
            .keys()
            .filter_map(|s| PeerId::from_bytes(s.as_bytes()).ok())
            .collect()
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_increment() {
        let peer = PeerId::random();
        let mut clock = VectorClock::new();

        assert_eq!(clock.get(&peer), 0);

        clock.increment(peer);
        assert_eq!(clock.get(&peer), 1);

        clock.increment(peer);
        assert_eq!(clock.get(&peer), 2);
    }

    #[test]
    fn test_merge() {
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        let mut clock1 = VectorClock::new();
        clock1.increment(peer_a);
        clock1.increment(peer_a);
        // clock1 = {A: 2}

        let mut clock2 = VectorClock::new();
        clock2.increment(peer_b);
        // clock2 = {B: 1}

        clock1.merge(&clock2);
        // clock1 = {A: 2, B: 1}

        assert_eq!(clock1.get(&peer_a), 2);
        assert_eq!(clock1.get(&peer_b), 1);
    }

    #[test]
    fn test_happens_before() {
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        let mut clock1 = VectorClock::new();
        clock1.increment(peer_a);
        // clock1 = {A: 1}

        let mut clock2 = VectorClock::new();
        clock2.merge(&clock1);
        clock2.increment(peer_a);
        clock2.increment(peer_b);
        // clock2 = {A: 2, B: 1}

        assert!(clock1.happens_before(&clock2));
        assert!(!clock2.happens_before(&clock1));
    }

    #[test]
    fn test_happens_after() {
        let peer_a = PeerId::random();

        let mut clock1 = VectorClock::new();
        clock1.increment(peer_a);
        // clock1 = {A: 1}

        let mut clock2 = VectorClock::new();
        clock2.merge(&clock1);
        clock2.increment(peer_a);
        // clock2 = {A: 2}

        assert!(clock2.happens_after(&clock1));
        assert!(!clock1.happens_after(&clock2));
    }

    #[test]
    fn test_concurrent() {
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        // Alice makes operation 1
        let mut clock_a = VectorClock::new();
        clock_a.increment(peer_a);
        // clock_a = {A: 1}

        // Bob makes operation 1 (concurrent with Alice)
        let mut clock_b = VectorClock::new();
        clock_b.increment(peer_b);
        // clock_b = {B: 1}

        // These are concurrent
        assert!(clock_a.concurrent(&clock_b));
        assert!(clock_b.concurrent(&clock_a));

        // Alice receives Bob's operation
        clock_a.merge(&clock_b);
        clock_a.increment(peer_a);
        // clock_a = {A: 2, B: 1}

        // Now Alice's clock is after Bob's
        assert!(!clock_a.concurrent(&clock_b));
        assert!(clock_a.happens_after(&clock_b));
    }

    #[test]
    fn test_concurrent_complex() {
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        let mut clock1 = VectorClock::new();
        clock1.increment(peer_a);
        clock1.increment(peer_a);
        clock1.increment(peer_b);
        // clock1 = {A: 2, B: 1}

        let mut clock2 = VectorClock::new();
        clock2.increment(peer_a);
        clock2.increment(peer_b);
        clock2.increment(peer_b);
        // clock2 = {A: 1, B: 2}

        // These are concurrent (A:2 > A:1 but B:1 < B:2)
        assert!(clock1.concurrent(&clock2));
    }

    #[test]
    fn test_identical_clocks() {
        let peer_a = PeerId::random();

        let mut clock1 = VectorClock::new();
        clock1.increment(peer_a);

        let mut clock2 = VectorClock::new();
        clock2.increment(peer_a);

        assert_eq!(clock1, clock2);
        assert!(!clock1.happens_before(&clock2));
        assert!(!clock1.happens_after(&clock2));
        assert!(!clock1.concurrent(&clock2)); // Identical, not concurrent
    }

    #[test]
    fn test_serialization() {
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        let mut clock = VectorClock::new();
        clock.increment(peer_a);
        clock.increment(peer_b);
        clock.increment(peer_a);

        // Serialize to JSON
        let json = serde_json::to_string(&clock).unwrap();

        // Deserialize back
        let deserialized: VectorClock = serde_json::from_str(&json).unwrap();

        assert_eq!(clock, deserialized);
        assert_eq!(deserialized.get(&peer_a), 2);
        assert_eq!(deserialized.get(&peer_b), 1);
    }
}
