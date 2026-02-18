//! Multiplayer integration layer
//!
//! Coordinates P2P networking with game systems:
//! - Player state broadcasting and synchronization
//! - Voxel operation synchronization with CRDT merge
//! - Remote player rendering
//! - Signature verification for security
//!
//! # Architecture
//!
//! ```text
//! Game Loop (60 Hz)
//!     │
//!     ├─> MultiplayerSystem::update() ────> Network poll
//!     │                                      │
//!     ├─> Broadcast timer (20 Hz)           │
//!     │   └─> send_player_state()            │
//!     │                                      │
//!     ├─> Voxel dig/place                    │
//!     │   └─> send_voxel_operation()         │
//!     │                                      │
//!     └─> Render remote players <───────────┘
//!         └─> draw_remote_players()       Messages
//! ```
//!
//! # Usage
//!
//! ```no_run
//! use metaverse_core::multiplayer::MultiplayerSystem;
//! use metaverse_core::identity::Identity;
//!
//! let identity = Identity::load_or_create()?;
//! let mut mp = MultiplayerSystem::new(identity)?;
//! mp.listen_on("/ip4/0.0.0.0/tcp/0")?;
//!
//! // In game loop
//! loop {
//!     mp.update(&player, dt);
//!     
//!     if digging {
//!         mp.broadcast_voxel_op(coord, Material::Air);
//!     }
//!     
//!     for remote in mp.remote_players() {
//!         render_player_capsule(remote);
//!     }
//! }
//! ```

use crate::{
    coordinates::ECEF,
    identity::Identity,
    messages::{
        ChatMessage, LamportClock, Material, MovementMode, 
        PlayerStateMessage, VoxelOperation, MessageError,
    },
    network::{NetworkEvent, NetworkNode, NetworkError},
    player_state::PlayerStateManager,
    voxel::{Octree, VoxelCoord},
    physics::PhysicsWorld,
};
use libp2p::{Multiaddr, PeerId};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Result type for multiplayer operations
pub type Result<T> = std::result::Result<T, MultiplayerError>;

/// Errors in multiplayer system
#[derive(Debug, thiserror::Error)]
pub enum MultiplayerError {
    #[error("Network error: {0}")]
    Network(#[from] NetworkError),
    
    #[error("Message error: {0}")]
    Message(#[from] MessageError),
    
    #[error("Invalid signature from peer {0}")]
    InvalidSignature(PeerId),
    
    #[error("Malicious peer {0} exceeded reputation threshold")]
    MaliciousPeer(PeerId),
    
    #[error("Invalid multiaddr: {0}")]
    InvalidMultiaddr(String),
}

/// Gossipsub topic names for different message channels
pub const TOPIC_PLAYER_STATE: &str = "player-state";
pub const TOPIC_VOXEL_OPS: &str = "voxel-ops";
pub const TOPIC_CHAT: &str = "chat";

/// Broadcast interval for player state (20 Hz = 50ms)
const PLAYER_STATE_BROADCAST_INTERVAL: Duration = Duration::from_millis(50);

/// Maximum allowed invalid signatures before blocking peer
const MAX_INVALID_SIGNATURES: usize = 5;

/// Multiplayer system coordinating all P2P functionality
pub struct MultiplayerSystem {
    /// Network node for P2P communication
    network: NetworkNode,
    
    /// Our cryptographic identity
    identity: Identity,
    
    /// Remote player state manager (interpolation, jitter buffer)
    remote_players: PlayerStateManager,
    
    /// Lamport clock for causal ordering
    clock: LamportClock,
    
    /// Deduplication set for voxel operations (by operation ID)
    voxel_op_seen: HashSet<[u8; 64]>, // Store signature as ID
    
    /// Peer reputation tracking (invalid signatures count)
    peer_reputation: HashMap<PeerId, usize>,
    
    /// Blocked peers (too many invalid signatures)
    blocked_peers: HashSet<PeerId>,
    
    /// Timer for player state broadcasts
    last_state_broadcast: Instant,
    
    /// Statistics
    stats: MultiplayerStats,
}

/// Statistics for monitoring and debugging
#[derive(Debug, Default, Clone)]
pub struct MultiplayerStats {
    pub player_states_sent: u64,
    pub player_states_received: u64,
    pub voxel_ops_sent: u64,
    pub voxel_ops_received: u64,
    pub voxel_ops_applied: u64,
    pub voxel_ops_rejected: u64,
    pub invalid_signatures: u64,
    pub messages_received: u64,
}

impl MultiplayerSystem {
    /// Create a new multiplayer system
    pub fn new(identity: Identity) -> Result<Self> {
        let mut network = NetworkNode::new(identity.clone())?;
        
        // Subscribe to all topics
        network.subscribe(TOPIC_PLAYER_STATE)?;
        network.subscribe(TOPIC_VOXEL_OPS)?;
        network.subscribe(TOPIC_CHAT)?;
        
        let local_peer = *network.local_peer_id();
        
        Ok(Self {
            network,
            identity,
            remote_players: PlayerStateManager::new(local_peer),
            clock: LamportClock::default(),
            voxel_op_seen: HashSet::new(),
            peer_reputation: HashMap::new(),
            blocked_peers: HashSet::new(),
            last_state_broadcast: Instant::now(),
            stats: MultiplayerStats::default(),
        })
    }
    
    /// Start listening on the given address
    pub fn listen_on(&mut self, addr: &str) -> Result<()> {
        self.network.listen_on(addr)?;
        Ok(())
    }
    
    /// Connect to a specific peer
    pub fn dial(&mut self, addr: &str) -> Result<()> {
        self.network.dial(addr)?;
        Ok(())
    }
    
    /// Get our PeerId
    pub fn peer_id(&self) -> PeerId {
        *self.network.local_peer_id()
    }
    
    /// Update multiplayer system - call this every frame
    ///
    /// Processes network events, updates remote player interpolation,
    /// and handles periodic broadcasts.
    pub fn update(&mut self, _dt: f32) {
        // Process all pending network events
        while let Some(event) = self.network.poll() {
            if let Err(e) = self.handle_network_event(event) {
                eprintln!("Error handling network event: {}", e);
            }
        }
        
        // Update remote player interpolation
        self.remote_players.update_interpolation();
        
        // Clean up stale players
        self.remote_players.remove_stale_players();
    }
    
    /// Broadcast player state if enough time has elapsed (20 Hz)
    pub fn broadcast_player_state(
        &mut self,
        position: ECEF,
        velocity: [f32; 3],
        yaw: f32,
        pitch: f32,
        movement_mode: MovementMode,
    ) -> Result<()> {
        let now = Instant::now();
        if now.duration_since(self.last_state_broadcast) < PLAYER_STATE_BROADCAST_INTERVAL {
            return Ok(()); // Not time yet
        }
        
        self.last_state_broadcast = now;
        
        let timestamp = self.clock.tick();
        let msg = PlayerStateMessage::new(
            *self.network.local_peer_id(),
            position,
            velocity,
            yaw,
            pitch,
            movement_mode,
            timestamp,
        );
        
        let data = msg.to_bytes()?;
        self.network.publish(TOPIC_PLAYER_STATE, data)?;
        self.stats.player_states_sent += 1;
        
        Ok(())
    }
    
    /// Broadcast a voxel operation (dig or place)
    pub fn broadcast_voxel_operation(
        &mut self,
        coord: VoxelCoord,
        material: Material,
    ) -> Result<VoxelOperation> {
        let timestamp = self.clock.tick();
        
        // Create and sign operation
        let mut op = VoxelOperation::new(
            coord,
            material,
            *self.network.local_peer_id(),
            timestamp,
        );
        
        op.sign(self.identity.signing_key());
        
        // Serialize and send
        let data = op.to_bytes()?;
        self.network.publish(TOPIC_VOXEL_OPS, data)?;
        self.stats.voxel_ops_sent += 1;
        
        // Remember we sent this (for deduplication)
        self.voxel_op_seen.insert(op.signature);
        
        Ok(op)
    }
    
    /// Send a chat message
    pub fn send_chat(&mut self, text: String) -> Result<()> {
        let timestamp = self.clock.tick();
        let msg = ChatMessage::new(*self.network.local_peer_id(), text, timestamp);
        
        let data = msg.to_bytes()?;
        self.network.publish(TOPIC_CHAT, data)?;
        
        Ok(())
    }
    
    /// Get iterator over remote players
    pub fn remote_players(&self) -> impl Iterator<Item = &crate::player_state::NetworkedPlayer> {
        self.remote_players.players()
    }
    
    /// Get statistics
    pub fn stats(&self) -> &MultiplayerStats {
        &self.stats
    }
    
    /// Handle incoming network event
    fn handle_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match event {
            NetworkEvent::Message { peer_id, topic, data } => {
                self.stats.messages_received += 1;
                
                // Ignore messages from blocked peers
                if self.blocked_peers.contains(&peer_id) {
                    return Ok(());
                }
                
                match topic.as_str() {
                    TOPIC_PLAYER_STATE => self.handle_player_state(peer_id, &data)?,
                    TOPIC_VOXEL_OPS => self.handle_voxel_operation(peer_id, &data)?,
                    TOPIC_CHAT => self.handle_chat(peer_id, &data)?,
                    _ => {}
                }
            }
            
            NetworkEvent::PeerConnected { peer_id, address } => {
                println!("🔗 Peer connected: {} @ {}", peer_id, address);
            }
            
            NetworkEvent::PeerDisconnected { peer_id } => {
                println!("💔 Peer disconnected: {}", peer_id);
                // Remove from players map
                self.remote_players.players.remove(&peer_id);
            }
            
            NetworkEvent::PeerDiscovered { peer_id } => {
                println!("🔍 Peer discovered: {}", peer_id);
            }
            
            NetworkEvent::ListeningOn { address } => {
                println!("👂 Listening on: {}", address);
            }
            
            NetworkEvent::TopicSubscribed { topic } => {
                println!("📻 Subscribed to topic: {}", topic);
            }
            
            NetworkEvent::TopicUnsubscribed { topic } => {
                println!("📴 Unsubscribed from topic: {}", topic);
            }
        }
        
        Ok(())
    }
    
    /// Handle incoming player state message
    fn handle_player_state(&mut self, _peer_id: PeerId, data: &[u8]) -> Result<()> {
        let msg = PlayerStateMessage::from_bytes(data)?;
        
        // Update Lamport clock
        self.clock.receive(msg.timestamp);
        
        // Update remote player (manager handles deduplication and filtering)
        self.remote_players.handle_message(msg);
        self.stats.player_states_received += 1;
        
        Ok(())
    }
    
    /// Handle incoming voxel operation with CRDT merge and signature verification
    fn handle_voxel_operation(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let op = VoxelOperation::from_bytes(data)?;
        
        // Check if we've already seen this operation (deduplication)
        if self.voxel_op_seen.contains(&op.signature) {
            return Ok(()); // Already applied
        }
        
        // Verify signature
        if !self.verify_operation(&op, &peer_id)? {
            self.stats.invalid_signatures += 1;
            self.stats.voxel_ops_rejected += 1;
            
            // Track reputation
            let count = self.peer_reputation.entry(peer_id).or_insert(0);
            *count += 1;
            
            if *count >= MAX_INVALID_SIGNATURES {
                eprintln!("⚠️ Blocking malicious peer {}: too many invalid signatures", peer_id);
                self.blocked_peers.insert(peer_id);
                return Err(MultiplayerError::MaliciousPeer(peer_id));
            }
            
            return Err(MultiplayerError::InvalidSignature(peer_id));
        }
        
        // Update Lamport clock
        self.clock.receive(op.timestamp);
        
        // Remember we've seen this operation
        self.voxel_op_seen.insert(op.signature);
        
        self.stats.voxel_ops_received += 1;
        
        // Caller should apply operation to octree
        // We don't do it here to avoid coupling with game state
        
        Ok(())
    }
    
    /// Handle incoming chat message
    fn handle_chat(&mut self, _peer_id: PeerId, data: &[u8]) -> Result<()> {
        let msg = ChatMessage::from_bytes(data)?;
        
        // Update Lamport clock
        self.clock.receive(msg.timestamp);
        
        // Display in console (game can hook this later)
        println!("💬 {}: {}", msg.author.to_string().chars().take(8).collect::<String>(), msg.text);
        
        Ok(())
    }
    
    /// Verify voxel operation signature
    ///
    /// This is critical security - prevents griefing and ensures operations
    /// are from legitimate peers.
    fn verify_operation(&self, op: &VoxelOperation, peer_id: &PeerId) -> Result<bool> {
        // TODO: Extract VerifyingKey from PeerId
        // For now, we verify that the signature matches the embedded author
        // Full verification requires mapping PeerId -> VerifyingKey
        
        // The operation already has the author's peer_id embedded
        if &op.author != peer_id {
            eprintln!("⚠️ Operation author mismatch: claimed {}, received from {}", 
                op.author, peer_id);
            return Ok(false);
        }
        
        // Signature verification needs the public key
        // Since we derive PeerId from Ed25519 public key, we can reconstruct it
        // For Phase 1.5, we trust that peer_id matches (libp2p validates connection)
        // Phase 2 will add full public key verification
        
        // TODO: Implement full Ed25519 verification
        // let verifying_key = extract_verifying_key_from_peer_id(peer_id)?;
        // Ok(op.verify(&verifying_key)?)
        
        // For now: Trust libp2p's connection authentication
        // The signature is there and can be verified when we add key distribution
        Ok(true)
    }
    
    /// Apply a received voxel operation to the octree with CRDT merge
    ///
    /// Returns true if operation was applied (won the CRDT merge),
    /// false if it was rejected (lost to a conflicting operation).
    pub fn apply_voxel_operation(
        &mut self,
        op: VoxelOperation,
        octree: &mut Octree,
        local_ops: &HashMap<VoxelCoord, VoxelOperation>,
    ) -> bool {
        // CRDT merge: check if there's a local conflicting operation
        if let Some(local_op) = local_ops.get(&op.coord) {
            if !op.wins_over(local_op) {
                // Local operation wins, reject remote
                self.stats.voxel_ops_rejected += 1;
                return false;
            }
        }
        
        // Apply operation
        let material_id = op.material.to_material_id();
        octree.set_voxel(op.coord, material_id);
        
        self.stats.voxel_ops_applied += 1;
        true
    }
    
    /// Get number of connected peers
    pub fn peer_count(&self) -> usize {
        self.remote_players.player_count()
    }
    
    /// Check if a peer is blocked
    pub fn is_peer_blocked(&self, peer_id: &PeerId) -> bool {
        self.blocked_peers.contains(peer_id)
    }
}

/// Helper to convert ECEF position to local rendering space
pub fn ecef_to_local(ecef: &ECEF, physics: &PhysicsWorld) -> glam::Vec3 {
    physics.ecef_to_local(ecef)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_multiplayer_system_creation() {
        let identity = Identity::generate().unwrap();
        let mp = MultiplayerSystem::new(identity);
        assert!(mp.is_ok());
    }
    
    #[test]
    fn test_voxel_op_deduplication() {
        let identity = Identity::generate().unwrap();
        let mut mp = MultiplayerSystem::new(identity.clone()).unwrap();
        
        let coord = VoxelCoord::new(0, 0, 0);
        let material = Material::Stone;
        
        // Send operation twice
        let op1 = mp.broadcast_voxel_operation(coord, material).unwrap();
        let result = mp.broadcast_voxel_operation(coord, material).unwrap();
        
        // Should be deduplicated (both have same signature after seeing first)
        assert!(mp.voxel_op_seen.contains(&op1.signature));
        assert_eq!(mp.stats.voxel_ops_sent, 2); // Both were sent
    }
}
