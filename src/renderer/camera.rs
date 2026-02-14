//! Camera system with floating-origin support
//!
//! The camera uses f64 precision for ECEF coordinates to avoid jitter at Earth-scale distances.
//! The floating origin technique subtracts the camera position from all world coordinates
//! before converting to f32 for GPU rendering.

use glam::{DVec3, DQuat, Mat4, Vec3};

/// Camera with floating-origin support
///
/// Position stored as f64 ECEF coordinates to handle large values (~6.4 million meters)
/// without precision loss. The view matrix translates the world by -camera_position
/// before converting to f32 for the GPU.
pub struct Camera {
    /// Camera position in ECEF coordinates (f64 for precision)
    pub position: DVec3,
    
    /// Camera orientation as a quaternion (f64 for precision)
    pub orientation: DQuat,
    
    /// Field of view in degrees
    pub fov_deg: f64,
    
    /// Near plane distance
    pub near: f32,
    
    /// Far plane distance
    pub far: f32,
    
    /// Movement speed in meters per second (base speed)
    pub base_speed: f64,
    
    /// Speed multiplier (adjusted by altitude)
    pub speed_multiplier: f64,
}

impl Camera {
    /// Create a new camera at the given ECEF position
    pub fn new(position: DVec3, look_at: DVec3) -> Self {
        let forward = (look_at - position).normalize();
        let up = DVec3::Z; // ECEF Z axis points to north pole
        
        // Build orientation quaternion from forward vector
        let right = forward.cross(up).normalize();
        let up = right.cross(forward).normalize();
        
        // Create rotation matrix and convert to quaternion
        let rotation_matrix = glam::DMat3::from_cols(right, up, -forward);
        let orientation = DQuat::from_mat3(&rotation_matrix);
        
        Self {
            position,
            orientation,
            fov_deg: 60.0,
            near: 1.0,        // 1 meter near plane
            far: 100_000.0,   // 100 km far plane (sufficient for local view)
            base_speed: 10.0, // 10 m/s base
            speed_multiplier: 1.0,
        }
    }
    
    /// Create a camera at Brisbane looking at the city center
    pub fn brisbane() -> Self {
        use crate::coordinates::{gps_to_ecef, GpsPos};
        
        // Brisbane CBD coordinates
        let gps = GpsPos {
            lat_deg: -27.4698,
            lon_deg: 153.0251,
            elevation_m: 100.0, // Start 100m above ground
        };
        let position_ecef = gps_to_ecef(&gps);
        let position = DVec3::new(position_ecef.x, position_ecef.y, position_ecef.z);
        
        // Look toward Earth center (down) - simplest approach that guarantees correct orientation
        let look_at = DVec3::ZERO; // Earth center in ECEF
        
        Self::new(position, look_at)
    }
    
    /// Get camera forward direction (in camera's local space)
    pub fn forward(&self) -> DVec3 {
        self.orientation * DVec3::NEG_Z
    }
    
    /// Get camera right direction
    pub fn right(&self) -> DVec3 {
        self.orientation * DVec3::X
    }
    
    /// Get camera up direction
    pub fn up(&self) -> DVec3 {
        self.orientation * DVec3::Y
    }
    
    /// Move camera relative to its orientation
    pub fn move_relative(&mut self, forward: f64, right: f64, up: f64, delta_time: f64) {
        let speed = self.base_speed * self.speed_multiplier;
        let movement = self.forward() * forward * speed * delta_time
            + self.right() * right * speed * delta_time
            + self.up() * up * speed * delta_time;
        self.position += movement;
        
        // Update speed multiplier based on altitude
        self.update_speed_by_altitude();
    }
    
    /// Rotate camera by pitch (up/down) and yaw (left/right)
    pub fn rotate(&mut self, pitch_delta: f64, yaw_delta: f64) {
        // Yaw around world up axis (for horizon stability)
        let yaw_rotation = DQuat::from_axis_angle(DVec3::Z, yaw_delta);
        
        // Pitch around camera's right axis
        let right = self.right();
        let pitch_rotation = DQuat::from_axis_angle(right, pitch_delta);
        
        // Apply rotations
        self.orientation = yaw_rotation * self.orientation * pitch_rotation;
        self.orientation = self.orientation.normalize();
    }
    
    /// Update speed multiplier based on altitude above Earth surface
    fn update_speed_by_altitude(&mut self) {
        const EARTH_RADIUS: f64 = 6_371_000.0; // meters
        let altitude = self.position.length() - EARTH_RADIUS;
        
        // Speed scales with altitude: 1x at ground, 10x at 10km, 100x at 100km, etc.
        self.speed_multiplier = (1.0 + altitude / 1000.0).max(1.0);
    }
    
    /// Compute view matrix with floating origin
    ///
    /// Returns (view_matrix, camera_offset) where:
    /// - view_matrix: f32 matrix for GPU
    /// - camera_offset: DVec3 offset to subtract from world coordinates (= camera position)
    pub fn view_matrix(&self) -> (Mat4, DVec3) {
        // The floating origin offset is the camera position
        let offset = self.position;
        
        // Convert orientation to f32 Mat4
        let rotation = Mat4::from_quat(glam::Quat::from_xyzw(
            self.orientation.x as f32,
            self.orientation.y as f32,
            self.orientation.z as f32,
            self.orientation.w as f32,
        ));
        
        // View matrix is the INVERSE of the camera transform
        // For an orthonormal rotation matrix, inverse = transpose
        let view = rotation.transpose();
        
        (view, offset)
    }
    
    /// Compute projection matrix
    pub fn projection_matrix(&self, aspect_ratio: f32) -> Mat4 {
        Mat4::perspective_rh(
            (self.fov_deg as f32).to_radians(),
            aspect_ratio,
            self.near,
            self.far,
        )
    }
    
    /// Compute combined view-projection matrix with floating origin
    ///
    /// Returns (view_proj_matrix, camera_offset)
    pub fn view_projection_matrix(&self, aspect_ratio: f32) -> (Mat4, DVec3) {
        let (view, offset) = self.view_matrix();
        let proj = self.projection_matrix(aspect_ratio);
        (proj * view, offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_camera_creation() {
        let pos = DVec3::new(6_371_000.0, 0.0, 0.0); // On equator at sea level
        let look_at = DVec3::new(6_371_100.0, 0.0, 0.0); // Looking outward
        let camera = Camera::new(pos, look_at);
        
        assert_eq!(camera.position, pos);
        assert!(camera.fov_deg > 0.0);
        assert!(camera.near > 0.0);
        assert!(camera.far > camera.near);
    }
    
    #[test]
    fn test_camera_brisbane() {
        let camera = Camera::brisbane();
        
        // Should be roughly 6.4 million meters from Earth center
        let distance = camera.position.length();
        assert!(distance > 6_300_000.0 && distance < 6_400_000.0);
    }
    
    #[test]
    fn test_camera_movement() {
        let mut camera = Camera::brisbane();
        let initial_pos = camera.position;
        
        // Move forward
        camera.move_relative(1.0, 0.0, 0.0, 1.0);
        
        let distance_moved = (camera.position - initial_pos).length();
        assert!(distance_moved > 0.0);
        assert!(distance_moved < 1000.0); // Should move reasonable distance
    }
    
    #[test]
    fn test_camera_rotation() {
        let mut camera = Camera::brisbane();
        let initial_orientation = camera.orientation;
        
        // Rotate
        camera.rotate(0.1, 0.1);
        
        // Orientation should have changed
        let dot = initial_orientation.dot(camera.orientation);
        assert!(dot < 1.0); // Not exactly the same
        assert!(dot > 0.9); // But still close (small rotation)
    }
    
    #[test]
    fn test_view_projection_matrix() {
        let camera = Camera::brisbane();
        let aspect = 16.0 / 9.0;
        
        let (view_proj, offset) = camera.view_projection_matrix(aspect);
        
        // Offset should be camera position
        assert_eq!(offset, camera.position);
        
        // View-projection matrix should be valid (non-zero, finite)
        assert!(view_proj.to_cols_array().iter().all(|v| v.is_finite()));
        assert!(view_proj.to_cols_array().iter().any(|v| *v != 0.0));
    }
    
    #[test]
    fn test_speed_scales_with_altitude() {
        let mut camera = Camera::brisbane();
        camera.position = DVec3::new(6_371_000.0, 0.0, 0.0); // Sea level
        camera.update_speed_by_altitude();
        let speed_at_sea = camera.speed_multiplier;
        
        // Move to 10km altitude
        camera.position = DVec3::new(6_381_000.0, 0.0, 0.0);
        camera.update_speed_by_altitude();
        let speed_at_10km = camera.speed_multiplier;
        
        // Speed should increase with altitude
        assert!(speed_at_10km > speed_at_sea);
    }
    
    #[test]
    fn test_no_jitter_at_large_coords() {
        // Test that camera works correctly at Brisbane ECEF coordinates
        let camera = Camera::brisbane();
        
        // Position magnitude should be around 6.4 million
        let mag = camera.position.length();
        assert!(mag > 6_000_000.0);
        
        // View matrix should be finite and valid
        let (view, offset) = camera.view_matrix();
        assert!(view.to_cols_array().iter().all(|v| v.is_finite()));
        
        // Offset should match position exactly (f64 → f64, no conversion loss)
        assert_eq!(offset, camera.position);
    }
}
