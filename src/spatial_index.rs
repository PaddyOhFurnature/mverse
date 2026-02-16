//! Spatial Index for Continuous Query System
//!
//! Uses R*-tree for efficient range queries of voxel blocks.
//! Blocks are 8m × 8m × 8m volumes containing 512 voxels each.

use rstar::{RTree, AABB as RTreeAABB, RTreeObject, PointDistance};
use serde::{Serialize, Deserialize};
use crate::svo::MaterialId;

/// AABB (Axis-Aligned Bounding Box) in ECEF coordinates
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABB {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl AABB {
    /// Create AABB from min/max corners
    pub fn from_corners(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }
    
    /// Create AABB centered at point with extent
    pub fn from_center(center: [f64; 3], extent: f64) -> Self {
        Self {
            min: [center[0] - extent, center[1] - extent, center[2] - extent],
            max: [center[0] + extent, center[1] + extent, center[2] + extent],
        }
    }
    
    /// Check if point is inside AABB
    pub fn contains(&self, point: [f64; 3]) -> bool {
        point[0] >= self.min[0] && point[0] <= self.max[0]
            && point[1] >= self.min[1] && point[1] <= self.max[1]
            && point[2] >= self.min[2] && point[2] <= self.max[2]
    }
    
    /// Check if two AABBs intersect
    pub fn intersects(&self, other: &AABB) -> bool {
        self.min[0] <= other.max[0] && self.max[0] >= other.min[0]
            && self.min[1] <= other.max[1] && self.max[1] >= other.min[1]
            && self.min[2] <= other.max[2] && self.max[2] >= other.min[2]
    }
    
    /// Expand AABB by margin in all directions
    pub fn expand(&self, margin: f64) -> Self {
        Self {
            min: [self.min[0] - margin, self.min[1] - margin, self.min[2] - margin],
            max: [self.max[0] + margin, self.max[1] + margin, self.max[2] + margin],
        }
    }
}

/// Block of voxels (8m × 8m × 8m = 512 voxels)
///
/// Storage granularity for spatial index. Each block contains 8³ = 512 voxels.
/// Block size chosen as balance between index overhead and query granularity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoxelBlock {
    /// Block position (minimum corner in ECEF)
    pub ecef_min: [f64; 3],
    
    /// Block size in meters (8.0m)
    pub size: f64,
    
    /// Voxel materials (8×8×8 = 512)
    /// Index: z*64 + y*8 + x where x,y,z in [0..8)
    #[serde(with = "serde_arrays")]
    pub voxels: Box<[MaterialId; 512]>,
}

// Custom serde for large arrays
mod serde_arrays {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use crate::svo::MaterialId;
    
    pub fn serialize<S>(arr: &Box<[MaterialId; 512]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        arr.as_slice().serialize(serializer)
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Box<[MaterialId; 512]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<MaterialId> = Vec::deserialize(deserializer)?;
        let arr: [MaterialId; 512] = vec.try_into()
            .map_err(|v: Vec<_>| serde::de::Error::invalid_length(v.len(), &"512 elements"))?;
        Ok(Box::new(arr))
    }
}

impl VoxelBlock {
    /// Create new empty block (all AIR)
    pub fn new(ecef_min: [f64; 3], size: f64) -> Self {
        Self {
            ecef_min,
            size,
            voxels: Box::new([crate::svo::AIR; 512]),
        }
    }
    
    /// Get voxel at local coordinates (x, y, z in [0..8))
    pub fn get_voxel(&self, x: usize, y: usize, z: usize) -> MaterialId {
        assert!(x < 8 && y < 8 && z < 8, "Voxel index out of bounds");
        let index = z * 64 + y * 8 + x;
        self.voxels[index]
    }
    
    /// Set voxel at local coordinates
    pub fn set_voxel(&mut self, x: usize, y: usize, z: usize, material: MaterialId) {
        assert!(x < 8 && y < 8 && z < 8, "Voxel index out of bounds");
        let index = z * 64 + y * 8 + x;
        self.voxels[index] = material;
    }
    
    /// Sample voxel at global ECEF position
    pub fn sample_voxel(&self, ecef: [f64; 3]) -> Option<MaterialId> {
        // Convert to local coordinates
        let local_x = (ecef[0] - self.ecef_min[0]) / self.size;
        let local_y = (ecef[1] - self.ecef_min[1]) / self.size;
        let local_z = (ecef[2] - self.ecef_min[2]) / self.size;
        
        // Check if point is inside block
        if local_x < 0.0 || local_x >= 1.0
            || local_y < 0.0 || local_y >= 1.0
            || local_z < 0.0 || local_z >= 1.0 {
            return None;
        }
        
        // Convert to voxel index (floor to get voxel containing point)
        let x = (local_x * 8.0).floor() as usize;
        let y = (local_y * 8.0).floor() as usize;
        let z = (local_z * 8.0).floor() as usize;
        
        Some(self.get_voxel(x, y, z))
    }
    
    /// Get AABB for this block
    pub fn aabb(&self) -> AABB {
        AABB {
            min: self.ecef_min,
            max: [
                self.ecef_min[0] + self.size,
                self.ecef_min[1] + self.size,
                self.ecef_min[2] + self.size,
            ],
        }
    }
}

/// Implement R-tree traits for VoxelBlock
impl RTreeObject for VoxelBlock {
    type Envelope = RTreeAABB<[f64; 3]>;
    
    fn envelope(&self) -> Self::Envelope {
        let max = [
            self.ecef_min[0] + self.size,
            self.ecef_min[1] + self.size,
            self.ecef_min[2] + self.size,
        ];
        RTreeAABB::from_corners(self.ecef_min, max)
    }
}

impl PointDistance for VoxelBlock {
    fn distance_2(&self, point: &[f64; 3]) -> f64 {
        let envelope = self.envelope();
        envelope.distance_2(point)
    }
}

/// Spatial index using R*-tree
///
/// Provides efficient range queries for voxel blocks.
pub struct SpatialIndex {
    tree: RTree<VoxelBlock>,
    bounds: AABB,
}

impl SpatialIndex {
    /// Create new spatial index for bounded region
    pub fn new(bounds: AABB) -> Self {
        Self {
            tree: RTree::new(),
            bounds,
        }
    }
    
    /// Insert block into index
    pub fn insert(&mut self, block: VoxelBlock) {
        self.tree.insert(block);
    }
    
    /// Remove block from index (returns false if not found)
    pub fn remove(&mut self, block: &VoxelBlock) -> bool {
        // R-tree doesn't have direct remove by value, need to iterate
        // For now, rebuild tree without this block
        let blocks: Vec<VoxelBlock> = self.tree
            .iter()
            .filter(|b| b != &block)
            .cloned()
            .collect();
        
        let found = blocks.len() < self.tree.size();
        self.tree = RTree::bulk_load(blocks);
        found
    }
    
    /// Query all blocks intersecting AABB
    pub fn query_range(&self, bounds: AABB) -> Vec<VoxelBlock> {
        let rtree_bounds = RTreeAABB::from_corners(bounds.min, bounds.max);
        self.tree
            .locate_in_envelope_intersecting(&rtree_bounds)
            .cloned()
            .collect()
    }
    
    /// Find nearest block to point
    pub fn nearest(&self, point: [f64; 3]) -> Option<VoxelBlock> {
        self.tree.nearest_neighbor(&point).cloned()
    }
    
    /// Get total number of blocks in index
    pub fn len(&self) -> usize {
        self.tree.size()
    }
    
    /// Check if index is empty
    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }
    
    /// Get bounds of indexed region
    pub fn bounds(&self) -> AABB {
        self.bounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_aabb_contains() {
        let aabb = AABB::from_corners([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        
        assert!(aabb.contains([5.0, 5.0, 5.0]));
        assert!(aabb.contains([0.0, 0.0, 0.0]));
        assert!(aabb.contains([10.0, 10.0, 10.0]));
        assert!(!aabb.contains([-1.0, 5.0, 5.0]));
        assert!(!aabb.contains([11.0, 5.0, 5.0]));
    }
    
    #[test]
    fn test_aabb_intersects() {
        let aabb1 = AABB::from_corners([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let aabb2 = AABB::from_corners([5.0, 5.0, 5.0], [15.0, 15.0, 15.0]);
        let aabb3 = AABB::from_corners([20.0, 20.0, 20.0], [30.0, 30.0, 30.0]);
        
        assert!(aabb1.intersects(&aabb2));
        assert!(aabb2.intersects(&aabb1));
        assert!(!aabb1.intersects(&aabb3));
        assert!(!aabb3.intersects(&aabb1));
    }
    
    #[test]
    fn test_voxel_block_creation() {
        let block = VoxelBlock::new([0.0, 0.0, 0.0], 8.0);
        
        assert_eq!(block.ecef_min, [0.0, 0.0, 0.0]);
        assert_eq!(block.size, 8.0);
        assert_eq!(block.get_voxel(0, 0, 0), crate::svo::AIR);
    }
    
    #[test]
    fn test_voxel_block_get_set() {
        let mut block = VoxelBlock::new([0.0, 0.0, 0.0], 8.0);
        
        block.set_voxel(3, 4, 5, crate::svo::STONE);
        assert_eq!(block.get_voxel(3, 4, 5), crate::svo::STONE);
        assert_eq!(block.get_voxel(0, 0, 0), crate::svo::AIR);
    }
    
    #[test]
    fn test_voxel_block_sample() {
        let mut block = VoxelBlock::new([100.0, 200.0, 300.0], 8.0);
        block.set_voxel(4, 4, 4, crate::svo::DIRT);
        
        // Sample center of voxel [4,4,4]
        let ecef = [100.0 + 4.5, 200.0 + 4.5, 300.0 + 4.5];
        assert_eq!(block.sample_voxel(ecef), Some(crate::svo::DIRT));
        
        // Sample outside block
        let outside = [50.0, 200.0, 300.0];
        assert_eq!(block.sample_voxel(outside), None);
    }
    
    #[test]
    fn test_spatial_index_insert_query() {
        let bounds = AABB::from_corners([0.0, 0.0, 0.0], [100.0, 100.0, 100.0]);
        let mut index = SpatialIndex::new(bounds);
        
        // Insert two blocks
        let block1 = VoxelBlock::new([0.0, 0.0, 0.0], 8.0);
        let block2 = VoxelBlock::new([10.0, 10.0, 10.0], 8.0);
        
        index.insert(block1);
        index.insert(block2);
        
        assert_eq!(index.len(), 2);
        
        // Query range containing both blocks
        let query_bounds = AABB::from_corners([0.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        let results = index.query_range(query_bounds);
        assert_eq!(results.len(), 2);
        
        // Query range containing only first block
        let query_bounds = AABB::from_corners([0.0, 0.0, 0.0], [5.0, 5.0, 5.0]);
        let results = index.query_range(query_bounds);
        assert_eq!(results.len(), 1);
    }
    
    #[test]
    fn test_spatial_index_nearest() {
        let bounds = AABB::from_corners([0.0, 0.0, 0.0], [100.0, 100.0, 100.0]);
        let mut index = SpatialIndex::new(bounds);
        
        let block1 = VoxelBlock::new([0.0, 0.0, 0.0], 8.0);
        let block2 = VoxelBlock::new([50.0, 50.0, 50.0], 8.0);
        
        index.insert(block1);
        index.insert(block2);
        
        // Find nearest to origin (should be block1)
        let nearest = index.nearest([1.0, 1.0, 1.0]).unwrap();
        assert_eq!(nearest.ecef_min, [0.0, 0.0, 0.0]);
        
        // Find nearest to center (should be block2)
        let nearest = index.nearest([50.0, 50.0, 50.0]).unwrap();
        assert_eq!(nearest.ecef_min, [50.0, 50.0, 50.0]);
    }
    
    #[test]
    fn test_block_alignment() {
        // Verify adjacent blocks align perfectly (no gaps)
        let block1 = VoxelBlock::new([0.0, 0.0, 0.0], 8.0);
        let block2 = VoxelBlock::new([8.0, 0.0, 0.0], 8.0);
        
        let aabb1 = block1.aabb();
        let aabb2 = block2.aabb();
        
        // Block 1 max X should equal Block 2 min X
        assert_eq!(aabb1.max[0], aabb2.min[0]);
        
        // They should be adjacent (touch but not overlap)
        assert!(aabb1.intersects(&aabb2));
    }
}
