//! Level of Detail (LOD) system for distance-based geometry simplification
//!
//! Reduces polygon count and complexity based on distance from camera.
//! 5 LOD levels from full detail to culled.

use glam::DVec3;

/// LOD level for geometry rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LodLevel {
    /// LOD 0: Full detail (0-50m)
    /// - All polygons rendered
    /// - 3D road volumes with thickness
    /// - Full building geometry
    Full = 0,
    
    /// LOD 1: High detail (50-200m)
    /// - 75% of polygons
    /// - Simplified road geometry
    /// - Detailed buildings
    High = 1,
    
    /// LOD 2: Medium detail (200-500m)
    /// - 50% of polygons
    /// - Flat road ribbons
    /// - Basic building shapes (boxes)
    Medium = 2,
    
    /// LOD 3: Low detail (500m-1km)
    /// - 25% of polygons
    /// - Roads as single lines
    /// - Buildings as billboards/impostors
    Low = 3,
    
    /// LOD 4: Culled (1km+)
    /// - Not rendered at all
    /// - Saves GPU bandwidth
    Culled = 4,
}

impl LodLevel {
    /// Determines LOD level based on distance from camera
    ///
    /// # Arguments
    /// * `distance_m` - Distance in meters from camera to object
    ///
    /// # Returns
    /// Appropriate LOD level for this distance
    pub fn from_distance(distance_m: f64) -> Self {
        if distance_m < 50.0 {
            LodLevel::Full
        } else if distance_m < 200.0 {
            LodLevel::High
        } else if distance_m < 500.0 {
            LodLevel::Medium
        } else if distance_m < 1000.0 {
            LodLevel::Low
        } else {
            LodLevel::Culled
        }
    }
    
    /// Gets the polygon reduction factor for this LOD
    ///
    /// # Returns
    /// Multiplier for polygon count (1.0 = all, 0.0 = none)
    pub fn polygon_factor(&self) -> f32 {
        match self {
            LodLevel::Full => 1.0,
            LodLevel::High => 0.75,
            LodLevel::Medium => 0.5,
            LodLevel::Low => 0.25,
            LodLevel::Culled => 0.0,
        }
    }
    
    /// Whether to use 3D road volumes at this LOD
    pub fn use_3d_roads(&self) -> bool {
        matches!(self, LodLevel::Full)
    }
    
    /// Whether to use simplified building geometry
    pub fn simplified_buildings(&self) -> bool {
        matches!(self, LodLevel::Medium | LodLevel::Low)
    }
    
    /// Whether to render at all
    pub fn should_render(&self) -> bool {
        !matches!(self, LodLevel::Culled)
    }
}

/// Calculates distance from camera to a point in ECEF coordinates
///
/// # Arguments
/// * `camera_ecef` - Camera position in ECEF
/// * `point_ecef` - Point position in ECEF
///
/// # Returns
/// Distance in meters
pub fn distance_to_camera(camera_ecef: &DVec3, point_ecef: &DVec3) -> f64 {
    (*camera_ecef - *point_ecef).length()
}

/// LOD manager for tracking and updating LOD levels
pub struct LodManager {
    /// Current camera position
    camera_position: DVec3,
    
    /// LOD distance thresholds
    thresholds: [f64; 4], // [full->high, high->med, med->low, low->culled]
}

impl LodManager {
    /// Creates a new LOD manager with default thresholds
    pub fn new() -> Self {
        Self {
            camera_position: DVec3::ZERO,
            thresholds: [50.0, 200.0, 500.0, 1000.0],
        }
    }
    
    /// Creates a LOD manager with custom distance thresholds
    ///
    /// # Arguments
    /// * `thresholds` - Array of 4 distance thresholds in meters [full->high, high->med, med->low, low->culled]
    pub fn with_thresholds(thresholds: [f64; 4]) -> Self {
        Self {
            camera_position: DVec3::ZERO,
            thresholds,
        }
    }
    
    /// Updates camera position
    pub fn update_camera(&mut self, position: DVec3) {
        self.camera_position = position;
    }
    
    /// Gets LOD level for a point
    pub fn get_lod(&self, point_ecef: &DVec3) -> LodLevel {
        let distance = distance_to_camera(&self.camera_position, point_ecef);
        
        if distance < self.thresholds[0] {
            LodLevel::Full
        } else if distance < self.thresholds[1] {
            LodLevel::High
        } else if distance < self.thresholds[2] {
            LodLevel::Medium
        } else if distance < self.thresholds[3] {
            LodLevel::Low
        } else {
            LodLevel::Culled
        }
    }
    
    /// Gets current camera position
    pub fn camera_position(&self) -> DVec3 {
        self.camera_position
    }
}

impl Default for LodManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lod_from_distance() {
        assert_eq!(LodLevel::from_distance(25.0), LodLevel::Full);
        assert_eq!(LodLevel::from_distance(100.0), LodLevel::High);
        assert_eq!(LodLevel::from_distance(350.0), LodLevel::Medium);
        assert_eq!(LodLevel::from_distance(750.0), LodLevel::Low);
        assert_eq!(LodLevel::from_distance(1500.0), LodLevel::Culled);
    }
    
    #[test]
    fn test_polygon_factors() {
        assert_eq!(LodLevel::Full.polygon_factor(), 1.0);
        assert_eq!(LodLevel::High.polygon_factor(), 0.75);
        assert_eq!(LodLevel::Medium.polygon_factor(), 0.5);
        assert_eq!(LodLevel::Low.polygon_factor(), 0.25);
        assert_eq!(LodLevel::Culled.polygon_factor(), 0.0);
    }
    
    #[test]
    fn test_lod_manager() {
        let mut manager = LodManager::new();
        manager.update_camera(DVec3::ZERO);
        
        let close_point = DVec3::new(25.0, 0.0, 0.0);
        let far_point = DVec3::new(2000.0, 0.0, 0.0);
        
        assert_eq!(manager.get_lod(&close_point), LodLevel::Full);
        assert_eq!(manager.get_lod(&far_point), LodLevel::Culled);
    }
    
    #[test]
    fn test_custom_thresholds() {
        let manager = LodManager::with_thresholds([100.0, 300.0, 700.0, 1500.0]);
        assert_eq!(manager.thresholds[0], 100.0);
        assert_eq!(manager.thresholds[3], 1500.0);
    }
    
    #[test]
    fn test_should_render() {
        assert!(LodLevel::Full.should_render());
        assert!(LodLevel::High.should_render());
        assert!(LodLevel::Medium.should_render());
        assert!(LodLevel::Low.should_render());
        assert!(!LodLevel::Culled.should_render());
    }
}
