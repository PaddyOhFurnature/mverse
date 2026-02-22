//! Chunk streaming system for infinite open world
//!
//! Dynamically loads and unloads chunks based on player position.
//! Manages memory usage, prevents stutter, handles asynchronous loading.
//!
//! # Core Philosophy
//!
//! - **Graceful degradation:** System keeps working even under stress
//! - **Time budgets:** Never block frame for more than 5ms
//! - **Safe zones:** Never unload chunk player is standing on
//! - **Progressive loading:** Load closest chunks first
//!
//! # Expected Failures (Planned For)
//!
//! 1. **Memory exhaustion:** Hard limit on loaded chunks, emergency unload
//! 2. **Loading stutter:** Time budget prevents frame drops
//! 3. **Player fall-through:** Safe zone keeps player's chunk always loaded
//! 4. **Chunk boundaries:** Overlap chunks slightly (future)
//!
//! # Usage
//!
//! ```no_run
//! let mut streamer = ChunkStreamer::new(config);
//!
//! // Every frame
//! streamer.update(player_position);
//! streamer.process_queues(5.0); // 5ms budget
//!
//! // Check what's loaded
//! if streamer.is_chunk_loaded(&chunk_id) {
//!     // Render chunk
//! }
//! ```

use crate::{
    chunk::{ChunkId, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z},
    chunk_loader::ChunkLoader,
    coordinates::ECEF,
    materials::MaterialId,
    renderer::MeshBuffer,
    terrain::TerrainGenerator,
    terrain_sync,
    user_content::UserContentLayer,
    voxel::Octree,
};
use rapier3d::prelude::ColliderHandle;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Configuration for chunk streaming behavior
#[derive(Debug, Clone)]
pub struct ChunkStreamerConfig {
    /// Load chunks within this radius (meters)
    pub load_radius_m: f64,
    
    /// Unload chunks beyond this radius (meters)
    pub unload_radius_m: f64,
    
    /// Maximum number of chunks to keep loaded
    pub max_loaded_chunks: usize,
    
    /// Safe zone radius (chunks, always keep loaded around player)
    pub safe_zone_radius: i32,
    
    /// Maximum time per frame for chunk operations (milliseconds)
    pub frame_budget_ms: f64,
}

impl Default for ChunkStreamerConfig {
    fn default() -> Self {
        Self {
            load_radius_m: 500.0,        // Load chunks within 500m
            unload_radius_m: 1000.0,     // Unload chunks beyond 1km
            max_loaded_chunks: 100,      // Hard limit: 100 chunks
            safe_zone_radius: 1,         // Keep 3x3 grid around player
            frame_budget_ms: 5.0,        // Max 5ms per frame
        }
    }
}

/// Data stored for each loaded chunk
pub struct LoadedChunk {
    /// Chunk ID
    pub id: ChunkId,
    
    /// Voxel data (octree)
    pub octree: Octree,
    
    /// GPU mesh buffer (None until generated)
    pub mesh_buffer: Option<MeshBuffer>,
    
    /// Physics collider (None until generated)
    pub collider: Option<ColliderHandle>,
    
    /// Needs mesh regeneration?
    pub dirty: bool,
    
    /// Distance from player (cached for sorting)
    pub distance_m: f64,
    
    /// Is this chunk in the safe zone? (never unload)
    pub in_safe_zone: bool,
    
    /// Loading state
    pub state: ChunkLoadState,

    /// Unix timestamp (secs) when this chunk was last modified by terrain gen or user op.
    /// Used for chunk terrain sync: newer timestamp wins.
    pub last_modified: u64,
}

/// Loading states for chunks
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkLoadState {
    /// Not loaded
    Unloaded,
    
    /// Queued for loading
    Queued,
    
    /// Currently loading in background thread
    Loading,
    
    /// Fully loaded and ready
    Loaded,
    
    /// Placeholder shown while loading
    Placeholder,
}

/// Chunk streaming manager
pub struct ChunkStreamer {
    /// Configuration
    config: ChunkStreamerConfig,
    
    /// Currently loaded chunks
    loaded_chunks: HashMap<ChunkId, LoadedChunk>,
    
    /// Chunks queued for loading (priority queue, sorted by distance)
    loading_queue: VecDeque<ChunkId>,
    
    /// Chunks queued for unloading
    unloading_queue: Vec<ChunkId>,
    
    /// Set of chunks currently being loaded (deduplication)
    loading_in_progress: HashSet<ChunkId>,
    
    /// Background chunk loader (parallel workers)
    chunk_loader: ChunkLoader,
    
    /// Terrain generator for synchronous terrain generation
    terrain_generator: Arc<Mutex<TerrainGenerator>>,
    
    /// User content layer for voxel operations (edits/modifications)
    user_content: Arc<Mutex<UserContentLayer>>,
    
    /// World data directory for persistence
    world_dir: PathBuf,
    
    /// Last player position (for detecting movement)
    last_player_pos: Option<ECEF>,
    
    /// Chunks that finished loading this frame — game loop uses this to broadcast terrain to peers
    pub newly_loaded_chunks: Vec<ChunkId>,

    /// Statistics
    pub stats: StreamerStats,
}

/// Statistics for monitoring
#[derive(Debug, Default, Clone)]
pub struct StreamerStats {
    pub chunks_loaded: usize,
    pub chunks_queued: usize,
    pub chunks_loading: usize,
    pub chunks_unloaded_this_frame: usize,
    pub chunks_loaded_this_frame: usize,
    pub emergency_unloads: u64,
    pub total_memory_mb: f64,
}

impl ChunkStreamer {
    /// Create a new chunk streamer
    pub fn new(
        config: ChunkStreamerConfig, 
        terrain_generator: Arc<Mutex<TerrainGenerator>>,
        user_content: Arc<Mutex<UserContentLayer>>,
        world_dir: PathBuf,
    ) -> Self {
        // Create chunk loader with parallel workers (4 threads for 4-core minimum)
        let num_workers = 4;  // Can make this configurable later
        let chunk_loader = ChunkLoader::new(terrain_generator.clone(), num_workers);
        
        Self {
            config,
            loaded_chunks: HashMap::new(),
            loading_queue: VecDeque::new(),
            unloading_queue: Vec::new(),
            loading_in_progress: HashSet::new(),
            chunk_loader,
            terrain_generator,
            user_content,
            world_dir,
            last_player_pos: None,
            newly_loaded_chunks: Vec::new(),
            stats: StreamerStats::default(),
        }
    }
    
    /// Create with default configuration
    pub fn new_default(
        terrain_generator: Arc<Mutex<TerrainGenerator>>,
        user_content: Arc<Mutex<UserContentLayer>>,
        world_dir: PathBuf,
    ) -> Self {
        Self::new(ChunkStreamerConfig::default(), terrain_generator, user_content, world_dir)
    }
    
    /// Update based on player position
    ///
    /// Call this every frame. Calculates which chunks should be loaded/unloaded.
    pub fn update(&mut self, player_pos: ECEF) {
        // Reset frame stats and newly-loaded list
        self.stats.chunks_unloaded_this_frame = 0;
        self.stats.chunks_loaded_this_frame = 0;
        self.newly_loaded_chunks.clear();
        
        // Calculate desired chunks (within load radius)
        let desired_chunks = self.chunks_in_radius(player_pos, self.config.load_radius_m);
        
        // Calculate chunks to unload (beyond unload radius)
        let unload_chunks = self.chunks_beyond_radius(player_pos, self.config.unload_radius_m);
        
        // Find chunks to load (desired but not loaded)
        let currently_loaded: HashSet<ChunkId> = self.loaded_chunks.keys().copied().collect();
        let to_load: Vec<ChunkId> = desired_chunks
            .difference(&currently_loaded)
            .filter(|id| !self.loading_in_progress.contains(id))
            .copied()
            .collect();
        
        // Sort by distance (closest first)
        let mut to_load_sorted = to_load;
        to_load_sorted.sort_by(|a, b| {
            let dist_a = a.center_ecef().distance_to(&player_pos);
            let dist_b = b.center_ecef().distance_to(&player_pos);
            dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
        });
        
        let to_load_count = to_load_sorted.len();
        
        // Queue for loading
        for chunk_id in to_load_sorted {
            if !self.loading_queue.contains(&chunk_id) {
                self.loading_queue.push_back(chunk_id);
            }
        }
        
        // Queue for unloading (but skip safe zone!)
        for chunk_id in unload_chunks {
            if let Some(chunk) = self.loaded_chunks.get(&chunk_id) {
                if !chunk.in_safe_zone {
                    self.unloading_queue.push(chunk_id);
                }
            }
        }
        
        // Update distances for all loaded chunks
        let safe_zone_radius = self.config.safe_zone_radius as i64;
        let player_chunk = ChunkId::from_ecef(&player_pos);
        
        for chunk in self.loaded_chunks.values_mut() {
            chunk.distance_m = chunk.id.center_ecef().distance_to(&player_pos);
            
            // Calculate safe zone without borrowing self
            chunk.in_safe_zone = (chunk.id.x - player_chunk.x).abs() <= safe_zone_radius &&
                                 (chunk.id.y - player_chunk.y).abs() <= safe_zone_radius &&
                                 (chunk.id.z - player_chunk.z).abs() <= safe_zone_radius;
        }
        
        // Update stats
        self.stats.chunks_loaded = self.loaded_chunks.len();
        self.stats.chunks_queued = self.loading_queue.len();
        self.stats.chunks_loading = self.loading_in_progress.len();
        
        // Debug: Log chunk streaming activity
        if to_load_count > 0 || !self.unloading_queue.is_empty() {
            println!("🌍 ChunkStreamer: {} loaded, {} queued, {} loading, {} to unload",
                self.stats.chunks_loaded, self.stats.chunks_queued, 
                self.stats.chunks_loading, self.unloading_queue.len());
        }
        
        self.last_player_pos = Some(player_pos);
    }
    
    /// Process loading and unloading queues with time budget
    ///
    /// Returns true if there's more work to do (queue not empty)
    pub fn process_queues(&mut self, budget_ms: f64) -> bool {
        let start = Instant::now();
        
        // Emergency unload if over limit
        if self.loaded_chunks.len() > self.config.max_loaded_chunks {
            let over_limit = self.loaded_chunks.len() - self.config.max_loaded_chunks;
            self.emergency_unload(over_limit);
        }
        
        // Unload first (free memory)
        while let Some(chunk_id) = self.unloading_queue.pop() {
            self.unload_chunk(&chunk_id);
            self.stats.chunks_unloaded_this_frame += 1;
            
            if start.elapsed().as_secs_f64() * 1000.0 > budget_ms {
                break;
            }
        }
        
        // Request loading from worker threads (non-blocking)
        while let Some(chunk_id) = self.loading_queue.pop_front() {
            // Skip if already loaded or loading
            if self.loaded_chunks.contains_key(&chunk_id) {
                continue;
            }
            if self.loading_in_progress.contains(&chunk_id) {
                continue;
            }
            
            // Request parallel loading (returns immediately)
            if let Ok(_) = self.chunk_loader.request_load(chunk_id, 1.0) {
                self.loading_in_progress.insert(chunk_id);
            }
            
            // Keep requesting until queue empty or budget exhausted
            if start.elapsed().as_secs_f64() * 1000.0 > budget_ms {
                break;
            }
        }
        
        // Poll for completed chunks from worker threads (always poll, regardless of budget)
        let completed = self.chunk_loader.poll_completed();
        for result in completed {
            self.loading_in_progress.remove(&result.chunk_id);
            
            if let Some(mut octree) = result.octree {
                // Load and apply user operations from disk
                let ops_loaded = match self.user_content.lock().unwrap().load_chunk(&self.world_dir, &result.chunk_id) {
                    Ok(count) => count,
                    Err(e) => {
                        eprintln!("⚠️  Failed to load operations for {}: {}", result.chunk_id, e);
                        0
                    }
                };
                
                // Apply loaded operations to octree
                if ops_loaded > 0 {
                    let user_content = self.user_content.lock().unwrap();
                    for op in user_content.operations_for_chunk(&result.chunk_id) {
                        // Convert Material to MaterialId
                        let material_id = match op.material {
                            crate::messages::Material::Air => MaterialId::AIR,
                            crate::messages::Material::Stone => MaterialId::STONE,
                            crate::messages::Material::Dirt => MaterialId::DIRT,
                            crate::messages::Material::Grass => MaterialId::GRASS,
                            crate::messages::Material::Water => MaterialId::AIR, // TODO: Add water material
                        };
                        octree.set_voxel(op.coord, material_id);
                    }
                    println!("   📝 Applied {} saved operations to {}", ops_loaded, result.chunk_id);
                }
                
                let chunk = LoadedChunk {
                    id: result.chunk_id,
                    octree,
                    mesh_buffer: None,
                    collider: None,
                    dirty: true,
                    distance_m: 0.0,
                    in_safe_zone: false,
                    state: ChunkLoadState::Loaded,
                    last_modified: now_secs(),
                };
                self.loaded_chunks.insert(result.chunk_id, chunk);
                self.newly_loaded_chunks.push(result.chunk_id);
                self.stats.chunks_loaded_this_frame += 1;
                
                // Log when chunks complete (for debugging parallel loading)
                if self.stats.chunks_loaded_this_frame <= 3 {
                    println!("   ✅ Chunk {} loaded ({:.2}s generation time)", 
                        result.chunk_id, result.load_time_ms as f64 / 1000.0);
                }
            } else {
                // Chunk generation failed - retry?
                eprintln!("   ❌ Chunk {} generation failed", result.chunk_id);
            }
        }
        
        // Return true if queues still have work
        !self.loading_queue.is_empty() || !self.unloading_queue.is_empty()
    }
    
    /// Get all chunk IDs within radius of position
    fn chunks_in_radius(&self, center: ECEF, radius_m: f64) -> HashSet<ChunkId> {
        let mut chunks = HashSet::new();
        
        // Calculate how many chunks fit in radius
        let chunk_size_m = CHUNK_SIZE_X as f64; // Assuming ~30m chunks
        let chunk_radius = (radius_m / chunk_size_m).ceil() as i32;
        
        // Get player's chunk
        let player_chunk = ChunkId::from_ecef(&center);
        
        // Add chunks in grid around player
        for dx in -chunk_radius..=chunk_radius {
            for dy in -1..=1 {  // Y is vertical, keep small
                for dz in -chunk_radius..=chunk_radius {
                    let chunk_id = ChunkId {
                        x: player_chunk.x + dx as i64,
                        y: player_chunk.y + dy as i64,
                        z: player_chunk.z + dz as i64,
                    };
                    
                    // Check if actually within radius (not just grid)
                    let chunk_center = chunk_id.center_ecef();
                    if chunk_center.distance_to(&center) <= radius_m {
                        chunks.insert(chunk_id);
                    }
                }
            }
        }
        
        chunks
    }
    
    /// Get all chunks beyond radius (for unloading)
    fn chunks_beyond_radius(&self, center: ECEF, radius_m: f64) -> Vec<ChunkId> {
        self.loaded_chunks
            .values()
            .filter(|chunk| chunk.distance_m > radius_m)
            .map(|chunk| chunk.id)
            .collect()
    }
    
    /// Check if chunk is in safe zone around player
    fn is_in_safe_zone(&self, chunk_id: &ChunkId, player_pos: &ECEF) -> bool {
        let player_chunk = ChunkId::from_ecef(player_pos);
        let radius = self.config.safe_zone_radius as i64;
        
        (chunk_id.x - player_chunk.x).abs() <= radius &&
        (chunk_id.y - player_chunk.y).abs() <= radius &&
        (chunk_id.z - player_chunk.z).abs() <= radius
    }
    
    /// Load chunk immediately (synchronous, will be async later)
    fn load_chunk_immediate(&mut self, chunk_id: ChunkId) -> Result<LoadedChunk, String> {
        // Generate real terrain using synchronous terrain generator
        let generator = self.terrain_generator.lock()
            .map_err(|e| format!("Failed to lock terrain generator: {}", e))?;
        
        let octree = generator.generate_chunk(&chunk_id)
            .map_err(|e| format!("Failed to generate terrain for {}: {}", chunk_id, e))?;
        
        Ok(LoadedChunk {
            id: chunk_id,
            octree,
            mesh_buffer: None,
            collider: None,
            dirty: true,  // New chunk needs mesh generation
            distance_m: 0.0, // Will be updated
            in_safe_zone: false, // Will be updated
            state: ChunkLoadState::Loaded,
            last_modified: now_secs(),
        })
    }
    
    /// Unload a chunk (saves modifications to disk)
    fn unload_chunk(&mut self, chunk_id: &ChunkId) {
        if let Some(_chunk) = self.loaded_chunks.remove(chunk_id) {
            // Save operations for this chunk to disk
            // Note: save_chunks() saves ALL operations, but we only care about this chunk
            // A more efficient approach would be to track dirty chunks
            if let Err(e) = self.user_content.lock().unwrap().save_chunks(&self.world_dir) {
                eprintln!("⚠️  Failed to save operations for {}: {}", chunk_id, e);
            }
            
            // TODO: Free GPU resources (mesh, collision)
        }
        self.loading_in_progress.remove(chunk_id);
    }
    
    /// Emergency unload furthest chunks
    fn emergency_unload(&mut self, count: usize) {
        println!("⚠️ Emergency unload: {} chunks (over limit)", count);
        self.stats.emergency_unloads += 1;
        
        // Sort chunks by distance (furthest first)
        let mut chunks_by_distance: Vec<_> = self.loaded_chunks
            .values()
            .filter(|c| !c.in_safe_zone)  // Don't unload safe zone!
            .map(|c| (c.id, c.distance_m))
            .collect();
        
        chunks_by_distance.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Unload furthest chunks
        for (chunk_id, _) in chunks_by_distance.iter().take(count) {
            self.unload_chunk(chunk_id);
        }
    }
    
    /// Check if chunk is loaded
    pub fn is_chunk_loaded(&self, chunk_id: &ChunkId) -> bool {
        self.loaded_chunks.contains_key(chunk_id)
    }
    
    /// Get loaded chunk
    pub fn get_chunk(&self, chunk_id: &ChunkId) -> Option<&LoadedChunk> {
        self.loaded_chunks.get(chunk_id)
    }
    
    /// Get mutable loaded chunk
    pub fn get_chunk_mut(&mut self, chunk_id: &ChunkId) -> Option<&mut LoadedChunk> {
        self.loaded_chunks.get_mut(chunk_id)
    }
    
    /// Get all loaded chunks (immutable)
    pub fn loaded_chunks(&self) -> impl Iterator<Item = &LoadedChunk> {
        self.loaded_chunks.values()
    }
    
    /// Get all loaded chunks (mutable)
    pub fn loaded_chunks_mut(&mut self) -> impl Iterator<Item = &mut LoadedChunk> {
        self.loaded_chunks.values_mut()
    }
    
    /// Get all loaded chunk IDs
    pub fn loaded_chunk_ids(&self) -> Vec<ChunkId> {
        self.loaded_chunks.keys().copied().collect()
    }

    /// Replace a chunk's octree with authoritative data received from a peer.
    /// Only applies if `received_last_modified` is NEWER than what we have.
    /// Returns true if the chunk was replaced, false if rejected (ours is newer/equal or chunk not loaded).
    pub fn replace_chunk_octree(&mut self, chunk_id: &ChunkId, octree: crate::voxel::Octree, received_last_modified: u64) -> bool {
        if let Some(chunk) = self.loaded_chunks.get_mut(chunk_id) {
            if received_last_modified > chunk.last_modified {
                chunk.octree = octree;
                chunk.dirty = true;
                chunk.last_modified = received_last_modified;
                true
            } else {
                false // our version is same age or newer — keep it
            }
        } else {
            false // chunk not loaded yet
        }
    }

    /// Get a manifest of all loaded chunks: (ChunkId, last_modified).
    /// Used to negotiate which chunks to exchange with peers — newer wins.
    pub fn chunk_manifest(&self) -> Vec<(ChunkId, u64)> {
        self.loaded_chunks.values()
            .map(|c| (c.id, c.last_modified))
            .collect()
    }

    /// Update a chunk's last_modified timestamp (called when a user op is applied).
    pub fn touch_chunk(&mut self, chunk_id: &ChunkId) {
        if let Some(chunk) = self.loaded_chunks.get_mut(chunk_id) {
            chunk.last_modified = now_secs();
        }
    }
    
    /// Get statistics
    pub fn stats(&self) -> &StreamerStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chunks_in_radius() {
        let streamer = ChunkStreamer::new_default();
        let center = ECEF::new(0.0, 0.0, 6371000.0);
        
        let chunks = streamer.chunks_in_radius(center, 100.0);
        
        // Should have at least player's chunk
        assert!(!chunks.is_empty());
        
        // All chunks should be within radius
        for chunk_id in chunks {
            let dist = chunk_id.center_ecef().distance_to(&center);
            assert!(dist <= 100.0);
        }
    }
    
    #[test]
    fn test_safe_zone() {
        let streamer = ChunkStreamer::new_default();
        let player_pos = ECEF::new(0.0, 0.0, 6371000.0);
        let player_chunk = ChunkId::from_ecef(&player_pos);
        
        // Player's chunk should be in safe zone
        assert!(streamer.is_in_safe_zone(&player_chunk, &player_pos));
        
        // Distant chunk should not be in safe zone
        let distant_chunk = ChunkId {
            x: player_chunk.x + 100,
            y: player_chunk.y,
            z: player_chunk.z,
        };
        assert!(!streamer.is_in_safe_zone(&distant_chunk, &player_pos));
    }
    
    #[test]
    fn test_max_loaded_chunks() {
        let config = ChunkStreamerConfig {
            max_loaded_chunks: 10,
            ..Default::default()
        };
        let mut streamer = ChunkStreamer::new(config);
        
        // Load 20 chunks manually
        for i in 0..20 {
            let chunk_id = ChunkId { x: i, y: 0, z: 0 };
            let chunk = LoadedChunk {
                id: chunk_id,
                octree: Octree::new(),
                distance_m: i as f64,
                in_safe_zone: false,
                state: ChunkLoadState::Loaded,
                mesh_buffer: None,
                collider: None,
                dirty: false,
                last_modified: 0,
            };
            streamer.loaded_chunks.insert(chunk_id, chunk);
        }
        
        // Emergency unload should trigger
        streamer.emergency_unload(10);
        
        // Should have unloaded 10 chunks
        assert_eq!(streamer.loaded_chunks.len(), 10);
    }
}
