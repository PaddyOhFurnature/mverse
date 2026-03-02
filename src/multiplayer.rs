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
    bandwidth::{BandwidthManager, MessagePriority},
    chunk::ChunkId,
    coordinates::ECEF,
    identity::Identity,
    key_registry::{KeyRegistry, KeyRegistryMessage, revocation_signable_bytes},
    messages::{
        Action, ChatMessage, ChunkStateRequest, ChunkStateResponse, ChunkTerrainData, ChunkManifest,
        CompactPlayerState, LamportClock, Material, MovementMode, PlayerStateMessage,
        PlayerSessionRecord, SignedOperation, VoxelOperation, MessageError,
    },
    network::{NetworkCommand, NetworkEvent, NetworkNode, NetworkError},
    permissions::{action_to_class, check_key_level_permission, PermissionConfig, PermissionResult},
    player_state::PlayerStateManager,
    spatial_sharding::{SpatialSharding, SpatialConfig},
    voxel::{Octree, VoxelCoord},
    physics::PhysicsWorld,
};
use libp2p::PeerId;
use crossbeam::channel::{self, Sender, Receiver};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sha2::{Sha256, Digest as _};

/// Lightweight handle that terrain workers can use to request tiles from DHT peers.
/// Clone-cheap (Arc inside). Can be used from sync (rayon) threads — blocks with timeout.
#[derive(Clone)]
pub struct TileFetcher {
    cmd_tx: crossbeam::channel::Sender<NetworkCommand>,
    /// Known peers that serve tiles (Server tier or storage > 0).
    known_tile_servers: Arc<std::sync::Mutex<Vec<PeerId>>>,
}

impl TileFetcher {
    /// Try to fetch an OSM tile from a known peer. Tries ALL known servers in order (3s per peer).
    /// Returns raw bincode bytes on success, None on failure.
    pub fn fetch_osm(&self, s: f64, w: f64, n: f64, e: f64) -> Option<Vec<u8>> {
        let servers: Vec<PeerId> = self.known_tile_servers.lock().ok()?.clone();
        for peer in servers {
            let (tx, rx) = crossbeam::channel::bounded(1);
            if self.cmd_tx.send(NetworkCommand::RequestTile {
                peer_id: peer,
                request: crate::tile_protocol::TileRequest::OsmTile { s, w, n, e },
                response_tx: tx,
            }).is_err() { continue; }
            match rx.recv_timeout(std::time::Duration::from_secs(3)) {
                Ok(crate::tile_protocol::TileResponse::Found(bytes)) => return Some(bytes),
                _ => continue,
            }
        }
        None
    }

    /// Announce to the DHT that this node now has an OSM tile cached.
    /// Non-blocking — fire and forget.
    pub fn announce_osm(&self, s: f64, w: f64, n: f64, e: f64) {
        let key = crate::osm::osm_dht_key(s, w, n, e);
        let _ = self.cmd_tx.send(NetworkCommand::StartProvidingKey { key });
    }

    /// Try to fetch an elevation tile (1°×1° SRTM) from a known peer. Blocks up to 9s.
    /// Returns raw GeoTIFF bytes on success, None on failure.
    pub fn fetch_elevation(&self, lat: i32, lon: i32) -> Option<Vec<u8>> {
        let servers: Vec<PeerId> = self.known_tile_servers.lock().ok()?.clone();
        for peer in servers {
            let (tx, rx) = crossbeam::channel::bounded(1);
            if self.cmd_tx.send(NetworkCommand::RequestTile {
                peer_id: peer,
                request: crate::tile_protocol::TileRequest::ElevationTile { lat, lon },
                response_tx: tx,
            }).is_err() { continue; }
            match rx.recv_timeout(std::time::Duration::from_secs(3)) {
                Ok(crate::tile_protocol::TileResponse::Found(bytes)) => return Some(bytes),
                _ => continue,
            }
        }
        None
    }

    /// Announce to the DHT that this node now has an elevation tile cached.
    pub fn announce_elevation(&self, lat: i32, lon: i32) {
        let key = crate::elevation::elevation_dht_key(lat, lon);
        let _ = self.cmd_tx.send(NetworkCommand::StartProvidingKey { key });
    }
}

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
pub const TOPIC_CHUNK_TERRAIN: &str = "chunk-terrain";
pub const TOPIC_CHUNK_MANIFEST: &str = "chunk-manifest";
pub const TOPIC_KEY_REGISTRY: &str = "key-registry";
pub const TOPIC_KEY_REVOCATIONS: &str = "key-revocations";
pub const TOPIC_SIGNED_OPS: &str = "signed-ops";
/// Compact player state — replaces full `PlayerStateMessage` on degraded/LoRa links.
pub const TOPIC_PLAYER_STATE_COMPACT: &str = "player-state-compact";

/// Keepalive interval when standing still (prevents peer timeout)
const PLAYER_STATE_KEEPALIVE_INTERVAL: Duration = Duration::from_millis(500);

/// Minimum position change (metres) before we send an update
const POSITION_DELTA_THRESHOLD: f64 = 0.05;

/// Minimum rotation change (radians ~1°) before we send an update
const ROTATION_DELTA_THRESHOLD: f32 = 0.017;

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
    
    /// Spatial sharding for intelligent peer selection and bandwidth optimization
    spatial_sharding: Option<SpatialSharding>,
    
    /// Local player position (for spatial sharding distance calculations)
    local_position: ECEF,
    
    /// Lamport clock for causal ordering (kept for backwards compat)
    clock: LamportClock,
    
    /// Vector clock for proper CRDT causality
    vector_clock: crate::vector_clock::VectorClock,
    
    /// Deduplication set for voxel operations (by operation ID)
    voxel_op_seen: HashSet<[u8; 64]>, // Store signature as ID
    
    /// Pending voxel operations to be applied to world
    pending_ops: Vec<SignedOperation>,
    
    /// Pending state synchronization operations (from ChunkStateResponse)
    pending_state_ops: Vec<SignedOperation>,
    
    /// Pending state requests from peers
    pending_state_requests: Vec<(PeerId, ChunkStateRequest)>,
    
    /// Peers we've requested state from (to avoid duplicate requests)
    state_requested_from: HashSet<PeerId>,

    /// Peers that just connected and need a chunk state sync
    peers_needing_sync: Vec<PeerId>,

    /// Received chunk terrain data waiting to be applied to the world
    pending_chunk_terrain: Vec<(ChunkId, Vec<u8>, u64)>,

    /// Received chunk manifests waiting to be processed (compare + send our newer chunks)
    pending_chunk_manifests: Vec<Vec<(ChunkId, u64)>>,

    /// DHT provider results — (dht_key, providers) from GetProviders queries
    pending_chunk_providers: Vec<(Vec<u8>, Vec<PeerId>)>,
    
    /// Peer reputation tracking (invalid signatures count)
    peer_reputation: HashMap<PeerId, usize>,
    
    /// Blocked peers (too many invalid signatures)
    blocked_peers: HashSet<PeerId>,
    
    /// Timer for keepalive player state broadcasts (500ms)
    last_state_broadcast: Instant,

    /// Last position we actually transmitted (for delta suppression)
    last_sent_position: ECEF,

    /// Last yaw we transmitted (for delta suppression)
    last_sent_yaw: f32,

    /// Last pitch we transmitted (for delta suppression)
    last_sent_pitch: f32,

    /// Last movement mode we transmitted (always resend on change)
    last_sent_movement_mode: Option<MovementMode>,

    /// Gossipsub topics we are currently subscribed to for per-chunk AOI
    subscribed_chunk_topics: HashSet<String>,

    /// Bandwidth profile manager — controls what gets sent under degraded conditions
    pub bandwidth: BandwidthManager,

    /// Connected peers (for state exchange)
    connected_peers: HashSet<PeerId>,

    /// P2P identity registry — cached KeyRecords for all known peers
    pub key_registry: KeyRegistry,
    
    /// Permission configuration — controls what is checked on incoming ops
    pub perm_config: PermissionConfig,

    /// Fetched session record from DHT — populated when a remote session was found
    /// on startup. The game loop reads this once to restore the player's last position.
    pub pending_session_record: Option<PlayerSessionRecord>,

    /// Compact 2-byte session IDs assigned to each known peer.
    ///
    /// Hot-path messages (position updates on degraded links) use these instead
    /// of the full 39-byte `PeerId`, saving ~37 bytes per packet.
    /// Token is derived deterministically from PeerId so all peers agree without coordination.
    session_ids: HashMap<PeerId, u16>,
    /// Reverse map: session_id → PeerId (for decoding compact messages).
    session_id_map: HashMap<u16, PeerId>,
    /// Our own compact token (deterministic from local PeerId).
    /// Used when broadcasting `CompactPlayerState`.
    local_session_token: u16,
    /// Number of full `PlayerStateMessage` broadcasts sent.
    /// Non-zero means peers know our PeerId, so compact messages can be sent.
    full_state_broadcasts: u64,

    /// Pending key revocation events — drained by `take_key_revocations()`.
    pending_revocations: Vec<(PeerId, PeerId)>,

    /// Local cache of meshsite content items received from the mesh.
    /// Keyed by section name, newest-first. Max 200 items per section (simple LRU).
    pub content_cache: std::collections::HashMap<String, Vec<crate::meshsite::ContentItem>>,

    /// Local cache of placed world objects, keyed by chunk (cx, cz).
    /// Populated from DHT `GetRecord("world/chunk/{cx}/{cz}")` responses.
    pub world_objects_cache: std::collections::HashMap<(i32, i32), Vec<crate::world_objects::PlacedObject>>,

    /// Statistics
    stats: MultiplayerStats,

    /// Known peers that can serve tiles (servers with storage > 0).
    known_tile_servers: Arc<std::sync::Mutex<Vec<PeerId>>>,

    /// Root directory for world data (osm/, elevation_cache/, chunks/).
    world_data_dir: std::path::PathBuf,
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
        let world_data_dir = std::path::PathBuf::from("./world_data");
        let world_data_dir_clone = world_data_dir.clone();
        std::thread::spawn(move || {
            run_network_thread(identity_clone, cmd_rx, event_tx, world_data_dir_clone);
        });
        
        Ok(Self {
            cmd_tx,
            event_rx,
            identity,
            local_peer_id,
            remote_players: PlayerStateManager::new(local_peer_id),
            spatial_sharding: None, // Initialized when position is known
            local_position: ECEF::new(0.0, 0.0, 0.0), // Will be updated
            clock: LamportClock::default(),
            vector_clock: crate::vector_clock::VectorClock::new(),
            voxel_op_seen: HashSet::new(),
            pending_ops: Vec::new(),
            pending_state_ops: Vec::new(),
            pending_state_requests: Vec::new(),
            state_requested_from: HashSet::new(),
            peers_needing_sync: Vec::new(),
            pending_chunk_terrain: Vec::new(),
            pending_chunk_manifests: Vec::new(),
            pending_chunk_providers: Vec::new(),
            peer_reputation: HashMap::new(),
            blocked_peers: HashSet::new(),
            last_state_broadcast: Instant::now(),
            last_sent_position: ECEF::new(0.0, 0.0, 0.0),
            last_sent_yaw: 0.0,
            last_sent_pitch: 0.0,
            last_sent_movement_mode: None,
            subscribed_chunk_topics: HashSet::new(),
            bandwidth: BandwidthManager::default(),
            connected_peers: HashSet::new(),
            key_registry: {
                let mut reg = KeyRegistry::with_local_peer(local_peer_id);
                reg.load_from_disk().ok();
                reg
            },
            perm_config: PermissionConfig::default(),
            pending_session_record: None,
            session_ids: HashMap::new(),
            session_id_map: HashMap::new(),
            local_session_token: peer_id_to_token(&local_peer_id),
            full_state_broadcasts: 0,
            pending_revocations: Vec::new(),
            content_cache: std::collections::HashMap::new(),
            world_objects_cache: std::collections::HashMap::new(),
            stats: MultiplayerStats::default(),
            known_tile_servers: Arc::new(std::sync::Mutex::new(Vec::new())),
            world_data_dir,
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
    pub fn new(_identity: Identity) -> Result<Self> {
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
    
    /// Broadcast player state using delta suppression.
    ///
    /// Only transmits when:
    /// - Position changed more than POSITION_DELTA_THRESHOLD (5cm)
    /// - Rotation changed more than ROTATION_DELTA_THRESHOLD (~1°)
    /// - Movement mode changed
    /// - No transmission in the last PLAYER_STATE_KEEPALIVE_INTERVAL (500ms) — keepalive
    ///
    /// Under bandwidth-restricted profiles (LoRa), position is suppressed entirely.
    /// This reduces idle-player bandwidth ~10× compared to always-on 20Hz broadcasting.
    pub fn broadcast_player_state(
        &mut self,
        position: ECEF,
        velocity: [f32; 3],
        yaw: f32,
        pitch: f32,
        movement_mode: MovementMode,
    ) -> Result<()> {
        // Bandwidth profile gate: suppress position under LoRa / very constrained links
        if !self.bandwidth.allows(MessagePriority::Normal) {
            return Ok(());
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_state_broadcast);

        // Compute deltas
        let pos_delta = {
            let dx = position.x - self.last_sent_position.x;
            let dy = position.y - self.last_sent_position.y;
            let dz = position.z - self.last_sent_position.z;
            (dx * dx + dy * dy + dz * dz).sqrt()
        };
        let yaw_delta = (yaw - self.last_sent_yaw).abs();
        let pitch_delta = (pitch - self.last_sent_pitch).abs();
        let mode_changed = self.last_sent_movement_mode != Some(movement_mode);

        let has_delta = pos_delta > POSITION_DELTA_THRESHOLD
            || yaw_delta > ROTATION_DELTA_THRESHOLD
            || pitch_delta > ROTATION_DELTA_THRESHOLD
            || mode_changed;
        let keepalive_due = elapsed >= PLAYER_STATE_KEEPALIVE_INTERVAL;

        if !has_delta && !keepalive_due {
            return Ok(());
        }

        self.last_state_broadcast = now;
        self.last_sent_position = position;
        self.last_sent_yaw = yaw;
        self.last_sent_pitch = pitch;
        self.last_sent_movement_mode = Some(movement_mode);

        // Update local position for spatial sharding
        self.local_position = position;
        let region_changed = if let Some(ref mut sharding) = self.spatial_sharding {
            sharding.update_local_position(position)
        } else {
            false
        };

        // If we moved to a new region, resubscribe to new region topics
        if region_changed {
            if let Some(ref sharding) = self.spatial_sharding {
                println!("📍 Moved to new region: {}", sharding.current_region());

                let new_topics = sharding.get_subscribe_topics("voxel-ops");
                let player_topics = sharding.get_subscribe_topics("player-state");

                let mut all_topics = new_topics;
                all_topics.extend(player_topics);

                self.cmd_tx.send(NetworkCommand::SubscribeBulk {
                    topics: all_topics.clone(),
                }).map_err(|_| MultiplayerError::ChannelSendError)?;

                println!("   ✅ Subscribed to {} regional topics", all_topics.len());
            }
        }

        let timestamp = self.clock.tick();

        // Hot-path delta: use compact format (18 bytes) once peers know our PeerId.
        // Full message is sent on keepalive (every 5s) or on the very first broadcast.
        let use_compact = !keepalive_due && self.full_state_broadcasts > 0;

        let (data, pub_topic) = if use_compact {
            // Compact: session_token + position (f32) + rotation
            let pos = [position.x as f32, position.y as f32, position.z as f32];
            let compact = crate::messages::CompactPlayerState {
                session_id: self.local_session_token,
                position: pos,
                rotation: [yaw, pitch],
                timestamp_ms: (timestamp & 0xFFFF_FFFF) as u32,
            };
            let d = compact.to_bytes()?;
            (d, TOPIC_PLAYER_STATE_COMPACT.to_string())
        } else {
            // Full message: includes PeerId so receivers learn our compact token
            let msg = PlayerStateMessage::new(
                self.local_peer_id,
                position,
                velocity,
                yaw,
                pitch,
                movement_mode,
                timestamp,
            );
            self.full_state_broadcasts += 1;
            (msg.to_bytes()?, String::new()) // topic chosen below
        };

        if use_compact {
            self.cmd_tx.send(NetworkCommand::Publish { topic: pub_topic, data })
                .map_err(|_| MultiplayerError::ChannelSendError)?;
        } else {
            // Full message: publish on per-chunk AOI topic when subscribed, else regional/global
            let topic = chunk_player_topic(&ChunkId::from_ecef(&position));
            if self.subscribed_chunk_topics.contains(&topic) {
                self.cmd_tx.send(NetworkCommand::Publish { topic, data })
                    .map_err(|_| MultiplayerError::ChannelSendError)?;
            } else if let Some(ref sharding) = self.spatial_sharding {
                let fallback = sharding.get_publish_topic("player-state");
                self.cmd_tx.send(NetworkCommand::Publish { topic: fallback, data })
                    .map_err(|_| MultiplayerError::ChannelSendError)?;
            } else {
                self.cmd_tx.send(NetworkCommand::Publish {
                    topic: TOPIC_PLAYER_STATE.to_string(),
                    data,
                }).map_err(|_| MultiplayerError::ChannelSendError)?;
            }
        }

        self.stats.player_states_sent += 1;
        Ok(())
    }
    
    /// Broadcast a voxel operation (dig or place)
    pub fn broadcast_voxel_operation(
        &mut self,
        coord: VoxelCoord,
        material: Material,
    ) -> Result<SignedOperation> {
        // Increment clocks
        let timestamp = self.clock.tick();
        self.vector_clock.increment(self.local_peer_id);
        
        // Create and sign operation
        let mut op = SignedOperation::new(
            Action::SetVoxel { coord, material },
            timestamp,
            self.vector_clock.clone(),
            self.local_peer_id,
            self.identity.verifying_key().to_bytes(),
        );
        op.sign(self.identity.signing_key());

        // Remember we sent this (for deduplication on receive-back)
        self.voxel_op_seen.insert(op.signature);

        // Best-effort publish to peers — network failure must NOT prevent local persistence.
        // The op is always returned to the caller regardless of publish success.
        if let Ok(data) = op.to_bytes() {
            let chunk_id = ChunkId::from_voxel(&coord);
            let chunk_topic = chunk_voxel_topic(&chunk_id);
            let topic = if self.subscribed_chunk_topics.contains(&chunk_topic) {
                chunk_topic
            } else if let Some(ref sharding) = self.spatial_sharding {
                sharding.get_publish_topic("voxel-ops")
            } else {
                TOPIC_VOXEL_OPS.to_string()
            };
            
            if self.cmd_tx.send(NetworkCommand::Publish { topic, data }).is_ok() {
                self.stats.voxel_ops_sent += 1;
            }
        }

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

    /// Synchronise per-chunk AOI topic subscriptions with the current set of loaded chunks.
    ///
    /// Call this whenever the chunk streamer's loaded set changes (new chunks loaded or old ones
    /// unloaded). The multiplayer layer subscribes to the gossipsub topics for each loaded chunk
    /// so we only receive data relevant to our render distance, and nothing beyond it.
    ///
    /// Topic naming:
    /// - `player-state-{x}-{y}-{z}` — position updates for players in that chunk
    /// - `voxel-ops-{x}-{y}-{z}`    — block edits in that chunk
    pub fn update_subscribed_chunks(&mut self, loaded: &HashSet<ChunkId>) -> Result<()> {
        // Build the full set of topics we want
        let mut desired: HashSet<String> = HashSet::new();
        for id in loaded {
            desired.insert(chunk_player_topic(id));
            desired.insert(chunk_voxel_topic(id));
        }

        let to_add: Vec<String> = desired.difference(&self.subscribed_chunk_topics).cloned().collect();
        let to_remove: Vec<String> = self.subscribed_chunk_topics.difference(&desired).cloned().collect();

        if !to_add.is_empty() {
            self.cmd_tx.send(NetworkCommand::SubscribeBulk { topics: to_add.clone() })
                .map_err(|_| MultiplayerError::ChannelSendError)?;
            for t in &to_add {
                self.subscribed_chunk_topics.insert(t.clone());
            }
        }

        if !to_remove.is_empty() {
            self.cmd_tx.send(NetworkCommand::UnsubscribeBulk { topics: to_remove.clone() })
                .map_err(|_| MultiplayerError::ChannelSendError)?;
            for t in &to_remove {
                self.subscribed_chunk_topics.remove(t);
            }
        }

        Ok(())
    }
    
    /// Enable spatial sharding with custom configuration
    ///
    /// Spatial sharding implements hierarchical peer selection for planet-scale P2P:
    /// - **Tier 1 (100m):** Visibility range - immediate rendering, low latency
    /// - **Tier 2 (500m):** Nearby region - backup storage, medium latency
    /// - **Tier 3 (1km):** Local area - wider backup, acceptable latency  
    /// - **Tier 4 (Global):** Any distance - guaranteed redundancy
    ///
    /// # Example
    /// ```no_run
    /// use metaverse_core::spatial_sharding::SpatialConfig;
    /// 
    /// mp.enable_spatial_sharding_with_config(SpatialConfig {
    ///     redundancy_target: 10, // 10 copies per operation
    ///     tier1_radius_m: 100.0,
    ///     tier2_radius_m: 500.0,
    ///     tier3_radius_m: 1000.0,
    ///     gossip_percentage: 0.20,
    ///     ..Default::default()
    /// });
    /// ```
    pub fn enable_spatial_sharding_with_config(&mut self, config: SpatialConfig) {
        self.spatial_sharding = Some(SpatialSharding::new(self.local_position, config));
        println!("✨ Spatial sharding enabled with custom config");
    }
    
    /// Enable spatial sharding with default configuration
    ///
    /// Default configuration:
    /// - Redundancy target: 5 copies
    /// - Tier 1: 100m (visibility)
    /// - Tier 2: 500m (nearby)
    /// - Tier 3: 1km (local area)
    /// - Gossip: 20% of peers every 10 seconds
    pub fn enable_spatial_sharding(&mut self) {
        self.spatial_sharding = Some(SpatialSharding::new_default(self.local_position));
        println!("✨ Spatial sharding enabled with default config");
    }
    
    /// Disable spatial sharding (back to broadcast-all mode)
    pub fn disable_spatial_sharding(&mut self) {
        self.spatial_sharding = None;
        println!("📡 Spatial sharding disabled - using broadcast mode");
    }
    
    /// Check if spatial sharding is enabled
    pub fn is_spatial_sharding_enabled(&self) -> bool {
        self.spatial_sharding.is_some()
    }
    
    /// Get spatial sharding statistics (if enabled)
    ///
    /// Returns information about peer distribution across tiers:
    /// - How many peers in visibility range (100m)
    /// - How many peers nearby (500m)
    /// - How many peers in local area (1km)
    /// - How many peers globally
    /// - Number of relay nodes
    pub fn get_spatial_stats(&self) -> Option<crate::spatial_sharding::SpatialStats> {
        self.spatial_sharding.as_ref().map(|s| s.stats())
    }
    
    /// Update spatial sharding configuration at runtime
    pub fn update_spatial_config(&mut self, config: SpatialConfig) {
        if let Some(ref mut sharding) = self.spatial_sharding {
            sharding.set_config(config);
            println!("🔧 Spatial sharding configuration updated");
        }
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
                    TOPIC_CHUNK_TERRAIN => self.handle_chunk_terrain(peer_id, &data)?,
                    TOPIC_CHUNK_MANIFEST => self.handle_chunk_manifest(peer_id, &data)?,
                    TOPIC_KEY_REGISTRY => self.handle_key_registry(peer_id, &data),
                    TOPIC_KEY_REVOCATIONS => self.handle_key_revocation(peer_id, &data),
                    TOPIC_PLAYER_STATE_COMPACT => self.handle_compact_player_state(peer_id, &data)?,
                    // Handle regional topics (e.g., "player-state-L3-x0042-y-0015")
                    t if t.starts_with("player-state") => self.handle_player_state(peer_id, &data)?,
                    t if t.starts_with("voxel-ops") => self.handle_voxel_operation(peer_id, &data)?,
                    t if t.starts_with("meshsite/") => self.handle_meshsite_content(&data),
                    _ => {}
                }
            }
            
            NetworkEvent::PeerConnected { peer_id, address } => {
                println!("🔗 Peer connected: {} @ {}", peer_id, address);
                self.connected_peers.insert(peer_id);
                if !self.state_requested_from.contains(&peer_id) {
                    self.peers_needing_sync.push(peer_id);
                }
                // Publish our own KeyRecord so the new peer can recognise us
                self.publish_own_key_record();
            }
            
            NetworkEvent::PeerDisconnected { peer_id } => {
                println!("💔 Peer disconnected: {}", peer_id);
                self.remote_players.remove_player(&peer_id);
                self.connected_peers.remove(&peer_id);
                
                // Remove from spatial sharding
                if let Some(ref mut sharding) = self.spatial_sharding {
                    sharding.remove_peer(&peer_id);
                }

                // Remove from tile server list
                if let Ok(mut servers) = self.known_tile_servers.lock() {
                    servers.retain(|p| p != &peer_id);
                }
            }
            
            NetworkEvent::PeerDiscovered { peer_id } => {
                println!("🔍 Peer discovered: {}", peer_id);
                self.connected_peers.insert(peer_id);
                if !self.state_requested_from.contains(&peer_id) {
                    self.peers_needing_sync.push(peer_id);
                }
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
            
            NetworkEvent::NatStatusChanged { old_status, new_status, external_address } => {
                println!("🔍 NAT status: {} → {}", old_status, new_status);
                if let Some(addr) = external_address {
                    println!("   External address: {}", addr);
                }
            }
            
            NetworkEvent::ConnectionUpgraded { peer_id, from_relay } => {
                if from_relay {
                    println!("⚡ Direct P2P connection established with: {}", peer_id);
                }
            }
            NetworkEvent::DirectConnectionUpgraded { peer_id } => {
                println!("⚡ [DCUtR] Hole punch succeeded — now direct with: {}", peer_id);
            }
            // These events are generated locally (not from the network thread) — no-op here.
            NetworkEvent::KeyRevoked { .. } | NetworkEvent::SessionIdAssigned { .. } => {}
            NetworkEvent::ChunkProvidersFound { key, providers } => {
                self.pending_chunk_providers.push((key.clone(), providers.clone()));
            }
            NetworkEvent::DhtRecordFound { key, value } => {
                // trying the session namespace key first, then falling back to KeyRecord.
                let session_key = PlayerSessionRecord::dht_key(&self.local_peer_id);
                if key == session_key {
                    // This is our own session record — offer it to the game loop
                    match PlayerSessionRecord::from_bytes(&value) {
                        Ok(rec) if rec.verify() => {
                            println!("📍 [Session] Found last session: pos [{:.1},{:.1},{:.1}] logged out {}s ago",
                                rec.position[0], rec.position[1], rec.position[2],
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as u64)
                                    .unwrap_or(0)
                                    .saturating_sub(rec.logged_out_at) / 1000);
                            self.pending_session_record = Some(rec);
                        }
                        Ok(_)  => eprintln!("📍 [Session] Session record signature invalid — ignoring"),
                        Err(e) => eprintln!("📍 [Session] Failed to deserialize session record: {}", e),
                    }
                } else if key.starts_with(b"caps/") {
                    // NodeCapabilities record — track as tile server if eligible
                    if let Some(caps) = crate::node_capabilities::NodeCapabilities::from_bytes(&value) {
                        if let Ok(key_str) = std::str::from_utf8(&key) {
                            if let Some(peer_id_str) = key_str.strip_prefix("caps/") {
                                if let Ok(peer_id) = peer_id_str.parse::<PeerId>() {
                                    use crate::node_capabilities::NodeTier;
                                    if matches!(caps.tier, NodeTier::Server) || caps.available_storage_bytes > 0 {
                                        if let Ok(mut servers) = self.known_tile_servers.lock() {
                                            if !servers.contains(&peer_id) {
                                                servers.push(peer_id);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Attempt to deserialize as a KeyRecord and update the registry
                    match crate::identity::KeyRecord::from_bytes(&value) {
                        Ok(record) => {
                            let peer_id = record.peer_id;
                            match self.key_registry.apply_update(record) {
                                Ok(true)  => println!("🔑 [DHT] Updated KeyRecord for {} from DHT", peer_id),
                                Ok(false) => {}  // already had this record
                                Err(e)    => eprintln!("🔑 [DHT] Rejected DHT record for {}: {}", peer_id, e),
                            }
                        }
                        Err(_) => {
                            // Not a KeyRecord — try as a world chunk object list
                            if let Some(chunk_list) = crate::world_objects::ChunkObjectList::from_bytes(&value) {
                                let count = chunk_list.objects.len();
                                self.world_objects_cache.insert(
                                    (chunk_list.cx, chunk_list.cz),
                                    chunk_list.objects,
                                );
                                if count > 0 {
                                    println!("🗺️  [DHT] Loaded {} world objects for chunk ({},{})",
                                        count, chunk_list.cx, chunk_list.cz);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Handle incoming player state message
    fn handle_player_state(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let msg = PlayerStateMessage::from_bytes(data)?;
        
        println!("📥 Received player state from {}: pos=({:.1}, {:.1}, {:.1})", 
            peer_id, msg.position.x, msg.position.y, msg.position.z);
        
        // Assign a session ID on first contact — used by compact hot-path messages.
        if !self.session_ids.contains_key(&peer_id) {
            let sid = self.assign_session_id(peer_id);
            println!("🔢 [SessionID] Assigned session_id={} to {}", sid, peer_id);
        }

        // Update Lamport clock
        self.clock.receive(msg.timestamp);
        
        // Update remote player (manager handles deduplication and filtering)
        self.remote_players.handle_message(msg.clone());
        self.stats.player_states_received += 1;
        
        // Update spatial sharding with peer position
        if let Some(ref mut sharding) = self.spatial_sharding {
            // Assume regular players, not relay nodes (can add relay detection later)
            sharding.update_peer(peer_id, msg.position, false);
        }
        
        println!("   Total remote players tracked: {}", self.remote_players.player_count());
        
        Ok(())
    }
    
    /// Handle incoming voxel operation with CRDT merge and signature verification
    fn handle_voxel_operation(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        // Try new SignedOperation format first; fall back to legacy VoxelOperation
        let op: SignedOperation = if let Ok(signed) = SignedOperation::from_bytes(data) {
            signed
        } else {
            #[allow(deprecated)]
            match VoxelOperation::from_bytes(data) {
                Ok(legacy) => {
                    self.clock.receive(legacy.timestamp);
                    self.vector_clock.merge(&legacy.vector_clock);
                    self.vector_clock.increment(self.local_peer_id);
                    #[allow(deprecated)]
                    SignedOperation::from(legacy)
                }
                Err(e) => {
                    eprintln!("⚠️  [VoxelOp] Failed to deserialize from {}: {}", peer_id, e);
                    return Ok(());
                }
            }
        };

        let ecef = op.coord().map(|c| c.to_ecef());
        if let Some(ecef) = ecef {
            println!("🔨 Received voxel op from {}: {:?} at ecef=({:.1},{:.1},{:.1})",
                peer_id, op.action.name(), ecef.x, ecef.y, ecef.z);
        }

        // Deduplication
        if self.voxel_op_seen.contains(&op.signature) {
            return Ok(());
        }

        // Author sanity check
        if op.author != peer_id {
            eprintln!("⚠️  [VoxelOp] Author mismatch: claimed {}, received from {}", op.author, peer_id);
            self.stats.voxel_ops_rejected += 1;
            return Ok(());
        }

        // Permission check: sig + key type + expiry + revocation
        let action_class = action_to_class(&op.action);
        let op_bytes = op.signable_bytes();
        let perm = check_key_level_permission(
            &mut self.key_registry,
            &op.author,
            &op.public_key,
            &op_bytes,
            &op.signature,
            action_class,
            &self.perm_config,
        );
        if perm.is_denied() {
            eprintln!("🔒 [VoxelOp] Op from {} denied: {}", peer_id, perm);
            self.stats.voxel_ops_rejected += 1;
            let count = self.peer_reputation.entry(peer_id).or_insert(0);
            *count += 1;
            if *count >= MAX_INVALID_SIGNATURES {
                eprintln!("⚠️  Blocking peer {}: too many denied ops", peer_id);
                self.blocked_peers.insert(peer_id);
            }
            return Ok(());
        }

        self.clock.receive(op.lamport);
        self.vector_clock.merge(&op.vector_clock);
        self.vector_clock.increment(self.local_peer_id);

        self.voxel_op_seen.insert(op.signature);
        self.stats.voxel_ops_received += 1;
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
    /// Peer is requesting our operations for specific chunks. Queue the request
    /// for the game loop to handle (needs access to ChunkManager).
    fn handle_state_request(&mut self, peer_id: PeerId, data: &[u8]) -> Result<()> {
        let request = ChunkStateRequest::from_bytes(data)
            .map_err(|e| MultiplayerError::SerializationError(e.to_string()))?;
        
        println!("📨 Received state request from {} for {} chunks",
            peer_id, request.chunk_ids.len());
        
        // Queue for game loop to handle (needs ChunkManager access)
        self.pending_state_requests.push((peer_id, request));
        
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
                // Check for duplicates (by signature)
                if self.voxel_op_seen.contains(&op.signature) {
                    continue;
                }

                // Permission check: sig + key type + expiry + revocation
                let action_class = action_to_class(&op.action);
                let op_bytes = op.signable_bytes();
                let perm = check_key_level_permission(
                    &mut self.key_registry,
                    &op.author,
                    &op.public_key,
                    &op_bytes,
                    &op.signature,
                    action_class,
                    &self.perm_config,
                );
                if perm.is_denied() {
                    eprintln!("🔒 [StateSync] Op from {} denied: {}", op.author, perm);
                    self.stats.voxel_ops_rejected += 1;
                    continue;
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
    
    /// Check whether the local player's key is allowed to perform an action.
    ///
    /// Call this before creating and broadcasting a local operation.
    /// Returns `PermissionResult::Allowed` if the action is permitted.
    pub fn check_local_op_permission(&mut self, action: &Action) -> PermissionResult {
        let record = self.key_registry.get_or_default(self.local_peer_id);
        let class = action_to_class(action);
        crate::permissions::check_record_permission(&record, class, &self.perm_config)
    }
    
    /// Apply a received voxel operation to the octree with CRDT merge
    ///
    /// Returns true if operation was applied (won the CRDT merge),
    /// false if it was rejected (lost to a conflicting operation).
    pub fn apply_voxel_operation(
        &mut self,
        op: SignedOperation,
        octree: &mut Octree,
        local_ops: &HashMap<VoxelCoord, SignedOperation>,
    ) -> bool {
        // CRDT merge: check if there's a local conflicting operation
        if let Some(coord) = op.coord() {
            if let Some(local_op) = local_ops.get(&coord) {
                if !op.wins_over(local_op) {
                    self.stats.voxel_ops_rejected += 1;
                    return false;
                }
            }
            if let Some(material) = op.material() {
                octree.set_voxel(coord, material.to_material_id());
            }
        }
        
        self.stats.voxel_ops_applied += 1;
        true
    }
    
    /// Get all pending voxel operations and clear the queue
    ///
    /// Call this in your game loop to process received operations.
    pub fn take_pending_operations(&mut self) -> Vec<SignedOperation> {
        std::mem::take(&mut self.pending_ops)
    }
    
    /// Take pending state synchronization operations
    ///
    /// Returns operations received from ChunkStateResponse messages.
    /// These should be applied to chunks and added to local op_log.
    ///
    /// Called once per frame after update().
    pub fn take_pending_state_operations(&mut self) -> Vec<SignedOperation> {
        std::mem::take(&mut self.pending_state_ops)
    }
    
    /// Get pending state requests from peers.
    ///
    /// Game loop should call this, filter operations using ChunkManager,
    /// and send responses back using send_chunk_state_response().
    pub fn take_pending_state_requests(&mut self) -> Vec<(PeerId, ChunkStateRequest)> {
        std::mem::take(&mut self.pending_state_requests)
    }

    /// Returns peers that just connected and need a full chunk state sync.
    /// Game loop should call request_chunk_state() for loaded chunks when this is non-empty.
    pub fn take_peers_needing_sync(&mut self) -> Vec<PeerId> {
        std::mem::take(&mut self.peers_needing_sync)
    }
    
    /// Send chunk state response to a peer.
    ///
    /// Called by game loop after filtering operations using ChunkManager.
    /// Operations are grouped by chunk ID and sent with our vector clock.
    ///
    /// # Chunking Strategy
    /// If response is large (>100 operations), splits into multiple messages.
    /// Each message is independently deliverable, enabling graceful degradation.
    ///
    /// # Arguments
    /// * `operations_by_chunk` - Operations grouped by chunk ID
    ///
    /// # Returns
    /// Number of messages sent
    pub fn send_chunk_state_response(
        &mut self, 
        operations_by_chunk: HashMap<ChunkId, Vec<SignedOperation>>
    ) -> Result<usize> {
        if operations_by_chunk.is_empty() {
            return Ok(0); // Nothing to send
        }
        
        let total_ops: usize = operations_by_chunk.values().map(|v| v.len()).sum();
        
        // Adaptive chunking based on operation count
        const OPS_PER_CHUNK: usize = 100; // ~10-20 KB per message with binary encoding
        
        if total_ops <= OPS_PER_CHUNK {
            // Small response - send in one message
            let response = ChunkStateResponse::new(
                operations_by_chunk,
                self.vector_clock.clone()
            );
            let bytes = response.to_bytes()
                .map_err(|e| MultiplayerError::SerializationError(e.to_string()))?;
            
            self.cmd_tx.send(NetworkCommand::Publish {
                topic: TOPIC_STATE_RESPONSE.to_string(),
                data: bytes,
            }).map_err(|_| MultiplayerError::ChannelSendError)?;
            
            self.stats.state_responses_received += 1;
            return Ok(1);
        }
        
        // Large response - chunk it
        let response_id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        
        // Flatten operations into single vec for chunking
        let mut all_ops: Vec<(ChunkId, SignedOperation)> = Vec::new();
        for (chunk_id, ops) in operations_by_chunk {
            for op in ops {
                all_ops.push((chunk_id, op));
            }
        }
        
        let chunks: Vec<_> = all_ops.chunks(OPS_PER_CHUNK).collect();
        let total_chunks = chunks.len() as u32;
        
        println!("   📦 Chunking {} ops into {} messages ({} ops/msg)", 
            total_ops, total_chunks, OPS_PER_CHUNK);
        
        for (i, chunk) in chunks.iter().enumerate() {
            // Rebuild operations_by_chunk for this chunk
            let mut chunk_ops: HashMap<ChunkId, Vec<SignedOperation>> = HashMap::new();
            for (chunk_id, op) in chunk.iter() {
                chunk_ops.entry(*chunk_id)
                    .or_insert_with(Vec::new)
                    .push(op.clone());
            }
            
            let response = ChunkStateResponse::new_chunked(
                chunk_ops,
                self.vector_clock.clone(),
                i as u32,
                total_chunks,
                response_id,
            );
            
            let bytes = response.to_bytes()
                .map_err(|e| MultiplayerError::SerializationError(e.to_string()))?;
            
            self.cmd_tx.send(NetworkCommand::Publish {
                topic: TOPIC_STATE_RESPONSE.to_string(),
                data: bytes,
            }).map_err(|_| MultiplayerError::ChannelSendError)?;
        }
        
        self.stats.state_responses_received += total_chunks as u64;
        Ok(total_chunks as usize)
    }
    
    /// Broadcast chunk terrain (raw octree bytes + timestamp) to all peers.
    /// Peers only apply this if the received timestamp is newer than what they have.
    pub fn broadcast_chunk_terrain(&mut self, chunk_id: ChunkId, octree_bytes: Vec<u8>, last_modified: u64) -> Result<()> {
        // Bandwidth gate: suppress terrain transfers on constrained/LoRa links
        if !self.bandwidth.should_send_terrain() {
            return Ok(());
        }
        let data = ChunkTerrainData { chunk_id, octree_bytes, last_modified };
        let bytes = data.to_bytes()?;
        if let Err(e) = self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_CHUNK_TERRAIN.to_string(),
            data: bytes,
        }) {
            eprintln!("⚠️ [TERRAIN SYNC] Failed to broadcast chunk {:?}: {}", chunk_id, e);
        }
        Ok(())
    }

    /// Receive handler for chunk-terrain gossipsub messages.
    fn handle_chunk_terrain(&mut self, _peer_id: PeerId, data: &[u8]) -> Result<()> {
        let terrain_data = ChunkTerrainData::from_bytes(data)?;
        println!("📦 [TERRAIN SYNC] Received terrain for chunk {:?} ({} bytes, t={})",
            terrain_data.chunk_id, terrain_data.octree_bytes.len(), terrain_data.last_modified);
        self.pending_chunk_terrain.push((terrain_data.chunk_id, terrain_data.octree_bytes, terrain_data.last_modified));
        Ok(())
    }

    /// Take all pending chunk terrain data for application to the world.
    /// Returns (chunk_id, octree_bytes, last_modified).
    pub fn take_pending_chunk_terrain(&mut self) -> Vec<(ChunkId, Vec<u8>, u64)> {
        std::mem::take(&mut self.pending_chunk_terrain)
    }

    /// Broadcast a chunk manifest so peers can compare and send us newer chunks.
    pub fn broadcast_chunk_manifest(&mut self, entries: Vec<(ChunkId, u64)>) -> Result<()> {
        let manifest = ChunkManifest { entries };
        let bytes = manifest.to_bytes()?;
        if let Err(e) = self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_CHUNK_MANIFEST.to_string(),
            data: bytes,
        }) {
            eprintln!("⚠️ [TERRAIN SYNC] Failed to broadcast manifest: {}", e);
        }
        Ok(())
    }

    /// Receive handler for chunk-manifest messages.
    /// Queues manifest for the game loop to process (it has access to chunk_streamer).
    fn handle_chunk_manifest(&mut self, _peer_id: PeerId, data: &[u8]) -> Result<()> {
        let manifest = ChunkManifest::from_bytes(data)?;
        println!("📋 [TERRAIN SYNC] Received manifest with {} entries", manifest.entries.len());
        self.pending_chunk_manifests.push(manifest.entries);
        Ok(())
    }

    /// Handle an incoming key-registry gossipsub message.
    ///
    /// Deserializes a [`KeyRegistryMessage`] and applies each contained
    /// `KeyRecord` to the local registry. Invalid or stale records are silently
    /// ignored (the registry logs stats internally).
    fn handle_key_registry(&mut self, peer_id: PeerId, data: &[u8]) {
        let msg: KeyRegistryMessage = match bincode::deserialize(data) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("🔑 [KeyRegistry] Failed to deserialize message from {}: {}", peer_id, e);
                return;
            }
        };
        let records = match msg {
            KeyRegistryMessage::Publish(record) => vec![record],
            KeyRegistryMessage::Batch(records) => records,
            KeyRegistryMessage::Revocation { .. } => {
                // Revocations should arrive on TOPIC_KEY_REVOCATIONS, but handle gracefully
                self.handle_key_revocation(peer_id, data);
                return;
            }
        };
        for record in records {
            match self.key_registry.apply_update(record) {
                Ok(true)  => {} // accepted new/updated record — no log spam
                Ok(false) => {} // idempotent re-insert
                Err(e)    => eprintln!("🔑 [KeyRegistry] Rejected record from {}: {}", peer_id, e),
            }
        }
    }

    /// Handle an incoming key-revocations gossipsub message.
    ///
    /// Verifies the Ed25519 signature on the revocation notice, checks that the
    /// revoker has authority (Admin/Server/Genesis/Relay or self-revocation), then
    /// calls `KeyRegistry::mark_revoked()` and emits a `NetworkEvent::KeyRevoked`.
    fn handle_key_revocation(&mut self, peer_id: PeerId, data: &[u8]) {
        use ed25519_dalek::{VerifyingKey, Signature as DalekSig, Verifier};

        let msg: KeyRegistryMessage = match bincode::deserialize(data) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("🔑 [KeyRevocations] Failed to deserialize from {}: {}", peer_id, e);
                return;
            }
        };
        let (target_bytes, revoker_bytes, reason, revoked_at_ms, sig, revoker_pub) = match msg {
            KeyRegistryMessage::Revocation {
                target_peer_id_bytes, revoker_peer_id_bytes,
                reason, revoked_at_ms, sig, revoker_public_key,
            } => (target_peer_id_bytes, revoker_peer_id_bytes, reason, revoked_at_ms, sig, revoker_public_key),
            _ => {
                eprintln!("🔑 [KeyRevocations] Unexpected message type on revocations topic from {}", peer_id);
                return;
            }
        };

        // Verify Ed25519 signature
        let vk = match VerifyingKey::from_bytes(&revoker_pub) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("🔑 [KeyRevocations] Invalid revoker public key from {}", peer_id);
                return;
            }
        };
        let signable = revocation_signable_bytes(&target_bytes, &revoker_bytes, reason.as_deref(), revoked_at_ms);
        let signature = DalekSig::from_bytes(&sig);
        if vk.verify(&signable, &signature).is_err() {
            eprintln!("🔑 [KeyRevocations] Invalid signature from {}", peer_id);
            return;
        }

        // Decode PeerIds
        let Ok(target_peer_id) = PeerId::from_bytes(&target_bytes) else {
            eprintln!("🔑 [KeyRevocations] Invalid target PeerId from {}", peer_id);
            return;
        };
        let Ok(revoker_peer_id) = PeerId::from_bytes(&revoker_bytes) else {
            eprintln!("🔑 [KeyRevocations] Invalid revoker PeerId from {}", peer_id);
            return;
        };

        // Authority check: self-revocation OR Admin/Server/Genesis/Relay key type
        let is_self_revoke = revoker_peer_id == target_peer_id;
        let has_authority = is_self_revoke || {
            use crate::identity::KeyType;
            let ktype = self.key_registry.get_or_default(revoker_peer_id).key_type;
            matches!(ktype, KeyType::Admin | KeyType::Server | KeyType::Genesis | KeyType::Relay)
        };
        if !has_authority {
            eprintln!("🔑 [KeyRevocations] Revoker {} has no authority to revoke {}", revoker_peer_id, target_peer_id);
            return;
        }

        if self.key_registry.mark_revoked(&target_peer_id, &revoker_peer_id, reason, revoked_at_ms) {
            println!("🔑 [KeyRevocations] Revoked key for {} (by {})", target_peer_id, revoker_peer_id);
            self.pending_revocations.push((target_peer_id, revoker_peer_id));
        }
    }

    /// Handle a compact player state message (degraded/LoRa mode).
    ///
    /// Translates the `u16` session ID back to a `PeerId` and emits a
    /// `PlayerState` event as if a full `PlayerStateMessage` had arrived.
    fn handle_compact_player_state(&mut self, _peer_id: PeerId, data: &[u8]) -> Result<()> {
        let compact = CompactPlayerState::from_bytes(data)
            .map_err(|e| MultiplayerError::Message(e.into()))?;
        let Some(&resolved_peer) = self.session_id_map.get(&compact.session_id) else {
            // Unknown session ID — could be a new peer we haven't seen a full message from yet.
            // Silently drop; they'll send a full message soon.
            return Ok(());
        };
        // Reconstruct a minimal PlayerStateMessage for the state manager.
        let full = PlayerStateMessage::new(
            resolved_peer,
            crate::coordinates::ECEF::new(
                compact.position[0] as f64,
                compact.position[1] as f64,
                compact.position[2] as f64,
            ),
            [0.0, 0.0, 0.0], // velocity not carried in compact form
            compact.rotation[0],
            compact.rotation[1],
            MovementMode::Walk,
            compact.timestamp_ms as u64,
        );
        self.remote_players.handle_message(full);
        self.stats.player_states_received += 1;
        Ok(())
    }

    /// Assign the deterministic 2-byte compact token for a peer.
    /// All peers derive the same token for the same PeerId — no coordination needed.
    fn assign_session_id(&mut self, peer: PeerId) -> u16 {
        if let Some(&existing) = self.session_ids.get(&peer) {
            return existing;
        }
        let token = peer_id_to_token(&peer);
        self.session_ids.insert(peer, token);
        self.session_id_map.insert(token, peer);
        token
    }

    /// Get the session ID for a peer, if one has been assigned.
    pub fn session_id_for_peer(&self, peer: &PeerId) -> Option<u16> {
        self.session_ids.get(peer).copied()
    }

    /// Get the `PeerId` for a session ID, if known.
    pub fn peer_for_session_id(&self, session_id: u16) -> Option<&PeerId> {
        self.session_id_map.get(&session_id)
    }

    /// Drain any pending key revocations and return them.
    ///
    /// The game loop should call this every frame to handle revocations
    /// (e.g., kick revoked players, clear their parcel rights, etc.).
    pub fn take_key_revocations(&mut self) -> Vec<(PeerId, PeerId)> {
        std::mem::take(&mut self.pending_revocations)
    }

    /// Publish a compact player state update (for degraded / LoRa links).
    ///
    /// Only works if a session ID has been assigned for the remote peer via
    /// `assign_session_id()`.  In normal operation the full `PlayerStateMessage`
    /// is also sent; on very constrained links only compact is sent.
    pub fn publish_compact_player_state(
        &self,
        session_id: u16,
        position: [f32; 3],
        rotation: [f32; 2],
    ) {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u32;
        let compact = CompactPlayerState { session_id, position, rotation, timestamp_ms: now_ms };
        let Ok(data) = compact.to_bytes() else { return };
        let _ = self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_PLAYER_STATE_COMPACT.to_string(),
            data,
        });
    }

    /// Publish our own `KeyRecord` to the key-registry gossipsub topic.
    ///
    /// Called on every `PeerConnected` event and at startup. Ensures all peers
    /// in the network can identify us and check our key type.
    fn publish_own_key_record(&self) {
        let record = match self.identity.load_key_record() {
            Some(r) => r,
            None => {
                // No .keyrec file yet — create a Guest record.
                // ALL key types (including Guest) are published so that report/ban/invite/msg
                // work regardless of key type. Guest keys carry key_type=Guest so peers
                // know to apply Guest-level permissions.
                self.identity.create_key_record(
                    crate::identity::KeyType::Guest,
                    None, None, None, None, None,
                )
            }
        };
        let msg = KeyRegistryMessage::Publish(record);
        let data = match bincode::serialize(&msg) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("🔑 [KeyRegistry] Failed to serialize own KeyRecord: {}", e);
                return;
            }
        };
        let _ = self.cmd_tx.send(NetworkCommand::Publish {
            topic: TOPIC_KEY_REGISTRY.to_string(),
            data,
        });

        // Also push to the DHT so newly joining peers that miss the gossipsub
        // announcement can still discover our KeyRecord.
        if let KeyRegistryMessage::Publish(ref raw_record) = msg {
            if let Ok(record_bytes) = raw_record.to_bytes() {
                let dht_key = Self::peer_dht_key(&self.local_peer_id);
                let _ = self.cmd_tx.send(NetworkCommand::PutDhtRecord {
                    key: dht_key.clone(),
                    value: record_bytes,
                });
                let _ = self.cmd_tx.send(NetworkCommand::StartProvidingKey {
                    key: dht_key,
                });
            }
        }
    }

    /// Derive the deterministic Kademlia key for a peer's KeyRecord.
    /// SHA-256 of the peer's raw PeerId bytes gives a 32-byte DHT key.
    fn peer_dht_key(peer_id: &PeerId) -> Vec<u8> {
        Sha256::digest(peer_id.to_bytes()).to_vec()
    }

    /// Request a peer's KeyRecord from the DHT.
    /// The result is returned later as a `NetworkEvent::DhtRecordFound` and handled in
    /// `handle_network_event()`.
    pub fn request_dht_key_lookup(&self, peer_id: &PeerId) {
        let key = Self::peer_dht_key(peer_id);
        let _ = self.cmd_tx.send(NetworkCommand::GetDhtRecord { key });
    }

    /// Publish this client's `NodeCapabilities` to the DHT.
    ///
    /// Advertises the client as a `NodeTier::Client` with an optional storage
    /// budget so servers and other peers can route chunk fetches to it.
    pub fn publish_node_capabilities(&self, storage_budget_gb: u64) {
        use crate::node_capabilities::NodeCapabilities;
        let caps = NodeCapabilities::for_client(storage_budget_gb);
        let key = NodeCapabilities::dht_key(&self.local_peer_id.to_string());
        let _ = self.cmd_tx.send(NetworkCommand::PutDhtRecord { key, value: caps.to_bytes() });
    }

    /// Returns a lightweight tile fetcher handle that terrain workers can use.
    pub fn tile_fetcher(&self) -> TileFetcher {
        TileFetcher {
            cmd_tx: self.cmd_tx.clone(),
            known_tile_servers: Arc::clone(&self.known_tile_servers),
        }
    }

    /// Announce that this node has an OSM tile cached — other peers can find and fetch it.
    pub fn announce_osm_tile(&self, s: f64, w: f64, n: f64, e: f64) {
        let key = crate::osm::osm_dht_key(s, w, n, e);
        let _ = self.cmd_tx.send(NetworkCommand::StartProvidingKey { key });
    }

    /// Scan a local OSM cache directory and announce every tile already on disk.
    /// Call once on startup so peers can find tiles this node caches.
    pub fn announce_cached_osm_tiles(&self, cache_dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(cache_dir) else { return };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // filenames: osm_S_W_N_E.bin  e.g. osm_-27.4600_153.0300_-27.4500_153.0400.bin
            if !name.starts_with("osm_") || !name.ends_with(".bin") { continue; }
            let inner = &name[4..name.len()-4]; // strip "osm_" prefix and ".bin"
            let parts: Vec<&str> = inner.split('_').collect();
            // parts may be like ["", "27.4600", "153.0300", "", "27.4500", "153.0400"]
            // because negative numbers produce extra '_' splits; re-join carefully
            if let Some((s, w, n, e)) = parse_osm_filename_coords(inner) {
                self.announce_osm_tile(s, w, n, e);
            }
        }
    }

    /// Announce that this node has an elevation tile cached.
    pub fn announce_elevation_tile(&self, lat: i32, lon: i32) {
        let key = crate::elevation::elevation_dht_key(lat, lon);
        let _ = self.cmd_tx.send(NetworkCommand::StartProvidingKey { key });
    }

    /// Scan a local elevation cache directory and announce every tile already on disk.
    /// Call once at startup after the network is running.
    pub fn announce_cached_elevation_tiles(&self, cache_dir: &std::path::Path) {
        // Files are at: cache_dir/N{lat}/E{lon}/srtm_n{lat}_e{lon}.tif
        if let Ok(entries) = std::fs::read_dir(cache_dir) {
            for lat_dir in entries.flatten() {
                let lat_name = lat_dir.file_name().to_string_lossy().to_string();
                let lat: i32 = if let Some(n) = lat_name.strip_prefix('N') {
                    n.parse().unwrap_or(i32::MAX)
                } else if let Some(s) = lat_name.strip_prefix('S') {
                    -(s.parse::<i32>().unwrap_or(i32::MAX))
                } else { continue };
                if lat == i32::MAX { continue; }
                if let Ok(lon_dirs) = std::fs::read_dir(lat_dir.path()) {
                    for lon_dir in lon_dirs.flatten() {
                        let lon_name = lon_dir.file_name().to_string_lossy().to_string();
                        let lon: i32 = if let Some(e) = lon_name.strip_prefix('E') {
                            e.parse().unwrap_or(i32::MAX)
                        } else if let Some(w) = lon_name.strip_prefix('W') {
                            -(w.parse::<i32>().unwrap_or(i32::MAX))
                        } else { continue };
                        if lon == i32::MAX { continue; }
                        self.announce_elevation_tile(lat, lon);
                    }
                }
            }
        }
    }

    // ── Meshsite content ──────────────────────────────────────────────────────

    /// Publish a signed `ContentItem` to the gossipsub mesh.
    ///
    /// The item propagates to every subscribed node (server, client, relay).
    /// Each receiver stores it locally and re-puts it into the DHT.
    pub fn publish_content(&mut self, item: &crate::meshsite::ContentItem) {
        use crate::meshsite::topic_for_section;
        let topic  = topic_for_section(&item.section).to_string();
        let data   = item.to_bytes();
        let dht_k  = item.dht_key();
        let dht_v  = data.clone();
        // Gossip so live peers receive it immediately
        let _ = self.cmd_tx.send(NetworkCommand::Publish { topic, data });
        // DHT put for offline persistence / late-joiners
        let _ = self.cmd_tx.send(NetworkCommand::PutDhtRecord { key: dht_k, value: dht_v });
        // Add to local cache immediately so the poster sees it without network roundtrip
        let section = item.section.as_str().to_string();
        let cache = self.content_cache.entry(section).or_default();
        if !cache.iter().any(|c| c.id == item.id) {
            cache.insert(0, item.clone());
            if cache.len() > 200 { cache.truncate(200); }
        }
    }

    /// Return cached content for a section, newest-first.
    pub fn get_content(&self, section: &str) -> &[crate::meshsite::ContentItem] {
        self.content_cache.get(section).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Inject content items directly into the cache (e.g. from a server HTTP fetch).
    /// Deduplicates by id. Existing items are not replaced.
    pub fn inject_content(&mut self, items: Vec<crate::meshsite::ContentItem>) {
        for item in items {
            let section = item.section.as_str().to_string();
            let cache = self.content_cache.entry(section).or_default();
            if !cache.iter().any(|c| c.id == item.id) {
                cache.push(item);
            }
        }
        // Sort each section newest-first
        for cache in self.content_cache.values_mut() {
            cache.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            if cache.len() > 200 { cache.truncate(200); }
        }
    }

    // ── World placed objects ──────────────────────────────────────────────────

    /// Request world objects for a chunk from the DHT.
    ///
    /// Results arrive asynchronously as a `DhtRecordFound` event and are stored
    /// in `world_objects_cache`.  Call [`get_world_objects`] after the next
    /// [`update`] call to read them.
    pub fn fetch_world_objects_for_chunk(&self, cx: i32, cz: i32) {
        let key = crate::world_objects::chunk_dht_key(cx, cz);
        let _ = self.cmd_tx.send(NetworkCommand::GetDhtRecord { key });
    }

    /// Request world objects for all chunks within `radius_chunks` of (cx, cz).
    pub fn fetch_world_objects_for_area(&self, cx: i32, cz: i32, radius_chunks: i32) {
        for dx in -radius_chunks..=radius_chunks {
            for dz in -radius_chunks..=radius_chunks {
                self.fetch_world_objects_for_chunk(cx + dx, cz + dz);
            }
        }
    }

    /// Return cached placed objects for a chunk (if available).
    pub fn get_world_objects(&self, cx: i32, cz: i32) -> &[crate::world_objects::PlacedObject] {
        self.world_objects_cache.get(&(cx, cz)).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Return all cached placed objects across all loaded chunks.
    pub fn all_world_objects(&self) -> impl Iterator<Item = &crate::world_objects::PlacedObject> {
        self.world_objects_cache.values().flat_map(|v| v.iter())
    }

    /// Insert an inferred object into the cache without overwriting existing DHT objects.
    /// Key is the chunk grid cell for the object's (x, z) position.
    pub fn register_inferred_object(&mut self, obj: crate::world_objects::PlacedObject) {
        use crate::world_objects::chunk_coords_for_pos;
        let (cx, cz) = chunk_coords_for_pos(obj.position[0], obj.position[2]);
        let bucket = self.world_objects_cache.entry((cx, cz)).or_default();
        if !bucket.iter().any(|o| o.id == obj.id) {
            bucket.push(obj);
        }
    }

    /// Handle an incoming meshsite content message from gossipsub.
    fn handle_meshsite_content(&mut self, data: &[u8]) {
        use crate::meshsite::ContentItem;
        let item = match ContentItem::from_bytes(data) {
            Some(i) => i,
            None => return,
        };
        if !item.id_valid() {
            return; // id doesn't match sha256 of fields — discard
        }
        let section = item.section.as_str().to_string();
        let cache = self.content_cache.entry(section).or_default();
        // Deduplicate by id
        if cache.iter().any(|c| c.id == item.id) {
            return;
        }
        cache.insert(0, item.clone()); // newest first
        if cache.len() > 200 {
            cache.truncate(200);
        }
        // Also persist to DHT so late-joining nodes can fetch it
        let _ = self.cmd_tx.send(NetworkCommand::PutDhtRecord {
            key: item.dht_key(),
            value: data.to_vec(),
        });
    }

    /// Publish a `PlayerSessionRecord` to the DHT on clean logout.
    ///
    /// Signs the record with the player's identity key and stores it at the
    /// well-known session DHT key.  Any machine that loads the same `identity.key`
    /// can fetch this record on startup and resume from the exact logout position.
    pub fn publish_session_record(
        &self,
        position: [f64; 3],
        rotation: [f32; 2],
        movement_mode: u8,
        chunk_id: [i64; 3],
    ) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let public_key = self.identity.verifying_key().to_bytes();
        let mut record = PlayerSessionRecord {
            version: 1,
            peer_id: self.local_peer_id,
            position,
            rotation,
            movement_mode,
            chunk_id,
            logged_out_at: now,
            public_key,
            signature: [0u8; 64],
        };

        // Sign the canonical bytes
        let sig = self.identity.sign(&record.signable_bytes());
        record.signature = sig.to_bytes();

        match record.to_bytes() {
            Ok(bytes) => {
                let dht_key = PlayerSessionRecord::dht_key(&self.local_peer_id);
                let _ = self.cmd_tx.send(NetworkCommand::PutDhtRecord {
                    key: dht_key,
                    value: bytes,
                });
                println!("📍 [Session] Published session record to DHT");
            }
            Err(e) => eprintln!("📍 [Session] Failed to serialize session record: {}", e),
        }
    }

    /// Fetch our own session record from the DHT on startup.
    ///
    /// Call this early in startup when no local save exists (new machine).
    /// The result arrives asynchronously via `take_pending_session_record()`.
    pub fn fetch_session_record(&self) {
        let key = PlayerSessionRecord::dht_key(&self.local_peer_id);
        let _ = self.cmd_tx.send(NetworkCommand::GetDhtRecord { key });
        println!("📍 [Session] Requesting last session from DHT...");
    }

    /// Take the pending session record (if one arrived from DHT).
    ///
    /// Returns `Some(record)` once and then `None` on subsequent calls.
    /// The game loop should call this after connection and use it to restore
    /// the player's position if no local save file was found.
    pub fn take_pending_session_record(&mut self) -> Option<PlayerSessionRecord> {
        self.pending_session_record.take()
    }

    /// Advertise to the DHT that we are a provider for the given chunks.
    ///
    /// Call this:
    /// - At startup after loading chunks from disk
    /// - After applying a voxel edit (so peers can find the updated chunk)
    ///
    /// This lets other peers discover us via `find_chunk_providers()` even
    /// when we are not connected to them via gossipsub.
    pub fn advertise_chunks(&self, chunk_ids: &[ChunkId]) {
        for chunk_id in chunk_ids {
            let _ = self.cmd_tx.send(NetworkCommand::StartProvidingKey {
                key: chunk_id.dht_key(),
            });
        }
    }

    /// Ask the DHT who has a specific chunk.
    ///
    /// Results arrive as `NetworkEvent::ChunkProvidersFound` and are accessible
    /// via `take_pending_chunk_providers()`.
    pub fn find_chunk_providers(&self, chunk_id: &ChunkId) {
        let _ = self.cmd_tx.send(NetworkCommand::GetProviders {
            key: chunk_id.dht_key(),
        });
    }

    /// Query DHT providers for all the given chunks.
    ///
    /// Call this as a fallback after gossipsub sync hasn't delivered ops for
    /// some chunks (e.g. 10s after entering OpenWorld with no connected peers).
    /// Results arrive via `take_pending_chunk_providers()`.
    pub fn query_missing_chunks(&self, chunk_ids: &[ChunkId]) {
        for chunk_id in chunk_ids {
            let _ = self.cmd_tx.send(NetworkCommand::GetProviders {
                key: chunk_id.dht_key(),
            });
        }
        if !chunk_ids.is_empty() {
            println!("🔍 [DHT] Querying providers for {} missing chunk(s)", chunk_ids.len());
        }
    }

    /// Attempt to connect to a provider peer discovered via DHT.
    ///
    /// Uses the Kademlia routing table to resolve their address. If the peer
    /// is not yet in the routing table this is a no-op — they will be connected
    /// automatically if they appear via DHT routing updates later.
    pub fn connect_to_provider(&self, peer_id: PeerId) {
        let _ = self.cmd_tx.send(NetworkCommand::DialPeer { peer_id });
    }

    /// Take any pending provider results from DHT queries.
    /// Returns `(dht_key, providers)` pairs.
    pub fn take_pending_chunk_providers(&mut self) -> Vec<(Vec<u8>, Vec<PeerId>)> {
        std::mem::take(&mut self.pending_chunk_providers)
    }

    /// Take pending manifests for the game loop to process.
    /// Game loop compares against its own chunk timestamps and sends newer chunks.
    pub fn take_pending_chunk_manifests(&mut self) -> Vec<Vec<(ChunkId, u64)>> {
        std::mem::take(&mut self.pending_chunk_manifests)
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
            topic: TOPIC_STATE_REQUEST.to_string(),
            data,
        }).map_err(|_| MultiplayerError::ChannelSendError)?;
        
        // Mark all connected peers as requested
        for peer_id in &self.connected_peers {
            self.state_requested_from.insert(*peer_id);
        }
        
        self.stats.state_requests_sent += 1;
        
        println!("📡 Requested state for {} chunks from {} peers", 
            chunk_ids.len(), self.connected_peers.len());
        
        Ok(())
    }
    
    /// Check if there are new peers we should request state from
    ///
    /// Returns true if there are peers we haven't requested state from yet.
    /// Game loop should call request_chunk_state() when this returns true.
    pub fn has_new_peers(&self) -> bool {
        self.connected_peers.iter().any(|p| !self.state_requested_from.contains(p))
    }
    
    /// Get list of peers we haven't requested state from
    pub fn get_new_peers(&self) -> Vec<PeerId> {
        self.connected_peers.iter()
            .filter(|p| !self.state_requested_from.contains(p))
            .copied()
            .collect()
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

    /// Check if a peer is currently connected
    pub fn is_connected_peer(&self, peer_id: &PeerId) -> bool {
        self.connected_peers.contains(peer_id)
    }
    
    /// Check if a peer is blocked
    pub fn is_peer_blocked(&self, peer_id: &PeerId) -> bool {
        self.blocked_peers.contains(peer_id)
    }
}

/// Derive a stable 2-byte compact token from a PeerId.
///
/// XOR-folds all bytes of the multihash into a u16. Deterministic and consistent
/// across all peers — no coordination or server assignment required.
/// The only pathological case is a u16 collision (1 in 65535 chance per additional peer),
/// which is fine for expected group sizes (< 100 simultaneous neighbours).
fn peer_id_to_token(peer_id: &PeerId) -> u16 {
    let bytes = peer_id.to_bytes();
    let mut h: u32 = 0x5A5A_5A5A;
    for chunk in bytes.chunks(4) {
        let mut arr = [0u8; 4];
        arr[..chunk.len()].copy_from_slice(chunk);
        h ^= u32::from_le_bytes(arr);
        h = h.wrapping_mul(0x9E37_79B9); // knuth multiplicative hash step
    }
    let token = ((h ^ (h >> 16)) as u16).max(1); // never return 0
    token
}

/// Parse `s, w, n, e` from the inner part of an OSM cache filename.
/// Input: the part between "osm_" and ".bin", e.g. "-27.4600_153.0300_-27.4500_153.0400".
/// Returns None if parsing fails.
fn parse_osm_filename_coords(inner: &str) -> Option<(f64, f64, f64, f64)> {
    // The format is S_W_N_E where each value may start with '-'.
    // We split on '_' but negative values mean the first char after a separator is '-'.
    // Strategy: find the four numbers by splitting on '_' while allowing '-' after '_'.
    let mut nums = Vec::with_capacity(4);
    let mut current = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if let Ok(v) = current.parse::<f64>() { nums.push(v); }
            current.clear();
            // If next char is '-', consume it as part of the next number
            if chars.peek() == Some(&'-') {
                current.push(chars.next().unwrap());
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        if let Ok(v) = current.parse::<f64>() { nums.push(v); }
    }
    if nums.len() == 4 { Some((nums[0], nums[1], nums[2], nums[3])) } else { None }
}

/// Background network thread runner
///
/// Runs in a dedicated thread with tokio runtime.
/// Processes commands from main thread and sends events back.
fn run_network_thread(
    identity: Identity,
    cmd_rx: Receiver<NetworkCommand>,
    event_tx: Sender<NetworkEvent>,
    world_data_dir: std::path::PathBuf,
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
            Ok(mut n) => {
                n.set_tile_cache_root(world_data_dir);
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
        if let Err(e) = network.subscribe(TOPIC_STATE_REQUEST) {
            eprintln!("Failed to subscribe to state-request: {}", e);
        } else {
            println!("📻 Subscribed to topic: state-request");
        }
        if let Err(e) = network.subscribe(TOPIC_STATE_RESPONSE) {
            eprintln!("Failed to subscribe to state-response: {}", e);
        } else {
            println!("📻 Subscribed to topic: state-response");
        }

        if let Err(e) = network.subscribe(TOPIC_CHUNK_TERRAIN) {
            eprintln!("Failed to subscribe to chunk-terrain: {}", e);
        } else {
            println!("📻 Subscribed to topic: chunk-terrain");
        }

        if let Err(e) = network.subscribe(TOPIC_CHUNK_MANIFEST) {
            eprintln!("Failed to subscribe to chunk-manifest: {}", e);
        } else {
            println!("📻 Subscribed to topic: chunk-manifest");
        }

        if let Err(e) = network.subscribe(TOPIC_KEY_REGISTRY) {
            eprintln!("Failed to subscribe to key-registry: {}", e);
        } else {
            println!("📻 Subscribed to topic: key-registry");
        }

        if let Err(e) = network.subscribe(TOPIC_KEY_REVOCATIONS) {
            eprintln!("Failed to subscribe to key-revocations: {}", e);
        } else {
            println!("📻 Subscribed to topic: key-revocations");
        }

        if let Err(e) = network.subscribe(TOPIC_PLAYER_STATE_COMPACT) {
            eprintln!("Failed to subscribe to player-state-compact: {}", e);
        } else {
            println!("📻 Subscribed to topic: player-state-compact");
        }

        for topic in crate::meshsite::MESHSITE_TOPICS {
            if let Err(e) = network.subscribe(topic) {
                eprintln!("Failed to subscribe to {}: {}", topic, e);
            } else {
                println!("📻 Subscribed to topic: {}", topic);
            }
        }
        
        println!("🔍 Network thread started - polling for mDNS and connections...");
        println!("🔧 [Network Thread] About to enter tokio::select! loop...");

        // Start listening then connect to bootstrap nodes
        if let Err(e) = network.listen_on("/ip4/0.0.0.0/tcp/0") {
            eprintln!("Failed to listen on TCP: {}", e);
        }
        if let Err(e) = network.listen_on("/ip4/0.0.0.0/udp/0/quic-v1") {
            eprintln!("Failed to listen on QUIC: {}", e);
        }
        println!("🌐 [Network Thread] Fetching bootstrap nodes...");
        network.connect_to_bootstrap().await;
        println!("🌐 [Network Thread] Bootstrap dial initiated");

        let mut heartbeat_counter = 0u64;
        let mut last_peer_seen = tokio::time::Instant::now();
        let mut last_reconnect = tokio::time::Instant::now();
        // Queue for retrying failed publishes (voxel ops that failed due to no mesh peers)
        let mut publish_retry_queue: Vec<(String, Vec<u8>, tokio::time::Instant)> = Vec::new();
        
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
                    NetworkCommand::SubscribeBulk { topics } => {
                        for topic in topics {
                            if let Err(e) = network.subscribe(&topic) {
                                eprintln!("Failed to subscribe to {}: {}", topic, e);
                            }
                        }
                    }
                    NetworkCommand::UnsubscribeBulk { topics } => {
                        for topic in topics {
                            if let Err(e) = network.unsubscribe(&topic) {
                                eprintln!("Failed to unsubscribe from {}: {}", topic, e);
                            }
                        }
                    }
                    NetworkCommand::Publish { topic, data } => {
                        match network.publish(&topic, data.clone()) {
                            Ok(()) => {
                                // Delivered - remove from retry queue if it was there
                            }
                            Err(e) => {
                                let e_str = e.to_string();
                                // Queue voxel ops for retry - they're one-shot and must not be lost
                                if topic == "voxel-ops" && 
                                   (e_str.contains("InsufficientPeers") || e_str.contains("NoPeers")) {
                                    println!("⚠️  [NETWORK] voxel-op publish failed ({}), queuing retry", e_str);
                                    publish_retry_queue.push((topic, data, tokio::time::Instant::now()));
                                } else if !e_str.contains("NoPeers") && !e_str.contains("AllQueuesFull") {
                                    // AllQueuesFull is transient gossipsub congestion — not an error
                                    eprintln!("Failed to publish to {}: {}", topic, e_str);
                                }
                            }
                        }
                    }
                    NetworkCommand::Shutdown => {
                        println!("Network thread shutting down");
                        return;
                    }
                    NetworkCommand::PutDhtRecord { key, value } => {
                        network.put_dht_record(key, value);
                    }
                    NetworkCommand::StartProvidingKey { key } => {
                        network.start_providing_key(key);
                    }
                    NetworkCommand::GetDhtRecord { key } => {
                        network.get_dht_record(key);
                    }
                    NetworkCommand::GetProviders { key } => {
                        network.get_providers(key);
                    }
                    NetworkCommand::DialPeer { peer_id } => {
                        network.dial_peer_id(peer_id);
                    }
                    NetworkCommand::RequestTile { peer_id, request, response_tx } => {
                        network.request_tile(peer_id, request, response_tx);
                    }
                }
            }
            
            // Now poll the network for events
            while let Some(event) = network.poll() {
                match &event {
                    NetworkEvent::PeerDiscovered { peer_id } => {
                        println!("🔍 [Network Thread] mDNS discovered peer: {}", peer_id);
                        last_peer_seen = tokio::time::Instant::now();
                    }
                    NetworkEvent::PeerConnected { peer_id, .. } => {
                        println!("🔗 [Network Thread] Peer connected: {}", peer_id);
                        last_peer_seen = tokio::time::Instant::now();
                    }
                    NetworkEvent::ListeningOn { address } => {
                        println!("👂 Listening on: {}", address);
                    }
                    _ => {}
                }
                
                let _ = event_tx.try_send(event);
            }

            // Auto-reconnect: only re-bootstrap when fully isolated (no relay connections either).
            // If we have a relay connection, circuits to other players will form via DHT — no need
            // to hammer bootstrap every 10s just because no direct game peers are visible yet.
            let no_game_peers = network.game_peer_count() == 0;
            let no_relay_peers = network.connected_peer_count() == 0;
            let time_since_peer = last_peer_seen.elapsed().as_secs();
            let time_since_reconnect = last_reconnect.elapsed().as_secs();
            let reconnect_interval = if no_relay_peers { 10 } else { 60 };
            if no_game_peers && no_relay_peers && time_since_peer > 5 && time_since_reconnect > reconnect_interval {
                println!("🔄 [Network] Fully isolated for {}s, reconnecting...", time_since_peer);
                network.connect_to_bootstrap().await;
                last_reconnect = tokio::time::Instant::now();
            }

            // Retry queued voxel ops - retry every loop, give up after 30s
            if network.game_peer_count() > 0 && !publish_retry_queue.is_empty() {
                let now = tokio::time::Instant::now();
                publish_retry_queue.retain(|(_, _, queued_at)| {
                    if now.duration_since(*queued_at).as_secs() > 30 {
                        eprintln!("⚠️  [NETWORK] Dropping voxel-op after 30s retry timeout");
                        return false; // drop
                    }
                    true // keep
                });
                // Try sending all queued ops
                let to_retry: Vec<_> = publish_retry_queue.drain(..).collect();
                for (topic, data, queued_at) in to_retry {
                    match network.publish(&topic, data.clone()) {
                        Ok(()) => println!("✅ [NETWORK] Retried voxel-op delivered after {}ms",
                            now.duration_since(queued_at).as_millis()),
                        Err(_) => publish_retry_queue.push((topic, data, queued_at)), // keep retrying
                    }
                }
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

/// Per-chunk gossipsub topic for player state (AOI).
/// Only peers subscribed to this chunk's topic receive position updates published here.
pub fn chunk_player_topic(id: &ChunkId) -> String {
    format!("player-state-{}-{}-{}", id.x, id.y, id.z)
}

/// Per-chunk gossipsub topic for voxel operations (AOI).
/// Only peers subscribed to this chunk's topic receive block-edit events published here.
pub fn chunk_voxel_topic(id: &ChunkId) -> String {
    format!("voxel-ops-{}-{}-{}", id.x, id.y, id.z)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_multiplayer_system_creation() {
        let identity = Identity::generate();
        let mp = MultiplayerSystem::new_with_runtime(identity);
        assert!(mp.is_ok());
        // Give background thread time to initialize
        std::thread::sleep(Duration::from_millis(100));
    }
    
    #[test]
    fn test_voxel_op_deduplication() {
        let identity = Identity::generate();
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
