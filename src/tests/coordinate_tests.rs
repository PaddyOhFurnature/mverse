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

// ============================================================================
// ECEF to GPS conversion tests
// ============================================================================

#[test]
fn test_ecef_to_gps_round_trip_equator() {
    let original = GpsPos {
        lat_deg: 0.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1, 
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
    assert!((result.lon_deg - original.lon_deg).abs() < 0.000_000_1,
        "Lon: {} vs {}", result.lon_deg, original.lon_deg);
    assert!((result.elevation_m - original.elevation_m).abs() < 0.001,
        "Elev: {} vs {}", result.elevation_m, original.elevation_m);
}

#[test]
fn test_ecef_to_gps_round_trip_north_pole() {
    let original = GpsPos {
        lat_deg: 90.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    // At pole, longitude is undefined, only check latitude and elevation
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1,
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
    assert!((result.elevation_m - original.elevation_m).abs() < 0.001,
        "Elev: {} vs {}", result.elevation_m, original.elevation_m);
}

#[test]
fn test_ecef_to_gps_round_trip_south_pole() {
    let original = GpsPos {
        lat_deg: -90.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1,
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
    assert!((result.elevation_m - original.elevation_m).abs() < 0.001,
        "Elev: {} vs {}", result.elevation_m, original.elevation_m);
}

#[test]
fn test_ecef_to_gps_round_trip_brisbane() {
    let original = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1,
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
    assert!((result.lon_deg - original.lon_deg).abs() < 0.000_000_1,
        "Lon: {} vs {}", result.lon_deg, original.lon_deg);
    assert!((result.elevation_m - original.elevation_m).abs() < 0.001,
        "Elev: {} vs {}", result.elevation_m, original.elevation_m);
}

#[test]
fn test_ecef_to_gps_round_trip_everest() {
    let original = GpsPos {
        lat_deg: 27.9881,
        lon_deg: 86.9250,
        elevation_m: 8848.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1,
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
    assert!((result.lon_deg - original.lon_deg).abs() < 0.000_000_1,
        "Lon: {} vs {}", result.lon_deg, original.lon_deg);
    assert!((result.elevation_m - original.elevation_m).abs() < 0.001,
        "Elev: {} vs {}", result.elevation_m, original.elevation_m);
}

#[test]
fn test_ecef_to_gps_round_trip_antimeridian() {
    // Test near the 180° longitude line
    let original = GpsPos {
        lat_deg: 0.0,
        lon_deg: 180.0,
        elevation_m: 0.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    // Longitude could be ±180°, both are equivalent
    let lon_diff = (result.lon_deg - original.lon_deg).abs();
    assert!(lon_diff < 0.000_000_1 || (lon_diff - 360.0).abs() < 0.000_000_1,
        "Lon: {} vs {}", result.lon_deg, original.lon_deg);
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1,
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
}

#[test]
fn test_ecef_to_gps_round_trip_negative_elevation() {
    // Dead Sea: below sea level
    let original = GpsPos {
        lat_deg: 31.5,
        lon_deg: 35.5,
        elevation_m: -430.0,
    };
    let ecef = gps_to_ecef(&original);
    let result = ecef_to_gps(&ecef);
    
    assert!((result.lat_deg - original.lat_deg).abs() < 0.000_000_1,
        "Lat: {} vs {}", result.lat_deg, original.lat_deg);
    assert!((result.lon_deg - original.lon_deg).abs() < 0.000_000_1,
        "Lon: {} vs {}", result.lon_deg, original.lon_deg);
    assert!((result.elevation_m - original.elevation_m).abs() < 0.001,
        "Elev: {} vs {}", result.elevation_m, original.elevation_m);
}

// ============================================================================
// Haversine great-circle distance tests
// ============================================================================

#[test]
fn test_haversine_same_point() {
    let point = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let distance = haversine_distance(&point, &point);
    assert_eq!(distance, 0.0, "Distance to same point should be exactly 0");
}

#[test]
fn test_haversine_brisbane_to_story_bridge() {
    // Queen Street Mall to Story Bridge (Brisbane CBD)
    let queen_st = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let story_bridge = GpsPos {
        lat_deg: -27.4634,
        lon_deg: 153.0394,
        elevation_m: 0.0,
    };
    
    let distance = haversine_distance(&queen_st, &story_bridge);
    let expected = 1582.0; // metres
    let tolerance = 10.0;
    
    assert!(
        (distance - expected).abs() < tolerance,
        "Distance = {:.1}m, expected ≈ {}m (±{}m)",
        distance, expected, tolerance
    );
}

#[test]
fn test_haversine_brisbane_to_sydney() {
    let brisbane = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let sydney = GpsPos {
        lat_deg: -33.8688,
        lon_deg: 151.2093,
        elevation_m: 0.0,
    };
    
    let distance = haversine_distance(&brisbane, &sydney);
    let expected = 732_000.0; // metres
    let tolerance = 5_000.0;
    
    assert!(
        (distance - expected).abs() < tolerance,
        "Distance = {:.0}m, expected ≈ {}m (±{}m)",
        distance, expected, tolerance
    );
}

#[test]
fn test_haversine_brisbane_to_london() {
    let brisbane = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let london = GpsPos {
        lat_deg: 51.5074,
        lon_deg: -0.1278,
        elevation_m: 0.0,
    };
    
    let distance = haversine_distance(&brisbane, &london);
    let expected = 16_500_000.0; // metres
    let tolerance = 100_000.0;
    
    assert!(
        (distance - expected).abs() < tolerance,
        "Distance = {:.0}m, expected ≈ {}m (±{}m)",
        distance, expected, tolerance
    );
}

#[test]
fn test_haversine_antipodal() {
    // Antipodal points: opposite sides of Earth
    let point1 = GpsPos {
        lat_deg: 0.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let point2 = GpsPos {
        lat_deg: 0.0,
        lon_deg: 180.0,
        elevation_m: 0.0,
    };
    
    let distance = haversine_distance(&point1, &point2);
    let expected = 20_015_000.0; // Half Earth's circumference
    let tolerance = 50_000.0;
    
    assert!(
        (distance - expected).abs() < tolerance,
        "Distance = {:.0}m, expected ≈ {}m (±{}m)",
        distance, expected, tolerance
    );
}

#[test]
fn test_haversine_short_distance() {
    // Two points approximately 1m apart
    let point1 = GpsPos {
        lat_deg: 0.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let point2 = GpsPos {
        lat_deg: 0.000009, // ~1 metre north at equator
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    
    let distance = haversine_distance(&point1, &point2);
    let expected = 1.0; // metres
    let tolerance = 0.1;
    
    assert!(
        (distance - expected).abs() < tolerance,
        "Distance = {:.3}m, expected ≈ {}m (±{}m)",
        distance, expected, tolerance
    );
}
