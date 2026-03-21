/// Legacy terrain chunk binary format version.
/// Payload was `[u32 version][bincode Octree]` with no persisted surface cache.
const LEGACY_TERRAIN_CACHE_VERSION: u32 = 13;

/// Terrain chunk binary format version.
/// Payload is `[u32 version][bincode StoredChunkPayload]`, which carries the
/// exact smooth-surface cache used by marching cubes. Legacy chunks are still
/// decoded for backward compatibility.
pub const TERRAIN_CACHE_VERSION: u32 = 14; // v0.1.82: persist exact SurfaceCache with stored chunks

/// Asynchronous chunk loading subsystem
///
/// Loads chunks in background thread to avoid blocking main game loop.
/// Supports terrain generation, mesh generation, and collider generation.
use crate::chunk::ChunkId;
use crate::materials::MaterialId;
use crate::terrain::{SurfaceCache, TerrainGenerator};
use crate::tile_store::{PassId, TileStore};
use crate::voxel::Octree;
use crate::voxel::VoxelCoord;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, channel},
};
use std::thread;
use std::time::Instant;

#[derive(serde::Serialize)]
struct StoredChunkPayloadRef<'a> {
    octree: &'a Octree,
    surface_cache: Option<&'a SurfaceCache>,
}

#[derive(serde::Deserialize)]
struct StoredChunkPayload {
    octree: Octree,
    surface_cache: Option<SurfaceCache>,
}

pub fn encode_stored_chunk(
    octree: &Octree,
    surface_cache: Option<&SurfaceCache>,
) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&TERRAIN_CACHE_VERSION.to_le_bytes());
    let payload = StoredChunkPayloadRef {
        octree,
        surface_cache,
    };
    let encoded = bincode::serialize(&payload).map_err(|e| format!("bincode error: {e}"))?;
    buf.extend_from_slice(&encoded);
    Ok(buf)
}

pub fn decode_stored_chunk(data: &[u8]) -> Option<(Octree, Option<SurfaceCache>)> {
    if data.len() < 4 {
        return None;
    }
    let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let payload = &data[4..];

    match version {
        TERRAIN_CACHE_VERSION => bincode::deserialize::<StoredChunkPayload>(payload)
            .ok()
            .map(|stored| (stored.octree, stored.surface_cache)),
        LEGACY_TERRAIN_CACHE_VERSION => Octree::from_bytes(payload).ok().map(|octree| (octree, None)),
        _ => None,
    }
}

/// Request to load a chunk in the background
#[derive(Debug, Clone)]
pub struct LoadRequest {
    pub chunk_id: ChunkId,
    pub priority: f64, // Distance from player (lower = higher priority)
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
    /// * `tile_store` - Optional TileStore for chunk caching (None = generate every time)
    pub fn new(
        terrain_generator: Arc<Mutex<TerrainGenerator>>,
        num_workers: usize,
        tile_store: Option<Arc<TileStore>>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = channel();
        let (result_tx, result_rx) = channel();

        // Spawn multiple worker threads for parallel generation
        let cmd_rx = Arc::new(Mutex::new(cmd_rx));

        for worker_id in 0..num_workers {
            let cmd_rx_clone = Arc::clone(&cmd_rx);
            let result_tx_clone = result_tx.clone();
            let terrain_gen_clone = Arc::clone(&terrain_generator);
            let ts_clone = tile_store.clone();

            thread::spawn(move || {
                Self::worker_thread(
                    worker_id,
                    cmd_rx_clone,
                    result_tx_clone,
                    terrain_gen_clone,
                    ts_clone,
                );
            });
        }

        println!(
            "✅ ChunkLoader initialized with {} worker threads",
            num_workers
        );

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
        self.cmd_tx
            .send(LoaderCommand::Load(LoadRequest { chunk_id, priority }))
            .map_err(|e| format!("Failed to send load request: {}", e))
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
        tile_store: Option<Arc<TileStore>>,
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
                    let id = &request.chunk_id;

                    // Try TileStore first — avoids ~2s regeneration on every launch
                    let (octree, surface_cache) = if let Some(ref ts) = tile_store {
                        match Self::load_from_store(ts, id) {
                            Some((pass_id, cached, cached_surface_cache)) => {
                                let sc = if let Some(surface_cache) = cached_surface_cache {
                                    if pass_id == PassId::Terrain {
                                        surface_cache
                                    } else {
                                        let upper_cached = Self::load_from_store(
                                            ts,
                                            &ChunkId::new(id.x, id.y + 1, id.z),
                                        )
                                        .map(|(_, octree, _)| octree);
                                        Self::merge_stored_surface_cache(
                                            &cached,
                                            upper_cached.as_ref(),
                                            id,
                                            surface_cache,
                                        )
                                    }
                                } else {
                                    let upper_cached = Self::load_from_store(
                                        ts,
                                        &ChunkId::new(id.x, id.y + 1, id.z),
                                    )
                                    .map(|(_, octree, _)| octree);
                                    Self::compute_surface_cache(&cached, upper_cached.as_ref(), id)
                                };
                                (Some(cached), Some(sc))
                            }
                            None => {
                                // Cache miss: generate, then persist
                                match terrain_generator.lock().unwrap().generate_chunk(id) {
                                    Ok((octree, cache)) => {
                                        Self::save_to_store(ts, id, &octree, Some(&cache));
                                        (Some(octree), Some(cache))
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "[Worker {}] Failed to generate chunk {}: {}",
                                            worker_id, id, e
                                        );
                                        (None, None)
                                    }
                                }
                            }
                        }
                    } else {
                        match terrain_generator.lock().unwrap().generate_chunk(id) {
                            Ok((octree, cache)) => (Some(octree), Some(cache)),
                            Err(e) => {
                                eprintln!(
                                    "[Worker {}] Failed to generate chunk {}: {}",
                                    worker_id, id, e
                                );
                                (None, None)
                            }
                        }
                    };

                    let elapsed = start.elapsed().as_millis();

                    if octree.is_some() && elapsed > 1000 {
                        println!(
                            "[Worker {}] Generated chunk {} in {:.2}s",
                            worker_id,
                            request.chunk_id,
                            elapsed as f64 / 1000.0
                        );
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
    ///
    /// Three cases:
    /// - Column is all-air              → surface_y = min_v.y - 1.0  (density always < 0, no mesh)
    /// - Surface somewhere in the chunk  → surface_y = topmost_solid + 0.5  (correct position)
    /// - Column is all-solid            → surface_y = max_v.y + 0.5  (density always > 0, no phantom surface)
    ///
    /// The all-solid case arises for chunks that are fully below the terrain surface (e.g. the
    /// lower Y layer under a cliff).  Without this fix, surface_y = max_v.y - 0.5 causes the
    /// smooth MC to draw a phantom horizontal plane at the very top of the chunk — the
    /// "green cloud" visible inside cliffs and near sea-level terrain.
    fn column_has_solid(octree: &Octree, vx: i64, vz: i64, min_y: i64, max_y: i64) -> bool {
        for vy in (min_y..max_y).rev() {
            let mat = octree.get_voxel(VoxelCoord::new(vx, vy, vz));
            if mat != MaterialId::AIR && mat != MaterialId::WATER {
                return true;
            }
        }
        false
    }

    fn top_material_in_column(
        octree: &Octree,
        vx: i64,
        vz: i64,
        min_y: i64,
        max_y: i64,
    ) -> Option<MaterialId> {
        for vy in (min_y..max_y).rev() {
            let mat = octree.get_voxel(VoxelCoord::new(vx, vy, vz));
            if mat != MaterialId::AIR {
                return Some(mat);
            }
        }
        None
    }

    fn compute_surface_cache_column(
        octree: &Octree,
        upper_octree: Option<&Octree>,
        chunk_id: &ChunkId,
        vx: i64,
        vz: i64,
    ) -> f64 {
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let upper_chunk_id = ChunkId::new(chunk_id.x, chunk_id.y + 1, chunk_id.z);
        let upper_min_v = upper_chunk_id.min_voxel();
        let upper_max_v = upper_chunk_id.max_voxel();

        let mut top_vy: Option<i64> = None;
        for vy in (min_v.y..max_v.y).rev() {
            let mat = octree.get_voxel(VoxelCoord::new(vx, vy, vz));
            if mat != MaterialId::AIR && mat != MaterialId::WATER {
                top_vy = Some(vy);
                break;
            }
        }

        match top_vy {
            None => min_v.y as f64 - 1.0,
            Some(vy) if vy < max_v.y - 1 => vy as f64 + 0.5,
            Some(_) => {
                if upper_octree
                    .map(|upper| {
                        Self::column_has_solid(upper, vx, vz, upper_min_v.y, upper_max_v.y)
                    })
                    .unwrap_or(false)
                {
                    max_v.y as f64 + 2.0
                } else {
                    (max_v.y - 1) as f64 + 0.5
                }
            }
        }
    }

    fn column_prefers_reconstructed_surface(
        top_material: Option<MaterialId>,
        stored_surface_y: f64,
        reconstructed_surface_y: f64,
    ) -> bool {
        let height_delta = (reconstructed_surface_y - stored_surface_y).abs();
        let road_like_surface = matches!(
            top_material,
            Some(
                MaterialId::ASPHALT
                    | MaterialId::CONCRETE
                    | MaterialId::GRAVEL
                    | MaterialId::STONE
            )
        );
        road_like_surface || height_delta > 1.0
    }

    fn merge_stored_surface_cache(
        octree: &Octree,
        upper_octree: Option<&Octree>,
        chunk_id: &ChunkId,
        mut stored_surface_cache: SurfaceCache,
    ) -> SurfaceCache {
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();

        for vx in min_v.x..max_v.x {
            for vz in min_v.z..max_v.z {
                let reconstructed_surface_y =
                    Self::compute_surface_cache_column(octree, upper_octree, chunk_id, vx, vz);
                let top_material =
                    Self::top_material_in_column(octree, vx, vz, min_v.y, max_v.y);

                match stored_surface_cache.get_mut(&(vx, vz)) {
                    Some(stored_surface_y)
                        if Self::column_prefers_reconstructed_surface(
                            top_material,
                            *stored_surface_y,
                            reconstructed_surface_y,
                        ) =>
                    {
                        *stored_surface_y = reconstructed_surface_y;
                    }
                    Some(_) => {}
                    None => {
                        stored_surface_cache.insert((vx, vz), reconstructed_surface_y);
                    }
                }
            }
        }

        stored_surface_cache
    }

    fn compute_surface_cache(
        octree: &Octree,
        upper_octree: Option<&Octree>,
        chunk_id: &ChunkId,
    ) -> SurfaceCache {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let mut cache = SurfaceCache::with_capacity((CHUNK_SIZE_X * CHUNK_SIZE_Z) as usize);
        for vx in min_v.x..max_v.x {
            for vz in min_v.z..max_v.z {
                let surface_y =
                    Self::compute_surface_cache_column(octree, upper_octree, chunk_id, vx, vz);
                cache.insert((vx, vz), surface_y);
            }
        }
        cache
    }

    /// Load a chunk payload from the TileStore.
    /// Returns `(selected_pass, octree, surface_cache)` on hit; legacy octree-only payloads
    /// decode with `surface_cache = None` so callers can fall back to
    /// reconstruction when exact smooth-surface data is unavailable.
    fn load_from_store(
        ts: &TileStore,
        chunk_id: &ChunkId,
    ) -> Option<(PassId, Octree, Option<SurfaceCache>)> {
        let (pass_id, data) = [PassId::Roads, PassId::Hydro, PassId::Terrain]
            .into_iter()
            .find_map(|pass| {
                ts.get_chunk_pass(
                    chunk_id.x as i32,
                    chunk_id.y as i32,
                    chunk_id.z as i32,
                    pass,
                )
                .map(|data| (pass, data))
            })?;
        decode_stored_chunk(&data).map(|(octree, surface_cache)| (pass_id, octree, surface_cache))
    }

    /// Persist a generated chunk octree to the TileStore.
    fn save_to_store(
        ts: &TileStore,
        chunk_id: &ChunkId,
        octree: &Octree,
        surface_cache: Option<&SurfaceCache>,
    ) {
        if let Ok(data) = encode_stored_chunk(octree, surface_cache) {
            ts.put_chunk_pass(
                chunk_id.x as i32,
                chunk_id.y as i32,
                chunk_id.z as i32,
                PassId::Terrain,
                &data,
            );
        }
    }

    /// Shutdown background thread gracefully
    pub fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(LoaderCommand::Shutdown);
    }
}

/// One-time migration: import all `terrain_cache/*.bin` flat files into the TileStore,
/// then delete the flat files.  Safe to call every startup — skips files whose chunk
/// is already in the DB, and exits cleanly if the directory doesn't exist.
pub fn migrate_flat_terrain_cache(cache_dir: &Path, ts: &TileStore) {
    let dir = match std::fs::read_dir(cache_dir) {
        Ok(d) => d,
        Err(_) => return, // directory gone or never existed
    };

    let mut migrated = 0u32;
    let mut skipped = 0u32;

    for entry in dir.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) if n.ends_with(".bin") => n.to_owned(),
            _ => continue,
        };
        // Parse "x_y_z.bin"
        let parts: Vec<&str> = name.trim_end_matches(".bin").split('_').collect();
        if parts.len() != 3 {
            continue;
        }
        let (cx, cy, cz) = match (
            parts[0].parse::<i32>(),
            parts[1].parse::<i32>(),
            parts[2].parse::<i32>(),
        ) {
            (Ok(x), Ok(y), Ok(z)) => (x, y, z),
            _ => continue,
        };

        // Skip if already in DB (e.g. partial migration from a previous run)
        if ts.has_chunk_pass(cx, cy, cz, PassId::Terrain) {
            let _ = std::fs::remove_file(&path);
            skipped += 1;
            continue;
        }

        // Read, validate version header, insert
        if let Ok(data) = std::fs::read(&path) {
            if data.len() >= 4 {
                let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                if version == TERRAIN_CACHE_VERSION || version == LEGACY_TERRAIN_CACHE_VERSION {
                    ts.put_chunk_pass(cx, cy, cz, PassId::Terrain, &data);
                    migrated += 1;
                }
                // Stale version: just delete without migrating
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    // Remove the now-empty directory (best-effort; ignore if non-empty)
    let _ = std::fs::remove_dir(cache_dir);

    if migrated > 0 || skipped > 0 {
        println!(
            "📦 Migrated {} terrain chunks from flat files to TileStore ({} already present)",
            migrated, skipped
        );
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
    use crate::{
        coordinates::GPS, elevation::ElevationPipeline, terrain::TerrainGenerator,
        voxel::VoxelCoord,
    };
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    fn make_loader() -> ChunkLoader {
        let pipeline = ElevationPipeline::new();
        let terrain_gen =
            TerrainGenerator::new(pipeline, GPS::new(0.0, 0.0, 0.0), VoxelCoord::new(0, 0, 0));
        ChunkLoader::new(Arc::new(Mutex::new(terrain_gen)), 2, None)
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
            loader
                .request_load(ChunkId::new(i, 0, 0), i as f64)
                .unwrap();
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

    #[test]
    fn compute_surface_cache_preserves_chunk_top_surface_when_upper_chunk_is_air() {
        let chunk_id = ChunkId::new(0, 0, 0);
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let vx = min_v.x;
        let vz = min_v.z;
        let mut octree = Octree::new();
        octree.set_voxel(VoxelCoord::new(vx, max_v.y - 1, vz), MaterialId::GRASS_DRY);
        let upper_octree = Octree::new();

        let cache = ChunkLoader::compute_surface_cache(&octree, Some(&upper_octree), &chunk_id);

        assert_eq!(
            cache.get(&(vx, vz)).copied(),
            Some((max_v.y - 1) as f64 + 0.5)
        );
    }

    #[test]
    fn compute_surface_cache_pushes_buried_chunk_top_when_upper_chunk_has_solid() {
        let chunk_id = ChunkId::new(0, 0, 0);
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let vx = min_v.x;
        let vz = min_v.z;
        let mut octree = Octree::new();
        octree.set_voxel(VoxelCoord::new(vx, max_v.y - 1, vz), MaterialId::DIRT);

        let upper_chunk_id = ChunkId::new(0, 1, 0);
        let upper_min_v = upper_chunk_id.min_voxel();
        let mut upper_octree = Octree::new();
        upper_octree.set_voxel(VoxelCoord::new(vx, upper_min_v.y, vz), MaterialId::STONE);

        let cache = ChunkLoader::compute_surface_cache(&octree, Some(&upper_octree), &chunk_id);

        assert_eq!(cache.get(&(vx, vz)).copied(), Some(max_v.y as f64 + 2.0));
    }

    #[test]
    fn compute_surface_cache_preserves_half_voxel_precision_at_high_world_y() {
        let chunk_id = ChunkId::new(0, 45_148, 0);
        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let vx = min_v.x;
        let vz = min_v.z;
        let mut octree = Octree::new();
        octree.set_voxel(VoxelCoord::new(vx, max_v.y - 1, vz), MaterialId::GRASS_DRY);

        let cache = ChunkLoader::compute_surface_cache(&octree, None, &chunk_id);

        assert_eq!(
            cache.get(&(vx, vz)).copied(),
            Some((max_v.y - 1) as f64 + 0.5)
        );
    }

    #[test]
    fn stored_chunk_payload_round_trips_surface_cache() {
        let pos = VoxelCoord::new(10, 20, 30);
        let mut octree = Octree::new();
        octree.set_voxel(pos, MaterialId::GRASS_DRY);

        let mut surface_cache = SurfaceCache::new();
        surface_cache.insert((10, 30), 1234.25);

        let data = encode_stored_chunk(&octree, Some(&surface_cache)).expect("encode stored chunk");
        let (decoded_octree, decoded_surface_cache) =
            decode_stored_chunk(&data).expect("decode stored chunk");

        assert_eq!(decoded_octree.get_voxel(pos), MaterialId::GRASS_DRY);
        assert_eq!(
            decoded_surface_cache
                .and_then(|cache| cache.get(&(10, 30)).copied()),
            Some(1234.25)
        );
    }

    #[test]
    fn stored_chunk_payload_decodes_legacy_octree_only_format() {
        let pos = VoxelCoord::new(8, 9, 10);
        let mut octree = Octree::new();
        octree.set_voxel(pos, MaterialId::STONE);

        let mut data = LEGACY_TERRAIN_CACHE_VERSION.to_le_bytes().to_vec();
        data.extend_from_slice(&octree.to_bytes().expect("legacy encode octree"));

        let (decoded_octree, decoded_surface_cache) =
            decode_stored_chunk(&data).expect("decode legacy stored chunk");

        assert_eq!(decoded_octree.get_voxel(pos), MaterialId::STONE);
        assert!(decoded_surface_cache.is_none());
    }
}
