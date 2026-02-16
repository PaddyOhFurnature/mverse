//! Continuous World - Public API for Continuous Query System
//!
//! Provides seamless query interface for voxel data without chunk awareness.
//! Integrates spatial index, adaptive cache, and procedural generation.

use crate::spatial_index::{AABB, VoxelBlock, SpatialIndex};
use crate::adaptive_cache::{AdaptiveCache, BlockKey, CacheStats};
use crate::procedural_generator::{ProceduralGenerator, GeneratorConfig};
use crate::coordinates::EcefPos;
use crate::svo::MaterialId;
use std::path::PathBuf;
use dirs;

/// Camera frustum for visibility queries
#[derive(Debug, Clone)]
pub struct Frustum {
    /// Camera position in ECEF
    pub position: [f64; 3],
    /// View direction (unit vector)
    pub direction: [f64; 3],
    /// Field of view in degrees
    pub fov_degrees: f64,
    /// Aspect ratio (width / height)
    pub aspect: f64,
    /// Near plane distance
    pub near: f64,
    /// Far plane distance
    pub far: f64,
}

impl Frustum {
    /// Create frustum from camera parameters
    pub fn from_camera(
        position: [f64; 3],
        direction: [f64; 3],
        fov_degrees: f64,
        aspect: f64,
    ) -> Self {
        Self {
            position,
            direction,
            fov_degrees,
            aspect,
            near: 0.1,
            far: 1000.0,
        }
    }
    
    /// Get bounding AABB that encompasses entire frustum
    /// (Conservative approximation - includes more than strictly visible)
    pub fn bounding_aabb(&self) -> AABB {
        // Simplified: sphere around camera position
        // TODO Phase 3: True frustum culling
        let radius = self.far;
        AABB::from_center(self.position, radius)
    }
}

/// Continuous world query interface
///
/// Provides seamless access to voxel data without chunk boundaries.
/// Uses spatial index + cache + procedural generation.
pub struct ContinuousWorld {
    /// Spatial index for voxel blocks
    index: SpatialIndex,
    
    /// Adaptive cache (hot/warm/cold)
    cache: AdaptiveCache,
    
    /// Procedural generator
    generator: ProceduralGenerator,
    
    /// Test area bounds (for prototype)
    bounds: AABB,
    
    /// Block size in meters
    block_size: f64,
}

impl ContinuousWorld {
    /// Create new continuous world for test area
    ///
    /// # Arguments
    /// - `center_ecef` - Center of test area in ECEF coordinates
    /// - `extent` - Half-size of test area in meters (e.g., 100.0 for 200m area)
    /// - `block_size` - Block size in meters (default: 8.0)
    ///
    /// # Example
    /// ```no_run
    /// use metaverse_core::continuous_world::ContinuousWorld;
    /// 
    /// // Kangaroo Point test location
    /// let center = [-5047081.96, 2567891.19, -2925600.68];
    /// let world = ContinuousWorld::new(center, 100.0).unwrap();
    /// ```
    pub fn new(center_ecef: [f64; 3], extent: f64) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_block_size(center_ecef, extent, 8.0)
    }
    
    /// Create with custom block size (for testing different granularities)
    pub fn with_block_size(center_ecef: [f64; 3], extent: f64, block_size: f64) -> Result<Self, Box<dyn std::error::Error>> {
        let bounds = AABB::from_center(center_ecef, extent);
        
        // Cache configuration for test area
        let hot_capacity = 1000;   // ~1 MB
        let warm_capacity = 5000;  // ~5 MB
        
        // Cache directory
        let cache_base = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("metaverse");
        
        let block_cache_path = cache_base.join("blocks");
        let srtm_cache_path = cache_base.join("srtm");
        let osm_cache_path = cache_base.join("osm");
        
        // Create procedural generator
        let generator_config = GeneratorConfig {
            srtm_cache_path,
            osm_cache_path,
            area_center: EcefPos {
                x: center_ecef[0],
                y: center_ecef[1],
                z: center_ecef[2],
            },
            area_radius: extent,
        };
        
        let generator = ProceduralGenerator::new(generator_config)?;
        
        Ok(Self {
            index: SpatialIndex::new(bounds),
            cache: AdaptiveCache::new(hot_capacity, warm_capacity, block_cache_path, block_size),
            generator,
            bounds,
            block_size,
        })
    }
    
    /// Query voxel blocks in arbitrary AABB
    ///
    /// Returns all blocks that intersect the query bounds.
    /// Blocks are fetched from cache or generated on demand.
    ///
    /// # Arguments
    /// - `query_bounds` - AABB to query (in ECEF coordinates)
    ///
    /// # Returns
    /// Vector of blocks intersecting the bounds
    pub fn query_range(&mut self, query_bounds: AABB) -> Vec<VoxelBlock> {
        // Clamp to world bounds
        let clamped = self.clamp_to_bounds(query_bounds);
        
        // Find all block keys in this range
        let keys = self.block_keys_in_bounds(clamped);
        
        // Get or generate each block
        keys.into_iter()
            .filter_map(|key| self.get_or_generate_block(key))
            .collect()
    }
    
    /// Query voxel blocks visible in camera frustum
    ///
    /// Returns blocks visible from camera position and direction.
    /// Currently uses conservative bounding AABB.
    ///
    /// # Arguments
    /// - `frustum` - Camera frustum
    ///
    /// # Returns
    /// Vector of visible blocks
    pub fn query_frustum(&mut self, frustum: &Frustum) -> Vec<VoxelBlock> {
        // TODO Phase 3: True frustum culling
        // For now, use bounding AABB (conservative but correct)
        let bounds = frustum.bounding_aabb();
        self.query_range(bounds)
    }
    
    /// Sample material at single point
    ///
    /// Returns material at exact ECEF position.
    /// Fast path for single-point queries (e.g., collision detection).
    ///
    /// # Arguments
    /// - `ecef` - Position in ECEF coordinates
    ///
    /// # Returns
    /// Material at position, or AIR if outside bounds
    pub fn sample_point(&mut self, ecef: [f64; 3]) -> MaterialId {
        // Check if point is in world bounds
        if !self.bounds.contains(ecef) {
            return crate::svo::AIR;
        }
        
        // Get block containing this point
        let key = BlockKey::from_ecef(ecef, self.block_size);
        
        if let Some(block) = self.get_or_generate_block(key) {
            block.sample_voxel(ecef).unwrap_or(crate::svo::AIR)
        } else {
            crate::svo::AIR
        }
    }
    
    /// Get cache statistics
    pub fn cache_stats(&self) -> &CacheStats {
        self.cache.stats()
    }
    
    /// Reset cache statistics
    pub fn reset_cache_stats(&mut self) {
        self.cache.reset_stats();
    }
    
    /// Get world bounds
    pub fn bounds(&self) -> AABB {
        self.bounds
    }
    
    /// Get block size
    pub fn block_size(&self) -> f64 {
        self.block_size
    }
    
    // === Private Helper Methods ===
    
    /// Get block from cache or generate if missing
    fn get_or_generate_block(&mut self, key: BlockKey) -> Option<VoxelBlock> {
        let ecef = key.to_ecef();
        
        // Try cache first
        if let Some(block) = self.cache.get(ecef) {
            return Some(block);
        }
        
        // Cache miss - generate block
        let block = self.generator.generate_block(ecef);
        
        // Insert into cache
        self.cache.insert(block.clone());
        
        Some(block)
    }
    
    
    /// Pre-load SRTM elevation data for test area
    pub fn load_elevation_data(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.generator.load_srtm_tiles()
    }
    
    /// Pre-load OSM features for test area
    pub fn load_osm_features(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.generator.load_osm_features()
    }
    
    /// Find all block keys that intersect given bounds
    fn block_keys_in_bounds(&self, bounds: AABB) -> Vec<BlockKey> {
        let mut keys = Vec::new();
        
        // Calculate block grid range
        let min_x = (bounds.min[0] / self.block_size).floor() * self.block_size;
        let min_y = (bounds.min[1] / self.block_size).floor() * self.block_size;
        let min_z = (bounds.min[2] / self.block_size).floor() * self.block_size;
        
        let max_x = (bounds.max[0] / self.block_size).ceil() * self.block_size;
        let max_y = (bounds.max[1] / self.block_size).ceil() * self.block_size;
        let max_z = (bounds.max[2] / self.block_size).ceil() * self.block_size;
        
        // Iterate over grid
        let mut x = min_x;
        while x < max_x {
            let mut y = min_y;
            while y < max_y {
                let mut z = min_z;
                while z < max_z {
                    let key = BlockKey::from_ecef([x, y, z], self.block_size);
                    keys.push(key);
                    z += self.block_size;
                }
                y += self.block_size;
            }
            x += self.block_size;
        }
        
        keys
    }
    
    /// Clamp query bounds to world bounds
    fn clamp_to_bounds(&self, bounds: AABB) -> AABB {
        AABB {
            min: [
                bounds.min[0].max(self.bounds.min[0]),
                bounds.min[1].max(self.bounds.min[1]),
                bounds.min[2].max(self.bounds.min[2]),
            ],
            max: [
                bounds.max[0].min(self.bounds.max[0]),
                bounds.max[1].min(self.bounds.max[1]),
                bounds.max[2].min(self.bounds.max[2]),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // Test location: Kangaroo Point, Brisbane
    const TEST_CENTER: [f64; 3] = [-5047081.96, 2567891.19, -2925600.68];
    
    #[test]
    fn test_continuous_world_creation() {
        let world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        let bounds = world.bounds();
        assert_eq!(bounds.min[0], TEST_CENTER[0] - 100.0);
        assert_eq!(bounds.max[0], TEST_CENTER[0] + 100.0);
        assert_eq!(world.block_size(), 8.0);
    }
    
    #[test]
    fn test_query_range_single_block() {
        let mut world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        // Query small region (1m cube - should fit in 1 block)
        let query = AABB::from_center(TEST_CENTER, 0.5); // 1m cube
        let blocks = world.query_range(query);
        
        assert!(blocks.len() >= 1 && blocks.len() <= 8, 
                "Small query should return 1-8 blocks depending on alignment, got {}", blocks.len());
    }
    
    #[test]
    fn test_query_range_multiple_blocks() {
        let mut world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        // Query larger region (should span multiple blocks)
        let query = AABB::from_center(TEST_CENTER, 20.0); // 40m cube
        let blocks = world.query_range(query);
        
        // 40m span with 8m blocks could be up to ceil(40/8) = 5 blocks per axis
        // But with alignment, could be 6 per axis = 6³ = 216 blocks max
        assert!(blocks.len() > 1, "Large query should return multiple blocks");
        assert!(blocks.len() <= 216, "Should not exceed 6³=216 blocks, got {}", blocks.len());
    }
    
    #[test]
    fn test_sample_point_inside_bounds() {
        let mut world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        // Sample point inside bounds
        let material = world.sample_point(TEST_CENTER);
        
        // Should return valid material (AIR for now since we're using placeholder)
        assert_eq!(material, crate::svo::AIR);
    }
    
    #[test]
    fn test_sample_point_outside_bounds() {
        let mut world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        // Sample point far outside bounds
        let far_point = [TEST_CENTER[0] + 1000.0, TEST_CENTER[1], TEST_CENTER[2]];
        let material = world.sample_point(far_point);
        
        // Should return AIR for outside bounds
        assert_eq!(material, crate::svo::AIR);
    }
    
    #[test]
    fn test_cache_hit_on_second_query() {
        let mut world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        let query = AABB::from_center(TEST_CENTER, 5.0);
        
        // First query
        world.reset_cache_stats();
        let _blocks1 = world.query_range(query);
        let stats1 = world.cache_stats();
        assert!(stats1.total_queries > 0, "First query should have queries");
        
        // Second query (should hit cache - blocks already generated)
        world.reset_cache_stats();
        let _blocks2 = world.query_range(query);
        let stats2 = world.cache_stats();
        
        assert!(stats2.total_queries > 0, "Second query should have queries");
        assert!(stats2.hot_hits > 0 || stats2.warm_hits > 0 || stats2.cold_hits > 0, 
                "Second query should hit cache");
        assert_eq!(stats2.misses, 0, "Second query should have no misses");
    }
    
    #[test]
    fn test_frustum_bounding_aabb() {
        let frustum = Frustum::from_camera(
            TEST_CENTER,
            [0.0, 0.0, -1.0], // Looking down
            90.0,
            16.0 / 9.0,
        );
        
        let aabb = frustum.bounding_aabb();
        
        // Should be sphere around camera
        assert_eq!(aabb.min[0], TEST_CENTER[0] - frustum.far);
        assert_eq!(aabb.max[0], TEST_CENTER[0] + frustum.far);
    }
    
    #[test]
    fn test_query_frustum() {
        let mut world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        let frustum = Frustum::from_camera(
            TEST_CENTER,
            [0.0, 0.0, -1.0],
            90.0,
            16.0 / 9.0,
        );
        
        let blocks = world.query_frustum(&frustum);
        
        // Should return blocks (using bounding AABB for now)
        assert!(!blocks.is_empty(), "Frustum query should return blocks");
    }
    
    #[test]
    fn test_block_keys_in_bounds() {
        let world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        // Query exact 8m cube (should span 1 block if perfectly aligned)
        // But since center may not align with grid, could be up to 2³=8 blocks
        let bounds = AABB::from_corners(
            TEST_CENTER,
            [TEST_CENTER[0] + 8.0, TEST_CENTER[1] + 8.0, TEST_CENTER[2] + 8.0]
        );
        let keys = world.block_keys_in_bounds(bounds);
        
        // Should be reasonable number of blocks
        assert!(keys.len() >= 1 && keys.len() <= 8, 
                "8m cube should span 1-8 blocks depending on alignment, got {}", keys.len());
    }
    
    #[test]
    fn test_clamp_to_bounds() {
        let world = ContinuousWorld::new(TEST_CENTER, 100.0).unwrap();
        
        // Query partially outside world bounds
        let outside = AABB::from_center(
            [TEST_CENTER[0] + 150.0, TEST_CENTER[1], TEST_CENTER[2]],
            50.0,
        );
        
        let clamped = world.clamp_to_bounds(outside);
        
        // Should be clamped to world max
        assert!(clamped.max[0] <= world.bounds().max[0]);
        assert!(clamped.min[0] >= world.bounds().min[0]);
    }
    
    #[test]
    fn test_custom_block_size() {
        let mut world = ContinuousWorld::with_block_size(TEST_CENTER, 100.0, 16.0).unwrap();
        
        assert_eq!(world.block_size(), 16.0);
        
        // Query should use 16m blocks
        let query = AABB::from_center(TEST_CENTER, 16.0); // 32m cube
        let blocks = world.query_range(query);
        
        // Should get reasonable number of blocks for 32m cube with 16m blocks
        assert!(blocks.len() >= 1, "Query should return at least 1 block");
        assert!(blocks.len() <= 27, "32m cube with 16m blocks should not exceed 27 (3³)");
    }
}
