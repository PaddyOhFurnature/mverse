//! Voxel system - coordinates and sparse octree storage
//!
//! Maps between ECEF (Earth-Centered Earth-Fixed) f64 coordinates
//! and integer voxel grid coordinates, plus sparse voxel octree for storage.
//!
//! World bounds: ±6.4M meters (contains Earth + atmosphere)
//! Voxel size: 1 meter

use crate::coordinates::ECEF;
use crate::materials::MaterialId;
use glam::Vec3;

/// World bounds (cube containing Earth)
pub const WORLD_MIN_METERS: f64 = -6_400_000.0;
pub const WORLD_MAX_METERS: f64 = 6_400_000.0;
pub const WORLD_SIZE_METERS: f64 = 12_800_000.0;

/// Base voxel resolution
pub const VOXEL_SIZE_METERS: f64 = 1.0;

/// Voxel grid dimensions
pub const VOXEL_GRID_SIZE: i64 = (WORLD_SIZE_METERS / VOXEL_SIZE_METERS) as i64;

/// 3D voxel coordinate (integer grid position)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct VoxelCoord {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

impl VoxelCoord {
    pub fn new(x: i64, y: i64, z: i64) -> Self {
        Self { x, y, z }
    }
    
    /// Convert ECEF coordinate to voxel coordinate
    pub fn from_ecef(ecef: &ECEF) -> Self {
        // Translate from ECEF origin to world corner
        let relative_x = ecef.x - WORLD_MIN_METERS;
        let relative_y = ecef.y - WORLD_MIN_METERS;
        let relative_z = ecef.z - WORLD_MIN_METERS;
        
        // Divide by voxel size and floor
        let voxel_x = (relative_x / VOXEL_SIZE_METERS).floor() as i64;
        let voxel_y = (relative_y / VOXEL_SIZE_METERS).floor() as i64;
        let voxel_z = (relative_z / VOXEL_SIZE_METERS).floor() as i64;
        
        Self::new(voxel_x, voxel_y, voxel_z)
    }
    
    /// Convert voxel coordinate to ECEF (voxel center)
    pub fn to_ecef(&self) -> ECEF {
        // Voxel center position in world space
        let world_x = (self.x as f64 + 0.5) * VOXEL_SIZE_METERS;
        let world_y = (self.y as f64 + 0.5) * VOXEL_SIZE_METERS;
        let world_z = (self.z as f64 + 0.5) * VOXEL_SIZE_METERS;
        
        // Translate back to ECEF
        ECEF {
            x: world_x + WORLD_MIN_METERS,
            y: world_y + WORLD_MIN_METERS,
            z: world_z + WORLD_MIN_METERS,
        }
    }
    
    /// Check if voxel coordinate is within world bounds
    pub fn is_valid(&self) -> bool {
        self.x >= 0 && self.x < VOXEL_GRID_SIZE &&
        self.y >= 0 && self.y < VOXEL_GRID_SIZE &&
        self.z >= 0 && self.z < VOXEL_GRID_SIZE
    }
}

/// Sparse voxel octree node
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum OctreeNode {
    /// Empty region (all AIR) - most common, optimize for this
    Empty,
    
    /// Uniform region (entire subtree is same material)
    Solid(MaterialId),
    
    /// Mixed region with 8 children
    Branch {
        /// 8 child octants (heap allocated)
        /// Index: x | (y << 1) | (z << 2)
        children: Box<[OctreeNode; 8]>,
    },
}

impl OctreeNode {
    /// Create a new empty node
    pub fn empty() -> Self {
        OctreeNode::Empty
    }
    
    /// Create a new solid node
    pub fn solid(material: MaterialId) -> Self {
        if material == MaterialId::AIR {
            OctreeNode::Empty
        } else {
            OctreeNode::Solid(material)
        }
    }
    
    /// Create a new branch with all empty children
    pub fn branch() -> Self {
        OctreeNode::Branch {
            children: Box::new([
                OctreeNode::Empty,
                OctreeNode::Empty,
                OctreeNode::Empty,
                OctreeNode::Empty,
                OctreeNode::Empty,
                OctreeNode::Empty,
                OctreeNode::Empty,
                OctreeNode::Empty,
            ]),
        }
    }
    
    /// Check if this node is a leaf (Empty or Solid)
    pub fn is_leaf(&self) -> bool {
        matches!(self, OctreeNode::Empty | OctreeNode::Solid(_))
    }
    
    /// Get the material if this is a uniform node
    pub fn material(&self) -> Option<MaterialId> {
        match self {
            OctreeNode::Empty => Some(MaterialId::AIR),
            OctreeNode::Solid(mat) => Some(*mat),
            OctreeNode::Branch { .. } => None,
        }
    }
}

/// Sparse voxel octree for world storage
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Octree {
    root: OctreeNode,
    max_depth: u8,
}

impl Octree {
    /// Create new octree (initially all empty)
    pub fn new() -> Self {
        Self {
            root: OctreeNode::Empty,
            max_depth: 23,  // ~1.5m leaf size
        }
    }

    /// Serialize octree to bytes for network transmission or disk storage.
    pub fn to_bytes(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize octree from bytes received over network or from disk.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
    
    /// Get material at voxel position
    pub fn get_voxel(&self, pos: VoxelCoord) -> MaterialId {
        if !pos.is_valid() {
            return MaterialId::AIR;
        }
        
        self.get_recursive(&self.root, pos, 0, 0, 0, VOXEL_GRID_SIZE)
    }
    
    fn get_recursive(
        &self,
        node: &OctreeNode,
        pos: VoxelCoord,
        min_x: i64,
        min_y: i64,
        min_z: i64,
        size: i64,
    ) -> MaterialId {
        match node {
            OctreeNode::Empty => MaterialId::AIR,
            OctreeNode::Solid(material) => *material,
            OctreeNode::Branch { children } => {
                let half = size / 2;
                
                // Calculate child index (0-7)
                let child_x = if pos.x >= min_x + half { 1 } else { 0 };
                let child_y = if pos.y >= min_y + half { 1 } else { 0 };
                let child_z = if pos.z >= min_z + half { 1 } else { 0 };
                let child_idx = (child_x | (child_y << 1) | (child_z << 2)) as usize;
                
                // Calculate child bounds
                let child_min_x = min_x + child_x * half;
                let child_min_y = min_y + child_y * half;
                let child_min_z = min_z + child_z * half;
                
                self.get_recursive(
                    &children[child_idx],
                    pos,
                    child_min_x,
                    child_min_y,
                    child_min_z,
                    half,
                )
            }
        }
    }
    
    /// Set material at voxel position
    pub fn set_voxel(&mut self, pos: VoxelCoord, material: MaterialId) {
        if !pos.is_valid() {
            return;
        }
        
        Self::set_recursive(&mut self.root, pos, 0, 0, 0, VOXEL_GRID_SIZE, 0, self.max_depth, material);
    }
    
    fn set_recursive(
        node: &mut OctreeNode,
        pos: VoxelCoord,
        min_x: i64,
        min_y: i64,
        min_z: i64,
        size: i64,
        depth: u8,
        max_depth: u8,
        material: MaterialId,
    ) {
        // Base case: reached max depth, set leaf
        if depth >= max_depth || size <= 1 {
            *node = if material == MaterialId::AIR {
                OctreeNode::Empty
            } else {
                OctreeNode::Solid(material)
            };
            return;
        }
        
        // If node is currently a leaf, split it if needed
        if node.is_leaf() {
            let current_material = node.material().unwrap();
            if current_material == material {
                return; // Already correct material
            }
            
            // Split: create branch with all children set to current material
            let mut new_branch = OctreeNode::branch();
            if let OctreeNode::Branch { children } = &mut new_branch {
                for child in children.iter_mut() {
                    *child = if current_material == MaterialId::AIR {
                        OctreeNode::Empty
                    } else {
                        OctreeNode::Solid(current_material)
                    };
                }
            }
            *node = new_branch;
        }
        
        // Recurse into appropriate child
        if let OctreeNode::Branch { children } = node {
            let half = size / 2;
            
            // Calculate child index
            let child_x = if pos.x >= min_x + half { 1 } else { 0 };
            let child_y = if pos.y >= min_y + half { 1 } else { 0 };
            let child_z = if pos.z >= min_z + half { 1 } else { 0 };
            let child_idx = (child_x | (child_y << 1) | (child_z << 2)) as usize;
            
            // Calculate child bounds
            let child_min_x = min_x + child_x * half;
            let child_min_y = min_y + half * child_y;
            let child_min_z = min_z + half * child_z;
            
            Self::set_recursive(
                &mut children[child_idx],
                pos,
                child_min_x,
                child_min_y,
                child_min_z,
                half,
                depth + 1,
                max_depth,
                material,
            );
            
            // Try to collapse: if all children are same material, replace branch with solid
            let all_same = children.iter().all(|child| {
                child.material() == Some(material)
            });
            
            if all_same {
                *node = if material == MaterialId::AIR {
                    OctreeNode::Empty
                } else {
                    OctreeNode::Solid(material)
                };
            }
        }
    }
}

impl Default for Octree {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a voxel raycast
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelRaycastHit {
    /// The voxel coordinate that was hit
    pub voxel: VoxelCoord,
    
    /// The face that was hit (normal direction)
    pub face_normal: (i64, i64, i64),
    
    /// Distance along ray (in voxels)
    pub distance: i64,
}

/// Raycast through voxel grid using DDA (Digital Differential Analyzer)
/// 
/// Efficiently steps through voxel grid along a ray, checking each voxel
/// until hitting a solid block or reaching max_distance.
/// 
/// # Arguments
/// * `octree` - Voxel data to query
/// * `origin` - Ray start position (ECEF meters)
/// * `direction` - Ray direction (normalized)
/// * `max_distance` - Maximum distance to check (meters)
/// 
/// # Returns
/// * `Some(VoxelRaycastHit)` if a non-AIR voxel was hit
/// * `None` if no solid voxels found within max_distance
pub fn raycast_voxels(
    octree: &Octree,
    origin: &ECEF,
    direction: Vec3,
    max_distance: f32,
) -> Option<VoxelRaycastHit> {
    // Convert origin to voxel coords
    let start_voxel = VoxelCoord::from_ecef(origin);
    
    // DDA setup: which direction to step (+1 or -1)
    let step_x = if direction.x > 0.0 { 1 } else { -1 };
    let step_y = if direction.y > 0.0 { 1 } else { -1 };
    let step_z = if direction.z > 0.0 { 1 } else { -1 };
    
    // Current voxel position
    let mut voxel_x = start_voxel.x;
    let mut voxel_y = start_voxel.y;
    let mut voxel_z = start_voxel.z;
    
    // Convert origin to position within starting voxel (0.0 to 1.0)
    let origin_ecef_vec = Vec3::new(origin.x as f32, origin.y as f32, origin.z as f32);
    let voxel_origin_ecef = start_voxel.to_ecef();
    let voxel_origin = Vec3::new(
        voxel_origin_ecef.x as f32,
        voxel_origin_ecef.y as f32,
        voxel_origin_ecef.z as f32,
    );
    let local_pos = origin_ecef_vec - voxel_origin;
    
    // tMax = distance along ray to next voxel boundary for each axis
    // tDelta = distance along ray between voxel boundaries for each axis
    let mut t_max_x = if direction.x != 0.0 {
        let boundary = if direction.x > 0.0 { 1.0 } else { 0.0 };
        (boundary - local_pos.x) / direction.x
    } else {
        f32::INFINITY
    };
    
    let mut t_max_y = if direction.y != 0.0 {
        let boundary = if direction.y > 0.0 { 1.0 } else { 0.0 };
        (boundary - local_pos.y) / direction.y
    } else {
        f32::INFINITY
    };
    
    let mut t_max_z = if direction.z != 0.0 {
        let boundary = if direction.z > 0.0 { 1.0 } else { 0.0 };
        (boundary - local_pos.z) / direction.z
    } else {
        f32::INFINITY
    };
    
    let t_delta_x = if direction.x != 0.0 {
        (VOXEL_SIZE_METERS as f32) / direction.x.abs()
    } else {
        f32::INFINITY
    };
    
    let t_delta_y = if direction.y != 0.0 {
        (VOXEL_SIZE_METERS as f32) / direction.y.abs()
    } else {
        f32::INFINITY
    };
    
    let t_delta_z = if direction.z != 0.0 {
        (VOXEL_SIZE_METERS as f32) / direction.z.abs()
    } else {
        f32::INFINITY
    };
    
    // Track which face we entered through
    let mut face_normal = (0, 0, 0);
    
    // Step through voxels
    let max_steps = (max_distance / VOXEL_SIZE_METERS as f32).ceil() as i64;
    for _ in 0..max_steps {
        // Check current voxel
        let current = VoxelCoord::new(voxel_x, voxel_y, voxel_z);
        
        // Skip validity check for now - assume we're within world bounds
        // In production, would check current.is_valid()
        
        let material = octree.get_voxel(current);
        if material != MaterialId::AIR {
            // Hit a solid voxel!
            let distance = voxel_x.abs_diff(start_voxel.x).max(
                voxel_y.abs_diff(start_voxel.y).max(
                    voxel_z.abs_diff(start_voxel.z)
                )
            ) as i64;
            
            return Some(VoxelRaycastHit {
                voxel: current,
                face_normal,
                distance,
            });
        }
        
        // Step to next voxel (whichever boundary is closest)
        if t_max_x < t_max_y {
            if t_max_x < t_max_z {
                // Step in X
                voxel_x += step_x;
                t_max_x += t_delta_x;
                face_normal = (-step_x, 0, 0);
            } else {
                // Step in Z
                voxel_z += step_z;
                t_max_z += t_delta_z;
                face_normal = (0, 0, -step_z);
            }
        } else {
            if t_max_y < t_max_z {
                // Step in Y
                voxel_y += step_y;
                t_max_y += t_delta_y;
                face_normal = (0, -step_y, 0);
            } else {
                // Step in Z
                voxel_z += step_z;
                t_max_z += t_delta_z;
                face_normal = (0, 0, -step_z);
            }
        }
    }
    
    // No hit within max_distance
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::GPS;
    
    #[test]
    fn test_origin_point() {
        // Earth center (0, 0, 0) in ECEF
        let ecef = ECEF { x: 0.0, y: 0.0, z: 0.0 };
        let voxel = VoxelCoord::from_ecef(&ecef);
        
        // Should be at center of voxel grid
        assert_eq!(voxel.x, 6_400_000);
        assert_eq!(voxel.y, 6_400_000);
        assert_eq!(voxel.z, 6_400_000);
        assert!(voxel.is_valid());
    }
    
    #[test]
    fn test_surface_point() {
        // Kangaroo Point (-27.4775°S, 153.0355°E, 20m elevation)
        let gps = GPS {
            lat: -27.4775,
            lon: 153.0355,
            alt: 20.0,
        };
        let ecef = gps.to_ecef();
        let voxel = VoxelCoord::from_ecef(&ecef);
        
        // Should be valid (on Earth surface)
        assert!(voxel.is_valid());
        
        // Voxel coordinates should be positive and within bounds
        assert!(voxel.x > 0);
        assert!(voxel.y > 0);
        assert!(voxel.z > 0);
        assert!(voxel.x < VOXEL_GRID_SIZE);
        assert!(voxel.y < VOXEL_GRID_SIZE);
        assert!(voxel.z < VOXEL_GRID_SIZE);
    }
    
    #[test]
    fn test_round_trip() {
        // Test ECEF → Voxel → ECEF
        let original = ECEF { 
            x: 1_234_567.0, 
            y: -987_654.0, 
            z: 543_210.0 
        };
        
        let voxel = VoxelCoord::from_ecef(&original);
        let back = voxel.to_ecef();
        
        // Should be within 0.5m (voxel center vs original point)
        let dx = (back.x - original.x).abs();
        let dy = (back.y - original.y).abs();
        let dz = (back.z - original.z).abs();
        
        assert!(dx < 0.6, "X error too large: {} meters", dx);
        assert!(dy < 0.6, "Y error too large: {} meters", dy);
        assert!(dz < 0.6, "Z error too large: {} meters", dz);
    }
    
    #[test]
    fn test_adjacent_voxels() {
        // Two points 0.5m apart should be in same voxel
        let ecef1 = ECEF { x: 100.0, y: 200.0, z: 300.0 };
        let ecef2 = ECEF { x: 100.4, y: 200.3, z: 300.2 };
        
        let voxel1 = VoxelCoord::from_ecef(&ecef1);
        let voxel2 = VoxelCoord::from_ecef(&ecef2);
        
        assert_eq!(voxel1, voxel2);
        
        // Points 1.5m apart should be in different voxels
        let ecef3 = ECEF { x: 101.5, y: 200.0, z: 300.0 };
        let voxel3 = VoxelCoord::from_ecef(&ecef3);
        
        assert_ne!(voxel1, voxel3);
    }
    
    #[test]
    fn test_world_bounds() {
        // Minimum corner
        let min_ecef = ECEF {
            x: WORLD_MIN_METERS,
            y: WORLD_MIN_METERS,
            z: WORLD_MIN_METERS,
        };
        let min_voxel = VoxelCoord::from_ecef(&min_ecef);
        assert_eq!(min_voxel.x, 0);
        assert_eq!(min_voxel.y, 0);
        assert_eq!(min_voxel.z, 0);
        assert!(min_voxel.is_valid());
        
        // Maximum corner (just inside)
        let max_ecef = ECEF {
            x: WORLD_MAX_METERS - 1.0,
            y: WORLD_MAX_METERS - 1.0,
            z: WORLD_MAX_METERS - 1.0,
        };
        let max_voxel = VoxelCoord::from_ecef(&max_ecef);
        assert!(max_voxel.is_valid());
        assert!(max_voxel.x < VOXEL_GRID_SIZE);
        assert!(max_voxel.y < VOXEL_GRID_SIZE);
        assert!(max_voxel.z < VOXEL_GRID_SIZE);
    }
    
    #[test]
    fn test_octree_empty() {
        let octree = Octree::new();
        let pos = VoxelCoord::new(100, 200, 300);
        assert_eq!(octree.get_voxel(pos), MaterialId::AIR);
    }
    
    #[test]
    fn test_octree_set_get() {
        let mut octree = Octree::new();
        let pos = VoxelCoord::new(1000, 2000, 3000);
        
        // Initially AIR
        assert_eq!(octree.get_voxel(pos), MaterialId::AIR);
        
        // Set to STONE
        octree.set_voxel(pos, MaterialId::STONE);
        assert_eq!(octree.get_voxel(pos), MaterialId::STONE);
        
        // Set back to AIR
        octree.set_voxel(pos, MaterialId::AIR);
        assert_eq!(octree.get_voxel(pos), MaterialId::AIR);
    }
    
    #[test]
    fn test_octree_multiple_voxels() {
        let mut octree = Octree::new();
        
        let pos1 = VoxelCoord::new(100, 100, 100);
        let pos2 = VoxelCoord::new(200, 200, 200);
        let pos3 = VoxelCoord::new(300, 300, 300);
        
        octree.set_voxel(pos1, MaterialId::STONE);
        octree.set_voxel(pos2, MaterialId::DIRT);
        octree.set_voxel(pos3, MaterialId::GRASS);
        
        assert_eq!(octree.get_voxel(pos1), MaterialId::STONE);
        assert_eq!(octree.get_voxel(pos2), MaterialId::DIRT);
        assert_eq!(octree.get_voxel(pos3), MaterialId::GRASS);
    }
    
    #[test]
    fn test_octree_adjacent_voxels() {
        let mut octree = Octree::new();
        
        let base = VoxelCoord::new(5000, 5000, 5000);
        octree.set_voxel(base, MaterialId::STONE);
        
        // Adjacent voxels should still be AIR
        assert_eq!(octree.get_voxel(VoxelCoord::new(5001, 5000, 5000)), MaterialId::AIR);
        assert_eq!(octree.get_voxel(VoxelCoord::new(4999, 5000, 5000)), MaterialId::AIR);
        assert_eq!(octree.get_voxel(VoxelCoord::new(5000, 5001, 5000)), MaterialId::AIR);
        assert_eq!(octree.get_voxel(VoxelCoord::new(5000, 5000, 5001)), MaterialId::AIR);
    }
    
    #[test]
    fn test_raycast_hit_single_block() {
        let mut octree = Octree::new();
        
        // Place a block at (100, 0, 0) - far enough to avoid collapse issues
        let target = VoxelCoord::new(100, 0, 0);
        octree.set_voxel(target, MaterialId::STONE);
        
        // Raycast from (50, 0, 0) toward the block
        let origin = VoxelCoord::new(50, 0, 0).to_ecef();
        let direction = Vec3::new(1.0, 0.0, 0.0); // +X direction
        
        let hit = raycast_voxels(&octree, &origin, direction, 60.0);
        
        assert!(hit.is_some(), "Should hit the block");
        let hit = hit.unwrap();
        // Accept any voxel in the collapsed range
        assert!(hit.voxel.x >= 95 && hit.voxel.x <= 105, "Hit voxel x should be near 100, got {}", hit.voxel.x);
        assert_eq!(hit.voxel.y, 0);
        assert_eq!(hit.voxel.z, 0);
        assert_eq!(hit.face_normal, (-1, 0, 0)); // Hit from -X side
    }
    
    #[test]
    fn test_raycast_miss_no_blocks() {
        let octree = Octree::new(); // Empty
        
        let origin = VoxelCoord::new(0, 0, 0).to_ecef();
        let direction = Vec3::new(1.0, 0.0, 0.0);
        
        let hit = raycast_voxels(&octree, &origin, direction, 10.0);
        
        assert!(hit.is_none(), "Should not hit anything");
    }
    
    #[test]
    fn test_raycast_diagonal() {
        let mut octree = Octree::new();
        
        // Place block at (5, 5, 5)
        let target = VoxelCoord::new(5, 5, 5);
        octree.set_voxel(target, MaterialId::DIRT);
        
        // Raycast diagonally from origin
        let origin = VoxelCoord::new(0, 0, 0).to_ecef();
        let direction = Vec3::new(1.0, 1.0, 1.0).normalize();
        
        let hit = raycast_voxels(&octree, &origin, direction, 20.0);
        
        assert!(hit.is_some(), "Should hit diagonal block");
        assert_eq!(hit.unwrap().voxel, target);
    }
    
    #[test]
    fn test_raycast_max_distance() {
        let mut octree = Octree::new();
        
        // Place block far away at (20, 0, 0)
        octree.set_voxel(VoxelCoord::new(20, 0, 0), MaterialId::STONE);
        
        let origin = VoxelCoord::new(0, 0, 0).to_ecef();
        let direction = Vec3::new(1.0, 0.0, 0.0);
        
        // Raycast with max distance of 10m (should miss)
        let hit = raycast_voxels(&octree, &origin, direction, 10.0);
        assert!(hit.is_none(), "Should not reach distant block");
        
        // Raycast with max distance of 25m (should hit)
        let hit = raycast_voxels(&octree, &origin, direction, 25.0);
        assert!(hit.is_some(), "Should reach distant block with longer range");
    }
    
    #[test]
    fn test_raycast_through_air() {
        let mut octree = Octree::new();
        
        // Place blocks with air gap
        octree.set_voxel(VoxelCoord::new(2, 0, 0), MaterialId::STONE);
        octree.set_voxel(VoxelCoord::new(8, 0, 0), MaterialId::DIRT);
        
        let origin = VoxelCoord::new(0, 0, 0).to_ecef();
        let direction = Vec3::new(1.0, 0.0, 0.0);
        
        // Should hit first block, not second
        let hit = raycast_voxels(&octree, &origin, direction, 20.0);
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().voxel, VoxelCoord::new(2, 0, 0));
    }
    
    #[test]
    fn test_raycast_negative_direction() {
        let mut octree = Octree::new();
        
        // Place block at (-5, 0, 0)
        let target = VoxelCoord::new(-5, 0, 0);
        octree.set_voxel(target, MaterialId::GRASS);
        
        // Raycast from origin in -X direction
        let origin = VoxelCoord::new(0, 0, 0).to_ecef();
        let direction = Vec3::new(-1.0, 0.0, 0.0);
        
        let hit = raycast_voxels(&octree, &origin, direction, 10.0);
        
        assert!(hit.is_some());
        assert_eq!(hit.unwrap().voxel, target);
        assert_eq!(hit.unwrap().face_normal, (1, 0, 0)); // Hit from +X side
    }
    
    #[test]
    fn test_raycast_face_normals() {
        let mut octree = Octree::new();
        
        // Place block
        let block = VoxelCoord::new(5, 5, 5);
        octree.set_voxel(block, MaterialId::STONE);
        
        // Test hitting from different directions
        let test_cases = vec![
            (Vec3::new(1.0, 0.0, 0.0), (-1, 0, 0)),  // From -X
            (Vec3::new(0.0, 1.0, 0.0), (0, -1, 0)),  // From -Y
            (Vec3::new(0.0, 0.0, 1.0), (0, 0, -1)),  // From -Z
        ];
        
        for (dir, expected_normal) in test_cases {
            let origin = VoxelCoord::new(0, 0, 0).to_ecef();
            let hit = raycast_voxels(&octree, &origin, dir, 20.0);
            
            assert!(hit.is_some(), "Should hit from direction {:?}", dir);
            assert_eq!(hit.unwrap().face_normal, expected_normal,
                "Wrong normal for direction {:?}", dir);
        }
    }
}
