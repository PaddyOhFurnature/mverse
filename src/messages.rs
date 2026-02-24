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

/// Compact player-state message for bandwidth-constrained links (LoRa, dialup, etc).
///
/// After session ID assignment, the server assigns each peer a `u16` token.
/// Hot-path position/rotation updates use this struct instead of the full
/// [`PlayerStateMessage`], saving ~37 bytes per packet (critical for 200-byte LoRa frames).
///
/// # Wire size
/// - `session_id`:  2 bytes
/// - `position`:   12 bytes (3 × f32)
/// - `rotation`:    8 bytes (2 × f32)
/// - `timestamp_ms`: 4 bytes (u32, wraps every ~49 days — sufficient for ordering)
/// **Total: 26 bytes** (vs ~64 bytes for full `PlayerStateMessage`).
///
/// # Upgrade path
/// Clients that don't know the session→peer mapping ignore this message.
/// Servers that know the mapping translate back to `PlayerState` events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CompactPlayerState {
    /// Server-assigned 2-byte session token (non-zero).
    pub session_id: u16,
    /// Position in ECEF coordinates — f32 precision (~1m at Earth scale).
    /// Sufficient for game-world movement; use full `PlayerStateMessage` for
    /// high-precision applications.
    pub position: [f32; 3],
    /// Yaw and pitch in radians.
    pub rotation: [f32; 2],
    /// Unix timestamp modulo 2^32 ms (wraps every ~49 days).
    pub timestamp_ms: u32,
}

impl CompactPlayerState {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }
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
/// Legacy operation type. Use [`SignedOperation`] with [`Action::SetVoxel`] for new code.
// Note: being migrated to SignedOperation
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
    pub operations: HashMap<ChunkId, Vec<SignedOperation>>,
    
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
    pub fn new(operations: HashMap<ChunkId, Vec<SignedOperation>>, responder_clock: VectorClock) -> Self {
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
        operations: HashMap<ChunkId, Vec<SignedOperation>>,
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
    
    /// Voxel modification operation (legacy — use SignedOp for new code)
    VoxelOp(VoxelOperation),
    
    /// Signed operation (all action types)
    SignedOp(SignedOperation),
    
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
            Message::SignedOp(op) => op.lamport,
            Message::Chat(msg) => msg.timestamp,
            Message::ChunkStateRequest(_) => 0, // State messages don't use Lamport clocks
            Message::ChunkStateResponse(_) => 0,
        }
    }
}

/// Every mutable operation in the metaverse is one of these action types.
///
/// The action is the payload of a [`SignedOperation`] — it describes what changed.
/// All actions are signed by the author's Ed25519 private key.
///
/// # Design
///
/// - `SetVoxel` / `RemoveVoxel` cover all terrain editing (legacy `VoxelOperation` maps here).
/// - `FillRegion` is a bulk SetVoxel capped at 32×32×32 (32768 voxels max) to prevent abuse.
/// - Object actions use a content-addressed `object_id` = first 32 bytes of the creation op's signature.
/// - Parcel actions use `min`/`max` VoxelCoord to identify the parcel bounds.
/// - Commerce and identity actions carry their own IDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    // ── Terrain editing ───────────────────────────────────────────────────────
    /// Set a voxel to a specific material.
    SetVoxel { coord: VoxelCoord, material: Material },
    /// Remove a voxel (equivalent to SetVoxel { material: Air }).
    RemoveVoxel { coord: VoxelCoord },
    /// Fill a cuboid region with a material.
    /// Max region: 32×32×32 = 32 768 voxels. Validated on apply.
    FillRegion { min: VoxelCoord, max: VoxelCoord, material: Material },

    // ── Object placement ──────────────────────────────────────────────────────
    /// Place a world object. `object_type` is a registry ID (u32).
    /// `orientation` is a normalised quaternion [x, y, z, w].
    PlaceObject { position: VoxelCoord, object_type: u32, orientation: [f32; 4] },
    /// Remove an existing object by its content-addressed ID.
    RemoveObject { object_id: [u8; 32] },
    /// Move or rotate an existing object.
    MoveObject { object_id: [u8; 32], new_position: VoxelCoord, orientation: [f32; 4] },
    /// Update an object's JSON configuration (max 4096 bytes enforced on apply).
    ConfigureObject { object_id: [u8; 32], config_json: String },

    // ── Parcel management ─────────────────────────────────────────────────────
    /// Claim an unclaimed rectangular parcel as owned land.
    ClaimParcel { min: VoxelCoord, max: VoxelCoord },
    /// Release ownership of a parcel.
    AbandonParcel { min: VoxelCoord, max: VoxelCoord },
    /// Transfer parcel ownership to another peer.
    TransferOwnership { min: VoxelCoord, max: VoxelCoord, new_owner: PeerId },
    /// Grant build access to another peer for the duration given (None = permanent).
    GrantAccess { min: VoxelCoord, max: VoxelCoord, grantee: PeerId, expires_at: Option<u64> },
    /// Revoke previously granted access.
    RevokeAccess { min: VoxelCoord, max: VoxelCoord, grantee: PeerId },

    // ── Commerce ──────────────────────────────────────────────────────────────
    /// Create a listing to sell an item or service.
    /// `item_id` is the object_id being listed; `price_microcredits` avoids floats.
    CreateListing { item_id: [u8; 32], price_microcredits: u64, description: String },
    /// Accept an existing listing (purchase). Initiates the exchange protocol.
    AcceptListing { listing_id: [u8; 32] },
    /// Cancel your own listing.
    CancelListing { listing_id: [u8; 32] },
    /// Sign a trade or service contract. Both parties must sign the same `contract_id`.
    SignContract { contract_id: [u8; 32], terms_hash: [u8; 32] },

    // ── Content creation ──────────────────────────────────────────────────────
    /// Publish a voxel blueprint or model. `data_hash` is the DHT key for the actual data.
    PublishBlueprint { blueprint_id: [u8; 32], name: String, data_hash: [u8; 32] },
    /// Import an external asset. `source_uri` is a URI; `data_hash` is its content hash.
    ImportAsset { asset_id: [u8; 32], source_uri: String, data_hash: [u8; 32] },

    // ── Identity ──────────────────────────────────────────────────────────────
    /// Publish or update a KeyRecord on the network (raw bincode-serialized bytes).
    /// Using raw bytes avoids a circular import with the identity module.
    PublishKeyRecord { record_bytes: Vec<u8> },
    /// Revoke a key (self-revoke or authority-revoke).
    RevokeKey { target_peer: PeerId, reason: Option<String> },

    // ── Infrastructure ────────────────────────────────────────────────────────
    /// Register or update a relay node configuration.
    RegisterRelay { relay_addr: String, capabilities: Vec<String> },
}

impl Action {
    /// Human-readable name of this action type, for logging.
    pub fn name(&self) -> &'static str {
        match self {
            Self::SetVoxel { .. }          => "SetVoxel",
            Self::RemoveVoxel { .. }       => "RemoveVoxel",
            Self::FillRegion { .. }        => "FillRegion",
            Self::PlaceObject { .. }       => "PlaceObject",
            Self::RemoveObject { .. }      => "RemoveObject",
            Self::MoveObject { .. }        => "MoveObject",
            Self::ConfigureObject { .. }   => "ConfigureObject",
            Self::ClaimParcel { .. }       => "ClaimParcel",
            Self::AbandonParcel { .. }     => "AbandonParcel",
            Self::TransferOwnership { .. } => "TransferOwnership",
            Self::GrantAccess { .. }       => "GrantAccess",
            Self::RevokeAccess { .. }      => "RevokeAccess",
            Self::CreateListing { .. }     => "CreateListing",
            Self::AcceptListing { .. }     => "AcceptListing",
            Self::CancelListing { .. }     => "CancelListing",
            Self::SignContract { .. }      => "SignContract",
            Self::PublishBlueprint { .. }  => "PublishBlueprint",
            Self::ImportAsset { .. }       => "ImportAsset",
            Self::PublishKeyRecord { .. }  => "PublishKeyRecord",
            Self::RevokeKey { .. }         => "RevokeKey",
            Self::RegisterRelay { .. }     => "RegisterRelay",
        }
    }

    /// True if this action modifies the terrain (voxel octree).
    pub fn is_terrain(&self) -> bool {
        matches!(self, Self::SetVoxel { .. } | Self::RemoveVoxel { .. } | Self::FillRegion { .. })
    }
}

/// The authoritative operation type for the metaverse.
///
/// Every mutable action — placing a voxel, claiming a parcel, trading an item,
/// revoking a key — is wrapped in a `SignedOperation`. The Ed25519 signature
/// proves the action came from the holder of `public_key`, whose corresponding
/// `author` PeerId is deterministically derived from that key.
///
/// # CRDT semantics
///
/// For terrain operations, `SignedOperation` implements Last-Write-Wins (LWW)
/// with deterministic conflict resolution:
/// 1. Vector clock causal ordering (causally later wins).
/// 2. Lamport timestamp (higher wins on concurrent ops).
/// 3. PeerId tiebreak (lexicographically larger wins on equal timestamp).
///
/// # Wire format
///
/// Serialized with bincode. The `signature` covers `signable_bytes()`.
/// The `op_id()` is derived from the signature (no extra storage needed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedOperation {
    /// Schema version. Current: 1. Increment when signable_bytes() format changes.
    pub version: u8,

    /// The action being performed.
    pub action: Action,

    /// Lamport timestamp — provides a total order for concurrent ops.
    pub lamport: u64,

    /// Vector clock for causal ordering.
    pub vector_clock: VectorClock,

    /// Unix timestamp (seconds) when this op was created.
    pub created_at: u64,

    /// Author's PeerId (derived from public_key).
    pub author: PeerId,

    /// Author's Ed25519 public key (32 bytes).
    /// Stored explicitly so peers can verify the signature without a KeyRegistry lookup.
    pub public_key: [u8; 32],

    /// Ed25519 signature over `signable_bytes()`.
    /// Zero-filled for unsigned ops (test use only — never broadcast unsigned).
    #[serde(with = "serde_arrays")]
    pub signature: [u8; 64],
}

impl SignedOperation {
    /// Construct an unsigned `SignedOperation`.
    ///
    /// Call `sign()` before broadcasting. The `created_at` timestamp is set
    /// to the current Unix time automatically.
    pub fn new(action: Action, lamport: u64, vector_clock: VectorClock, author: PeerId, public_key: [u8; 32]) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            version: 1,
            action,
            lamport,
            vector_clock,
            created_at,
            author,
            public_key,
            signature: [0u8; 64],
        }
    }

    /// Sign this operation using the author's Ed25519 signing key.
    pub fn sign(&mut self, signing_key: &impl Signer<Signature>) {
        let msg = self.signable_bytes();
        self.signature = signing_key.sign(&msg).to_bytes();
    }

    /// Verify the signature against the stored `public_key`.
    ///
    /// Returns `false` if the public key is invalid or the signature doesn't match.
    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.public_key) else { return false; };
        let sig = Signature::from_bytes(&self.signature);
        let msg = self.signable_bytes();
        vk.verify(&msg, &sig).is_ok()
    }

    /// Canonical bytes signed / verified.
    ///
    /// Covers all fields except `signature`. Field order is fixed — changing
    /// it requires bumping `version` and adding a migration path.
    /// Canonical bytes that are signed/verified.
    ///
    /// Covers all fields except `signature`. Field order is fixed — changing
    /// it requires bumping `version` and adding a migration path.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(self.version);
        // Action: length-prefixed bincode
        let action_bytes = bincode::serialize(&self.action).unwrap_or_default();
        out.extend_from_slice(&(action_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&action_bytes);
        // Lamport
        out.extend_from_slice(&self.lamport.to_le_bytes());
        // Vector clock: length-prefixed bincode
        let vc_bytes = bincode::serialize(&self.vector_clock).unwrap_or_default();
        out.extend_from_slice(&(vc_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&vc_bytes);
        // created_at
        out.extend_from_slice(&self.created_at.to_le_bytes());
        // author
        out.extend_from_slice(&self.author.to_bytes());
        // public_key
        out.extend_from_slice(&self.public_key);
        out
    }

    // ── Serialization ──────────────────────────────────────────────────────────

    /// Serialize to bytes for network transmission.
    pub fn to_bytes(&self) -> Result<Vec<u8>> { Ok(bincode::serialize(self)?) }

    /// Deserialize from bytes received over the network.
    pub fn from_bytes(data: &[u8]) -> Result<Self> { Ok(bincode::deserialize(data)?) }

    // ── Identity ───────────────────────────────────────────────────────────────

    /// Content-addressed operation ID: first 32 bytes of the signature.
    ///
    /// Unique per valid signed operation (no two valid signatures are the same).
    /// Used as the deduplication key across the network.
    pub fn op_id(&self) -> [u8; 32] {
        let mut id = [0u8; 32];
        id.copy_from_slice(&self.signature[..32]);
        id
    }

    // ── Terrain helpers ────────────────────────────────────────────────────────

    /// If this is a terrain operation, return the affected `(VoxelCoord, Material)`.
    ///
    /// Returns `None` for non-terrain actions.
    pub fn as_set_voxel(&self) -> Option<(VoxelCoord, Material)> {
        match &self.action {
            Action::SetVoxel { coord, material } => Some((*coord, *material)),
            Action::RemoveVoxel { coord }         => Some((*coord, Material::Air)),
            _ => None,
        }
    }

    /// Return the primary VoxelCoord for terrain ops. None for non-terrain actions.
    pub fn coord(&self) -> Option<VoxelCoord> {
        self.as_set_voxel().map(|(c, _)| c)
    }

    /// Return the material for terrain ops. None for non-terrain actions.
    pub fn material(&self) -> Option<Material> {
        self.as_set_voxel().map(|(_, m)| m)
    }

    /// Return all ChunkIds affected by this operation.
    ///
    /// Terrain ops may touch neighbouring chunks (mesh stitching).
    /// Non-terrain ops return an empty vec.
    pub fn affecting_chunks(&self) -> Vec<crate::chunk::ChunkId> {
        match &self.action {
            Action::SetVoxel { coord, .. } | Action::RemoveVoxel { coord } => {
                crate::chunk::ChunkId::affected_by_voxel(coord)
            }
            Action::FillRegion { min, max, .. } => {
                let min_c = crate::chunk::ChunkId::from_voxel(min);
                let max_c = crate::chunk::ChunkId::from_voxel(max);
                let mut chunks = Vec::new();
                for cx in min_c.x..=max_c.x {
                    for cy in min_c.y..=max_c.y {
                        for cz in min_c.z..=max_c.z {
                            chunks.push(crate::chunk::ChunkId::new(cx, cy, cz));
                        }
                    }
                }
                chunks
            }
            _ => vec![],
        }
    }

    // ── CRDT ordering ──────────────────────────────────────────────────────────

    /// Returns `true` if this operation should win over `other` in a CRDT conflict.
    ///
    /// Resolution order:
    /// 1. Vector clock causal ordering (causally later wins).
    /// 2. Lamport timestamp (higher wins when concurrent).
    /// 3. PeerId tiebreak (lexicographically larger PeerId wins on equal timestamp).
    pub fn wins_over(&self, other: &SignedOperation) -> bool {
        if self.vector_clock.happens_after(&other.vector_clock) { return true; }
        if self.vector_clock.happens_before(&other.vector_clock) { return false; }
        if self.lamport != other.lamport { return self.lamport > other.lamport; }
        self.author.to_bytes() > other.author.to_bytes()
    }

    /// Total ordering for deterministic replay (oldest-first causally).
    ///
    /// Inverse of `wins_over`: weakest op comes first so the receiver
    /// converges to the correct final state when applying sequentially.
    pub fn replay_cmp(&self, other: &SignedOperation) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        if self.vector_clock.happens_before(&other.vector_clock) { return Ordering::Less; }
        if self.vector_clock.happens_after(&other.vector_clock) { return Ordering::Greater; }
        match self.lamport.cmp(&other.lamport) {
            Ordering::Equal => self.author.to_bytes().cmp(&other.author.to_bytes()),
            ord => ord,
        }
    }
}

/// Convert a legacy `VoxelOperation` to a `SignedOperation`.
///
/// The `public_key` is zeroed (not stored in `VoxelOperation`) so the resulting
/// `SignedOperation` cannot be re-verified — but this is acceptable for migrating
/// ops that were already verified when first applied.
#[allow(deprecated)]
impl From<VoxelOperation> for SignedOperation {
    fn from(op: VoxelOperation) -> Self {
        Self {
            version: 1,
            action: Action::SetVoxel { coord: op.coord, material: op.material },
            lamport: op.timestamp,
            vector_clock: op.vector_clock,
            created_at: 0,
            author: op.author,
            public_key: [0u8; 32],
            signature: op.signature,
        }
    }
}

// ─── PlayerSessionRecord ──────────────────────────────────────────────────────

/// Portable session state — signed by the player's key and published to the DHT
/// on clean logout. Fetched on startup when no local save exists (e.g., new
/// machine with key on thumbdrive). Ensures the player resumes from their exact
/// last position regardless of which machine they log in from.
///
/// DHT key: SHA-256 of b"session:" + peer_id.to_bytes()
/// TTL: 90 days (refreshed on every clean logout).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerSessionRecord {
    /// Protocol version for forward compatibility.
    pub version: u8,

    /// The player this session belongs to.
    pub peer_id: PeerId,

    /// Last known position (ECEF metres). Stored as [x, y, z] f64.
    pub position: [f64; 3],

    /// Yaw and pitch in radians at logout.
    pub rotation: [f32; 2],

    /// Movement mode at logout (Walk / Fly / etc.)
    pub movement_mode: u8,

    /// Chunk the player was in — used to request terrain sync on re-login.
    pub chunk_id: [i64; 3],

    /// Unix milliseconds of the logout timestamp.
    pub logged_out_at: u64,

    /// Ed25519 public key bytes matching `peer_id`.
    pub public_key: [u8; 32],

    /// Ed25519 signature over all fields above (excluding this field).
    /// Signs the canonical bytes produced by `signable_bytes()`.
    #[serde(with = "serde_arrays")]
    pub signature: [u8; 64],
}

impl PlayerSessionRecord {
    /// Compute the canonical bytes that are signed (everything except signature).
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        out.push(self.version);
        out.extend_from_slice(&self.peer_id.to_bytes());
        for f in &self.position  { out.extend_from_slice(&f.to_le_bytes()); }
        for f in &self.rotation  { out.extend_from_slice(&f.to_le_bytes()); }
        out.push(self.movement_mode);
        for i in &self.chunk_id  { out.extend_from_slice(&i.to_le_bytes()); }
        out.extend_from_slice(&self.logged_out_at.to_le_bytes());
        out.extend_from_slice(&self.public_key);
        out
    }

    /// Serialise the full record to bytes for DHT storage.
    pub fn to_bytes(&self) -> std::result::Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialise from bytes fetched from DHT.
    pub fn from_bytes(data: &[u8]) -> std::result::Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    /// Verify the self-signature using the embedded public key.
    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.public_key) else { return false; };
        let Ok(sig) = Signature::from_slice(&self.signature) else { return false; };
        vk.verify(&self.signable_bytes(), &sig).is_ok()
    }

    /// Compute the DHT key for a peer's session record.
    /// SHA-256 of b"session:" + peer_id_bytes — distinct namespace from KeyRecord DHT keys.
    pub fn dht_key(peer_id: &PeerId) -> Vec<u8> {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(b"session:");
        hasher.update(peer_id.to_bytes());
        hasher.finalize().to_vec()
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

    #[test]
    fn test_signed_operation_sign_verify() {
        let id = crate::identity::Identity::generate();
        let coord = VoxelCoord::new(10, 20, 30);
        let vc = VectorClock::new();
        let mut op = SignedOperation::new(
            Action::SetVoxel { coord, material: Material::Stone },
            1, vc, *id.peer_id(), id.verifying_key().to_bytes(),
        );
        op.sign(id.signing_key());
        assert!(op.verify(), "signed op must verify");
    }

    #[test]
    fn test_signed_operation_tamper_detected() {
        let id = crate::identity::Identity::generate();
        let mut op = SignedOperation::new(
            Action::SetVoxel { coord: VoxelCoord::new(1,1,1), material: Material::Stone },
            1, VectorClock::new(), *id.peer_id(), id.verifying_key().to_bytes(),
        );
        op.sign(id.signing_key());
        op.action = Action::SetVoxel { coord: VoxelCoord::new(1,1,1), material: Material::Dirt };
        assert!(!op.verify(), "tampered op must fail verification");
    }

    #[test]
    #[allow(deprecated)]
    fn test_signed_operation_from_voxel_op() {
        let id = crate::identity::Identity::generate();
        let coord = VoxelCoord::new(5, 5, 5);
        let vc = VectorClock::new();
        let mut legacy = VoxelOperation::new(coord, Material::Stone, *id.peer_id(), 42, vc);
        legacy.sign(id.signing_key());
        let signed = SignedOperation::from(legacy);
        assert_eq!(signed.coord(), Some(coord));
        assert_eq!(signed.material(), Some(Material::Stone));
        assert_eq!(signed.lamport, 42);
    }

    #[test]
    fn test_signed_operation_crdt_wins_over() {
        let id1 = crate::identity::Identity::generate();
        let id2 = crate::identity::Identity::generate();
        let coord = VoxelCoord::new(7, 7, 7);
        let vc = VectorClock::new();
        let op1 = SignedOperation::new(
            Action::SetVoxel { coord, material: Material::Stone },
            100, vc.clone(), *id1.peer_id(), id1.verifying_key().to_bytes(),
        );
        let op2 = SignedOperation::new(
            Action::SetVoxel { coord, material: Material::Dirt },
            200, vc.clone(), *id2.peer_id(), id2.verifying_key().to_bytes(),
        );
        assert!(op2.wins_over(&op1), "higher lamport wins");
        assert!(!op1.wins_over(&op2));
    }

    #[test]
    fn test_signed_operation_affecting_chunks() {
        let op = SignedOperation::new(
            Action::SetVoxel { coord: VoxelCoord::new(0, 0, 0), material: Material::Stone },
            1, VectorClock::new(), PeerId::random(), [0u8; 32],
        );
        let chunks = op.affecting_chunks();
        assert!(!chunks.is_empty(), "SetVoxel must affect at least one chunk");
    }

    #[test]
    fn test_action_is_terrain() {
        assert!(Action::SetVoxel { coord: VoxelCoord::new(0,0,0), material: Material::Air }.is_terrain());
        assert!(Action::RemoveVoxel { coord: VoxelCoord::new(0,0,0) }.is_terrain());
        assert!(!Action::ClaimParcel { min: VoxelCoord::new(0,0,0), max: VoxelCoord::new(10,10,10) }.is_terrain());
        assert!(!Action::RevokeKey { target_peer: PeerId::random(), reason: None }.is_terrain());
    }
}
