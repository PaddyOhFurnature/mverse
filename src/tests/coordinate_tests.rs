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
