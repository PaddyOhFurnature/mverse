// Unit tests for coordinate system conversions

use crate::coordinates::*;

#[test]
fn test_wgs84_e2_constant() {
    // First eccentricity squared for WGS84 ellipsoid
    // Computed as: f × (2 - f) where f = 1/298.257223563
    // Reference: https://en.wikipedia.org/wiki/World_Geodetic_System
    let expected = 0.006_694_379_990_141_316_6;
    let diff = (WGS84_E2 - expected).abs();
    assert!(
        diff < 1e-15,
        "WGS84_E2 = {}, expected ≈ {}, diff = {}",
        WGS84_E2, expected, diff
    );
}

#[test]
fn test_wgs84_b_constant() {
    // Semi-minor axis for WGS84 ellipsoid (metres)
    // Reference: a × (1 - f) = 6378137 × (1 - 1/298.257223563) ≈ 6356752.314245
    let expected = 6_356_752.314_245;
    let diff = (WGS84_B - expected).abs();
    assert!(
        diff < 0.001,
        "WGS84_B = {}, expected ≈ {} (within 0.001m), diff = {}",
        WGS84_B, expected, diff
    );
}

#[test]
fn test_gps_pos_creation_and_traits() {
    // Test that GpsPos can be created, cloned, and debug-printed
    let pos = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    
    let cloned = pos.clone();
    assert_eq!(pos, cloned);
    
    let debug_str = format!("{:?}", pos);
    assert!(debug_str.contains("GpsPos"));
}

#[test]
fn test_ecef_pos_creation_and_traits() {
    // Test that EcefPos can be created, cloned, and debug-printed
    let pos = EcefPos {
        x: -5046125.0,
        y: 2568335.0,
        z: -2924861.0,
    };
    
    let cloned = pos.clone();
    assert_eq!(pos, cloned);
    
    let debug_str = format!("{:?}", pos);
    assert!(debug_str.contains("EcefPos"));
}

// ============================================================================
// GPS to ECEF conversion tests
// ============================================================================

#[test]
fn test_gps_to_ecef_equator_greenwich() {
    // Null Island: equator at prime meridian
    let gps = GpsPos {
        lat_deg: 0.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&gps);
    
    // At equator, all the radius is in X (prime meridian)
    assert!((ecef.x - WGS84_A).abs() < 1.0, "X = {}, expected ≈ {}", ecef.x, WGS84_A);
    assert!(ecef.y.abs() < 1.0, "Y = {}, expected ≈ 0", ecef.y);
    assert!(ecef.z.abs() < 1.0, "Z = {}, expected ≈ 0", ecef.z);
}

#[test]
fn test_gps_to_ecef_north_pole() {
    let gps = GpsPos {
        lat_deg: 90.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&gps);
    
    // At North Pole, all radius is in Z axis
    assert!(ecef.x.abs() < 1.0, "X = {}, expected ≈ 0", ecef.x);
    assert!(ecef.y.abs() < 1.0, "Y = {}, expected ≈ 0", ecef.y);
    assert!((ecef.z - WGS84_B).abs() < 1.0, "Z = {}, expected ≈ {}", ecef.z, WGS84_B);
}

#[test]
fn test_gps_to_ecef_south_pole() {
    let gps = GpsPos {
        lat_deg: -90.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&gps);
    
    // At South Pole, all radius is in negative Z axis
    assert!(ecef.x.abs() < 1.0, "X = {}, expected ≈ 0", ecef.x);
    assert!(ecef.y.abs() < 1.0, "Y = {}, expected ≈ 0", ecef.y);
    assert!((ecef.z + WGS84_B).abs() < 1.0, "Z = {}, expected ≈ {}", ecef.z, -WGS84_B);
}

#[test]
fn test_gps_to_ecef_brisbane_cbd() {
    // Queen Street Mall, Brisbane CBD
    let gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&gps);
    
    // Reference values calculated using WGS84 formulas
    let expected_x = -5046951.809;
    let expected_y = 2568766.054;
    let expected_z = -2924501.502;
    
    assert!((ecef.x - expected_x).abs() < 1.0, "X = {}, expected ≈ {}", ecef.x, expected_x);
    assert!((ecef.y - expected_y).abs() < 1.0, "Y = {}, expected ≈ {}", ecef.y, expected_y);
    assert!((ecef.z - expected_z).abs() < 1.0, "Z = {}, expected ≈ {}", ecef.z, expected_z);
}

#[test]
fn test_gps_to_ecef_mount_everest() {
    // Mount Everest summit
    let gps = GpsPos {
        lat_deg: 27.9881,
        lon_deg: 86.9250,
        elevation_m: 8848.0,
    };
    let ecef = gps_to_ecef(&gps);
    
    // Reference values calculated using WGS84 formulas
    let expected_x = 302769.894;
    let expected_y = 5636025.467;
    let expected_z = 2979493.087;
    
    assert!((ecef.x - expected_x).abs() < 1.0, "X = {}, expected ≈ {}", ecef.x, expected_x);
    assert!((ecef.y - expected_y).abs() < 1.0, "Y = {}, expected ≈ {}", ecef.y, expected_y);
    assert!((ecef.z - expected_z).abs() < 1.0, "Z = {}, expected ≈ {}", ecef.z, expected_z);
}

#[test]
fn test_gps_to_ecef_null_island() {
    // Null Island is same as equator/greenwich
    let gps = GpsPos {
        lat_deg: 0.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&gps);
    
    assert!((ecef.x - WGS84_A).abs() < 1.0, "X should be at equatorial radius");
    assert!(ecef.y.abs() < 1.0, "Y should be near zero");
    assert!(ecef.z.abs() < 1.0, "Z should be near zero");
}
