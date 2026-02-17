//! FPS camera for terrain navigation

use glam::{Mat4, Vec3};
use std::f32::consts::FRAC_PI_2;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::event::ElementState;

/// First-person camera with WASD movement and mouse look
pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,   // Rotation around Y axis (radians)
    pub pitch: f32, // Rotation around X axis (radians)
    pub fov: f32,   // Field of view (radians)
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
    pub speed: f32,
    pub sensitivity: f32,
}

impl Camera {
    /// Create camera at position looking at target
    pub fn new(position: Vec3, aspect: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov: 70.0_f32.to_radians(),
            aspect,
            near: 0.1,
            far: 10000.0, // 10km view distance
            speed: 10.0,  // 10 m/s
            sensitivity: 0.002,
        }
    }

    /// Build combined view-projection matrix
    pub fn build_view_projection_matrix(&self) -> Mat4 {
        let view = self.build_view_matrix();
        let proj = self.build_projection_matrix();
        proj * view
    }

    /// Build view matrix (world → camera space)
    fn build_view_matrix(&self) -> Mat4 {
        // Calculate forward vector from yaw/pitch
        let forward = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );

        let target = self.position + forward;
        Mat4::look_at_rh(self.position, target, Vec3::Y)
    }

    /// Build projection matrix (camera → clip space)
    fn build_projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov, self.aspect, self.near, self.far)
    }

    /// Get forward direction vector
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    /// Get right direction vector
    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    /// Update from keyboard input
    pub fn process_keyboard(&mut self, key: &PhysicalKey, state: ElementState, dt: f32) {
        let pressed = state == ElementState::Pressed;
        if !pressed {
            return;
        }

        let forward = self.forward();
        let right = self.right();
        let distance = self.speed * dt;

        if let PhysicalKey::Code(code) = key {
            match code {
                KeyCode::KeyW => self.position += forward * distance,
                KeyCode::KeyS => self.position -= forward * distance,
                KeyCode::KeyA => self.position -= right * distance,
                KeyCode::KeyD => self.position += right * distance,
                KeyCode::Space => self.position += Vec3::Y * distance,
                KeyCode::ShiftLeft => self.position -= Vec3::Y * distance,
                _ => {}
            }
        }
    }

    /// Update from mouse movement
    pub fn process_mouse(&mut self, delta_x: f64, delta_y: f64) {
        self.yaw += delta_x as f32 * self.sensitivity;
        self.pitch -= delta_y as f32 * self.sensitivity;

        // Clamp pitch to prevent gimbal lock
        self.pitch = self.pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
    }

    /// Update aspect ratio (window resize)
    pub fn resize(&mut self, width: u32, height: u32) {
        self.aspect = width as f32 / height as f32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_creation() {
        let camera = Camera::new(Vec3::ZERO, 16.0 / 9.0);
        assert_eq!(camera.position, Vec3::ZERO);
        assert_eq!(camera.aspect, 16.0 / 9.0);
    }

    #[test]
    fn test_camera_forward() {
        let camera = Camera::new(Vec3::ZERO, 1.0);
        let forward = camera.forward();
        
        // Looking forward (yaw=0, pitch=0) should be +X direction
        assert!((forward.x - 1.0).abs() < 0.01);
        assert!(forward.y.abs() < 0.01);
        assert!(forward.z.abs() < 0.01);
    }

    #[test]
    fn test_camera_right() {
        let camera = Camera::new(Vec3::ZERO, 1.0);
        let right = camera.right();
        
        // Right vector should be perpendicular to forward
        let forward = camera.forward();
        assert!(forward.dot(right).abs() < 0.01);
    }

    #[test]
    fn test_camera_mouse_look() {
        let mut camera = Camera::new(Vec3::ZERO, 1.0);
        let initial_yaw = camera.yaw;
        
        camera.process_mouse(100.0, 0.0);
        
        // Yaw should change
        assert_ne!(camera.yaw, initial_yaw);
        
        // Pitch should be clamped
        camera.process_mouse(0.0, 10000.0);
        assert!(camera.pitch > -FRAC_PI_2);
        assert!(camera.pitch < FRAC_PI_2);
    }
}
