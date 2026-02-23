//! Chunk lifecycle management for dynamic terrain loading
//!
//! Manages the loading, unloading, and updating of terrain chunks as the player moves.
//! Each chunk has its own octree, mesh, and collision state.
//!
//! # Architecture
//!
//! ```text
//! ChunkManager
//!   ├─> HashMap<ChunkId, ChunkData>
//!   │
//!   ├─> load_chunk()         → Generate terrain + load operations
//!   ├─> unload_chunk()       → Save operations + free memory
//!   ├─> update_visible()     → Load/unload based on player position
//!   └─> regenerate_dirty()   → Only regenerate modified chunks
//!
//! ChunkData (per chunk)
//!   ├─> octree: Octree       → Terrain voxels
//!   ├─> mesh_buffer: Option<MeshBuffer> → GPU mesh (lazy)
//!   ├─> collider: Option<ColliderHandle> → Physics (lazy)
//!   └─> dirty: bool          → Needs mesh regeneration
//! ```
//!
//! # Benefits
//!
//! - **Fixes "entire screen rerender"** - Only regenerate dirty chunks
//! - **Enables infinite world** - Load chunks on demand
//! - **Memory efficient** - Unload distant chunks
//! - **Foundation for LOD** - Different detail per chunk
//!
//! # Usage
//!
//! ```no_run
//! use metaverse_core::chunk_manager::ChunkManager;
//! use metaverse_core::chunk::ChunkId;
//! use metaverse_core::voxel::VoxelCoord;
//!
//! let mut manager = ChunkManager::new(terrain_generator, user_content);
//!
//! // In game loop
//! loop {
//!     // Update which chunks are loaded based on player position
//!     let player_chunk = ChunkId::from_voxel(&player_voxel);
//!     manager.update_visible_chunks(&player_chunk, 3); // 3 chunk radius
//!     
//!     // Dig voxel
//!     manager.set_voxel(voxel_coord, Material::Air);
//!     
//!     // Regenerate only dirty chunks
//!     manager.regenerate_dirty_chunks(&context.device);
//!     
//!     // Render all loaded chunks
//!     for chunk_data in manager.loaded_chunks() {
//!         if let Some(mesh) = &chunk_data.mesh_buffer {
//!             render_mesh(mesh);
//!         }
//!     }
//! }
//! ```

use crate::chunk::{ChunkId, chunks_in_radius, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};
use crate::messages::VoxelOperation;
use crate::renderer::MeshBuffer;
use crate::terrain::TerrainGenerator;
use crate::user_content::UserContentLayer;
use crate::vector_clock::VectorClock;
use crate::voxel::{Octree, VoxelCoord};
use crate::materials::MaterialId;
use rapier3d::prelude::ColliderHandle;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Per-chunk data (terrain, mesh, collision, state)
pub struct ChunkData {
    pub chunk_id: ChunkId,
    pub octree: Octree,
    pub mesh_buffer: Option<MeshBuffer>,  // GPU mesh
    pub collider: Option<ColliderHandle>,  // Physics collider
    pub dirty: bool,  // Needs mesh regeneration
}

impl ChunkData {
    pub fn new(chunk_id: ChunkId, octree: Octree) -> Self {
        Self {
            chunk_id,
            octree,
            mesh_buffer: None,
            collider: None,
            dirty: true,  // New chunks need mesh generation
        }
    }
}

/// Manages chunk loading, unloading, and lifecycle
pub struct ChunkManager {
    loaded_chunks: HashMap<ChunkId, ChunkData>,
    terrain_generator: TerrainGenerator,
    user_content: UserContentLayer,
    load_queue: Vec<ChunkId>,  // Chunks to load (gradual loading)
    unload_queue: Vec<ChunkId>,  // Chunks to unload (gradual unloading)
    /// Deduplication set for operations (by signature)
    /// Prevents applying the same operation multiple times from different sources
    seen_operations: std::collections::HashSet<[u8; 64]>,
    /// Per-voxel authority map: tracks the current winning operation for each
    /// modified voxel. Used for CRDT conflict resolution — when two concurrent
    /// edits target the same coordinate, only the winner is applied to the octree.
    voxel_authority: HashMap<VoxelCoord, VoxelOperation>,
}

impl ChunkManager {
    /// Create new chunk manager
    pub fn new(terrain_generator: TerrainGenerator, user_content: UserContentLayer) -> Self {
        Self {
            loaded_chunks: HashMap::new(),
            terrain_generator,
            user_content,
            load_queue: Vec::new(),
            unload_queue: Vec::new(),
            seen_operations: std::collections::HashSet::new(),
            voxel_authority: HashMap::new(),
        }
    }
    
    /// Load a chunk (generate terrain + load operations)
    ///
    /// Steps:
    /// 1. Generate base terrain from SRTM data
    /// 2. Load voxel operations from chunk file
    /// 3. Apply operations to octree
    /// 4. Mark dirty for mesh generation
    ///
    /// Returns Ok(()) if chunk loaded successfully.
    pub fn load_chunk(&mut self, chunk_id: ChunkId, world_dir: &Path) -> Result<(), String> {
        // Don't reload if already loaded
        if self.loaded_chunks.contains_key(&chunk_id) {
            return Ok(());
        }
        
        // 1. Generate base terrain
        let octree = self.terrain_generator.generate_chunk(&chunk_id)?;
        
        // 2. Create chunk data
        let mut chunk_data = ChunkData::new(chunk_id, octree);
        
        // 3. Load operations from file (modifies user_content internal state)
        let loaded_ops = match self.user_content.load_chunk(world_dir, &chunk_id) {
            Ok(count) => {
                if count > 0 {
                    println!("  {} loaded with {} operations", chunk_id, count);
                }
                count
            }
            Err(e) => {
                // File not existing is OK (no edits yet)
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!("Failed to load chunk {}: {}", chunk_id, e));
                }
                0
            }
        };
        
        // 4. Apply loaded operations to the chunk octree using CRDT authority
        //    For each coord, only the winning operation (per wins_over) is applied.
        if loaded_ops > 0 {
            // Collect ops for this chunk and resolve conflicts per coordinate
            let mut chunk_authority: HashMap<VoxelCoord, &crate::messages::VoxelOperation> = HashMap::new();
            for op in self.user_content.op_log() {
                if ChunkId::from_voxel(&op.coord) == chunk_id {
                    let wins = match chunk_authority.get(&op.coord) {
                        None => true,
                        Some(&current) => op.wins_over(current),
                    };
                    if wins {
                        chunk_authority.insert(op.coord, op);
                    }
                }
            }
            // Apply winners to octree and register in global authority map
            for (coord, op) in chunk_authority {
                chunk_data.octree.set_voxel(coord, op.material.to_material_id());
                chunk_data.dirty = true;
                // Merge into global authority (file load may have older data than already-merged
                // network ops — keep whichever wins globally)
                let global_wins = match self.voxel_authority.get(&coord) {
                    None => true,
                    Some(current) => op.wins_over(current),
                };
                if global_wins {
                    self.voxel_authority.insert(coord, op.clone());
                }
            }
        }
        
        // 5. Insert into loaded chunks
        self.loaded_chunks.insert(chunk_id, chunk_data);
        
        Ok(())
    }
    
    /// Unload a chunk (save operations + free memory)
    ///
    /// Saves any voxel operations to chunk file before unloading.
    /// Frees octree, mesh, and collider memory.
    pub fn unload_chunk(&mut self, chunk_id: &ChunkId, world_dir: &Path) -> Result<(), String> {
        if let Some(_chunk_data) = self.loaded_chunks.remove(chunk_id) {
            // Save operations (handled by UserContentLayer)
            // Note: Operations are saved globally, not per-chunk unload
            println!("  {} unloaded", chunk_id);
        }
        
        Ok(())
    }
    
    /// Load all chunks in radius immediately (for initialization)
    ///
    /// Unlike update_visible_chunks(), this loads all chunks at once
    /// without gradual loading. Use for initial world setup.
    ///
    /// # Arguments
    /// * `center_chunk` - Chunk to load around
    /// * `radius` - Chunk radius (e.g., 2 = 3×3×3 chunks with corners removed)
    pub fn load_chunks_immediate(&mut self, center_chunk: &ChunkId, radius: i64, world_dir: &Path) {
        let chunks_to_load = chunks_in_radius(center_chunk, radius);
        
        for chunk_id in chunks_to_load {
            if !self.loaded_chunks.contains_key(&chunk_id) {
                if let Err(e) = self.load_chunk(chunk_id, world_dir) {
                    eprintln!("Failed to load chunk {}: {}", chunk_id, e);
                }
            }
        }
    }
    
    /// Update which chunks are visible based on player position
    ///
    /// Loads chunks in radius, unloads chunks outside radius.
    /// Uses gradual loading (1-2 chunks per frame) to avoid stutter.
    ///
    /// # Arguments
    /// * `center_chunk` - Chunk player is currently in
    /// * `radius` - Chunk radius to keep loaded (e.g., 3 = 7×7×7 chunks)
    pub fn update_visible_chunks(&mut self, center_chunk: &ChunkId, radius: i64, world_dir: &Path) {
        let visible_chunks: HashSet<ChunkId> = chunks_in_radius(center_chunk, radius)
            .into_iter()
            .collect();
        
        // Find chunks to load (visible but not loaded)
        self.load_queue.clear();
        for chunk_id in &visible_chunks {
            if !self.loaded_chunks.contains_key(chunk_id) {
                self.load_queue.push(*chunk_id);
            }
        }
        
        // Find chunks to unload (loaded but not visible)
        self.unload_queue.clear();
        for chunk_id in self.loaded_chunks.keys() {
            if !visible_chunks.contains(chunk_id) {
                self.unload_queue.push(*chunk_id);
            }
        }
        
        // Gradual loading: Load up to 2 chunks per frame
        let chunks_to_load = self.load_queue.iter().take(2).cloned().collect::<Vec<_>>();
        for chunk_id in chunks_to_load {
            if let Err(e) = self.load_chunk(chunk_id, world_dir) {
                eprintln!("Failed to load chunk {}: {}", chunk_id, e);
            }
        }
        
        // Gradual unloading: Unload up to 2 chunks per frame
        let chunks_to_unload = self.unload_queue.iter().take(2).cloned().collect::<Vec<_>>();
        for chunk_id in chunks_to_unload {
            if let Err(e) = self.unload_chunk(&chunk_id, world_dir) {
                eprintln!("Failed to unload chunk {}: {}", chunk_id, e);
            }
        }
    }
    
    /// Set voxel material and mark affected chunks dirty
    ///
    /// # Cross-Chunk Boundary Handling
    ///
    /// When a voxel is modified at a chunk boundary, multiple chunks need
    /// to regenerate their meshes. This function:
    /// 1. Identifies all affected chunks (1-8 depending on position)
    /// 2. Updates voxel in all affected chunks' octrees
    /// 3. Marks all affected chunks as dirty for mesh regeneration
    ///
    /// This ensures visual consistency across chunk boundaries and proper
    /// data persistence for planet-scale worlds.
    ///
    /// Returns true if at least one chunk was updated.
    pub fn set_voxel(&mut self, coord: VoxelCoord, material: MaterialId) -> bool {
        // Find all chunks affected by this voxel change
        let affected_chunks = ChunkId::affected_by_voxel(&coord);
        
        let mut any_updated = false;
        
        for chunk_id in affected_chunks {
            if let Some(chunk_data) = self.loaded_chunks.get_mut(&chunk_id) {
                // Update voxel in this chunk's octree
                chunk_data.octree.set_voxel(coord, material);
                
                // Mark chunk as dirty for mesh regeneration
                chunk_data.dirty = true;
                
                any_updated = true;
            }
        }
        
        any_updated
    }
    
    /// Get voxel material
    ///
    /// Returns None if voxel is not in a loaded chunk.
    pub fn get_voxel(&self, coord: VoxelCoord) -> Option<MaterialId> {
        let chunk_id = ChunkId::from_voxel(&coord);
        
        self.loaded_chunks
            .get(&chunk_id)
            .map(|chunk_data| chunk_data.octree.get_voxel(coord))
    }
    
    /// Mark chunk as dirty (needs mesh regeneration)
    pub fn mark_dirty(&mut self, chunk_id: &ChunkId) {
        if let Some(chunk_data) = self.loaded_chunks.get_mut(chunk_id) {
            chunk_data.dirty = true;
        }
    }
    
    /// Get list of loaded chunks
    pub fn loaded_chunks(&self) -> impl Iterator<Item = &ChunkData> {
        self.loaded_chunks.values()
    }
    
    /// Get mutable access to loaded chunks
    pub fn loaded_chunks_mut(&mut self) -> impl Iterator<Item = &mut ChunkData> {
        self.loaded_chunks.values_mut()
    }
    
    /// Get specific chunk data
    pub fn get_chunk(&self, chunk_id: &ChunkId) -> Option<&ChunkData> {
        self.loaded_chunks.get(chunk_id)
    }
    
    /// Get mutable specific chunk data
    pub fn get_chunk_mut(&mut self, chunk_id: &ChunkId) -> Option<&mut ChunkData> {
        self.loaded_chunks.get_mut(chunk_id)
    }
    
    /// Get number of loaded chunks
    pub fn loaded_count(&self) -> usize {
        self.loaded_chunks.len()
    }
    
    /// Get number of chunks pending load
    pub fn pending_load_count(&self) -> usize {
        self.load_queue.len()
    }
    
    /// Get number of chunks pending unload
    pub fn pending_unload_count(&self) -> usize {
        self.unload_queue.len()
    }
    
    /// Save all loaded chunks to disk
    pub fn save_all_chunks(&mut self, world_dir: &Path) -> Result<(), String> {
        // Delegate to UserContentLayer which handles chunk-based saving
        self.user_content.save_chunks(world_dir)
            .map_err(|e| format!("Failed to save chunks: {}", e))?;
        Ok(())
    }
    
    /// Add a local voxel operation to the content layer
    ///
    /// This should be called after set_voxel() to ensure the operation
    /// is persisted to disk and registered as the authority for this coordinate.
    /// Includes deduplication check.
    pub fn add_operation(&mut self, op: VoxelOperation) {
        // Check for duplicates
        if self.seen_operations.contains(&op.signature) {
            return;
        }
        self.seen_operations.insert(op.signature);
        
        // Register as authority for this coordinate (local writes always
        // have a freshly incremented vector clock, so they always win over
        // any concurrent remote op we might have stored)
        self.voxel_authority.insert(op.coord, op.clone());
        
        self.user_content.add_local_operation(op);
    }
    
    /// Get list of loaded chunk IDs
    ///
    /// Used for requesting chunk state from peers. Only request chunks
    /// that are actually loaded to keep bandwidth usage reasonable.
    ///
    /// # Example
    /// ```rust
    /// let loaded_chunk_ids = chunk_manager.get_loaded_chunk_ids();
    /// multiplayer.request_chunk_state(loaded_chunk_ids)?;
    /// ```
    pub fn get_loaded_chunk_ids(&self) -> Vec<ChunkId> {
        self.loaded_chunks.keys().copied().collect()
    }
    
    /// Filter operations by chunk IDs
    ///
    /// Used when responding to state requests. Returns operations that
    /// modify the requested chunks.
    ///
    /// # Arguments
    /// * `chunk_ids` - List of chunks to filter for
    /// * `requester_clock` - Requester's vector clock (filter out ops they have)
    ///
    /// # Returns
    /// HashMap mapping chunk ID to list of operations for that chunk
    pub fn filter_operations_for_chunks(
        &self,
        chunk_ids: &[ChunkId],
        _requester_clock: &VectorClock, // Clock hint kept for API compat but not used for filtering
    ) -> HashMap<ChunkId, Vec<VoxelOperation>> {
        let mut result: HashMap<ChunkId, Vec<VoxelOperation>> = HashMap::new();
        
        // Get all operations from user content layer
        let all_ops = self.user_content.op_log();
        
        println!("🔍 Filtering from {} total operations for {} requested chunks", 
            all_ops.len(), chunk_ids.len());
        
        let chunk_set: std::collections::HashSet<&ChunkId> = chunk_ids.iter().collect();

        for op in all_ops {
            // NOTE: Do NOT filter by vector clock here. Vector clock "happens_after" only means
            // the requester has seen causal predecessors — NOT that they received this specific op.
            // Missed ops (packet loss) would never be recovered if we filter by clock.
            // Let the RECEIVER deduplicate by op signature instead (already done in handle_state_response).

            // Find all chunks affected by this operation
            let affected_chunks = ChunkId::affected_by_voxel(&op.coord);
            
            // Add to result if any affected chunk is in requested list
            for chunk_id in affected_chunks {
                if chunk_set.contains(&chunk_id) {
                    result.entry(chunk_id)
                        .or_insert_with(Vec::new)
                        .push(op.clone());
                    break; // Only add once per operation
                }
            }
        }
        
        println!("   → Filtered to {} operations across {} chunks", 
            result.values().map(|v| v.len()).sum::<usize>(),
            result.len());
        
        result
    }
    
    /// Merge received state operations into local state
    ///
    /// Called when receiving ChunkStateResponse from peer. Applies operations
    /// to appropriate chunks with deduplication and proper CRDT merge.
    ///
    /// # Arguments
    /// * `operations` - Operations to merge (already signature-verified)
    ///
    /// # Returns
    /// Number of operations actually applied (after deduplication and CRDT resolution)
    ///
    /// # Merge Strategy
    ///
    /// Each voxel coordinate is treated as a Last-Write-Wins (LWW) register.
    /// When two operations target the same coordinate, we pick the winner using:
    ///
    /// 1. **Vector clock ordering** — if one causally follows the other, it wins.
    /// 2. **Lamport timestamp** — higher timestamp wins when concurrent.
    /// 3. **PeerId tiebreak** — lexicographically larger PeerId wins when timestamps equal.
    ///
    /// This is deterministic: every node independently arrives at the same winner.
    ///
    /// # Example
    /// ```rust
    /// let state_ops = multiplayer.take_pending_state_operations();
    /// let applied = chunk_manager.merge_received_operations(state_ops);
    /// println!("Applied {} historical operations", applied);
    /// ```
    pub fn merge_received_operations(&mut self, operations: Vec<VoxelOperation>) -> usize {
        let mut applied_count = 0;
        let mut dirty_chunks = HashSet::new();

        for op in operations {
            // 1. Dedup by signature (handles exact duplicate retransmissions)
            if self.seen_operations.contains(&op.signature) {
                continue;
            }
            self.seen_operations.insert(op.signature);

            // 2. CRDT conflict resolution: does this op beat the current authority?
            let wins = match self.voxel_authority.get(&op.coord) {
                None => true,                    // No prior op → this wins by default
                Some(current) => op.wins_over(current),
            };

            if wins {
                // 3a. Update authority map
                self.voxel_authority.insert(op.coord, op.clone());

                // 3b. Apply to all loaded chunks that contain this voxel
                let affected_chunks = ChunkId::affected_by_voxel(&op.coord);
                let material_id = op.material.to_material_id();
                for chunk_id in affected_chunks {
                    if let Some(chunk_data) = self.loaded_chunks.get_mut(&chunk_id) {
                        chunk_data.octree.set_voxel(op.coord, material_id);
                        chunk_data.dirty = true;
                        dirty_chunks.insert(chunk_id);
                    }
                }

                applied_count += 1;
            }
            // Even if op doesn't win, add it to the log for CRDT history
            // (we need the full history for late-joining peers to replay)
            self.user_content.add_local_operation(op);
        }

        if !dirty_chunks.is_empty() {
            println!("   🔧 Marked {} chunks dirty for regeneration", dirty_chunks.len());
        }
        
        applied_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::{Material, VoxelOperation};
    use crate::vector_clock::VectorClock;
    use crate::voxel::VoxelCoord;
    use libp2p::PeerId;

    /// Build an unsigned VoxelOperation with a fixed (fake) signature derived from
    /// timestamp + author, so different ops have different signatures.
    fn make_op(
        coord: VoxelCoord,
        material: Material,
        author: PeerId,
        timestamp: u64,
        clock: VectorClock,
    ) -> VoxelOperation {
        let mut op = VoxelOperation::new(coord, material, author, timestamp, clock);
        // Fake signature: fill with timestamp bytes + author bytes for uniqueness
        let ts_bytes = timestamp.to_le_bytes();
        let au_bytes = author.to_bytes();
        for i in 0..8  { op.signature[i]    = ts_bytes[i % 8]; }
        for i in 0..39 { op.signature[8 + i] = if i < au_bytes.len() { au_bytes[i] } else { 0 }; }
        op
    }

    fn make_manager() -> ChunkManager {
        use crate::{elevation::ElevationPipeline, coordinates::GPS};
        use crate::terrain::TerrainGenerator;
        let terrain_gen = TerrainGenerator::new(
            ElevationPipeline::new(),
            GPS::new(0.0, 0.0, 0.0),
            VoxelCoord::new(0, 0, 0),
        );
        ChunkManager::new(terrain_gen, UserContentLayer::new())
    }

    // ── Basic merge ────────────────────────────────────────────────────────────

    #[test]
    fn test_merge_single_op_applied() {
        let mut mgr = make_manager();
        let peer = PeerId::random();
        let coord = VoxelCoord::new(100, 100, 100);
        let mut vc = VectorClock::new();
        vc.increment(peer);

        let op = make_op(coord, Material::Stone, peer, 1000, vc);
        let count = mgr.merge_received_operations(vec![op]);

        assert_eq!(count, 1);
        assert!(mgr.voxel_authority.contains_key(&coord));
    }

    // ── Deduplication ──────────────────────────────────────────────────────────

    #[test]
    fn test_merge_deduplication_by_signature() {
        let mut mgr = make_manager();
        let peer = PeerId::random();
        let coord = VoxelCoord::new(200, 200, 200);
        let mut vc = VectorClock::new();
        vc.increment(peer);

        let op = make_op(coord, Material::Stone, peer, 1000, vc);
        let op2 = op.clone();

        let first  = mgr.merge_received_operations(vec![op]);
        let second = mgr.merge_received_operations(vec![op2]);

        assert_eq!(first,  1, "first merge should apply");
        assert_eq!(second, 0, "duplicate should be ignored");
    }

    // ── Causal ordering: later clock wins ─────────────────────────────────────

    #[test]
    fn test_merge_causal_ordering_later_wins() {
        let mut mgr = make_manager();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();
        let coord = VoxelCoord::new(300, 300, 300);

        // op1: peer_a places Stone at t=1
        let mut vc1 = VectorClock::new();
        vc1.increment(peer_a);
        let op1 = make_op(coord, Material::Stone, peer_a, 1000, vc1.clone());

        // op2: peer_b knows about op1 (vc includes peer_a=1), then places Air at t=2
        let mut vc2 = vc1.clone();
        vc2.merge(&vc1);
        vc2.increment(peer_b);
        let op2 = make_op(coord, Material::Air, peer_b, 2000, vc2);

        // Receive in causal order: op1 first, then op2
        mgr.merge_received_operations(vec![op1, op2]);

        // op2 causally follows op1 → op2 (Air) wins
        let authority = mgr.voxel_authority.get(&coord).expect("should have authority");
        assert_eq!(authority.material, Material::Air);
    }

    #[test]
    fn test_merge_causal_ordering_out_of_order_delivery() {
        // Same scenario but ops arrive in reverse order (op2 before op1)
        let mut mgr = make_manager();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();
        let coord = VoxelCoord::new(400, 400, 400);

        let mut vc1 = VectorClock::new();
        vc1.increment(peer_a);
        let op1 = make_op(coord, Material::Stone, peer_a, 1000, vc1.clone());

        let mut vc2 = vc1.clone();
        vc2.increment(peer_b);
        let op2 = make_op(coord, Material::Air, peer_b, 2000, vc2);

        // Deliver op2 first (out-of-order network delivery)
        mgr.merge_received_operations(vec![op2, op1]);

        // op2 still wins (higher causal clock)
        let authority = mgr.voxel_authority.get(&coord).expect("should have authority");
        assert_eq!(authority.material, Material::Air);
    }

    // ── Concurrent ops: timestamp tiebreak ────────────────────────────────────

    #[test]
    fn test_merge_concurrent_higher_timestamp_wins() {
        let mut mgr = make_manager();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();
        let coord = VoxelCoord::new(500, 500, 500);

        // Both peers independently edit the same voxel (concurrent clocks)
        let mut vc_a = VectorClock::new();
        vc_a.increment(peer_a);
        let op_stone = make_op(coord, Material::Stone, peer_a, 1000, vc_a);

        let mut vc_b = VectorClock::new();
        vc_b.increment(peer_b);
        let op_sand = make_op(coord, Material::Grass, peer_b, 2000, vc_b); // higher ts

        mgr.merge_received_operations(vec![op_stone, op_sand]);

        // Sand (t=2000) beats Stone (t=1000)
        let authority = mgr.voxel_authority.get(&coord).expect("should have authority");
        assert_eq!(authority.material, Material::Grass);
    }

    #[test]
    fn test_merge_concurrent_peer_id_tiebreak() {
        let mut mgr = make_manager();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();
        let coord = VoxelCoord::new(600, 600, 600);

        // Same timestamp, concurrent clocks → PeerId tiebreak
        let mut vc_a = VectorClock::new();
        vc_a.increment(peer_a);
        let op_a = make_op(coord, Material::Stone, peer_a, 1000, vc_a);

        let mut vc_b = VectorClock::new();
        vc_b.increment(peer_b);
        let op_b = make_op(coord, Material::Grass, peer_b, 1000, vc_b);

        // Determine which peer wins the tiebreak
        let a_wins = peer_a.to_bytes() > peer_b.to_bytes();
        let expected = if a_wins { Material::Stone } else { Material::Grass };

        mgr.merge_received_operations(vec![op_a, op_b]);

        let authority = mgr.voxel_authority.get(&coord).expect("should have authority");
        assert_eq!(authority.material, expected,
            "PeerId tiebreak should be deterministic");
    }

    // ── Convergence: all orderings produce the same result ────────────────────

    #[test]
    fn test_merge_convergence_all_orderings() {
        // 1000 random concurrent ops on the same coord — every ordering must
        // converge to the same winner.
        let coord = VoxelCoord::new(700, 700, 700);
        let materials = [Material::Stone, Material::Grass, Material::Air,
                         Material::Dirt, Material::Water];

        let peers: Vec<PeerId> = (0..10).map(|_| PeerId::random()).collect();

        let ops: Vec<VoxelOperation> = (0..1000).map(|i| {
            let peer = peers[i % peers.len()];
            let mat  = materials[i % materials.len()];
            let ts   = (i as u64) * 7 + 1000; // varied timestamps
            let mut vc = VectorClock::new();
            vc.increment(peer);
            make_op(coord, mat, peer, ts, vc)
        }).collect();

        // Find expected winner independently using wins_over chain
        let expected_winner = ops.iter().max_by(|a, b| {
            if a.wins_over(b) { std::cmp::Ordering::Greater }
            else { std::cmp::Ordering::Less }
        }).expect("at least one op");
        let expected_material = expected_winner.material;

        // Test forward order
        let mut mgr1 = make_manager();
        mgr1.merge_received_operations(ops.clone());
        assert_eq!(mgr1.voxel_authority[&coord].material, expected_material);

        // Test reverse order
        let mut reversed = ops.clone();
        reversed.reverse();
        let mut mgr2 = make_manager();
        mgr2.merge_received_operations(reversed);
        assert_eq!(mgr2.voxel_authority[&coord].material, expected_material,
            "Reverse order must converge to same winner");

        // Test shuffled order
        let mut shuffled = ops.clone();
        // Deterministic shuffle using index-based swapping
        let n = shuffled.len();
        for i in (1..n).rev() {
            let j = (i.wrapping_mul(6364136223846793005usize).wrapping_add(1442695040888963407)) % (i + 1);
            shuffled.swap(i, j);
        }
        let mut mgr3 = make_manager();
        mgr3.merge_received_operations(shuffled);
        assert_eq!(mgr3.voxel_authority[&coord].material, expected_material,
            "Shuffled order must converge to same winner");
    }

    // ── Multiple coords don't interfere ───────────────────────────────────────

    #[test]
    fn test_merge_multiple_coords_independent() {
        let mut mgr = make_manager();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();

        let coord_a = VoxelCoord::new(100, 100, 100);
        let coord_b = VoxelCoord::new(200, 200, 200);

        let mut vc_a = VectorClock::new();
        vc_a.increment(peer_a);
        let mut vc_b = VectorClock::new();
        vc_b.increment(peer_b);

        let op_a = make_op(coord_a, Material::Stone, peer_a, 1000, vc_a);
        let op_b = make_op(coord_b, Material::Grass,  peer_b, 2000, vc_b);

        mgr.merge_received_operations(vec![op_a, op_b]);

        assert_eq!(mgr.voxel_authority[&coord_a].material, Material::Stone);
        assert_eq!(mgr.voxel_authority[&coord_b].material, Material::Grass);
    }

    // ── Op log preserved for history even when op doesn't win ─────────────────

    #[test]
    fn test_merge_losing_ops_stored_in_log() {
        let mut mgr = make_manager();
        let peer_a = PeerId::random();
        let peer_b = PeerId::random();
        let coord = VoxelCoord::new(800, 800, 800);

        let mut vc_a = VectorClock::new();
        vc_a.increment(peer_a);
        let op_loser = make_op(coord, Material::Stone, peer_a, 500, vc_a);

        let mut vc_b = VectorClock::new();
        vc_b.increment(peer_b);
        let op_winner = make_op(coord, Material::Grass, peer_b, 1500, vc_b);

        mgr.merge_received_operations(vec![op_winner, op_loser]);

        // Authority is the winner
        assert_eq!(mgr.voxel_authority[&coord].material, Material::Grass);
        // But both ops in the log (for late-joining peers to replay)
        assert_eq!(mgr.user_content.op_count(), 2,
            "Both ops must be in log for full CRDT history");
    }
}
