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
        
        // 4. Apply loaded operations to the chunk octree
        if loaded_ops > 0 {
            for op in self.user_content.op_log() {
                if ChunkId::from_voxel(&op.coord) == chunk_id {
                    chunk_data.octree.set_voxel(op.coord, op.material.to_material_id());
                    chunk_data.dirty = true;
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
    /// is persisted to disk. Includes deduplication check.
    pub fn add_operation(&mut self, op: VoxelOperation) {
        // Check for duplicates
        if self.seen_operations.contains(&op.signature) {
            return; // Already have this operation
        }
        
        self.seen_operations.insert(op.signature);
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
        requester_clock: &VectorClock,
    ) -> HashMap<ChunkId, Vec<VoxelOperation>> {
        let mut result: HashMap<ChunkId, Vec<VoxelOperation>> = HashMap::new();
        
        // Get all operations from user content layer
        let all_ops = self.user_content.op_log();
        
        println!("🔍 Filtering from {} total operations for {} requested chunks", 
            all_ops.len(), chunk_ids.len());
        
        for op in all_ops {
            // Check if requester already has this operation
            // If their vector clock happened-after this op's clock, they have it
            if requester_clock.happens_after(&op.vector_clock) {
                continue; // Requester already has this
            }
            
            // Find all chunks affected by this operation
            let affected_chunks = ChunkId::affected_by_voxel(&op.coord);
            
            // Add to result if any affected chunk is in requested list
            for chunk_id in affected_chunks {
                if chunk_ids.contains(&chunk_id) {
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
    /// Number of operations actually applied (after deduplication)
    ///
    /// # Example
    /// ```rust
    /// let state_ops = multiplayer.take_pending_state_operations();
    /// let applied = chunk_manager.merge_received_operations(state_ops);
    /// println!("Applied {} historical operations", applied);
    /// ```
    pub fn merge_received_operations(&mut self, operations: Vec<VoxelOperation>) -> usize {
        let mut applied_count = 0;
        
        for op in operations {
            // Check for duplicates (by signature)
            if self.seen_operations.contains(&op.signature) {
                continue; // Already have this operation
            }
            
            // Mark as seen
            self.seen_operations.insert(op.signature);
            
            // Find all chunks affected by this operation
            let affected_chunks = ChunkId::affected_by_voxel(&op.coord);
            
            // Apply to all affected chunks
            let material_id = op.material.to_material_id();
            for chunk_id in affected_chunks {
                if let Some(chunk_data) = self.loaded_chunks.get_mut(&chunk_id) {
                    chunk_data.octree.set_voxel(op.coord, material_id);
                    chunk_data.dirty = true;
                }
            }
            
            // Add to user content layer for persistence
            self.user_content.add_local_operation(op);
            
            applied_count += 1;
        }
        
        applied_count
    }
}
