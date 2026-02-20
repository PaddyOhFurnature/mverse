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
    voxel::Octree,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::Instant;

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
    
    /// Distance from player (cached for sorting)
    pub distance_m: f64,
    
    /// Is this chunk in the safe zone? (never unload)
    pub in_safe_zone: bool,
    
    /// Loading state
    pub state: ChunkLoadState,
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
    
    /// Background chunk loader
    chunk_loader: ChunkLoader,
    
    /// Last player position (for detecting movement)
    last_player_pos: Option<ECEF>,
    
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
    pub fn new(config: ChunkStreamerConfig) -> Self {
        Self {
            config,
            loaded_chunks: HashMap::new(),
            loading_queue: VecDeque::new(),
            unloading_queue: Vec::new(),
            loading_in_progress: HashSet::new(),
            chunk_loader: ChunkLoader::new(),
            last_player_pos: None,
            stats: StreamerStats::default(),
        }
    }
    
    /// Create with default configuration
    pub fn new_default() -> Self {
        Self::new(ChunkStreamerConfig::default())
    }
    
    /// Update based on player position
    ///
    /// Call this every frame. Calculates which chunks should be loaded/unloaded.
    pub fn update(&mut self, player_pos: ECEF) {
        // Reset frame stats
        self.stats.chunks_unloaded_this_frame = 0;
        self.stats.chunks_loaded_this_frame = 0;
        
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
        
        // Load second (use freed memory)
        while let Some(chunk_id) = self.loading_queue.pop_front() {
            // Skip if already loaded or loading
            if self.loaded_chunks.contains_key(&chunk_id) {
                continue;
            }
            if self.loading_in_progress.contains(&chunk_id) {
                continue;
            }
            
            // Load chunk (synchronous for now, will be async in next todo)
            if let Ok(chunk) = self.load_chunk_immediate(chunk_id) {
                self.loaded_chunks.insert(chunk_id, chunk);
                self.stats.chunks_loaded_this_frame += 1;
            }
            
            if start.elapsed().as_secs_f64() * 1000.0 > budget_ms {
                break;
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
        // For now, create empty chunk (terrain generation will be added later)
        let octree = Octree::new();
        
        Ok(LoadedChunk {
            id: chunk_id,
            octree,
            distance_m: 0.0, // Will be updated
            in_safe_zone: false, // Will be updated
            state: ChunkLoadState::Loaded,
        })
    }
    
    /// Unload a chunk
    fn unload_chunk(&mut self, chunk_id: &ChunkId) {
        if let Some(_chunk) = self.loaded_chunks.remove(chunk_id) {
            // TODO: Save chunk to disk if modified
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
    
    /// Get all loaded chunk IDs
    pub fn loaded_chunk_ids(&self) -> Vec<ChunkId> {
        self.loaded_chunks.keys().copied().collect()
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
            };
            streamer.loaded_chunks.insert(chunk_id, chunk);
        }
        
        // Emergency unload should trigger
        streamer.emergency_unload(10);
        
        // Should have unloaded 10 chunks
        assert_eq!(streamer.loaded_chunks.len(), 10);
    }
}
