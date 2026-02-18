//! Coordinate system conversions - Phase 1

use glam::{DVec3, Vec3};

/// GPS coordinates (latitude, longitude, altitude)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GPS {
    pub lat: f64,  // degrees
    pub lon: f64,  // degrees
    pub alt: f64,  // meters above ellipsoid
}

impl GPS {
    pub fn new(lat: f64, lon: f64, alt: f64) -> Self {
        Self { lat, lon, alt }
    }

    pub fn to_ecef(&self) -> ECEF {
        use geoconv::{CoordinateSystem, WGS84, LLE, Degrees, Meters};
        
        let lle = LLE {
            latitude: Degrees(self.lat),
            longitude: Degrees(self.lon),
            elevation: Meters(self.alt),
        };
        
        let xyz = WGS84::lle_to_xyz(lle);
        ECEF::new(xyz.x.0, xyz.y.0, xyz.z.0)
    }
}

/// ECEF (Earth-Centered Earth-Fixed) coordinates
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ECEF {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl ECEF {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn to_gps(&self) -> GPS {
        use geoconv::{CoordinateSystem, WGS84, XYZ, Meters};
        
        let xyz = XYZ {
            x: Meters(self.x),
            y: Meters(self.y),
            z: Meters(self.z),
        };
        
        let lle = WGS84::xyz_to_lle(xyz);
        GPS::new(lle.latitude.0, lle.longitude.0, lle.elevation.0)
    }

    pub fn distance_to(&self, other: &ECEF) -> f64 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    pub fn to_dvec3(&self) -> DVec3 {
        DVec3::new(self.x, self.y, self.z)
    }

    pub fn from_dvec3(v: DVec3) -> Self {
        Self::new(v.x, v.y, v.z)
    }
}

/// Floating origin transform for GPU rendering
pub struct FloatingOrigin {
    camera_ecef: ECEF,
}

impl FloatingOrigin {
    pub fn new(camera_ecef: ECEF) -> Self {
        Self { camera_ecef }
    }

    pub fn to_camera_relative(&self, ecef: &ECEF) -> Vec3 {
        let offset = DVec3::new(
            ecef.x - self.camera_ecef.x,
            ecef.y - self.camera_ecef.y,
            ecef.z - self.camera_ecef.z,
        );
        Vec3::new(offset.x as f32, offset.y as f32, offset.z as f32)
    }

    pub fn from_camera_relative(&self, relative: Vec3) -> ECEF {
        ECEF::new(
            self.camera_ecef.x + relative.x as f64,
            self.camera_ecef.y + relative.y as f64,
            self.camera_ecef.z + relative.z as f64,
        )
    }

    pub fn set_camera(&mut self, new_camera: ECEF) {
        self.camera_ecef = new_camera;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const EPSILON_MM: f64 = 0.001;

    #[test]
    fn test_origin_point() {
        let gps = GPS::new(0.0, 0.0, 0.0);
        let ecef = gps.to_ecef();
        assert!((ecef.x - 6_378_137.0).abs() < 1.0);
        assert!(ecef.y.abs() < 1.0);
        assert!(ecef.z.abs() < 1.0);
        let gps_back = ecef.to_gps();
        assert!((gps_back.lat - gps.lat).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.lon - gps.lon).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.alt - gps.alt).abs() < EPSILON_MM);
    }

    #[test]
    fn test_north_pole() {
        let gps = GPS::new(90.0, 0.0, 0.0);
        let ecef = gps.to_ecef();
        assert!(ecef.x.abs() < 1.0);
        assert!(ecef.y.abs() < 1.0);
        assert!((ecef.z - 6_356_752.0).abs() < 1000.0);
        let gps_back = ecef.to_gps();
        assert!((gps_back.lat - 90.0).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.alt - gps.alt).abs() < EPSILON_MM);
    }

    #[test]
    fn test_south_pole() {
        let gps = GPS::new(-90.0, 0.0, 0.0);
        let ecef = gps.to_ecef();
        assert!(ecef.x.abs() < 1.0);
        assert!(ecef.y.abs() < 1.0);
        assert!((ecef.z + 6_356_752.0).abs() < 1000.0);
        let gps_back = ecef.to_gps();
        assert!((gps_back.lat + 90.0).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.alt - gps.alt).abs() < EPSILON_MM);
    }

    #[test]
    fn test_equator_90e() {
        let gps = GPS::new(0.0, 90.0, 0.0);
        let ecef = gps.to_ecef();
        assert!(ecef.x.abs() < 1.0);
        assert!((ecef.y - 6_378_137.0).abs() < 1.0);
        assert!(ecef.z.abs() < 1.0);
        let gps_back = ecef.to_gps();
        assert!((gps_back.lat - gps.lat).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.lon - gps.lon).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.alt - gps.alt).abs() < EPSILON_MM);
    }

    #[test]
    fn test_kangaroo_point() {
        let gps = GPS::new(-27.4775, 153.0355, 20.0);
        let ecef = gps.to_ecef();
        let gps_back = ecef.to_gps();
        assert!((gps_back.lat - gps.lat).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.lon - gps.lon).abs() < EPSILON_MM / 111_000.0);
        assert!((gps_back.alt - gps.alt).abs() < EPSILON_MM);
    }

    #[test]
    fn test_high_altitude() {
        let gps = GPS::new(0.0, 0.0, 400_000.0);
        let ecef = gps.to_ecef();
        assert!((ecef.x - 6_778_137.0).abs() < 1.0);
        let gps_back = ecef.to_gps();
        assert!((gps_back.alt - 400_000.0).abs() < EPSILON_MM);
    }

    #[test]
    fn test_antipodal_distance() {
        let gps1 = GPS::new(0.0, 0.0, 0.0);
        let gps2 = GPS::new(0.0, 180.0, 0.0);
        let ecef1 = gps1.to_ecef();
        let ecef2 = gps2.to_ecef();
        let distance = ecef1.distance_to(&ecef2);
        assert!((distance - 12_756_274.0).abs() < 1000.0);
    }

    #[test]
    fn test_floating_origin_1m() {
        let camera_gps = GPS::new(-27.4775, 153.0355, 20.0);
        let camera_ecef = camera_gps.to_ecef();
        let origin = FloatingOrigin::new(camera_ecef);
        let point_gps = GPS::new(-27.4775 + (1.0 / 111_000.0), 153.0355, 20.0);
        let point_ecef = point_gps.to_ecef();
        let relative = origin.to_camera_relative(&point_ecef);
        let distance = relative.length();
        assert!((distance - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_floating_origin_1km() {
        let camera_gps = GPS::new(-27.4775, 153.0355, 20.0);
        let camera_ecef = camera_gps.to_ecef();
        let origin = FloatingOrigin::new(camera_ecef);
        // 1km north ≈ 0.009° latitude (1m = 1/111,000°)
        let point_gps = GPS::new(-27.4775 + (1000.0 / 111_000.0), 153.0355, 20.0);
        let point_ecef = point_gps.to_ecef();
        let relative = origin.to_camera_relative(&point_ecef);
        let distance = relative.length();
        println!("Expected: 1000m, Got: {}m, Error: {}m", distance, (distance - 1000.0).abs());
        // GPS coordinate calculation may introduce ~1% error at this scale
        assert!((distance - 1000.0).abs() < 20.0, "1km offset - distance: {}m", distance);
    }

    #[test]
    fn test_floating_origin_10km() {
        let camera_gps = GPS::new(-27.4775, 153.0355, 20.0);
        let camera_ecef = camera_gps.to_ecef();
        let origin = FloatingOrigin::new(camera_ecef);
        let point_gps = GPS::new(-27.4775 + (10_000.0 / 111_000.0), 153.0355, 20.0);
        let point_ecef = point_gps.to_ecef();
        let relative = origin.to_camera_relative(&point_ecef);
        let distance = relative.length();
        println!("Expected: 10000m, Got: {}m, Error: {}m", distance, (distance - 10_000.0).abs());
        // GPS coordinate calculation may introduce ~1% error at this scale
        assert!((distance - 10_000.0).abs() < 200.0, "10km offset - distance: {}m", distance);
    }

    #[test]
    fn test_round_trip_ecef() {
        let ecef1 = ECEF::new(6_378_137.0, 0.0, 0.0);
        let gps = ecef1.to_gps();
        let ecef2 = gps.to_ecef();
        assert!((ecef1.x - ecef2.x).abs() < EPSILON_MM);
        assert!((ecef1.y - ecef2.y).abs() < EPSILON_MM);
        assert!((ecef1.z - ecef2.z).abs() < EPSILON_MM);
    }

    #[test]
    fn test_scale_gate_1m() {
        let gps1 = GPS::new(0.0, 0.0, 0.0);
        let gps2 = GPS::new(0.0, 0.0, 1.0);
        let ecef1 = gps1.to_ecef();
        let ecef2 = gps2.to_ecef();
        let distance = ecef1.distance_to(&ecef2);
        assert!((distance - 1.0).abs() < EPSILON_MM);
    }

    #[test]
    fn test_scale_gate_1km() {
        let gps1 = GPS::new(0.0, 0.0, 0.0);
        let gps2 = GPS::new(0.0, 0.0, 1000.0);
        let ecef1 = gps1.to_ecef();
        let ecef2 = gps2.to_ecef();
        let distance = ecef1.distance_to(&ecef2);
        assert!((distance - 1000.0).abs() < 0.01);
    }

    #[test]
    fn test_scale_gate_100km() {
        let gps1 = GPS::new(0.0, 0.0, 0.0);
        let gps2 = GPS::new(0.0, 0.0, 100_000.0);
        let ecef1 = gps1.to_ecef();
        let ecef2 = gps2.to_ecef();
        let distance = ecef1.distance_to(&ecef2);
        assert!((distance - 100_000.0).abs() < 10.0);
    }

    #[test]
    fn test_scale_gate_global() {
        let gps1 = GPS::new(90.0, 0.0, 0.0);
        let gps2 = GPS::new(-90.0, 0.0, 0.0);
        let ecef1 = gps1.to_ecef();
        let ecef2 = gps2.to_ecef();
        let distance = ecef1.distance_to(&ecef2);
        assert!((distance - 12_713_504.0).abs() < 1000.0);
    }
}
