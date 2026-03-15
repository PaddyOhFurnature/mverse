/// Synchronous terrain generation for chunk streaming integration
///
/// Provides thread-safe terrain generation for immediate integration.
/// This is a bridge until ElevationSource becomes Send+Sync.
use crate::chunk::ChunkId;
use crate::terrain::TerrainGenerator;
use crate::voxel::Octree;
use std::sync::{Arc, Mutex};

/// Synchronous chunk-with-terrain generator
///
/// Generates terrain on the calling thread (main thread safe).
/// Use this for immediate integration with ChunkStreamer.
pub struct SyncTerrainLoader {
    generator: Arc<Mutex<TerrainGenerator>>,
}

impl SyncTerrainLoader {
    /// Create new synchronous terrain loader
    pub fn new(generator: TerrainGenerator) -> Self {
        Self {
            generator: Arc::new(Mutex::new(generator)),
        }
    }

    /// Generate terrain for a chunk (synchronous, blocks calling thread)
    ///
    /// Safe to call from main thread. Returns generated octree with real terrain.
    pub fn generate_chunk_with_terrain(&self, chunk_id: &ChunkId) -> Result<Octree, String> {
        let generator = self
            .generator
            .lock()
            .map_err(|e| format!("Failed to lock terrain generator: {}", e))?;

        generator.generate_chunk(chunk_id).map(|(octree, _)| octree)
    }
}

/// Generate terrain for a chunk using shared generator (main thread)
///
/// This is a convenience function for phase1_multiplayer integration.
/// Generates terrain synchronously on the calling thread.
///
/// # Example
/// ```no_run
/// use metaverse_core::terrain::TerrainGenerator;
/// use metaverse_core::terrain_sync::generate_chunk_terrain;
/// use metaverse_core::chunk::ChunkId;
/// use metaverse_core::elevation::ElevationPipeline;
/// use metaverse_core::coordinates::GPS;
/// use metaverse_core::voxel::VoxelCoord;
///
/// let elevation = ElevationPipeline::new();
/// let origin_gps = GPS::new(0.0, 0.0, 0.0);
/// let origin_voxel = VoxelCoord::new(0, 0, 0);
/// let generator = TerrainGenerator::new(elevation, origin_gps, origin_voxel);
///
/// let chunk_id = ChunkId::new(0, 0, 0);
/// let octree = generate_chunk_terrain(&generator, &chunk_id)?;
/// # Ok::<(), String>(())
/// ```
pub fn generate_chunk_terrain(
    generator: &TerrainGenerator,
    chunk_id: &ChunkId,
) -> Result<Octree, String> {
    // TerrainGenerator is thread-safe with Arc<RwLock<ElevationPipeline>> —
    generator.generate_chunk(chunk_id).map(|(octree, _)| octree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::GPS;
    use crate::elevation::ElevationPipeline;
    use crate::voxel::VoxelCoord;

    #[test]
    #[ignore] // Requires elevation data
    fn test_sync_terrain_loader() {
        let elevation = ElevationPipeline::new();
        let origin_gps = GPS::new(0.0, 0.0, 0.0);
        let origin_voxel = VoxelCoord::new(0, 0, 0);
        let generator = TerrainGenerator::new(elevation, origin_gps, origin_voxel);

        let loader = SyncTerrainLoader::new(generator);

        let chunk_id = ChunkId::new(0, 0, 0);
        let octree = loader.generate_chunk_with_terrain(&chunk_id).unwrap();

        // Octree should not be empty (should have terrain)
        // This is just a smoke test — generation succeeded if we got here
        let _ = octree; // use the value
    }
}
