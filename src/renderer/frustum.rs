//! Frustum culling for efficient rendering
//!
//! Extracts camera frustum planes from view-projection matrix and tests AABBs for visibility.

use glam::{Mat4, Vec3};

/// Camera frustum defined by 6 planes
#[derive(Debug, Clone)]
pub struct Frustum {
    /// Frustum planes: [left, right, bottom, top, near, far]
    planes: [Plane; 6],
}

/// A plane defined by normal + distance
#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vec3,
    distance: f32,
}

impl Frustum {
    /// Extract frustum planes from view-projection matrix
    pub fn from_view_projection(view_proj: &Mat4) -> Self {
        // Extract planes using Gribb-Hartmann method
        let m = view_proj.to_cols_array();
        
        // Left plane: m3 + m0
        let left = Plane::normalize(
            Vec3::new(m[3] + m[0], m[7] + m[4], m[11] + m[8]),
            m[15] + m[12],
        );
        
        // Right plane: m3 - m0
        let right = Plane::normalize(
            Vec3::new(m[3] - m[0], m[7] - m[4], m[11] - m[8]),
            m[15] - m[12],
        );
        
        // Bottom plane: m3 + m1
        let bottom = Plane::normalize(
            Vec3::new(m[3] + m[1], m[7] + m[5], m[11] + m[9]),
            m[15] + m[13],
        );
        
        // Top plane: m3 - m1
        let top = Plane::normalize(
            Vec3::new(m[3] - m[1], m[7] - m[5], m[11] - m[9]),
            m[15] - m[13],
        );
        
        // Near plane: m3 + m2
        let near = Plane::normalize(
            Vec3::new(m[3] + m[2], m[7] + m[6], m[11] + m[10]),
            m[15] + m[14],
        );
        
        // Far plane: m3 - m2
        let far = Plane::normalize(
            Vec3::new(m[3] - m[2], m[7] - m[6], m[11] - m[10]),
            m[15] - m[14],
        );
        
        Self {
            planes: [left, right, bottom, top, near, far],
        }
    }
    
    /// Test if an AABB is visible (not culled)
    /// 
    /// Returns true if the AABB intersects or is inside the frustum.
    /// Conservative test: may have false positives (rendering invisible objects)
    /// but never false negatives (culling visible objects).
    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        // Get AABB center and extents
        let center = (min + max) * 0.5;
        let extents = max - center;
        
        // Test against each plane
        for plane in &self.planes {
            // Distance from plane to AABB center
            let dist = plane.normal.dot(center) + plane.distance;
            
            // Project extents onto plane normal (maximum extent along normal)
            let radius = extents.x * plane.normal.x.abs()
                + extents.y * plane.normal.y.abs()
                + extents.z * plane.normal.z.abs();
            
            // If center is farther than radius behind plane, AABB is completely outside
            if dist < -radius {
                return false; // Culled
            }
        }
        
        true // Visible (intersects or inside)
    }
}

impl Plane {
    /// Normalize a plane equation
    fn normalize(normal: Vec3, distance: f32) -> Self {
        let len = normal.length();
        Self {
            normal: normal / len,
            distance: distance / len,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4;

    #[test]
    fn test_frustum_basic() {
        // Create a simple perspective projection
        let proj = Mat4::perspective_rh(90f32.to_radians(), 1.0, 0.1, 100.0);
        let view = Mat4::IDENTITY;
        let view_proj = proj * view;
        
        let frustum = Frustum::from_view_projection(&view_proj);
        
        // AABB at origin should be visible
        let visible = frustum.intersects_aabb(
            Vec3::new(-1.0, -1.0, -2.0),
            Vec3::new(1.0, 1.0, -1.0),
        );
        assert!(visible, "AABB at origin should be visible");
        
        // AABB far behind camera should be culled
        let culled = frustum.intersects_aabb(
            Vec3::new(-1.0, -1.0, 150.0),
            Vec3::new(1.0, 1.0, 200.0),
        );
        assert!(!culled, "AABB behind far plane should be culled");
    }

    #[test]
    fn test_frustum_off_screen() {
        let proj = Mat4::perspective_rh(60f32.to_radians(), 1.0, 0.1, 100.0);
        let view = Mat4::IDENTITY;
        let view_proj = proj * view;
        
        let frustum = Frustum::from_view_projection(&view_proj);
        
        // AABB far to the side should be culled
        let culled = frustum.intersects_aabb(
            Vec3::new(50.0, -1.0, -10.0),
            Vec3::new(51.0, 1.0, -9.0),
        );
        assert!(!culled, "AABB far outside frustum should be culled");
    }
}
