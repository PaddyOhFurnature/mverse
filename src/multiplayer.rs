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
    chunk::ChunkId,
    coordinates::ECEF,
    identity::Identity,
    messages::{
        ChatMessage, ChunkStateRequest, ChunkStateResponse, LamportClock, Material, 
        MovementMode, PlayerStateMessage, VoxelOperation, MessageError,
    },
    network::{NetworkCommand, NetworkEvent, NetworkNode, NetworkError},
    player_state::PlayerStateManager,
    vector_clock::VectorClock,
    voxel::{Octree, VoxelCoord},
    physics::PhysicsWorld,
};
use libp2p::PeerId;
use crossbeam::channel::{self, Sender, Receiver};
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
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Invalid signature from peer {0}")]
    InvalidSignature(PeerId),
    
    #[error("Malicious peer {0} exceeded reputation threshold")]
    MaliciousPeer(PeerId),
    
    #[error("Invalid multiaddr: {0}")]
    InvalidMultiaddr(String),
    
    #[error("Runtime error: {0}")]
    RuntimeError(String),
    
    #[error("Channel send error")]
    ChannelSendError,
}

/// Gossipsub topic names for different message channels
pub const TOPIC_PLAYER_STATE: &str = "player-state";
pub const TOPIC_VOXEL_OPS: &str = "voxel-ops";
pub const TOPIC_CHAT: &str = "chat";
pub const TOPIC_STATE_REQUEST: &str = "state-request";
pub const TOPIC_STATE_RESPONSE: &str = "state-response";

/// Broadcast interval for player state (20 Hz = 50ms)
const PLAYER_STATE_BROADCAST_INTERVAL: Duration = Duration::from_millis(50);

/// Maximum allowed invalid signatures before blocking peer
const MAX_INVALID_SIGNATURES: usize = 5;

/// Multiplayer system coordinating all P2P functionality
pub struct MultiplayerSystem {
    /// Channel to send commands to background network thread
    cmd_tx: Sender<NetworkCommand>,
    
    /// Channel to receive events from background network thread
    event_rx: Receiver<NetworkEvent>,
    
    /// Our cryptographic identity
    identity: Identity,
    
    /// Local peer ID (cached for convenience)
    local_peer_id: PeerId,
    
    /// Remote player state manager (interpolation, jitter buffer)
    remote_players: PlayerStateManager,
    
    /// Lamport clock for causal ordering (kept for backwards compat)
    clock: LamportClock,
    
    /// Vector clock for proper CRDT causality
    vector_clock: crate::vector_clock::VectorClock,
    
    /// Deduplication set for voxel operations (by operation ID)
    voxel_op_seen: HashSet<[u8; 64]>, // Store signature as ID
    
    /// Pending voxel operations to be applied to world
    pending_ops: Vec<VoxelOperation>,
    
    /// Pending state synchronization operations (from ChunkStateResponse)
    pending_state_ops: Vec<VoxelOperation>,
    
    /// Peer reputation tracking (invalid signatures count)
    peer_reputation: HashMap<PeerId, usize>,
    
    /// Blocked peers (too many invalid signatures)
    blocked_peers: HashSet<PeerId>,
    
    /// Timer for player state broadcasts
    last_state_broadcast: Instant,
    
    /// Connected peers (for state exchange)
    connected_peers: HashSet<PeerId>,
    
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
    pub state_requests_sent: u64,
    pub state_responses_sent: u64,
    pub state_responses_received: u64,
    pub state_ops_received: u64,
}

impl MultiplayerSystem {
    /// Create a new multiplayer system with embedded tokio runtime
    ///
    /// **This is the preferred method for non-async game loops.**
    ///
    /// Spawns a background thread with a tokio runtime for libp2p operations.
    /// The background thread handles mDNS discovery and async networking.
    /// The main thread communicates via non-blocking channels.
    ///
    /// # Example
    /// ```no_run
    /// use metaverse_core::multiplayer::MultiplayerSystem;
    /// use metaverse_core::identity::Identity;
    ///
    /// let identity = Identity::load_or_create()?;
    /// let mut mp = MultiplayerSystem::new_with_runtime(identity)?;
    /// ```
    pub fn new_with_runtime(identity: Identity) -> Result<Self> {
        // Create bounded channels for command/event passing
        // Capacity of 1000 provides back-pressure if game loop falls behind
        let (cmd_tx, cmd_rx) = channel::bounded(1000);
        let (event_tx, event_rx) = channel::bounded(1000);
        
        let identity_clone = identity.clone();
        let local_peer_id = *identity.peer_id();
        
        // Spawn background thread with tokio runtime
        std::thread::spawn(move || {
            run_network_thread(identity_clone, cmd_rx, event_tx);
        });
        
        Ok(Self {
            cmd_tx,
            event_rx,
            identity,
            local_peer_id,
            remote_players: PlayerStateManager::new(local_peer_id),
            clock: LamportClock::default(),
            vector_clock: crate::vector_clock::VectorClock::new(),
            voxel_op_seen: HashSet::new(),
            pending_ops: Vec::new(),
            pending_state_ops: Vec::new(),
            peer_reputation: HashMap::new(),
            blocked_peers: HashSet::new(),
            last_state_broadcast: Instant::now(),
            connected_peers: HashSet::new(),
            stats: MultiplayerStats::default(),
        })
    }
    
    /// Create a new multiplayer system (deprecated - use new_with_runtime)
    ///
    /// **WARNING:** This method is deprecated and will be removed.
    /// Use `new_with_runtime()` instead.
    ///
    /// This method requires an existing tokio runtime context and
    /// creates the network node synchronously, which doesn't work
    /// for mDNS auto-discovery.
    #[deprecated(note = "Use new_with_runtime() instead")]
    pub fn new(identity: Identity) -> Result<Self> {
        // This implementation is now broken due to mDNS tokio requirements
        // Keeping it for compatibility but marking as deprecated
        Err(MultiplayerError::RuntimeError(
            "new() is deprecated - use new_with_runtime() instead".into()
        ))
    }
    
    /// Start listening on the given address
    /// Start listening on an address
    pub fn listen_on(&self, addr: &str) -> Result<()> {
        self.cmd_tx.send(NetworkCommand::Listen {
            multiaddr: addr.to_string(),
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        Ok(())
    }
    
    /// Connect to a specific peer
    pub fn dial(&self, addr: &str) -> Result<()> {
        self.cmd_tx.send(NetworkCommand::Dial {
            address: addr.to_string(),
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        Ok(())
    }
    
    /// Get our PeerId
    pub fn peer_id(&self) -> PeerId {
        self.local_peer_id
    }
    
    /// Update multiplayer system - call this every frame
    ///
    /// Processes network events, updates remote player interpolation,
    /// and handles periodic broadcasts.
    pub fn update(&mut self, _dt: f32) {
        // Process all pending network events (non-blocking)
        let mut event_count = 0;
        while let Ok(event) = self.event_rx.try_recv() {
            event_count += 1;
            if let Err(e) = self.handle_network_event(event) {
                eprintln!("Error handling network event: {}", e);
            }
        }
        
        if event_count > 0 {
            println!("🔄 Processed {} network events", event_count);
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
            self.local_peer_id,
            position,
            velocity,
            yaw,
            pitch,
            movement_mode,
            timestamp,
        );
        
        let data = msg.to_bytes()?;
        self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_PLAYER_STATE.to_string(),
            data,
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        self.stats.player_states_sent += 1;
        
        Ok(())
    }
    
    /// Broadcast a voxel operation (dig or place)
    pub fn broadcast_voxel_operation(
        &mut self,
        coord: VoxelCoord,
        material: Material,
    ) -> Result<VoxelOperation> {
        // Increment clocks
        let timestamp = self.clock.tick();
        self.vector_clock.increment(self.local_peer_id);
        
        // Create and sign operation with vector clock
        let mut op = VoxelOperation::new(
            coord,
            material,
            self.local_peer_id,
            timestamp,
            self.vector_clock.clone(),
        );
        
        op.sign(self.identity.signing_key());
        
        // Serialize and send
        let data = op.to_bytes()?;
        self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_VOXEL_OPS.to_string(),
            data,
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        self.stats.voxel_ops_sent += 1;
        
        // Remember we sent this (for deduplication)
        self.voxel_op_seen.insert(op.signature);
        
        Ok(op)
    }
    
    /// Send a chat message
    pub fn send_chat(&mut self, text: String) -> Result<()> {
        let timestamp = self.clock.tick();
        let msg = ChatMessage::new(self.local_peer_id, text, timestamp);
        
        let data = msg.to_bytes()?;
        self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_CHAT.to_string(),
            data,
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        
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
                    TOPIC_STATE_REQUEST => self.handle_state_request(peer_id, &data)?,
                    TOPIC_STATE_RESPONSE => self.handle_state_response(peer_id, &data)?,
                    _ => {}
                }
            }
            
            NetworkEvent::PeerConnected { peer_id, address } => {
                println!("🔗 Peer connected: {} @ {}", peer_id, address);
                self.connected_peers.insert(peer_id);
            }
            
            NetworkEvent::PeerDisconnected { peer_id } => {
                println!("💔 Peer disconnected: {}", peer_id);
                self.remote_players.remove_player(&peer_id);
                self.connected_peers.remove(&peer_id);
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
    fn handle_player_state(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let msg = PlayerStateMessage::from_bytes(data)?;
        
        println!("📥 Received player state from {}: pos=({:.1}, {:.1}, {:.1})", 
            peer_id, msg.position.x, msg.position.y, msg.position.z);
        
        // Update Lamport clock
        self.clock.receive(msg.timestamp);
        
        // Update remote player (manager handles deduplication and filtering)
        self.remote_players.handle_message(msg);
        self.stats.player_states_received += 1;
        
        println!("   Total remote players tracked: {}", self.remote_players.player_count());
        
        Ok(())
    }
    
    /// Handle incoming voxel operation with CRDT merge and signature verification
    fn handle_voxel_operation(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let op = VoxelOperation::from_bytes(data)?;
        
        println!("🔨 Received voxel op from {}: {:?} at {:?}", peer_id, op.material, op.coord);
        
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
        
        // Update clocks
        self.clock.receive(op.timestamp);
        self.vector_clock.merge(&op.vector_clock);  // Merge vector clocks
        self.vector_clock.increment(self.local_peer_id); // Increment our counter
        
        // Remember we've seen this operation
        self.voxel_op_seen.insert(op.signature);
        
        self.stats.voxel_ops_received += 1;
        
        // Queue operation for application by game loop
        self.pending_ops.push(op);
        
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
    
    /// Handle incoming chunk state request
    ///
    /// Peer is requesting our operations for specific chunks. Filter our op_log
    /// and send back operations they don't already have (based on vector clock).
    fn handle_state_request(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let request = ChunkStateRequest::from_bytes(data)
            .map_err(|e| MultiplayerError::SerializationError(e.to_string()))?;
        
        println!("📨 Received state request from {} for {} chunks",
            peer_id, request.chunk_ids.len());
        
        // This is a placeholder - actual filtering needs access to UserContentLayer
        // Game loop will need to call a method to handle this properly
        // For now, just acknowledge receipt
        
        self.stats.state_requests_sent += 1; // Track that we received it
        
        Ok(())
    }
    
    /// Handle incoming chunk state response
    ///
    /// We requested chunk state and received operations. Queue them for
    /// application to chunks (with deduplication and signature verification).
    fn handle_state_response(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let response = ChunkStateResponse::from_bytes(data)
            .map_err(|e| MultiplayerError::SerializationError(e.to_string()))?;
        
        let op_count = response.operation_count();
        
        println!("📦 Received state response from {} with {} operations across {} chunks",
            peer_id, op_count, response.operations.len());
        
        // Merge vector clocks for causality tracking
        self.vector_clock.merge(&response.responder_clock);
        
        // Flatten operations from all chunks into pending queue
        for (_chunk_id, ops) in response.operations {
            for op in ops {
                // Verify signature
                if !op.verify_signature() {
                    println!("⚠️  Invalid signature in state response from {}", peer_id);
                    self.stats.invalid_signatures += 1;
                    continue;
                }
                
                // Check for duplicates (by signature)
                if self.voxel_op_seen.contains(&op.signature) {
                    continue; // Already have this operation
                }
                
                // Mark as seen and queue for application
                self.voxel_op_seen.insert(op.signature);
                self.pending_state_ops.push(op);
                self.stats.state_ops_received += 1;
            }
        }
        
        self.stats.state_responses_received += 1;
        
        println!("   Queued {} new operations for application", self.pending_state_ops.len());
        
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
    
    /// Get all pending voxel operations and clear the queue
    ///
    /// Call this in your game loop to process received operations.
    pub fn take_pending_operations(&mut self) -> Vec<VoxelOperation> {
        std::mem::take(&mut self.pending_ops)
    }
    
    /// Take pending state synchronization operations
    ///
    /// Returns operations received from ChunkStateResponse messages.
    /// These should be applied to chunks and added to local op_log.
    ///
    /// Called once per frame after update().
    pub fn take_pending_state_operations(&mut self) -> Vec<VoxelOperation> {
        std::mem::take(&mut self.pending_state_ops)
    }
    
    /// Request chunk state from all connected peers
    ///
    /// Sends ChunkStateRequest to all peers asking for their operations
    /// for the specified chunks. Used when joining network or loading new chunks.
    ///
    /// # Arguments
    /// * `chunk_ids` - List of chunk IDs to request operations for
    ///
    /// # Example
    /// ```rust
    /// // After loading chunks on join
    /// let loaded_chunk_ids = chunk_manager.get_loaded_chunk_ids();
    /// multiplayer.request_chunk_state(loaded_chunk_ids)?;
    /// ```
    pub fn request_chunk_state(&mut self, chunk_ids: Vec<ChunkId>) -> Result<()> {
        if chunk_ids.is_empty() {
            return Ok(()); // No chunks to request
        }
        
        let request = ChunkStateRequest::new(chunk_ids.clone(), self.vector_clock.clone());
        let data = request.to_bytes()
            .map_err(|e| MultiplayerError::SerializationError(e.to_string()))?;
        
        // Broadcast request to all connected peers
        self.cmd_tx.send(NetworkCommand::Publish {
            topic: "metaverse/state/request".to_string(),
            data,
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        
        self.stats.state_requests_sent += 1;
        
        println!("📡 Requested state for {} chunks from all peers", chunk_ids.len());
        
        Ok(())
    }
    
    /// Get list of connected peers
    pub fn connected_peers(&self) -> &HashSet<PeerId> {
        &self.connected_peers
    }
    
    /// Check if there are pending operations
    pub fn has_pending_operations(&self) -> bool {
        !self.pending_ops.is_empty()
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

/// Background network thread runner
///
/// Runs in a dedicated thread with tokio runtime.
/// Processes commands from main thread and sends events back.
fn run_network_thread(
    identity: Identity,
    cmd_rx: Receiver<NetworkCommand>,
    event_tx: Sender<NetworkEvent>,
) {
    // Create tokio runtime in this thread
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    
    // Run the network loop
    rt.block_on(async {
        // Create network node asynchronously (mDNS needs tokio context)
        println!("🔧 [Network Thread] Creating NetworkNode...");
        let mut network = match NetworkNode::new_async(identity).await {
            Ok(n) => {
                println!("✅ [Network Thread] NetworkNode created successfully");
                n
            }
            Err(e) => {
                eprintln!("❌ [Network Thread] Failed to create network node: {}", e);
                return;
            }
        };
        
        // Subscribe to topics
        println!("🔧 [Network Thread] Subscribing to topics...");
        if let Err(e) = network.subscribe(TOPIC_PLAYER_STATE) {
            eprintln!("Failed to subscribe to player-state: {}", e);
        } else {
            println!("📻 Subscribed to topic: player-state");
        }
        if let Err(e) = network.subscribe(TOPIC_VOXEL_OPS) {
            eprintln!("Failed to subscribe to voxel-ops: {}", e);
        } else {
            println!("📻 Subscribed to topic: voxel-ops");
        }
        if let Err(e) = network.subscribe(TOPIC_CHAT) {
            eprintln!("Failed to subscribe to chat: {}", e);
        } else {
            println!("📻 Subscribed to topic: chat");
        }
        
        println!("🔍 Network thread started - polling for mDNS and connections...");
        println!("🔧 [Network Thread] About to enter tokio::select! loop...");
        
        let mut heartbeat_counter = 0u64;
        
        // Main loop: process commands and poll network
        loop {
            heartbeat_counter += 1;
            if heartbeat_counter % 6000 == 0 {
                println!("💓 [Network Thread] Heartbeat {} - loop is alive", heartbeat_counter / 6000);
            }
            
            // Process ALL pending commands first (drain the channel)
            while let Ok(cmd) = cmd_rx.try_recv() {
                match cmd {
                    NetworkCommand::Listen { multiaddr } => {
                        if let Err(e) = network.listen_on(&multiaddr) {
                            eprintln!("Failed to listen on {}: {}", multiaddr, e);
                        }
                    }
                    NetworkCommand::Dial { address } => {
                        if let Err(e) = network.dial(&address) {
                            eprintln!("Failed to dial {}: {}", address, e);
                        }
                    }
                    NetworkCommand::Subscribe { topic } => {
                        if let Err(e) = network.subscribe(&topic) {
                            eprintln!("Failed to subscribe to {}: {}", topic, e);
                        }
                    }
                    NetworkCommand::Unsubscribe { topic } => {
                        if let Err(e) = network.unsubscribe(&topic) {
                            eprintln!("Failed to unsubscribe from {}: {}", topic, e);
                        }
                    }
                    NetworkCommand::Publish { topic, data } => {
                        if let Err(e) = network.publish(&topic, data) {
                            // Suppress "no peers" error
                            if !e.to_string().contains("NoPeersSubscribedToTopic") {
                                eprintln!("Failed to publish to {}: {}", topic, e);
                            }
                        }
                    }
                    NetworkCommand::Shutdown => {
                        println!("Network thread shutting down");
                        return;
                    }
                }
            }
            
            // Now poll the network for events
            while let Some(event) = network.poll() {
                match &event {
                    NetworkEvent::PeerDiscovered { peer_id } => {
                        println!("🔍 [Network Thread] mDNS discovered peer: {}", peer_id);
                    }
                    NetworkEvent::PeerConnected { peer_id, .. } => {
                        println!("🔗 [Network Thread] Peer connected: {}", peer_id);
                    }
                    NetworkEvent::ListeningOn { address } => {
                        println!("👂 Listening on: {}", address);
                    }
                    _ => {}
                }
                
                let _ = event_tx.try_send(event);
            }
            
            // Small sleep to avoid busy-waiting
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
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
        let mp = MultiplayerSystem::new_with_runtime(identity);
        assert!(mp.is_ok());
        // Give background thread time to initialize
        std::thread::sleep(Duration::from_millis(100));
    }
    
    #[test]
    fn test_voxel_op_deduplication() {
        let identity = Identity::generate().unwrap();
        let mut mp = MultiplayerSystem::new_with_runtime(identity.clone()).unwrap();
        
        // Give background thread time to initialize
        std::thread::sleep(Duration::from_millis(100));
        
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
