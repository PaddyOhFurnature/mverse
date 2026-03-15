//! Networked player state management
//!
//! Manages remote player state with interpolation for smooth movement
//! despite network jitter and packet loss.
//!
//! # Architecture
//!
//! - **Local Player:** Authoritative, broadcasts state at 20Hz
//! - **Remote Players:** Stores last N states, interpolates between them
//! - **Prediction:** Extrapolates position when packets are late
//! - **Jitter Buffer:** 100ms buffer to smooth out network variance
//!
//! # Bandwidth
//!
//! Each player broadcasts PlayerStateMessage at 20Hz:
//! - 64 bytes * 20 = 1.28 KB/s per player
//! - 10 nearby players = 12.8 KB/s total
//! - Fits Priority 2 bandwidth budget (1-5 KB/s base)

use crate::coordinates::ECEF;
use crate::messages::{LamportClock, MovementMode, PlayerStateMessage};
use libp2p::PeerId;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Maximum number of states to keep in history for interpolation
const STATE_HISTORY_SIZE: usize = 10;

/// Interpolation delay (jitter buffer) in milliseconds
const INTERPOLATION_DELAY_MS: u64 = 100;

/// Maximum age of a state before considering it stale
const MAX_STATE_AGE_MS: u64 = 5000;

/// Remote player state with interpolation
#[derive(Debug, Clone)]
pub struct NetworkedPlayer {
    /// Peer ID of this player
    pub peer_id: PeerId,

    /// Current interpolated position
    pub position: ECEF,

    /// Current interpolated velocity
    pub velocity: [f32; 3],

    /// Current yaw (horizontal rotation)
    pub yaw: f32,

    /// Current pitch (vertical rotation)
    pub pitch: f32,

    /// Current movement mode
    pub movement_mode: MovementMode,

    /// History of received states (for interpolation)
    state_history: VecDeque<TimestampedState>,

    /// Last update time (for staleness detection)
    last_update: Instant,
}

/// State snapshot with timestamp
#[derive(Debug, Clone)]
struct TimestampedState {
    /// ECEF position at this snapshot
    position: ECEF,

    /// Velocity at this snapshot
    velocity: [f32; 3],

    /// Yaw at this snapshot
    yaw: f32,

    /// Pitch at this snapshot
    pitch: f32,

    /// Lamport timestamp (for ordering)
    lamport_time: u64,

    /// Wall-clock time when received (for interpolation)
    received_at: Instant,
}

impl NetworkedPlayer {
    /// Create a new networked player from initial state message
    pub fn new(msg: &PlayerStateMessage) -> Self {
        let now = Instant::now();
        let state = TimestampedState {
            position: msg.position,
            velocity: msg.velocity,
            yaw: msg.yaw,
            pitch: msg.pitch,
            lamport_time: msg.timestamp,
            received_at: now,
        };

        let mut state_history = VecDeque::new();
        state_history.push_back(state.clone());

        Self {
            peer_id: msg.peer_id,
            position: msg.position,
            velocity: msg.velocity,
            yaw: msg.yaw,
            pitch: msg.pitch,
            movement_mode: msg.movement_mode,
            state_history,
            last_update: now,
        }
    }

    /// Update with new state message
    ///
    /// Adds to history, maintains sorted order by Lamport timestamp,
    /// and prunes old states beyond STATE_HISTORY_SIZE.
    pub fn update(&mut self, msg: &PlayerStateMessage) {
        let now = Instant::now();
        self.last_update = now;

        let state = TimestampedState {
            position: msg.position,
            velocity: msg.velocity,
            yaw: msg.yaw,
            pitch: msg.pitch,
            lamport_time: msg.timestamp,
            received_at: now,
        };

        // Insert maintaining Lamport timestamp order
        let insert_pos = self
            .state_history
            .iter()
            .position(|s| s.lamport_time > msg.timestamp)
            .unwrap_or(self.state_history.len());

        self.state_history.insert(insert_pos, state);

        // Prune old states
        while self.state_history.len() > STATE_HISTORY_SIZE {
            self.state_history.pop_front();
        }

        // Update immediate values (for UI display)
        self.movement_mode = msg.movement_mode;
    }

    /// Interpolate state based on current time
    ///
    /// Uses interpolation delay (jitter buffer) to smooth out network variance.
    /// Falls back to extrapolation if no recent states available.
    pub fn interpolate(&mut self, now: Instant) {
        if self.state_history.len() < 2 {
            // Not enough history, use latest state with velocity extrapolation
            if let Some(latest) = self.state_history.back().cloned() {
                self.extrapolate(&latest, now);
            }
            return;
        }

        // Target time is current time minus interpolation delay
        let target_time = now - Duration::from_millis(INTERPOLATION_DELAY_MS);

        // Find two states to interpolate between
        let mut prev: Option<TimestampedState> = None;
        let mut next: Option<TimestampedState> = None;

        for state in self.state_history.iter() {
            if state.received_at <= target_time {
                prev = Some(state.clone());
            } else {
                next = Some(state.clone());
                break;
            }
        }

        match (prev, next) {
            (Some(s1), Some(s2)) => {
                // Interpolate between s1 and s2
                let dt = (s2.received_at - s1.received_at).as_secs_f32();
                if dt < 0.001 {
                    // States too close together, use latest
                    self.apply_state(&s2);
                    return;
                }

                let t_elapsed = (target_time - s1.received_at).as_secs_f32();
                let alpha = (t_elapsed / dt).clamp(0.0, 1.0);

                // Linear interpolation
                self.position = ECEF {
                    x: lerp_f64(s1.position.x, s2.position.x, alpha as f64),
                    y: lerp_f64(s1.position.y, s2.position.y, alpha as f64),
                    z: lerp_f64(s1.position.z, s2.position.z, alpha as f64),
                };

                self.velocity = [
                    lerp(s1.velocity[0], s2.velocity[0], alpha),
                    lerp(s1.velocity[1], s2.velocity[1], alpha),
                    lerp(s1.velocity[2], s2.velocity[2], alpha),
                ];

                // Angle interpolation (handle wrapping)
                self.yaw = lerp_angle(s1.yaw, s2.yaw, alpha);
                self.pitch = lerp_angle(s1.pitch, s2.pitch, alpha);
            }
            (Some(latest), None) => {
                // Only past states available, extrapolate from latest
                self.extrapolate(&latest, target_time);
            }
            (None, Some(earliest)) => {
                // Only future states available (shouldn't happen with jitter buffer)
                self.apply_state(&earliest);
            }
            (None, None) => {
                // No states (shouldn't happen)
            }
        }
    }

    /// Apply state directly (no interpolation)
    fn apply_state(&mut self, state: &TimestampedState) {
        self.position = state.position;
        self.velocity = state.velocity;
        self.yaw = state.yaw;
        self.pitch = state.pitch;
    }

    /// Extrapolate position using velocity
    fn extrapolate(&mut self, state: &TimestampedState, target_time: Instant) {
        let dt = (target_time - state.received_at).as_secs_f64();

        // Dead reckoning: position += velocity * dt
        self.position = ECEF {
            x: state.position.x + (state.velocity[0] as f64 * dt),
            y: state.position.y + (state.velocity[1] as f64 * dt),
            z: state.position.z + (state.velocity[2] as f64 * dt),
        };

        self.velocity = state.velocity;
        self.yaw = state.yaw;
        self.pitch = state.pitch;
    }

    /// Check if this player is stale (no recent updates)
    pub fn is_stale(&self, now: Instant) -> bool {
        now.duration_since(self.last_update) > Duration::from_millis(MAX_STATE_AGE_MS)
    }
}

/// Player state manager
///
/// Manages all remote players, handles state updates, and performs
/// interpolation for smooth rendering.
pub struct PlayerStateManager {
    /// Map of peer ID to networked player
    players: HashMap<PeerId, NetworkedPlayer>,

    /// Lamport clock for local player state
    local_clock: LamportClock,

    /// Local peer ID (to ignore own messages)
    local_peer_id: PeerId,
}

impl PlayerStateManager {
    /// Create a new player state manager
    pub fn new(local_peer_id: PeerId) -> Self {
        Self {
            players: HashMap::new(),
            local_clock: LamportClock::new(),
            local_peer_id,
        }
    }

    /// Handle incoming player state message
    ///
    /// Ignores messages from local player, creates new NetworkedPlayer
    /// if first time seeing this peer, otherwise updates existing state.
    pub fn handle_message(&mut self, msg: PlayerStateMessage) {
        // Ignore own messages
        if msg.peer_id == self.local_peer_id {
            return;
        }

        // Update Lamport clock
        self.local_clock.receive(msg.timestamp);

        // Update or create player
        if let Some(player) = self.players.get_mut(&msg.peer_id) {
            player.update(&msg);
        } else {
            let player = NetworkedPlayer::new(&msg);
            self.players.insert(msg.peer_id, player);
        }
    }

    /// Create local player state message
    ///
    /// Increments Lamport clock and returns message ready to broadcast.
    pub fn create_local_message(
        &mut self,
        position: ECEF,
        velocity: [f32; 3],
        yaw: f32,
        pitch: f32,
        movement_mode: MovementMode,
    ) -> PlayerStateMessage {
        let timestamp = self.local_clock.tick();

        PlayerStateMessage::new(
            self.local_peer_id,
            position,
            velocity,
            yaw,
            pitch,
            movement_mode,
            timestamp,
        )
    }

    /// Update all players' interpolation
    ///
    /// Call this every frame to update interpolated positions.
    pub fn update_interpolation(&mut self) {
        let now = Instant::now();

        for player in self.players.values_mut() {
            player.interpolate(now);
        }
    }

    /// Remove stale players (disconnected or timed out)
    ///
    /// Returns list of removed peer IDs.
    pub fn remove_stale_players(&mut self) -> Vec<PeerId> {
        let now = Instant::now();
        let mut removed = Vec::new();

        self.players.retain(|peer_id, player| {
            if player.is_stale(now) {
                removed.push(*peer_id);
                false
            } else {
                true
            }
        });

        removed
    }

    /// Remove a specific player
    ///
    /// Returns true if player was removed, false if not found.
    pub fn remove_player(&mut self, peer_id: &PeerId) -> bool {
        self.players.remove(peer_id).is_some()
    }

    /// Get all active players
    pub fn players(&self) -> impl Iterator<Item = &NetworkedPlayer> {
        self.players.values()
    }

    /// Get specific player by peer ID
    pub fn get_player(&self, peer_id: &PeerId) -> Option<&NetworkedPlayer> {
        self.players.get(peer_id)
    }

    /// Get number of active players
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

/// Linear interpolation
fn lerp<T>(a: T, b: T, t: f32) -> T
where
    T: std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<f32, Output = T>
        + Copy,
{
    a + (b - a) * t
}

/// Linear interpolation for f64
fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Angle interpolation with wrapping
///
/// Handles -π to π wrapping correctly.
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    use std::f32::consts::PI;

    let mut delta = b - a;

    // Wrap to [-π, π]
    while delta > PI {
        delta -= 2.0 * PI;
    }
    while delta < -PI {
        delta += 2.0 * PI;
    }

    a + delta * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lerp_angle() {
        use std::f32::consts::PI;

        // Normal case
        assert!((lerp_angle(0.0, 1.0, 0.5) - 0.5).abs() < 0.001);

        // Wrapping case: 170° to -170° should go through 180°, not around
        let a = 170.0_f32.to_radians();
        let b = -170.0_f32.to_radians();
        let mid = lerp_angle(a, b, 0.5);
        assert!((mid - PI).abs() < 0.001 || (mid + PI).abs() < 0.001);
    }

    #[test]
    fn test_networked_player_creation() {
        let msg = PlayerStateMessage {
            peer_id: PeerId::random(),
            position: ECEF::new(0.0, 0.0, 0.0),
            velocity: [1.0, 2.0, 3.0],
            yaw: 0.5,
            pitch: 0.25,
            movement_mode: MovementMode::Walk,
            timestamp: 100,
        };

        let player = NetworkedPlayer::new(&msg);
        assert_eq!(player.velocity, [1.0, 2.0, 3.0]);
        assert_eq!(player.state_history.len(), 1);
    }

    #[test]
    fn test_player_state_manager() {
        let local_peer_id = PeerId::random();
        let mut manager = PlayerStateManager::new(local_peer_id);

        // Create local message
        let msg = manager.create_local_message(
            ECEF::new(1.0, 2.0, 3.0),
            [0.0, 0.0, 0.0],
            0.0,
            0.0,
            MovementMode::Fly,
        );

        assert_eq!(msg.timestamp, 1); // First tick

        // Handle remote message
        let remote_peer_id = PeerId::random();
        let remote_msg = PlayerStateMessage {
            peer_id: remote_peer_id,
            position: ECEF::new(10.0, 20.0, 30.0),
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            movement_mode: MovementMode::Walk,
            timestamp: 50,
        };

        manager.handle_message(remote_msg);
        assert_eq!(manager.player_count(), 1);

        // Lamport clock should update
        assert_eq!(manager.local_clock.current(), 51);

        // Handle local message (should be ignored)
        manager.handle_message(msg);
        assert_eq!(manager.player_count(), 1); // Still 1, not 2
    }

    #[test]
    fn test_state_ordering() {
        let peer_id = PeerId::random();
        let msg1 = PlayerStateMessage {
            peer_id,
            position: ECEF::new(0.0, 0.0, 0.0),
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            movement_mode: MovementMode::Walk,
            timestamp: 100,
        };

        let mut player = NetworkedPlayer::new(&msg1);

        // Add state with higher timestamp
        let msg2 = PlayerStateMessage {
            peer_id,
            position: ECEF::new(10.0, 0.0, 0.0),
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            movement_mode: MovementMode::Walk,
            timestamp: 200,
        };
        player.update(&msg2);

        // Add state with lower timestamp (out of order)
        let msg3 = PlayerStateMessage {
            peer_id,
            position: ECEF::new(5.0, 0.0, 0.0),
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            movement_mode: MovementMode::Walk,
            timestamp: 150,
        };
        player.update(&msg3);

        // States should be ordered by Lamport timestamp
        assert_eq!(player.state_history.len(), 3);
        assert_eq!(player.state_history[0].lamport_time, 100);
        assert_eq!(player.state_history[1].lamport_time, 150);
        assert_eq!(player.state_history[2].lamport_time, 200);
    }
}
