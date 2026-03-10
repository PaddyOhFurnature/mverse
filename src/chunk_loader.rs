/// Bump this when terrain generation logic changes — invalidates all cached chunks.
const TERRAIN_CACHE_VERSION: u32 = 13; // v0.1.62: density field requires fresh fractional surface heights

/// Asynchronous chunk loading subsystem
///
/// Loads chunks in background thread to avoid blocking main game loop.
/// Supports terrain generation, mesh generation, and collider generation.

use crate::chunk::ChunkId;
use crate::voxel::Octree;
use crate::terrain::{TerrainGenerator, SurfaceCache};
use crate::materials::MaterialId;
use crate::voxel::VoxelCoord;
use std::path::{Path, PathBuf};
use std::sync::{mpsc::{channel, Sender, Receiver}, Arc, Mutex};
use std::thread;
use std::time::Instant;

/// Request to load a chunk in the background
#[derive(Debug, Clone)]
pub struct LoadRequest {
    pub chunk_id: ChunkId,
    pub priority: f64,  // Distance from player (lower = higher priority)
}

/// Result of a chunk load operation
pub struct LoadResult {
    pub chunk_id: ChunkId,
    pub octree: Option<Octree>,
    pub surface_cache: Option<SurfaceCache>,
    pub load_time_ms: u128,
    pub error: Option<String>,
}

/// Commands sent to the background loader thread
enum LoaderCommand {
    Load(LoadRequest),
    Shutdown,
}

/// Background chunk loader
///
/// Runs in a separate thread, generating terrain/meshes asynchronously.
/// Main thread sends load requests, receives completed chunks via channels.
pub struct ChunkLoader {
    /// Send commands to background thread
    cmd_tx: Sender<LoaderCommand>,
    
    /// Receive completed chunks from background thread
    result_rx: Receiver<LoadResult>,
    
    /// Statistics
    pub chunks_loaded: usize,
    pub total_load_time_ms: u128,
}

impl ChunkLoader {
    /// Create new background chunk loader with parallel workers
    ///
    /// Spawns multiple background threads for parallel terrain generation.
    /// 
    /// # Arguments
    /// * `terrain_generator` - Thread-safe terrain generator (Send+Sync)
    /// * `num_workers` - Number of parallel worker threads (default: 4)
    pub fn new(terrain_generator: Arc<Mutex<TerrainGenerator>>, num_workers: usize, cache_dir: Option<PathBuf>) -> Self {
        let (cmd_tx, cmd_rx) = channel();
        let (result_tx, result_rx) = channel();
        
        // Create cache directory if provided
        if let Some(ref dir) = cache_dir {
            let _ = std::fs::create_dir_all(dir);
        }
        
        // Spawn multiple worker threads for parallel generation
        let cmd_rx = Arc::new(Mutex::new(cmd_rx));
        
        for worker_id in 0..num_workers {
            let cmd_rx_clone = Arc::clone(&cmd_rx);
            let result_tx_clone = result_tx.clone();
            let terrain_gen_clone = Arc::clone(&terrain_generator);
            let cache_dir_clone = cache_dir.clone();
            
            thread::spawn(move || {
                Self::worker_thread(worker_id, cmd_rx_clone, result_tx_clone, terrain_gen_clone, cache_dir_clone);
            });
        }
        
        println!("✅ ChunkLoader initialized with {} worker threads", num_workers);
        
        ChunkLoader {
            cmd_tx,
            result_rx,
            chunks_loaded: 0,
            total_load_time_ms: 0,
        }
    }
    
    /// Request a chunk to be loaded
    ///
    /// Non-blocking - adds to queue, returns immediately.
    pub fn request_load(&mut self, chunk_id: ChunkId, priority: f64) -> Result<(), String> {
        self.cmd_tx.send(LoaderCommand::Load(LoadRequest {
            chunk_id,
            priority,
        })).map_err(|e| format!("Failed to send load request: {}", e))
    }
    
    /// Poll for completed chunks (non-blocking)
    ///
    /// Returns all chunks that finished loading since last poll.
    pub fn poll_completed(&mut self) -> Vec<LoadResult> {
        let mut results = Vec::new();
        
        // Drain all available results
        while let Ok(result) = self.result_rx.try_recv() {
            self.chunks_loaded += 1;
            self.total_load_time_ms += result.load_time_ms;
            results.push(result);
        }
        
        results
    }
    
    /// Average load time per chunk (for performance monitoring)
    pub fn avg_load_time_ms(&self) -> f64 {
        if self.chunks_loaded == 0 {
            0.0
        } else {
            self.total_load_time_ms as f64 / self.chunks_loaded as f64
        }
    }
    
    /// Background worker thread (now with REAL terrain generation)
    ///
    /// Processes load requests, generates terrain using SRTM data, sends results back.
    /// Multiple workers run in parallel for maximum throughput.
    fn worker_thread(
        worker_id: usize,
        cmd_rx: Arc<Mutex<Receiver<LoaderCommand>>>,
        result_tx: Sender<LoadResult>,
        terrain_generator: Arc<Mutex<TerrainGenerator>>,
        cache_dir: Option<PathBuf>,
    ) {
        loop {
            // Lock only to receive command, then release
            let command = {
                let rx = cmd_rx.lock().unwrap();
                rx.recv()
            };
            
            match command {
                Ok(LoaderCommand::Load(request)) => {
                    let start = Instant::now();

                    // Try disk cache first — avoids ~2s regeneration on every launch
                    let (octree, surface_cache) = if let Some(ref dir) = cache_dir {
                        match Self::load_from_cache(dir, &request.chunk_id) {
                            Some(cached) => {
                                // Derive surface cache from the loaded octree (fast column scan)
                                let sc = Self::compute_surface_cache(&cached, &request.chunk_id);
                                (Some(cached), Some(sc))
                            }
                            None => {
                                // Cache miss: generate, then save
                                match terrain_generator.lock().unwrap().generate_chunk(&request.chunk_id) {
                                    Ok((octree, cache)) => {
                                        Self::save_to_cache(dir, &request.chunk_id, &octree);
                                        (Some(octree), Some(cache))
                                    }
                                    Err(e) => {
                                        eprintln!("[Worker {}] Failed to generate chunk {}: {}", worker_id, request.chunk_id, e);
                                        (None, None)
                                    }
                                }
                            }
                        }
                    } else {
                        match terrain_generator.lock().unwrap().generate_chunk(&request.chunk_id) {
                            Ok((octree, cache)) => (Some(octree), Some(cache)),
                            Err(e) => {
                                eprintln!("[Worker {}] Failed to generate chunk {}: {}", worker_id, request.chunk_id, e);
                                (None, None)
                            }
                        }
                    };
                    
                    let elapsed = start.elapsed().as_millis();
                    
                    if octree.is_some() && elapsed > 1000 {
                        println!("[Worker {}] Generated chunk {} in {:.2}s", 
                            worker_id, request.chunk_id, elapsed as f64 / 1000.0);
                    }
                    
                    let result = LoadResult {
                        chunk_id: request.chunk_id,
                        octree,
                        surface_cache,
                        load_time_ms: elapsed,
                        error: None,
                    };
                    
                    if result_tx.send(result).is_err() {
                        // Main thread dropped receiver, exit
                        break;
                    }
                }
                Ok(LoaderCommand::Shutdown) => {
                    println!("[Worker {}] Shutting down", worker_id);
                    break;
                }
                Err(_) => {
                    // Main thread dropped sender, exit
                    break;
                }
            }
        }
    }
    
    /// Derive a SurfaceCache by scanning the octree for the topmost solid voxel per column.
    /// Used when a chunk is loaded from disk cache (no SRTM data available).
    /// The fractional part is set to 0.5 (midpoint) so interpolation is smooth at height boundaries.
    fn compute_surface_cache(octree: &Octree, chunk_id: &ChunkId) -> SurfaceCache {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let mut cache = SurfaceCache::with_capacity((CHUNK_SIZE_X * CHUNK_SIZE_Z) as usize);
        for vx in min_v.x..max_v.x {
            for vz in min_v.z..max_v.z {
                let mut surface_y = min_v.y as f32 + 0.5;
                for vy in (min_v.y..max_v.y).rev() {
                    let mat = octree.get_voxel(VoxelCoord::new(vx, vy, vz));
                    if mat != MaterialId::AIR && mat != MaterialId::WATER {
                        // Put isosurface at midpoint of surface voxel for smooth interpolation
                        surface_y = vy as f32 + 0.5;
                        break;
                    }
                }
                cache.insert((vx, vz), surface_y);
            }
        }
        cache
    }

    /// Load a chunk octree from disk cache. Returns None on miss or version mismatch.
    fn load_from_cache(cache_dir: &Path, chunk_id: &ChunkId) -> Option<Octree> {
        let path = cache_dir.join(format!("{}_{}_{}.bin", chunk_id.x, chunk_id.y, chunk_id.z));
        let data = std::fs::read(&path).ok()?;
        if data.len() < 4 { return None; }
        let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if version != TERRAIN_CACHE_VERSION { return None; }
        Octree::from_bytes(&data[4..]).ok()
    }

    /// Persist a generated chunk octree to the disk cache.
    fn save_to_cache(cache_dir: &Path, chunk_id: &ChunkId, octree: &Octree) {
        let path = cache_dir.join(format!("{}_{}_{}.bin", chunk_id.x, chunk_id.y, chunk_id.z));
        if let Ok(octree_bytes) = octree.to_bytes() {
            let mut data = TERRAIN_CACHE_VERSION.to_le_bytes().to_vec();
            data.extend_from_slice(&octree_bytes);
            let _ = std::fs::write(&path, &data);
        }
    }

    /// Shutdown background thread gracefully
    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(LoaderCommand::Shutdown);
    }
}

impl Drop for ChunkLoader {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use crate::{elevation::ElevationPipeline, coordinates::GPS, voxel::VoxelCoord,
                terrain::TerrainGenerator};
    use std::sync::{Arc, Mutex};

    fn make_loader() -> ChunkLoader {
        let pipeline = ElevationPipeline::new();
        let terrain_gen = TerrainGenerator::new(pipeline, GPS::new(0.0, 0.0, 0.0), VoxelCoord::new(0, 0, 0));
        ChunkLoader::new(Arc::new(Mutex::new(terrain_gen)), 2)
    }
    
    #[test]
    fn test_chunk_loader_basic() {
        let mut loader = make_loader();
        
        // Request a chunk
        let chunk_id = ChunkId::new(0, 0, 0);
        loader.request_load(chunk_id, 0.0).unwrap();
        
        // Wait for completion (give background thread time to work)
        thread::sleep(Duration::from_millis(100));
        
        // Poll for results
        let results = loader.poll_completed();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk_id, chunk_id);
        assert!(results[0].octree.is_some());
        assert!(results[0].error.is_none());
    }
    
    #[test]
    fn test_chunk_loader_multiple() {
        let mut loader = make_loader();
        
        // Request multiple chunks
        for i in 0..5 {
            loader.request_load(ChunkId::new(i, 0, 0), i as f64).unwrap();
        }
        
        // Wait for completion
        thread::sleep(Duration::from_millis(200));
        
        // Poll for results
        let results = loader.poll_completed();
        assert_eq!(results.len(), 5);
        
        // All should succeed
        for result in &results {
            assert!(result.octree.is_some());
            assert!(result.error.is_none());
        }
    }
    
    #[test]
    fn test_chunk_loader_stats() {
        let mut loader = make_loader();
        
        // Load some chunks
        for i in 0..3 {
            loader.request_load(ChunkId::new(i, 0, 0), 0.0).unwrap();
        }
        
        thread::sleep(Duration::from_millis(150));
        loader.poll_completed();
        
        // Check stats
        assert_eq!(loader.chunks_loaded, 3);
        assert!(loader.avg_load_time_ms() > 0.0);
    }
}
