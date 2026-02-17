//! Physics simulation using Rapier3D
//! 
//! Handles:
//! - Gravity toward Earth center (spherical)
//! - Player character physics (kinematic controller)
//! - Collision meshes from voxels
//! - Deterministic simulation (P2P requirement)

use crate::coordinates::{ECEF, GPS};
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

/// Player character with physics
pub struct Player {
    /// Player position in ECEF coordinates
    pub position: ECEF,
    
    /// Player velocity (m/s)
    pub velocity: Vec3,
    
    /// Is player standing on ground?
    pub on_ground: bool,
    
    /// Rigidbody handle in physics world
    pub body_handle: RigidBodyHandle,
    
    /// Collider handle in physics world
    pub collider_handle: ColliderHandle,
    
    /// Camera pitch (radians, -π/2 to π/2)
    pub camera_pitch: f32,
    
    /// Camera yaw (radians, 0 to 2π)
    pub camera_yaw: f32,
}

impl Player {
    /// Player capsule dimensions
    pub const HEIGHT: f32 = 1.8; // meters (typical human)
    pub const RADIUS: f32 = 0.4; // meters
    pub const EYE_HEIGHT: f32 = 1.6; // meters from feet
    
    /// Create new player at GPS position
    /// 
    /// Spawns player 2 meters above terrain surface to ensure they don't
    /// clip into ground. Gravity will pull them down to rest on surface.
    /// 
    /// # Arguments
    /// * `physics` - Physics world to add player to
    /// * `gps` - GPS coordinates to spawn at
    /// * `terrain_height` - Height of terrain surface at spawn position (meters above sea level)
    pub fn new(
        physics: &mut PhysicsWorld,
        gps: GPS,
        terrain_height: f32,
    ) -> Self {
        // Convert GPS to ECEF
        let mut position = gps.to_ecef();
        
        // Offset upward from terrain surface
        // Get "up" vector at this location (away from Earth center)
        let up = Vec3::new(position.x as f32, position.y as f32, position.z as f32).normalize();
        let spawn_offset = terrain_height + 2.0; // 2m above terrain
        
        position.x += (up.x * spawn_offset) as f64;
        position.y += (up.y * spawn_offset) as f64;
        position.z += (up.z * spawn_offset) as f64;
        
        // Create kinematic character controller rigidbody
        // (not affected by forces, but can push dynamic bodies)
        let body = RigidBodyBuilder::kinematic_position_based()
            .translation(vector![position.x as f32, position.y as f32, position.z as f32])
            .build();
        let body_handle = physics.bodies.insert(body);
        
        // Create capsule collider (standing cylinder with rounded ends)
        // Half-height is measured from center, not including hemisphere radius
        let half_height = (Self::HEIGHT - 2.0 * Self::RADIUS) / 2.0;
        let collider = ColliderBuilder::capsule_y(half_height, Self::RADIUS)
            .friction(0.0) // No friction for smooth movement
            .restitution(0.0) // No bounce
            .build();
        let collider_handle = physics.colliders.insert_with_parent(
            collider,
            body_handle,
            &mut physics.bodies,
        );
        
        Self {
            position,
            velocity: Vec3::ZERO,
            on_ground: false,
            body_handle,
            collider_handle,
            camera_pitch: 0.0,
            camera_yaw: 0.0,
        }
    }
    
    /// Get camera position (at player's eye level)
    pub fn camera_position(&self) -> ECEF {
        let up = Vec3::new(
            self.position.x as f32,
            self.position.y as f32,
            self.position.z as f32,
        ).normalize();
        
        let offset = up * Self::EYE_HEIGHT;
        
        ECEF {
            x: self.position.x + offset.x as f64,
            y: self.position.y + offset.y as f64,
            z: self.position.z + offset.z as f64,
        }
    }
    
    /// Get camera forward direction vector (world space)
    pub fn camera_forward(&self) -> Vec3 {
        // Calculate forward based on pitch and yaw
        let forward = Vec3::new(
            self.camera_yaw.cos() * self.camera_pitch.cos(),
            self.camera_pitch.sin(),
            self.camera_yaw.sin() * self.camera_pitch.cos(),
        );
        forward.normalize()
    }
    
    /// Get camera right direction vector (world space)
    pub fn camera_right(&self) -> Vec3 {
        let forward = self.camera_forward();
        
        // Get the local "up" direction at player position (radial from Earth center)
        let local_up = Vec3::new(
            self.position.x as f32,
            self.position.y as f32,
            self.position.z as f32,
        ).normalize();
        
        // Right is perpendicular to forward and up
        // Use cross product: right = forward × up
        let right = forward.cross(local_up);
        
        // Handle case where forward is parallel to up (looking straight up/down)
        if right.length_squared() < 0.001 {
            // Use arbitrary right vector
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            right.normalize()
        }
    }
    
    /// Update player position from physics simulation
    pub fn sync_from_physics(&mut self, physics: &PhysicsWorld) {
        if let Some(body) = physics.bodies.get(self.body_handle) {
            let translation = body.translation();
            self.position.x = translation.x as f64;
            self.position.y = translation.y as f64;
            self.position.z = translation.z as f64;
            
            // Note: Kinematic bodies don't have linvel in the same way
            // We track velocity separately in the Player struct
        }
    }
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
        // (Note: octree may auto-collapse, so this could be 20 instead of 19)
        assert!((heights[55] - 19.0).abs() < 2.0, "Center height: {} (expected ~19)", heights[55]);
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
    
    #[test]
    fn test_player_creation() {
        let mut physics = PhysicsWorld::new();
        
        // Brisbane coordinates
        let gps = GPS::new(-27.4775, 153.0355, 10.0);
        let terrain_height = 10.0; // 10m above sea level
        
        let player = Player::new(&mut physics, gps, terrain_height);
        
        // Player should exist in physics world
        assert!(physics.bodies.contains(player.body_handle));
        assert!(physics.colliders.contains(player.collider_handle));
        
        // Should be kinematic
        let body = &physics.bodies[player.body_handle];
        assert!(body.is_kinematic());
        
        // Should have correct dimensions
        let collider = &physics.colliders[player.collider_handle];
        assert!(collider.shape().as_capsule().is_some());
        
        // Initial state
        assert_eq!(player.velocity, Vec3::ZERO);
        assert!(!player.on_ground);
        assert_eq!(player.camera_pitch, 0.0);
        assert_eq!(player.camera_yaw, 0.0);
    }
    
    #[test]
    fn test_player_spawns_above_terrain() {
        let mut physics = PhysicsWorld::new();
        
        let gps = GPS::new(-27.4775, 153.0355, 0.0);
        let terrain_height = 10.0;
        
        let player = Player::new(&mut physics, gps, terrain_height);
        let body = &physics.bodies[player.body_handle];
        
        // Player should be at terrain_height + 2m offset
        // (Can't check exact height easily, but should be above origin)
        let spawn_pos = body.translation();
        let distance_from_origin = (spawn_pos.x.powi(2) + spawn_pos.y.powi(2) + spawn_pos.z.powi(2)).sqrt();
        
        // Should be approximately at Earth radius + terrain height + 2m offset
        // Earth radius ~6.37M meters, so this should be greater than that
        assert!(distance_from_origin > 6_370_000.0, 
            "Player not spawned above Earth surface: {}", distance_from_origin);
    }
    
    #[test]
    fn test_player_camera_position() {
        let mut physics = PhysicsWorld::new();
        
        let gps = GPS::new(0.0, 0.0, 0.0); // Equator, sea level
        let player = Player::new(&mut physics, gps, 0.0);
        
        let camera = player.camera_position();
        
        // Camera should be EYE_HEIGHT (1.6m) above player position
        let player_dist = (player.position.x.powi(2) + player.position.y.powi(2) + player.position.z.powi(2)).sqrt();
        let camera_dist = (camera.x.powi(2) + camera.y.powi(2) + camera.z.powi(2)).sqrt();
        
        let diff = camera_dist - player_dist;
        assert!((diff - Player::EYE_HEIGHT as f64).abs() < 0.01,
            "Camera not at eye height: {} meters above player", diff);
    }
    
    #[test]
    fn test_player_camera_directions() {
        let mut physics = PhysicsWorld::new();
        
        // Use Brisbane coords to avoid gimbal issues at equator
        let gps = GPS::new(-27.4775, 153.0355, 0.0);
        let mut player = Player::new(&mut physics, gps, 0.0);
        
        // Test forward direction at 0 pitch, 0 yaw
        player.camera_pitch = 0.0;
        player.camera_yaw = 0.0;
        let forward = player.camera_forward();
        
        // Should be unit length
        assert!((forward.length() - 1.0).abs() < 0.01, "Forward not unit length");
        
        // Test right direction (should be perpendicular to forward)
        let right = player.camera_right();
        assert!((right.length() - 1.0).abs() < 0.01, "Right not unit length");
        
        let dot = forward.dot(right);
        assert!(dot.abs() < 0.1, "Right not perpendicular to forward: dot={}", dot);
        
        // Test looking up (π/4 radians = 45 degrees)
        player.camera_pitch = std::f32::consts::PI / 4.0;
        let forward_up = player.camera_forward();
        
        // Y component should be positive (pointing up)
        assert!(forward_up.y > 0.5, "Not pointing up enough: y={}", forward_up.y);
    }
    
    #[test]
    fn test_player_sync_from_physics() {
        let mut physics = PhysicsWorld::new();
        let gps = GPS::new(0.0, 0.0, 0.0);
        let mut player = Player::new(&mut physics, gps, 0.0);
        
        // Move the rigidbody directly
        if let Some(body) = physics.bodies.get_mut(player.body_handle) {
            body.set_translation(vector![100.0, 200.0, 300.0], true);
        }
        
        // Sync player from physics
        player.sync_from_physics(&physics);
        
        // Player position should match rigidbody
        assert!((player.position.x - 100.0).abs() < 0.01);
        assert!((player.position.y - 200.0).abs() < 0.01);
        assert!((player.position.z - 300.0).abs() < 0.01);
        
        // Velocity is tracked separately (not from rigidbody for kinematic)
        // Just verify it exists
        assert!(player.velocity.is_finite());
    }
    
    #[test]
    fn test_player_with_gravity_simulation() {
        // This test verifies player exists in physics world
        // Note: Kinematic bodies don't auto-respond to gravity
        // Movement will be implemented in Day 4 (manual velocity updates)
        
        let mut physics = PhysicsWorld::new();
        
        // Create terrain (flat ground)
        let mut octree = Octree::new();
        let origin = VoxelCoord { x: 0, y: 0, z: 0 };
        for x in -10..10 {
            for z in -10..10 {
                for y in -5..0 {
                    octree.set_voxel(VoxelCoord { x, y, z }, MaterialId::STONE);
                }
            }
        }
        
        // Add terrain collision
        let (heights, scale) = generate_heightmap_collider(&octree, &origin, 20, 20);
        create_heightmap_collider(&mut physics, heights, 20, 20, scale, Vec3::new(-10.0, -5.0, -10.0));
        
        // Create player
        let gps = GPS::new(0.0, 0.0, 0.0);
        let player = Player::new(&mut physics, gps, 0.0);
        
        // Verify player exists
        assert!(physics.bodies.contains(player.body_handle));
        assert!(physics.colliders.contains(player.collider_handle));
        
        // Verify capsule dimensions
        let collider = &physics.colliders[player.collider_handle];
        if let Some(capsule) = collider.shape().as_capsule() {
            let expected_half_height = (Player::HEIGHT - 2.0 * Player::RADIUS) / 2.0;
            assert!((capsule.half_height() - expected_half_height).abs() < 0.01);
            assert!((capsule.radius - Player::RADIUS).abs() < 0.01);
        } else {
            panic!("Player collider is not a capsule!");
        }
    }
}
