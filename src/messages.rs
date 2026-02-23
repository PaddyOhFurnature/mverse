//! Message protocol for P2P communication
//!
//! This module defines all message types exchanged between peers:
//! - Player state updates (position, rotation, velocity)
//! - Voxel operations (dig, place) with CRDT semantics
//! - Chat messages
//! - Entity updates
//!
//! # Design Principles
//!
//! 1. **Deterministic Serialization**: All messages use bincode for consistent byte representation
//! 2. **Cryptographic Signatures**: Critical operations (voxel edits) are signed with Ed25519
//! 3. **Lamport Clocks**: All messages have logical timestamps for causal ordering
//! 4. **Bandwidth Awareness**: Messages are designed to be compact (Priority 2: State Sync)
//! 5. **CRDT Semantics**: Voxel operations are commutative and convergent
//!
//! # Bandwidth Budget Mapping
//!
//! - PlayerStateMessage: ~64 bytes @ 20Hz = 1.28 KB/s per player
//! - VoxelOperation: ~128 bytes (includes signature)
//! - ChatMessage: ~20-200 bytes (variable length text)
//!
//! All messages fit within Priority 2 bandwidth budget (1-5 KB/s total).

use crate::chunk::ChunkId;
use crate::coordinates::ECEF;
use crate::vector_clock::VectorClock;
use crate::voxel::VoxelCoord;
use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;

// Custom serde for [u8; 64] arrays
mod serde_arrays {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    
    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Vec::deserialize(deserializer)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "Expected 64 bytes, got {}",
                bytes.len()
            )));
        }
        let mut array = [0u8; 64];
        array.copy_from_slice(&bytes);
        Ok(array)
    }
}

/// Result type for message operations
pub type Result<T> = std::result::Result<T, MessageError>;

/// Errors that can occur during message processing
#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("Serialization error: {0}")]
    SerializationError(#[from] bincode::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Message too old (timestamp {0} vs current {1})")]
    MessageTooOld(u64, u64),
    
    #[error("Invalid message format: {0}")]
    InvalidFormat(String),
}

/// Lamport logical clock for causal ordering
///
/// Every message carries a Lamport timestamp. When receiving a message,
/// the local clock is updated to max(local, received) + 1.
///
/// This ensures causal ordering: if event A causes event B, then timestamp(A) < timestamp(B).
///
/// # Usage
///
/// ```no_run
/// let mut clock = LamportClock::new();
/// let ts1 = clock.tick(); // Get timestamp for outgoing message
/// clock.receive(ts1);     // Update on incoming message
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LamportClock(u64);

impl LamportClock {
    /// Create a new clock starting at 0
    pub fn new() -> Self {
        Self(0)
    }
    
    /// Increment clock and return new value (for outgoing messages)
    pub fn tick(&mut self) -> u64 {
        self.0 += 1;
        self.0
    }
    
    /// Update clock based on received timestamp
    pub fn receive(&mut self, received: u64) {
        self.0 = self.0.max(received) + 1;
    }
    
    /// Get current clock value without incrementing
    pub fn current(&self) -> u64 {
        self.0
    }
}

impl Default for LamportClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Player movement mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MovementMode {
    /// Walking on ground with gravity
    Walk,
    /// Flying with no gravity
    Fly,
}

/// Player state message (Priority 2: State Sync)
///
/// Broadcast at 20Hz to all nearby players. Contains minimum information
/// needed to render another player's position and movement.
///
/// **Bandwidth:** ~64 bytes per message
/// - ECEF position: 24 bytes (3x f64)
/// - Velocity: 12 bytes (3x f32)
/// - Yaw/Pitch: 8 bytes (2x f32)
/// - Movement mode: 1 byte
/// - Lamport timestamp: 8 bytes
/// - PeerId: ~38 bytes (multihash)
/// - Padding/overhead: ~10 bytes
///
/// At 20Hz per player: 64 * 20 = 1.28 KB/s
/// For 10 nearby players: 12.8 KB/s (still within Priority 2 budget)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStateMessage {
    /// Unique peer identifier (author of this state update)
    pub peer_id: PeerId,
    
    /// Player position in Earth-Centered Earth-Fixed coordinates
    pub position: ECEF,
    
    /// Player velocity in meters/second (ECEF frame)
    pub velocity: [f32; 3],
    
    /// Camera yaw (horizontal rotation) in radians
    pub yaw: f32,
    
    /// Camera pitch (vertical rotation) in radians
    pub pitch: f32,
    
    /// Current movement mode (walk/fly)
    pub movement_mode: MovementMode,
    
    /// Lamport timestamp for causal ordering
    pub timestamp: u64,
}

impl PlayerStateMessage {
    /// Create a new player state message
    pub fn new(
        peer_id: PeerId,
        position: ECEF,
        velocity: [f32; 3],
        yaw: f32,
        pitch: f32,
        movement_mode: MovementMode,
        timestamp: u64,
    ) -> Self {
        Self {
            peer_id,
            position,
            velocity,
            yaw,
            pitch,
            movement_mode,
            timestamp,
        }
    }
    
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    
    /// Deserialize from bytes received from network
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
}

/// Material type for voxels (network protocol representation)
///
/// This is a simplified material enum for network messages.
/// Maps to MaterialId in the voxel system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Material {
    Air = 0,
    Stone = 1,
    Dirt = 2,
    Grass = 3,
    Water = 4,
}

impl Material {
    /// Convert to MaterialId for voxel system
    pub fn to_material_id(self) -> crate::materials::MaterialId {
        match self {
            Material::Air => crate::materials::MaterialId::AIR,
            Material::Stone => crate::materials::MaterialId::STONE,
            Material::Dirt => crate::materials::MaterialId::DIRT,
            Material::Grass => crate::materials::MaterialId::GRASS,
            Material::Water => crate::materials::MaterialId::WATER,
        }
    }
    
    /// Convert from MaterialId
    pub fn from_material_id(id: crate::materials::MaterialId) -> Self {
        match id {
            crate::materials::MaterialId::AIR => Material::Air,
            crate::materials::MaterialId::STONE => Material::Stone,
            crate::materials::MaterialId::DIRT => Material::Dirt,
            crate::materials::MaterialId::GRASS => Material::Grass,
            crate::materials::MaterialId::WATER => Material::Water,
            _ => Material::Stone, // Default fallback
        }
    }
}

/// Voxel operation with CRDT semantics (Priority 2: State Sync)
///
/// Represents a single voxel modification (dig or place). Operations are:
/// - **Commutative**: Can be applied in any order and converge to same state
/// - **Idempotent**: Applying the same operation twice has no additional effect
/// - **Signed**: Author proves ownership with Ed25519 signature
///
/// **Bandwidth:** ~160 bytes per operation (increased from 128 with vector clocks)
/// - VoxelCoord: 12 bytes (3x i32)
/// - Material: 1 byte
/// - PeerId: ~38 bytes
/// - Lamport timestamp: 8 bytes (kept for backwards compat + tiebreak)
/// - VectorClock: ~20-50 bytes (grows with peer count)
/// - Signature: 64 bytes
/// - Overhead: ~5 bytes
///
/// Typical usage: Bursts during building (100 ops/sec = 16 KB/s),
/// otherwise rare (1 op/sec = 160 bytes/sec)
///
/// # CRDT Merge Rules (with Vector Clocks)
///
/// When two peers edit the same voxel:
/// 1. **Causal ordering**: If vector clocks show A→B, B wins (B saw A)
/// 2. **Concurrent operations**: If vector clocks are concurrent:
///    - Compare Lamport timestamps (higher wins)
///    - If equal, PeerId tiebreak (deterministic)
/// 3. **Signature verification**: Only signed operations from parcel owner valid
///
/// This ensures all peers converge to the same state without coordination,
/// while properly handling causal relationships.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoxelOperation {
    /// Voxel coordinate being modified
    pub coord: VoxelCoord,
    
    /// New material at this coordinate
    pub material: Material,
    
    /// Peer who authored this operation
    pub author: PeerId,
    
    /// Lamport timestamp for backwards compatibility and tiebreak
    pub timestamp: u64,
    
    /// Vector clock for causal ordering
    pub vector_clock: crate::vector_clock::VectorClock,
    
    /// Ed25519 signature over (coord, material, author, timestamp, vector_clock)
    /// Proves this operation came from the author's private key
    #[serde(with = "serde_arrays")]
    pub signature: [u8; 64],
}

impl VoxelOperation {
    /// Create a new voxel operation with vector clock
    ///
    /// Call `sign()` to add signature before broadcasting.
    pub fn new(
        coord: VoxelCoord,
        material: Material,
        author: PeerId,
        timestamp: u64,
        vector_clock: crate::vector_clock::VectorClock,
    ) -> Self {
        Self {
            coord,
            material,
            author,
            timestamp,
            vector_clock,
            signature: [0u8; 64],
        }
    }
    
    /// Sign this operation with the author's signing key
    ///
    /// Creates deterministic signature over serialized operation data.
    pub fn sign(&mut self, signing_key: &impl Signer<Signature>) {
        let msg = self.signable_bytes();
        let sig = signing_key.sign(&msg);
        self.signature = sig.to_bytes();
    }
    
    /// Verify signature against author's public key
    ///
    /// Returns true if signature is valid, false otherwise.
    pub fn verify(&self, verifying_key: &VerifyingKey) -> bool {
        let msg = self.signable_bytes();
        let sig = Signature::from_bytes(&self.signature);
        verifying_key.verify(&msg, &sig).is_ok()
    }
    
    /// Get bytes to sign/verify (deterministic serialization)
    fn signable_bytes(&self) -> Vec<u8> {
        // Serialize everything except the signature itself
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.coord.x.to_le_bytes());
        bytes.extend_from_slice(&self.coord.y.to_le_bytes());
        bytes.extend_from_slice(&self.coord.z.to_le_bytes());
        bytes.push(self.material as u8);
        bytes.extend_from_slice(&self.author.to_bytes());
        bytes.extend_from_slice(&self.timestamp.to_le_bytes());
        
        // Include vector clock in signature
        let vc_bytes = bincode::serialize(&self.vector_clock).unwrap_or_default();
        bytes.extend_from_slice(&vc_bytes);
        
        bytes
    }
    
    /// Verify signature using author's public key (derived from PeerId)
    ///
    /// Returns true if signature is valid for this operation.
    pub fn verify_signature(&self) -> bool {
        // Extract public key from PeerId (libp2p PeerId contains the public key)
        // For Ed25519, the PeerId is derived from the public key
        // This requires access to the public key, which we should store
        
        // TODO: Store public keys separately or extract from PeerId
        // For now, this is a placeholder that always returns true
        // when signature checking is disabled in UserContentLayer
        
        // Once we have the public key:
        // let verifying_key = VerifyingKey::from_bytes(&public_key_bytes)?;
        // self.verify(&verifying_key)
        
        true // Placeholder - actual verification happens in multiplayer.rs
    }
    
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    
    /// Deserialize from bytes received from network
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
    
    /// Compare two operations for CRDT merge (with vector clocks)
    ///
    /// Returns true if this operation should win over `other` in a conflict.
    ///
    /// **Merge logic:**
    /// 1. If vector clocks show causal ordering (A→B), later one wins
    /// 2. If concurrent, use Lamport timestamp (LWW)
    /// 3. If timestamps equal, use PeerId tiebreak (deterministic)
    pub fn wins_over(&self, other: &VoxelOperation) -> bool {
        // Check for causal ordering first
        if self.vector_clock.happens_after(&other.vector_clock) {
            return true;  // Self causally after other → self wins
        }
        if self.vector_clock.happens_before(&other.vector_clock) {
            return false; // Self causally before other → other wins
        }
        
        // Operations are concurrent → use timestamp + PeerId tiebreak
        if self.timestamp != other.timestamp {
            self.timestamp > other.timestamp
        } else {
            // Tie-break: lexicographically larger PeerId wins (deterministic)
            self.author.to_bytes() > other.author.to_bytes()
        }
    }

    /// Total ordering for deterministic replay.
    ///
    /// Used when sending ops to a peer — sort oldest-first so the receiver can
    /// apply them in causal order and converge to the correct final state.
    ///
    /// Order: causal predecessor first → lower timestamp → smaller PeerId.
    /// This is the inverse of `wins_over` (weakest op first).
    pub fn replay_cmp(&self, other: &VoxelOperation) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        // Causal: happens-before → apply first
        if self.vector_clock.happens_before(&other.vector_clock) {
            return Ordering::Less;
        }
        if self.vector_clock.happens_after(&other.vector_clock) {
            return Ordering::Greater;
        }
        // Concurrent: lower timestamp first (older write replayed first, newer wins)
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Equal => self.author.to_bytes().cmp(&other.author.to_bytes()),
            ord => ord,
        }
    }
}

/// Chat message (Priority 2: State Sync)
///
/// Text messages between players. Variable length but capped at 500 chars.
///
/// **Bandwidth:** 20-500 bytes per message
/// - Text content: 1-500 bytes (UTF-8)
/// - PeerId: ~38 bytes
/// - Timestamp: 8 bytes
/// - Overhead: ~5 bytes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Author of the message
    pub author: PeerId,
    
    /// Message text (max 500 characters)
    pub text: String,
    
    /// Lamport timestamp
    pub timestamp: u64,
}

impl ChatMessage {
    /// Create a new chat message
    ///
    /// Text is truncated to 500 characters if longer.
    pub fn new(author: PeerId, text: String, timestamp: u64) -> Self {
        let text = if text.len() > 500 {
            text.chars().take(500).collect()
        } else {
            text
        };
        Self { author, text, timestamp }
    }
    
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    
    /// Deserialize from bytes received from network
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
}

/// Request historical chunk state from peer (Priority 2: State Sync)
///
/// When a player joins the network, they request historical voxel operations
/// for chunks they have loaded. This enables complete world state synchronization
/// even when joining after other players have made modifications.
///
/// # Planet-Scale Design
///
/// - **Chunk-Based**: Only request chunks actually loaded (natural bandwidth limit)
/// - **Vector Clock Filtering**: Responder filters out operations requester already has
/// - **Spatial Sharding**: Operations grouped by chunk for efficient filtering
/// - **Incremental**: Request new chunks as player explores
///
/// # Bandwidth
///
/// Typical: ~50 KB for spawn area  
/// Worst case: ~304 KB (19 chunks × 100 ops × 160 bytes)  
/// Fits within Priority 2 budget (1-5 KB/s sustained)
///
/// # Example
///
/// ```rust
/// // Bob joins network and loads chunks
/// let loaded_chunk_ids = chunk_manager.get_loaded_chunk_ids();
///
/// // Request state from all connected peers
/// let request = ChunkStateRequest {
///     chunk_ids: loaded_chunk_ids,
///     requester_clock: bob_vector_clock.clone(),
/// };
/// multiplayer.send_state_request(request)?;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkStateRequest {
    /// Only request chunks that are currently loaded. This provides natural
    /// bandwidth limiting and prevents requesting data for the entire planet.
    pub chunk_ids: Vec<ChunkId>,
    
    /// Requester's current vector clock
    ///
    /// Used by responder to filter out operations the requester already has.
    /// If requester's clock shows they've seen an operation, don't send it.
    /// This prevents bandwidth waste and duplicate application.
    pub requester_clock: VectorClock,
}

impl ChunkStateRequest {
    /// Create a new chunk state request
    pub fn new(chunk_ids: Vec<ChunkId>, requester_clock: VectorClock) -> Self {
        Self {
            chunk_ids,
            requester_clock,
        }
    }
    
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    
    /// Deserialize from bytes received from network
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
}

/// Response with historical chunk operations (Priority 2: State Sync)
///
/// Contains voxel operations for requested chunks, filtered by vector clock
/// to avoid sending operations the requester already has. Operations are
/// grouped by chunk for efficient application.
///
/// # CRDT Semantics
///
/// - **Causally Ordered**: Vector clocks ensure causal relationships preserved
/// - **Idempotent**: Safe to receive same operation multiple times (deduplication)
/// - **Commutative**: Operations can be applied in any order
/// - **Convergent**: All peers converge to same state
///
/// # Example
///
/// ```rust
/// // Alice receives request from Bob
/// let alice_ops = filter_operations_for_chunks(
///     &alice_op_log,
///     &request.chunk_ids,
///     &request.requester_clock
/// );
///
/// let response = ChunkStateResponse {
///     operations: alice_ops,
///     responder_clock: alice_vector_clock.clone(),
/// };
/// multiplayer.send_state_response(bob_peer_id, response)?;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkStateResponse {
    /// Operations grouped by chunk
    ///
    /// Each chunk ID maps to a list of operations that modify that chunk.
    /// Operations are filtered to only include those the requester doesn't have
    /// (based on vector clock comparison).
    pub operations: HashMap<ChunkId, Vec<VoxelOperation>>,
    
    /// Responder's current vector clock
    ///
    /// Requester merges this with their own clock to track causality.
    /// Ensures proper CRDT semantics and future operation filtering.
    pub responder_clock: VectorClock,
    
    /// Chunk index (0-based) if this is a multi-part response
    pub chunk_index: u32,
    
    /// Total number of chunks in this response set
    pub total_chunks: u32,
    
    /// Unique ID for this response set (multiple chunks share same ID)
    pub response_id: u64,
}

impl ChunkStateResponse {
    /// Create a new chunk state response
    pub fn new(operations: HashMap<ChunkId, Vec<VoxelOperation>>, responder_clock: VectorClock) -> Self {
        Self {
            operations,
            responder_clock,
            chunk_index: 0,
            total_chunks: 1,
            response_id: 0,
        }
    }
    
    /// Create a chunked response (part of multi-message set)
    pub fn new_chunked(
        operations: HashMap<ChunkId, Vec<VoxelOperation>>,
        responder_clock: VectorClock,
        chunk_index: u32,
        total_chunks: u32,
        response_id: u64,
    ) -> Self {
        Self {
            operations,
            responder_clock,
            chunk_index,
            total_chunks,
            response_id,
        }
    }
    
    /// Check if this is part of a multi-chunk response
    pub fn is_chunked(&self) -> bool {
        self.total_chunks > 1
    }
    
    /// Check if this is the final chunk in a set
    pub fn is_final_chunk(&self) -> bool {
        self.chunk_index + 1 == self.total_chunks
    }
    
    /// Count total operations in response
    pub fn operation_count(&self) -> usize {
        self.operations.values().map(|ops| ops.len()).sum()
    }
    
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    
    /// Deserialize from bytes received from network
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
}

/// Full chunk terrain broadcast — sent when a peer has a newer version of a chunk.
/// Receiver applies if received last_modified > their own last_modified for that chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkTerrainData {
    pub chunk_id: ChunkId,
    /// bincode-serialized Octree bytes (from Octree::to_bytes())
    pub octree_bytes: Vec<u8>,
    /// Unix timestamp (secs) when this chunk was last modified — newer wins
    pub last_modified: u64,
}

impl ChunkTerrainData {
    /// Serialize with zstd compression.
    /// Format: [1 byte version=1][zstd-compressed bincode]
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let raw = bincode::serialize(self)?;
        let compressed = zstd::encode_all(raw.as_slice(), 3)
            .map_err(|e| MessageError::Serialization(e.to_string()))?;
        let mut out = Vec::with_capacity(1 + compressed.len());
        out.push(1u8); // version byte
        out.extend_from_slice(&compressed);
        Ok(out)
    }

    /// Deserialize — handles both compressed (v1) and legacy uncompressed data.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.first() == Some(&1u8) {
            // v1: zstd compressed
            let decompressed = zstd::decode_all(&data[1..])
                .map_err(|e| MessageError::Serialization(e.to_string()))?;
            Ok(bincode::deserialize(&decompressed)?)
        } else {
            // legacy: raw bincode
            Ok(bincode::deserialize(data)?)
        }
    }
}

/// Chunk manifest — sent on peer connect so each side knows what the other has.
/// Each entry is (chunk_id, last_modified). After comparing, each side sends
/// chunks where their last_modified is strictly newer than the peer's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub entries: Vec<(ChunkId, u64)>,
}

impl ChunkManifest {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
}

/// Envelope for all message types
///
/// Wraps specific message types with metadata for routing and processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Player state update
    PlayerState(PlayerStateMessage),
    
    /// Voxel modification operation
    VoxelOp(VoxelOperation),
    
    /// Chat message
    Chat(ChatMessage),
    
    /// Request historical chunk state from peer
    ChunkStateRequest(ChunkStateRequest),
    
    /// Response with historical chunk operations
    ChunkStateResponse(ChunkStateResponse),
}

impl Message {
    /// Serialize to bytes for network transmission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
    
    /// Deserialize from bytes received from network
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        Ok(bincode::deserialize(data)?)
    }
    
    /// Get Lamport timestamp from any message type
    pub fn timestamp(&self) -> u64 {
        match self {
            Message::PlayerState(msg) => msg.timestamp,
            Message::VoxelOp(msg) => msg.timestamp,
            Message::Chat(msg) => msg.timestamp,
            Message::ChunkStateRequest(_) => 0, // State messages don't use Lamport clocks
            Message::ChunkStateResponse(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;
    
    #[test]
    fn test_lamport_clock() {
        let mut clock = LamportClock::new();
        assert_eq!(clock.current(), 0);
        
        let t1 = clock.tick();
        assert_eq!(t1, 1);
        assert_eq!(clock.current(), 1);
        
        clock.receive(5);
        assert_eq!(clock.current(), 6);
        
        let t2 = clock.tick();
        assert_eq!(t2, 7);
    }
    
    #[test]
    fn test_player_state_serialization() {
        let msg = PlayerStateMessage {
            peer_id: PeerId::random(),
            position: ECEF::new(0.0, 0.0, 0.0),
            velocity: [1.0, 2.0, 3.0],
            yaw: 0.5,
            pitch: 0.25,
            movement_mode: MovementMode::Walk,
            timestamp: 42,
        };
        
        let bytes = msg.to_bytes().unwrap();
        let decoded = PlayerStateMessage::from_bytes(&bytes).unwrap();
        
        assert_eq!(decoded.velocity, msg.velocity);
        assert_eq!(decoded.timestamp, msg.timestamp);
    }
    
    #[test]
    fn test_voxel_operation_signature() {
        let identity = Identity::generate();
        let coord = VoxelCoord::new(10, 20, 30);
        let vc = crate::vector_clock::VectorClock::new();
        
        let mut op = VoxelOperation::new(
            coord,
            Material::Stone,
            identity.peer_id().clone(),
            100,
            vc,
        );
        
        // Sign operation
        op.sign(identity.signing_key());
        
        // Verify with correct key
        assert!(op.verify(identity.verifying_key()));
        
        // Verify fails with different key
        let other_identity = Identity::generate();
        assert!(!op.verify(other_identity.verifying_key()));
    }
    
    #[test]
    fn test_voxel_operation_crdt_ordering() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        let coord = VoxelCoord::new(5, 5, 5);
        let vc = crate::vector_clock::VectorClock::new();
        
        // Later timestamp wins
        let op1 = VoxelOperation::new(coord, Material::Stone, id1.peer_id().clone(), 100, vc.clone());
        let op2 = VoxelOperation::new(coord, Material::Dirt, id2.peer_id().clone(), 101, vc.clone());
        assert!(op2.wins_over(&op1));
        assert!(!op1.wins_over(&op2));
        
        // Same timestamp: tie-break by PeerId
        let op3 = VoxelOperation::new(coord, Material::Stone, id1.peer_id().clone(), 100, vc.clone());
        let op4 = VoxelOperation::new(coord, Material::Dirt, id2.peer_id().clone(), 100, vc.clone());
        let winner = if op3.wins_over(&op4) { &op3 } else { &op4 };
        // Deterministic winner based on PeerId ordering
        assert!(winner.author.to_bytes() > if winner.author == op3.author { op4.author } else { op3.author }.to_bytes());
    }
    
    #[test]
    fn test_chat_message_truncation() {
        let long_text = "a".repeat(1000);
        let msg = ChatMessage::new(PeerId::random(), long_text, 42);
        assert_eq!(msg.text.len(), 500);
    }
    
    #[test]
    fn test_message_envelope() {
        let player_msg = PlayerStateMessage {
            peer_id: PeerId::random(),
            position: ECEF::new(1.0, 2.0, 3.0),
            velocity: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            movement_mode: MovementMode::Fly,
            timestamp: 123,
        };
        
        let envelope = Message::PlayerState(player_msg);
        assert_eq!(envelope.timestamp(), 123);
        
        let bytes = envelope.to_bytes().unwrap();
        let decoded = Message::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.timestamp(), 123);
    }
}
