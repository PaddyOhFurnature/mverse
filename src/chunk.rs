//! Chunk system for spatial partitioning
//!
//! Divides the world into fixed-size chunks for:
//! - Spatial sharding (only load/sync nearby chunks)
//! - Operation log organization (one file per chunk)
//! - Future: Spatial pub/sub topics (chunk-based gossipsub)
//! - Future: DHT storage (replicate per chunk, not globally)
//!
//! # Chunk Size
//!
//! Chunks are 100×100×100 voxels (100m³ cube):
//! - Player view distance: ~100-200m
//! - Chunk download: ~150 KB (reasonable for network)
//! - File count: Manageable (not millions of tiny files)
//! - Spatial granularity: Good for pub/sub topics
//!
//! # Coordinate System
//!
//! Chunk coordinates are signed integers:
//! - Origin chunk (0,0,0) contains voxels (0..100, 0..100, 0..100)
//! - Chunk (-1,0,0) contains voxels (-100..-1, 0..100, 0..100)
//! - Deterministic: Same voxel → same chunk ID always
//!
//! # File Organization
//!
//! ```text
//! world_data/
//!   chunks/
//!     chunk_0_0_0/
//!       operations.json       ← Voxel edits in this chunk
//!       metadata.json         ← Future: Chunk hash, version
//!     chunk_0_0_1/
//!       operations.json
//!     chunk_-1_5_3/
//!       operations.json
//! ```
//!
//! # Design Rationale
//!
//! **Why 100m chunks?**
//! - Player sees ~100m radius → ~8 chunks loaded
//! - Matches human-scale interaction distance
//! - Small enough for quick downloads
//! - Large enough to avoid millions of files
//!
//! **Why not dynamic sizing?**
//! - Deterministic chunk ID from coordinates (critical for P2P)
//! - Everyone calculates same chunk boundaries
//! - No central authority needed to assign chunks
//!
//! **Why cubic not spherical?**
//! - Simpler math (floor division)
//! - Aligns with voxel grid
//! - Easy to calculate neighbors
//! - Network pub/sub topics work better with discrete regions

use crate::coordinates::{ECEF, GPS};
use crate::voxel::VoxelCoord;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Size of chunk in voxels (100×100×100 = 1 million voxels)
pub const CHUNK_SIZE: i64 = 100;

/// Chunk identifier (3D grid position)
///
/// Deterministically calculated from voxel coordinates.
/// Same for all peers - no central authority needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct ChunkId {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

impl ChunkId {
    /// Create chunk ID from chunk coordinates
    pub fn new(x: i64, y: i64, z: i64) -> Self {
        Self { x, y, z }
    }
    
    /// Calculate chunk ID from voxel coordinate
    ///
    /// Uses floor division to handle negative coordinates correctly:
    /// - Voxel (50, 25, 75) → Chunk (0, 0, 0)
    /// - Voxel (150, 200, 50) → Chunk (1, 2, 0)
    /// - Voxel (-50, 25, 75) → Chunk (-1, 0, 0)
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::{ChunkId, CHUNK_SIZE};
    /// use metaverse_core::voxel::VoxelCoord;
    ///
    /// let voxel = VoxelCoord::new(150, 200, 50);
    /// let chunk = ChunkId::from_voxel(&voxel);
    /// assert_eq!(chunk, ChunkId::new(1, 2, 0));
    ///
    /// // Negative coordinates work correctly
    /// let voxel_neg = VoxelCoord::new(-50, 25, 75);
    /// let chunk_neg = ChunkId::from_voxel(&voxel_neg);
    /// assert_eq!(chunk_neg, ChunkId::new(-1, 0, 0));
    /// ```
    pub fn from_voxel(coord: &VoxelCoord) -> Self {
        Self {
            x: coord.x.div_euclid(CHUNK_SIZE),
            y: coord.y.div_euclid(CHUNK_SIZE),
            z: coord.z.div_euclid(CHUNK_SIZE),
        }
    }
    
    /// Get minimum voxel coordinate contained in this chunk (inclusive)
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::{ChunkId, CHUNK_SIZE};
    /// use metaverse_core::voxel::VoxelCoord;
    ///
    /// let chunk = ChunkId::new(1, 2, 0);
    /// let min = chunk.min_voxel();
    /// assert_eq!(min, VoxelCoord::new(100, 200, 0));
    /// ```
    pub fn min_voxel(&self) -> VoxelCoord {
        VoxelCoord::new(
            self.x * CHUNK_SIZE,
            self.y * CHUNK_SIZE,
            self.z * CHUNK_SIZE,
        )
    }
    
    /// Get maximum voxel coordinate contained in this chunk (exclusive)
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::{ChunkId, CHUNK_SIZE};
    /// use metaverse_core::voxel::VoxelCoord;
    ///
    /// let chunk = ChunkId::new(1, 2, 0);
    /// let max = chunk.max_voxel();
    /// assert_eq!(max, VoxelCoord::new(200, 300, 100));
    /// ```
    pub fn max_voxel(&self) -> VoxelCoord {
        VoxelCoord::new(
            (self.x + 1) * CHUNK_SIZE,
            (self.y + 1) * CHUNK_SIZE,
            (self.z + 1) * CHUNK_SIZE,
        )
    }
    
    /// Get GPS bounds for this chunk (min, max)
    ///
    /// Converts chunk voxel bounds to GPS coordinates for terrain generation.
    /// Returns (lat_min, lat_max, lon_min, lon_max) in degrees.
    ///
    /// # Example
    /// ```no_run
    /// use metaverse_core::chunk::ChunkId;
    ///
    /// let chunk = ChunkId::new(0, 0, 0);
    /// let (lat_min, lat_max, lon_min, lon_max) = chunk.gps_bounds();
    /// ```
    pub fn gps_bounds(&self) -> (f64, f64, f64, f64) {
        let min_voxel = self.min_voxel();
        let max_voxel = self.max_voxel();
        
        // Convert min corner to GPS
        let min_ecef = min_voxel.to_ecef();
        let min_gps = min_ecef.to_gps();
        
        // Convert max corner to GPS
        let max_ecef = max_voxel.to_ecef();
        let max_gps = max_ecef.to_gps();
        
        // Return bounding box (may not be perfectly aligned due to Earth curvature)
        (
            min_gps.lat.min(max_gps.lat),  // lat_min
            min_gps.lat.max(max_gps.lat),  // lat_max
            min_gps.lon.min(max_gps.lon),  // lon_min
            min_gps.lon.max(max_gps.lon),  // lon_max
        )
    }
    
    /// Get center GPS coordinate of this chunk
    ///
    /// Useful for terrain generation origin point.
    ///
    /// # Example
    /// ```no_run
    /// use metaverse_core::chunk::ChunkId;
    ///
    /// let chunk = ChunkId::new(0, 0, 0);
    /// let center = chunk.center_gps();
    /// ```
    pub fn center_gps(&self) -> GPS {
        // Calculate center voxel
        let min = self.min_voxel();
        let center_voxel = VoxelCoord::new(
            min.x + (CHUNK_SIZE / 2),
            min.y + (CHUNK_SIZE / 2),
            min.z + (CHUNK_SIZE / 2),
        );
        
        // Convert to GPS
        let center_ecef = center_voxel.to_ecef();
        center_ecef.to_gps()
    }
    
    /// Check if voxel coordinate is within this chunk
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::ChunkId;
    /// use metaverse_core::voxel::VoxelCoord;
    ///
    /// let chunk = ChunkId::new(0, 0, 0);
    /// assert!(chunk.contains(&VoxelCoord::new(50, 50, 50)));
    /// assert!(!chunk.contains(&VoxelCoord::new(150, 50, 50)));
    /// ```
    pub fn contains(&self, coord: &VoxelCoord) -> bool {
        let min = self.min_voxel();
        let max = self.max_voxel();
        
        coord.x >= min.x && coord.x < max.x &&
        coord.y >= min.y && coord.y < max.y &&
        coord.z >= min.z && coord.z < max.z
    }
    
    /// Get all 26 neighboring chunks (3×3×3 cube minus center)
    ///
    /// Useful for:
    /// - Loading chunks around player (spatial locality)
    /// - Subscribing to nearby gossipsub topics
    /// - Finding peers in adjacent chunks
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::ChunkId;
    ///
    /// let chunk = ChunkId::new(0, 0, 0);
    /// let neighbors = chunk.neighbors();
    /// assert_eq!(neighbors.len(), 26);
    /// assert!(neighbors.contains(&ChunkId::new(1, 0, 0)));
    /// assert!(neighbors.contains(&ChunkId::new(-1, 0, 0)));
    /// ```
    pub fn neighbors(&self) -> Vec<ChunkId> {
        let mut result = Vec::with_capacity(26);
        
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    // Skip center (this chunk)
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    
                    result.push(ChunkId::new(
                        self.x + dx,
                        self.y + dy,
                        self.z + dz,
                    ));
                }
            }
        }
        
        result
    }
    
    /// Get Manhattan distance to another chunk
    ///
    /// Useful for prioritizing chunk loads (closer = higher priority)
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::ChunkId;
    ///
    /// let chunk_a = ChunkId::new(0, 0, 0);
    /// let chunk_b = ChunkId::new(2, 1, 3);
    /// assert_eq!(chunk_a.manhattan_distance(&chunk_b), 6);
    /// ```
    pub fn manhattan_distance(&self, other: &ChunkId) -> i64 {
        (self.x - other.x).abs() +
        (self.y - other.y).abs() +
        (self.z - other.z).abs()
    }
    
    /// Convert to directory-safe string identifier
    ///
    /// Format: `chunk_X_Y_Z` where X, Y, Z are signed integers
    ///
    /// # Example
    /// ```
    /// use metaverse_core::chunk::ChunkId;
    ///
    /// let chunk = ChunkId::new(5, -3, 10);
    /// assert_eq!(chunk.to_string(), "chunk_5_-3_10");
    /// ```
    pub fn to_path_string(&self) -> String {
        format!("chunk_{}_{}_{}", self.x, self.y, self.z)
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "chunk_{}_{}_{}", self.x, self.y, self.z)
    }
}

/// Calculate which chunks are within a given radius
///
/// Useful for:
/// - Loading chunks around player position
/// - Subscribing to nearby gossipsub topics
/// - Determining which peers to sync with
///
/// # Arguments
/// * `center` - Center chunk
/// * `radius` - Radius in chunks (Manhattan distance)
///
/// # Example
/// ```
/// use metaverse_core::chunk::{ChunkId, chunks_in_radius};
///
/// let center = ChunkId::new(0, 0, 0);
/// let nearby = chunks_in_radius(&center, 1);
///
/// // Radius 1 = 3×3×3 cube = 27 chunks
/// assert_eq!(nearby.len(), 27);
/// assert!(nearby.contains(&center));
/// assert!(nearby.contains(&ChunkId::new(1, 0, 0)));
/// ```
pub fn chunks_in_radius(center: &ChunkId, radius: i64) -> Vec<ChunkId> {
    let mut result = Vec::new();
    
    // Only iterate horizontally (X, Z), keep same Y level
    // This prevents loading chunks above/below which would create stacked terrain
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            let chunk = ChunkId::new(
                center.x + dx,
                center.y,  // Keep same Y level
                center.z + dz,
            );
            
            // Only include if within horizontal Manhattan distance
            let horizontal_distance = dx.abs() + dz.abs();
            if horizontal_distance <= radius {
                result.push(chunk);
            }
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chunk_from_voxel() {
        // Positive coordinates
        assert_eq!(
            ChunkId::from_voxel(&VoxelCoord::new(50, 50, 50)),
            ChunkId::new(0, 0, 0)
        );
        assert_eq!(
            ChunkId::from_voxel(&VoxelCoord::new(150, 200, 50)),
            ChunkId::new(1, 2, 0)
        );
        
        // Negative coordinates (floor division)
        assert_eq!(
            ChunkId::from_voxel(&VoxelCoord::new(-1, 0, 0)),
            ChunkId::new(-1, 0, 0)
        );
        assert_eq!(
            ChunkId::from_voxel(&VoxelCoord::new(-150, 50, 0)),
            ChunkId::new(-2, 0, 0)
        );
        
        // Chunk boundaries
        assert_eq!(
            ChunkId::from_voxel(&VoxelCoord::new(99, 99, 99)),
            ChunkId::new(0, 0, 0)
        );
        assert_eq!(
            ChunkId::from_voxel(&VoxelCoord::new(100, 100, 100)),
            ChunkId::new(1, 1, 1)
        );
    }
    
    #[test]
    fn test_chunk_bounds() {
        let chunk = ChunkId::new(1, 2, 0);
        
        assert_eq!(chunk.min_voxel(), VoxelCoord::new(100, 200, 0));
        assert_eq!(chunk.max_voxel(), VoxelCoord::new(200, 300, 100));
        
        // Contains tests
        assert!(chunk.contains(&VoxelCoord::new(100, 200, 0))); // Min corner
        assert!(chunk.contains(&VoxelCoord::new(150, 250, 50))); // Center
        assert!(!chunk.contains(&VoxelCoord::new(200, 300, 100))); // Max corner (exclusive)
        assert!(!chunk.contains(&VoxelCoord::new(99, 200, 0))); // Just outside
    }
    
    #[test]
    fn test_chunk_neighbors() {
        let chunk = ChunkId::new(0, 0, 0);
        let neighbors = chunk.neighbors();
        
        assert_eq!(neighbors.len(), 26); // 3³ - 1
        
        // Check some specific neighbors
        assert!(neighbors.contains(&ChunkId::new(1, 0, 0)));
        assert!(neighbors.contains(&ChunkId::new(-1, 0, 0)));
        assert!(neighbors.contains(&ChunkId::new(0, 1, 0)));
        assert!(neighbors.contains(&ChunkId::new(0, 0, 1)));
        assert!(neighbors.contains(&ChunkId::new(1, 1, 1)));
        
        // Should not contain self
        assert!(!neighbors.contains(&chunk));
    }
    
    #[test]
    fn test_chunk_manhattan_distance() {
        let chunk_a = ChunkId::new(0, 0, 0);
        let chunk_b = ChunkId::new(2, 1, 3);
        
        assert_eq!(chunk_a.manhattan_distance(&chunk_b), 6);
        assert_eq!(chunk_b.manhattan_distance(&chunk_a), 6); // Symmetric
        
        assert_eq!(chunk_a.manhattan_distance(&chunk_a), 0); // Self
    }
    
    #[test]
    fn test_chunks_in_radius() {
        let center = ChunkId::new(0, 0, 0);
        
        // Radius 0 = just center
        let r0 = chunks_in_radius(&center, 0);
        assert_eq!(r0.len(), 1);
        assert!(r0.contains(&center));
        
        // Radius 1 = 3×3×3 cube
        let r1 = chunks_in_radius(&center, 1);
        assert_eq!(r1.len(), 27);
        assert!(r1.contains(&center));
        assert!(r1.contains(&ChunkId::new(1, 0, 0)));
        assert!(r1.contains(&ChunkId::new(1, 1, 1)));
        
        // Should not include chunks at distance > 1
        assert!(!r1.contains(&ChunkId::new(2, 0, 0)));
    }
    
    #[test]
    fn test_chunk_path_string() {
        assert_eq!(
            ChunkId::new(5, 10, 3).to_path_string(),
            "chunk_5_10_3"
        );
        
        // Negative coordinates
        assert_eq!(
            ChunkId::new(-5, 10, -3).to_path_string(),
            "chunk_-5_10_-3"
        );
    }
    
    #[test]
    fn test_chunk_display() {
        let chunk = ChunkId::new(1, -2, 3);
        assert_eq!(format!("{}", chunk), "chunk_1_-2_3");
    }
    
    #[test]
    fn test_chunk_determinism() {
        // Same voxel ALWAYS produces same chunk
        let voxel = VoxelCoord::new(12345, -6789, 42);
        let chunk1 = ChunkId::from_voxel(&voxel);
        let chunk2 = ChunkId::from_voxel(&voxel);
        assert_eq!(chunk1, chunk2);
        
        // Round-trip: voxel → chunk → contains
        assert!(chunk1.contains(&voxel));
    }
}
