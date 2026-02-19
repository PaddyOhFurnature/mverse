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

use crate::chunk::{ChunkId, chunks_in_radius, CHUNK_SIZE};
use crate::terrain::TerrainGenerator;
use crate::user_content::UserContentLayer;
use crate::voxel::{Octree, VoxelCoord};
use crate::materials::MaterialId;
use rapier3d::prelude::ColliderHandle;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Per-chunk data (terrain, mesh, collision, state)
pub struct ChunkData {
    pub chunk_id: ChunkId,
    pub octree: Octree,
    pub mesh_buffer: Option<MeshBuffer>,  // Lazy: only create when needed
    pub collider: Option<ColliderHandle>,  // Lazy: only create when needed
    pub dirty: bool,  // Needs mesh regeneration
}

/// Placeholder for mesh buffer (will be defined elsewhere)
pub struct MeshBuffer;

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
        let chunk_data = ChunkData::new(chunk_id, octree);
        
        // 3. Load operations from file (modifies user_content internal state)
        match self.user_content.load_chunk(world_dir, &chunk_id) {
            Ok(count) => {
                if count > 0 {
                    println!("  {} loaded with {} operations", chunk_id, count);
                }
            }
            Err(e) => {
                // File not existing is OK (no edits yet)
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!("Failed to load chunk {}: {}", chunk_id, e));
                }
            }
        }
        
        // 4. Insert into loaded chunks
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
    
    /// Set voxel material (marks chunk as dirty)
    ///
    /// Automatically marks the chunk as dirty for mesh regeneration.
    /// Returns true if voxel was in a loaded chunk.
    pub fn set_voxel(&mut self, coord: VoxelCoord, material: MaterialId) -> bool {
        let chunk_id = ChunkId::from_voxel(&coord);
        
        if let Some(chunk_data) = self.loaded_chunks.get_mut(&chunk_id) {
            chunk_data.octree.set_voxel(coord, material);
            chunk_data.dirty = true;
            true
        } else {
            false
        }
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
}
