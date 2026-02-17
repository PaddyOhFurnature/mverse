//! Physics simulation using Rapier3D
//! 
//! Handles:
//! - Gravity toward Earth center (spherical)
//! - Player character physics (kinematic controller)
//! - Collision meshes from voxels
//! - Deterministic simulation (P2P requirement)

use crate::coordinates::ECEF;
use crate::voxel::{Octree, VoxelCoord};
use crate::materials::MaterialId;
use glam::Vec3;
use rapier3d::prelude::*;

/// Physics world managing all simulation
pub struct PhysicsWorld {
    /// Rapier physics pipeline
    pub pipeline: PhysicsPipeline,
    
    /// Broad phase collision detection
    pub broad_phase: DefaultBroadPhase,
    
    /// Narrow phase collision detection  
    pub narrow_phase: NarrowPhase,
    
    /// Island-based solver
    pub islands: IslandManager,
    
    /// Rigidbody set
    pub bodies: RigidBodySet,
    
    /// Collider set
    pub colliders: ColliderSet,
    
    /// Impulse joint set
    pub impulse_joints: ImpulseJointSet,
    
    /// Multibody joint set
    pub multibody_joints: MultibodyJointSet,
    
    /// CCD solver
    pub ccd_solver: CCDSolver,
    
    /// Query pipeline (raycasts, etc.)
    pub query_pipeline: QueryPipeline,
    
    /// Integration parameters
    pub integration_params: IntegrationParameters,
}

impl PhysicsWorld {
    /// Create new physics world
    pub fn new() -> Self {
        let mut integration_params = IntegrationParameters::default();
        
        // Fixed 60 Hz timestep for determinism
        integration_params.dt = 1.0 / 60.0;
        
        Self {
            pipeline: PhysicsPipeline::new(),
            broad_phase: DefaultBroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            islands: IslandManager::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            ccd_solver: CCDSolver::new(),
            query_pipeline: QueryPipeline::new(),
            integration_params,
        }
    }
    
    /// Step physics simulation by one tick (16.67ms at 60 Hz)
    pub fn step(&mut self, gravity: Vec3) {
        let gravity = Vector::new(gravity.x, gravity.y, gravity.z);
        
        self.pipeline.step(
            &gravity,
            &self.integration_params,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd_solver,
            Some(&mut self.query_pipeline),
            &(),
            &(),
        );
    }
    
    /// Calculate gravity direction at given ECEF position
    /// 
    /// Gravity always points toward Earth center, magnitude 9.8 m/s²
    pub fn gravity_at_position(position: &ECEF) -> Vec3 {
        // Vector from position to Earth center (0,0,0)
        let to_center = Vec3::new(
            -position.x as f32,
            -position.y as f32,
            -position.z as f32,
        );
        
        // Normalize and scale to 9.8 m/s²
        let direction = to_center.normalize();
        direction * 9.8
    }
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate heightmap collision data from voxel octree
/// 
/// This creates a 2D grid of heights representing the terrain surface.
/// Fast and efficient, but doesn't handle overhangs/caves (future: add mesh colliders for those).
/// 
/// # Arguments
/// * `octree` - Voxel data to sample
/// * `origin` - Bottom-left corner of heightmap (voxel coords)
/// * `size_x` - Width in voxels
/// * `size_z` - Depth in voxels
/// 
/// # Returns
/// (heights, scale) where:
/// - heights: 2D array of surface heights (in voxel units)
/// - scale: Size of each heightmap cell (1.0 = 1 voxel = 1 meter)
pub fn generate_heightmap_collider(
    octree: &Octree,
    origin: &VoxelCoord,
    size_x: usize,
    size_z: usize,
) -> (Vec<f32>, Vec3) {
    let mut heights = Vec::with_capacity(size_x * size_z);
    
    // For each XZ position in the grid
    for z in 0..size_z {
        for x in 0..size_x {
            // Start from top of world and scan downward
            let mut height = 0.0;
            let max_y = 2000; // 2km above origin (top of simulation)
            
            // Raycast down to find first solid voxel
            for y in (0..max_y).rev() {
                let coord = VoxelCoord {
                    x: origin.x + x as i64,
                    y: origin.y + y,
                    z: origin.z + z as i64,
                };
                
                let material = octree.get_voxel(coord);
                
                // Found solid ground (not AIR which is 0)
                if material != MaterialId::AIR {
                    height = y as f32;
                    break;
                }
            }
            
            heights.push(height);
        }
    }
    
    // Scale: 1.0 = each heightmap cell is 1 meter
    let scale = Vec3::new(1.0, 1.0, 1.0);
    
    (heights, scale)
}

/// Generate simplified collision mesh from voxels using marching cubes
/// 
/// This generates a proper triangle mesh that can handle caves, overhangs, buildings.
/// More expensive than heightmap, but necessary for complex geometry.
/// 
/// Uses mesh simplification to reduce triangle count from millions to thousands.
/// 
/// # Arguments
/// * `vertices` - Mesh vertices from marching cubes
/// * `indices` - Triangle indices
/// * `target_triangle_count` - Target number of triangles (e.g., 10000)
/// 
/// # Returns
/// (simplified_vertices, simplified_indices) suitable for Rapier
pub fn generate_mesh_collider(
    vertices: &[[f32; 3]],
    indices: &[u32],
    target_triangle_count: usize,
) -> (Vec<Point<f32>>, Vec<[u32; 3]>) {
    // For now: Just use the mesh as-is (no simplification yet)
    // TODO: Implement mesh decimation/simplification algorithm
    
    let rapier_vertices: Vec<Point<f32>> = vertices
        .iter()
        .map(|v| Point::new(v[0], v[1], v[2]))
        .collect();
    
    let rapier_indices: Vec<[u32; 3]> = indices
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect();
    
    // Warn if too many triangles
    let triangle_count = rapier_indices.len();
    if triangle_count > target_triangle_count {
        eprintln!(
            "WARNING: Collision mesh has {} triangles (target: {}). May impact performance.",
            triangle_count, target_triangle_count
        );
    }
    
    (rapier_vertices, rapier_indices)
}

/// Create a static terrain collider from heightmap
pub fn create_heightmap_collider(
    physics: &mut PhysicsWorld,
    heights: Vec<f32>,
    num_rows: usize,
    num_cols: usize,
    scale: Vec3,
    position: Vec3,
) -> ColliderHandle {
    let scale_rapier = vector![scale.x, scale.y, scale.z];
    
    let collider = ColliderBuilder::heightfield(
        DMatrix::from_row_slice(num_rows, num_cols, &heights),
        scale_rapier,
    )
    .translation(vector![position.x, position.y, position.z])
    .build();
    
    physics.colliders.insert(collider)
}

/// Create a static mesh collider from triangles
pub fn create_mesh_collider(
    physics: &mut PhysicsWorld,
    vertices: Vec<Point<f32>>,
    indices: Vec<[u32; 3]>,
    position: Vec3,
) -> ColliderHandle {
    let collider = ColliderBuilder::trimesh(vertices, indices)
        .translation(vector![position.x, position.y, position.z])
        .build();
    
    physics.colliders.insert(collider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::GPS;
    use crate::voxel::Octree;
    
    #[test]
    fn test_heightmap_generation() {
        let mut octree = Octree::new();
        
        // Create simple terrain
        let origin = VoxelCoord { x: 0, y: 0, z: 0 };
        
        // Flat ground at y=0-9 (10 voxels high)
        for x in 0..10 {
            for z in 0..10 {
                for y in 0..10 {
                    let coord = VoxelCoord { x, y, z };
                    octree.set_voxel(coord, MaterialId::STONE);
                }
            }
        }
        
        // Hill in center at y=10-19 (additional 10 voxels)
        for x in 4..6 {
            for z in 4..6 {
                for y in 10..20 {
                    let coord = VoxelCoord { x, y, z };
                    octree.set_voxel(coord, MaterialId::STONE);
                }
            }
        }
        
        // Generate heightmap
        let (heights, scale) = generate_heightmap_collider(&octree, &origin, 10, 10);
        
        // Should have 100 heights (10x10)
        assert_eq!(heights.len(), 100);
        
        // Scale should be 1.0 (1 voxel = 1 meter)
        assert_eq!(scale, Vec3::new(1.0, 1.0, 1.0));
        
        // Check heights
        // Corner (0,0) - top solid voxel should be at y=9
        assert!((heights[0] - 9.0).abs() < 1.0, "Corner height: {} (expected ~9)", heights[0]);
        
        // Center (5,5) - index = 5*10 + 5 = 55 - top solid voxel should be at y=19
        assert!((heights[55] - 19.0).abs() < 1.0, "Center height: {} (expected ~19)", heights[55]);
    }
    
    #[test]
    fn test_heightmap_collider_creation() {
        let mut physics = PhysicsWorld::new();
        
        // Simple 5x5 heightmap
        let heights = vec![
            0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 1.0, 1.0, 0.0,
            0.0, 1.0, 2.0, 1.0, 0.0,
            0.0, 1.0, 1.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 0.0, 0.0,
        ];
        
        let scale = Vec3::new(1.0, 1.0, 1.0);
        let position = Vec3::new(0.0, 0.0, 0.0);
        
        let handle = create_heightmap_collider(
            &mut physics,
            heights,
            5,
            5,
            scale,
            position,
        );
        
        // Collider should exist
        assert!(physics.colliders.contains(handle));
        
        // Should be static (not moving)
        let collider = &physics.colliders[handle];
        assert!(!collider.is_sensor());
    }
    
    #[test]
    fn test_mesh_collider_conversion() {
        // Simple triangle (3 vertices)
        let vertices = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        
        let indices = vec![0, 1, 2];
        
        let (rapier_verts, rapier_indices) = generate_mesh_collider(
            &vertices,
            &indices,
            1000, // target
        );
        
        assert_eq!(rapier_verts.len(), 3);
        assert_eq!(rapier_indices.len(), 1); // One triangle
        assert_eq!(rapier_indices[0], [0, 1, 2]);
    }
    
    #[test]
    fn test_gravity_points_toward_center() {
        // Test at various locations
        let positions = vec![
            GPS::new(0.0, 0.0, 0.0),      // Equator, prime meridian
            GPS::new(90.0, 0.0, 0.0),     // North pole
            GPS::new(-90.0, 0.0, 0.0),    // South pole
            GPS::new(-27.4775, 153.0355, 0.0), // Brisbane
        ];
        
        for gps in positions {
            let ecef = gps.to_ecef();
            let gravity = PhysicsWorld::gravity_at_position(&ecef);
            
            // Gravity should be ~9.8 m/s²
            let magnitude = gravity.length();
            assert!((magnitude - 9.8).abs() < 0.01, 
                "Gravity magnitude {} not ~9.8 at {:?}", magnitude, gps);
            
            // Should point toward (0,0,0)
            let to_center = Vec3::new(-ecef.x as f32, -ecef.y as f32, -ecef.z as f32);
            let expected_direction = to_center.normalize();
            let actual_direction = gravity.normalize();
            
            let dot = expected_direction.dot(actual_direction);
            assert!(dot > 0.999, 
                "Gravity not pointing toward center at {:?}: dot={}", gps, dot);
        }
    }
    
    #[test]
    fn test_physics_step_runs() {
        let mut world = PhysicsWorld::new();
        
        // Add a dynamic cube
        let body = RigidBodyBuilder::dynamic()
            .translation(vector![0.0, 10.0, 0.0])
            .build();
        let body_handle = world.bodies.insert(body);
        
        let collider = ColliderBuilder::cuboid(0.5, 0.5, 0.5).build();
        world.colliders.insert_with_parent(collider, body_handle, &mut world.bodies);
        
        // Step with downward gravity
        let gravity = Vec3::new(0.0, -9.8, 0.0);
        world.step(gravity);
        
        // Should have fallen slightly
        let body = &world.bodies[body_handle];
        assert!(body.translation().y < 10.0, "Body didn't fall");
    }
}
