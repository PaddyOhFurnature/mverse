//! Physics simulation using Rapier3D
//! 
//! Handles:
//! - Gravity toward Earth center (spherical)
//! - Player character physics (kinematic controller)
//! - Collision meshes from voxels
//! - Deterministic simulation (P2P requirement)

use crate::coordinates::ECEF;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::GPS;
    
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
