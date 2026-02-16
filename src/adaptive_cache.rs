//! Adaptive Cache System for Continuous Queries
//!
//! Three-tier caching:
//! - Hot: Recent queries (HashMap, fast)
//! - Warm: Frequent queries (LRU, eviction)
//! - Cold: Disk storage (compressed)

use std::collections::HashMap;
use std::path::PathBuf;
use lru::LruCache;
use std::num::NonZeroUsize;
use crate::spatial_index::VoxelBlock;
use serde::{Serialize, Deserialize};

/// Cache key for voxel blocks
///
/// Uses integer coordinates (millimeter precision) to avoid floating-point
/// comparison issues. Snapped to 8m grid to ensure deterministic keys.
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockKey {
    /// Block position in millimeters (ECEF)
    pub ecef_mm: [i64; 3],
}

impl BlockKey {
    /// Create key from ECEF position (snapped to 8m grid)
    pub fn from_ecef(ecef: [f64; 3], block_size: f64) -> Self {
        // Snap to block grid
        let snapped = [
            snap_to_grid(ecef[0], block_size),
            snap_to_grid(ecef[1], block_size),
            snap_to_grid(ecef[2], block_size),
        ];
        
        // Convert to millimeters (integer)
        Self {
            ecef_mm: [
                (snapped[0] * 1000.0).round() as i64,
                (snapped[1] * 1000.0).round() as i64,
                (snapped[2] * 1000.0).round() as i64,
            ],
        }
    }
    
    /// Convert back to ECEF position (meters)
    pub fn to_ecef(&self) -> [f64; 3] {
        [
            self.ecef_mm[0] as f64 / 1000.0,
            self.ecef_mm[1] as f64 / 1000.0,
            self.ecef_mm[2] as f64 / 1000.0,
        ]
    }
}

/// Snap coordinate to block grid (deterministic)
fn snap_to_grid(coord: f64, block_size: f64) -> f64 {
    (coord / block_size).floor() * block_size
}

/// Cache statistics for monitoring
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hot_hits: u64,
    pub warm_hits: u64,
    pub cold_hits: u64,
    pub misses: u64,
    pub total_queries: u64,
}

impl CacheStats {
    /// Calculate overall cache hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        if self.total_queries == 0 {
            return 0.0;
        }
        let hits = self.hot_hits + self.warm_hits + self.cold_hits;
        hits as f64 / self.total_queries as f64
    }
    
    /// Reset statistics
    pub fn reset(&mut self) {
        *self = Default::default();
    }
}

/// Three-tier adaptive cache
///
/// - Hot: Recent queries (HashMap, O(1) access)
/// - Warm: Frequent queries (LRU, O(1) access with eviction)
/// - Cold: Disk storage (compressed, slower but unlimited)
pub struct AdaptiveCache {
    /// Hot cache - most recent queries
    hot: HashMap<BlockKey, VoxelBlock>,
    hot_capacity: usize,
    
    /// Warm cache - frequently accessed
    warm: LruCache<BlockKey, VoxelBlock>,
    
    /// Cold cache - disk storage
    cold: DiskCache,
    
    /// Statistics
    stats: CacheStats,
    
    /// Block size (for key generation)
    block_size: f64,
}

impl AdaptiveCache {
    /// Create new adaptive cache
    ///
    /// # Arguments
    /// - `hot_capacity` - Number of blocks in hot cache (e.g., 1000)
    /// - `warm_capacity` - Number of blocks in warm cache (e.g., 5000)
    /// - `cold_path` - Directory for disk cache
    /// - `block_size` - Block size in meters (e.g., 8.0)
    pub fn new(
        hot_capacity: usize,
        warm_capacity: usize,
        cold_path: PathBuf,
        block_size: f64,
    ) -> Self {
        Self {
            hot: HashMap::with_capacity(hot_capacity),
            hot_capacity,
            warm: LruCache::new(NonZeroUsize::new(warm_capacity).unwrap()),
            cold: DiskCache::new(cold_path),
            stats: Default::default(),
            block_size,
        }
    }
    
    /// Get block from cache (checks hot → warm → cold)
    pub fn get(&mut self, ecef: [f64; 3]) -> Option<VoxelBlock> {
        let key = BlockKey::from_ecef(ecef, self.block_size);
        self.stats.total_queries += 1;
        
        // Check hot cache
        if let Some(block) = self.hot.get(&key) {
            self.stats.hot_hits += 1;
            return Some(block.clone());
        }
        
        // Check warm cache
        if let Some(block) = self.warm.get(&key) {
            self.stats.warm_hits += 1;
            // Clone before promoting to avoid borrow conflict
            let block_clone = block.clone();
            // Promote to hot
            self.insert_hot(key, block_clone.clone());
            return Some(block_clone);
        }
        
        // Check cold cache (disk)
        if let Some(block) = self.cold.load(&key) {
            self.stats.cold_hits += 1;
            // Promote to warm
            self.warm.put(key, block.clone());
            return Some(block);
        }
        
        self.stats.misses += 1;
        None
    }
    
    /// Insert block into cache (all tiers)
    pub fn insert(&mut self, block: VoxelBlock) {
        let key = BlockKey::from_ecef(block.ecef_min, self.block_size);
        
        // Insert into all tiers
        self.cold.save(&key, &block);
        self.warm.put(key, block.clone());
        self.insert_hot(key, block);
    }
    
    /// Insert into hot cache with eviction
    fn insert_hot(&mut self, key: BlockKey, block: VoxelBlock) {
        // If at capacity, evict random entry
        if self.hot.len() >= self.hot_capacity {
            // Remove first entry (random from HashMap perspective)
            if let Some(first_key) = self.hot.keys().next().copied() {
                self.hot.remove(&first_key);
            }
        }
        
        self.hot.insert(key, block);
    }
    
    /// Get cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }
    
    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats.reset();
    }
    
    /// Get current cache sizes
    pub fn sizes(&self) -> (usize, usize) {
        (self.hot.len(), self.warm.len())
    }
    
    /// Clear all caches (keeps disk cache)
    pub fn clear(&mut self) {
        self.hot.clear();
        self.warm.clear();
    }
}

/// Disk cache with compression
pub struct DiskCache {
    base_path: PathBuf,
}

impl DiskCache {
    /// Create new disk cache
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }
    
    /// Save block to disk (compressed)
    pub fn save(&self, key: &BlockKey, block: &VoxelBlock) {
        let path = self.block_path(key);
        
        // Create parent directories
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        
        // Serialize with bincode
        if let Ok(serialized) = bincode::serialize(block) {
            // Write directly (compression can be added later if needed)
            let _ = std::fs::write(path, serialized);
        }
    }
    
    /// Load block from disk
    pub fn load(&self, key: &BlockKey) -> Option<VoxelBlock> {
        let path = self.block_path(key);
        
        if !path.exists() {
            return None;
        }
        
        let bytes = std::fs::read(path).ok()?;
        bincode::deserialize(&bytes).ok()
    }
    
    /// Check if block exists on disk
    pub fn exists(&self, key: &BlockKey) -> bool {
        self.block_path(key).exists()
    }
    
    /// Remove block from disk
    pub fn remove(&self, key: &BlockKey) -> bool {
        let path = self.block_path(key);
        std::fs::remove_file(path).is_ok()
    }
    
    /// Get file path for block
    ///
    /// Uses hierarchical directory structure to avoid millions of files
    /// in single directory:
    /// `base_path/X_bucket/Y_bucket/Z_bucket/block.bin`
    fn block_path(&self, key: &BlockKey) -> PathBuf {
        // Bucket by 1000m (1km) to create directory hierarchy
        let x_bucket = key.ecef_mm[0] / 1_000_000; // km
        let y_bucket = key.ecef_mm[1] / 1_000_000;
        let z_bucket = key.ecef_mm[2] / 1_000_000;
        
        // File name includes exact position
        let filename = format!(
            "block_{}_{}_{}. bin",
            key.ecef_mm[0], key.ecef_mm[1], key.ecef_mm[2]
        );
        
        self.base_path
            .join(format!("x{}", x_bucket))
            .join(format!("y{}", y_bucket))
            .join(format!("z{}", z_bucket))
            .join(filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svo::AIR;
    use tempfile::TempDir;
    
    fn create_test_block(ecef: [f64; 3]) -> VoxelBlock {
        VoxelBlock::new(ecef, 8.0)
    }
    
    #[test]
    fn test_block_key_from_ecef() {
        let key1 = BlockKey::from_ecef([100.0, 200.0, 300.0], 8.0);
        let key2 = BlockKey::from_ecef([100.5, 200.5, 300.5], 8.0);
        
        // Should snap to same grid position
        assert_eq!(key1, key2);
        
        // Different grid positions
        let key3 = BlockKey::from_ecef([108.0, 200.0, 300.0], 8.0);
        assert_ne!(key1, key3);
    }
    
    #[test]
    fn test_block_key_roundtrip() {
        let original = [100.0, 200.0, 300.0];
        let key = BlockKey::from_ecef(original, 8.0);
        let recovered = key.to_ecef();
        
        // Should recover snapped position
        assert_eq!(recovered, [96.0, 200.0, 296.0]); // Snapped to 8m grid
    }
    
    #[test]
    fn test_cache_stats() {
        let mut stats = CacheStats::default();
        
        assert_eq!(stats.hit_rate(), 0.0);
        
        stats.total_queries = 100;
        stats.hot_hits = 80;
        stats.warm_hits = 15;
        stats.cold_hits = 3;
        stats.misses = 2;
        
        assert_eq!(stats.hit_rate(), 0.98);
    }
    
    #[test]
    fn test_adaptive_cache_hot() {
        let temp_dir = TempDir::new().unwrap();
        let mut cache = AdaptiveCache::new(10, 100, temp_dir.path().to_path_buf(), 8.0);
        
        let block = create_test_block([100.0, 200.0, 300.0]);
        cache.insert(block.clone());
        
        // Should hit hot cache
        let retrieved = cache.get([100.0, 200.0, 300.0]).unwrap();
        assert_eq!(retrieved.ecef_min, block.ecef_min);
        assert_eq!(cache.stats().hot_hits, 1);
    }
    
    #[test]
    fn test_adaptive_cache_warm_promotion() {
        let temp_dir = TempDir::new().unwrap();
        let mut cache = AdaptiveCache::new(2, 100, temp_dir.path().to_path_buf(), 8.0);
        
        // Insert 3 blocks (will overflow hot cache)
        let block1 = create_test_block([100.0, 200.0, 300.0]);
        let block2 = create_test_block([108.0, 200.0, 300.0]);
        let block3 = create_test_block([116.0, 200.0, 300.0]);
        
        cache.insert(block1.clone());
        cache.insert(block2.clone());
        cache.insert(block3.clone());
        
        // Clear hot cache to force warm cache access
        cache.hot.clear();
        
        // Should hit warm cache and promote to hot
        cache.reset_stats();
        let _retrieved = cache.get([100.0, 200.0, 300.0]).unwrap();
        assert_eq!(cache.stats().warm_hits, 1);
        assert_eq!(cache.hot.len(), 1); // Promoted to hot
    }
    
    #[test]
    fn test_adaptive_cache_miss() {
        let temp_dir = TempDir::new().unwrap();
        let mut cache = AdaptiveCache::new(10, 100, temp_dir.path().to_path_buf(), 8.0);
        
        // Query non-existent block
        let result = cache.get([999.0, 999.0, 999.0]);
        assert!(result.is_none());
        assert_eq!(cache.stats().misses, 1);
    }
    
    #[test]
    fn test_disk_cache_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let cache = DiskCache::new(temp_dir.path().to_path_buf());
        
        let block = create_test_block([100.0, 200.0, 300.0]);
        let key = BlockKey::from_ecef(block.ecef_min, 8.0);
        
        // Save and load
        cache.save(&key, &block);
        let loaded = cache.load(&key).unwrap();
        
        assert_eq!(loaded.ecef_min, block.ecef_min);
        assert_eq!(loaded.size, block.size);
    }
    
    #[test]
    fn test_disk_cache_hierarchical_path() {
        let temp_dir = TempDir::new().unwrap();
        let cache = DiskCache::new(temp_dir.path().to_path_buf());
        
        let key = BlockKey::from_ecef([1000.0, 2000.0, 3000.0], 8.0);
        let path = cache.block_path(&key);
        
        // Should create hierarchical structure
        // Buckets are based on mm/1_000_000 (km)
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("x1")); // 1000m = 1km bucket
        assert!(path_str.contains("y2")); // 2000m = 2km bucket
        assert!(path_str.contains("z3")); // 3000m = 3km bucket
    }
    
    #[test]
    fn test_hot_cache_eviction() {
        let temp_dir = TempDir::new().unwrap();
        let mut cache = AdaptiveCache::new(3, 100, temp_dir.path().to_path_buf(), 8.0);
        
        // Insert 4 blocks (should evict 1 from hot)
        for i in 0..4 {
            let block = create_test_block([100.0 + i as f64 * 8.0, 200.0, 300.0]);
            cache.insert(block);
        }
        
        // Hot cache should be at capacity
        let (hot_size, _) = cache.sizes();
        assert_eq!(hot_size, 3);
    }
    
    #[test]
    fn test_cache_clear() {
        let temp_dir = TempDir::new().unwrap();
        let mut cache = AdaptiveCache::new(10, 100, temp_dir.path().to_path_buf(), 8.0);
        
        let block = create_test_block([100.0, 200.0, 300.0]);
        cache.insert(block);
        
        cache.clear();
        
        let (hot_size, warm_size) = cache.sizes();
        assert_eq!(hot_size, 0);
        assert_eq!(warm_size, 0);
    }
}
