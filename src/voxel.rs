//! Voxel coordinate system
//!
//! Maps between ECEF (Earth-Centered Earth-Fixed) f64 coordinates
//! and integer voxel grid coordinates.
//!
//! World bounds: ±6.4M meters (contains Earth + atmosphere)
//! Voxel size: 1 meter

use crate::coordinates::ECEF;

/// World bounds (cube containing Earth)
pub const WORLD_MIN_METERS: f64 = -6_400_000.0;
pub const WORLD_MAX_METERS: f64 = 6_400_000.0;
pub const WORLD_SIZE_METERS: f64 = 12_800_000.0;

/// Base voxel resolution
pub const VOXEL_SIZE_METERS: f64 = 1.0;

/// Voxel grid dimensions
pub const VOXEL_GRID_SIZE: i64 = (WORLD_SIZE_METERS / VOXEL_SIZE_METERS) as i64;

/// 3D voxel coordinate (integer grid position)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}
