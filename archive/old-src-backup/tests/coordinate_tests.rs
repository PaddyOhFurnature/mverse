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

// ============================================================================
// ECEF Euclidean distance tests
// ============================================================================

#[test]
fn test_ecef_distance_same_point() {
    let point = EcefPos {
        x: -5046951.809,
        y: 2568766.054,
        z: -2924501.502,
    };
    let distance = ecef_distance(&point, &point);
    assert_eq!(distance, 0.0, "Distance to same point should be exactly 0");
}

#[test]
fn test_ecef_distance_known_points() {
    // Two points with known ECEF coordinates
    let brisbane = gps_to_ecef(&GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    });
    let sydney = gps_to_ecef(&GpsPos {
        lat_deg: -33.8688,
        lon_deg: 151.2093,
        elevation_m: 0.0,
    });
    
    let distance = ecef_distance(&brisbane, &sydney);
    
    // ECEF distance should be positive and non-zero
    assert!(distance > 0.0, "Distance should be positive");
    
    // Verify it's calculated correctly using Pythagorean theorem
    let dx = sydney.x - brisbane.x;
    let dy = sydney.y - brisbane.y;
    let dz = sydney.z - brisbane.z;
    let expected = (dx * dx + dy * dy + dz * dz).sqrt();
    
    assert!((distance - expected).abs() < 0.001, 
        "Distance = {}, expected = {}", distance, expected);
}

#[test]
fn test_ecef_distance_shorter_than_haversine() {
    // ECEF (straight line through Earth) should be shorter than Haversine (surface)
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
    
    let ecef_brisbane = gps_to_ecef(&brisbane);
    let ecef_sydney = gps_to_ecef(&sydney);
    
    let straight_line = ecef_distance(&ecef_brisbane, &ecef_sydney);
    let surface = haversine_distance(&brisbane, &sydney);
    
    assert!(straight_line < surface,
        "Straight line ({:.0}m) should be shorter than surface ({:.0}m)",
        straight_line, surface);
}

// ============================================================================
// Batch parallel GPS to ECEF conversion tests
// ============================================================================

#[test]
fn test_gps_to_ecef_batch_empty() {
    let positions: Vec<GpsPos> = vec![];
    let result = gps_to_ecef_batch(&positions);
    assert_eq!(result.len(), 0, "Empty input should return empty output");
}

#[test]
fn test_gps_to_ecef_batch_matches_sequential() {
    // Generate test positions
    let positions = vec![
        GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 },
        GpsPos { lat_deg: -33.8688, lon_deg: 151.2093, elevation_m: 0.0 },
        GpsPos { lat_deg: 51.5074, lon_deg: -0.1278, elevation_m: 0.0 },
        GpsPos { lat_deg: 90.0, lon_deg: 0.0, elevation_m: 0.0 },
        GpsPos { lat_deg: -90.0, lon_deg: 0.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.0, lon_deg: 180.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 27.9881, lon_deg: 86.9250, elevation_m: 8848.0 },
    ];
    
    // Batch conversion
    let batch_result = gps_to_ecef_batch(&positions);
    
    // Sequential conversion
    let sequential_result: Vec<EcefPos> = positions.iter()
        .map(|pos| gps_to_ecef(pos))
        .collect();
    
    // Results must match exactly (bitwise identical)
    assert_eq!(batch_result.len(), sequential_result.len());
    for (i, (batch, seq)) in batch_result.iter().zip(sequential_result.iter()).enumerate() {
        assert_eq!(batch.x, seq.x, "Position {}: X mismatch", i);
        assert_eq!(batch.y, seq.y, "Position {}: Y mismatch", i);
        assert_eq!(batch.z, seq.z, "Position {}: Z mismatch", i);
    }
}

#[test]
#[ignore] // Run with --ignored for performance test
fn test_gps_to_ecef_batch_performance() {
    use std::time::Instant;
    
    // Generate 10 million random GPS positions
    let count = 10_000_000;
    let mut positions = Vec::with_capacity(count);
    
    // Use a simple deterministic "random" generator for reproducibility
    for i in 0..count {
        let lat = ((i as f64 * 0.123456) % 180.0) - 90.0;  // -90 to 90
        let lon = ((i as f64 * 0.789012) % 360.0) - 180.0; // -180 to 180
        let elev = (i as f64 * 0.345678) % 2000.0;       // 0 to 2000
        positions.push(GpsPos {
            lat_deg: lat,
            lon_deg: lon,
            elevation_m: elev,
        });
    }
    
    println!("Converting {} GPS positions to ECEF...", count);
    
    let start = Instant::now();
    let _result = gps_to_ecef_batch(&positions);
    let elapsed = start.elapsed();
    
    let throughput = count as f64 / elapsed.as_secs_f64();
    
    println!("Converted {} positions in {:.3}s", count, elapsed.as_secs_f64());
    println!("Throughput: {:.0} conversions/sec", throughput);
    
    // In release mode, must achieve >1M conversions/sec
    #[cfg(not(debug_assertions))]
    assert!(
        throughput > 1_000_000.0,
        "Throughput {:.0}/sec is below required 1M/sec (run with --release)",
        throughput
    );
    
    #[cfg(debug_assertions)]
    println!("Note: Debug mode - run with --release for accurate performance measurement");
}

// ============================================================================
// ENU (East-North-Up) local coordinate frame tests
// ============================================================================

#[test]
fn test_ecef_to_enu_origin_is_zero() {
    // Origin point in ENU frame should be (0, 0, 0)
    let origin_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    let enu = ecef_to_enu(&origin_ecef, &origin_ecef, &origin_gps);
    
    assert!(enu.east.abs() < 0.001, "East = {}, expected ≈ 0", enu.east);
    assert!(enu.north.abs() < 0.001, "North = {}, expected ≈ 0", enu.north);
    assert!(enu.up.abs() < 0.001, "Up = {}, expected ≈ 0", enu.up);
}

#[test]
fn test_ecef_to_enu_100m_east() {
    // Point ~100m east of origin
    let origin_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    // Calculate exact longitude offset for 100m at this latitude
    let lat_rad = origin_gps.lat_deg.to_radians();
    let lon_offset = 100.0 / (WGS84_A * lat_rad.cos()) * (180.0 / std::f64::consts::PI);
    
    let point_gps = GpsPos {
        lat_deg: origin_gps.lat_deg,
        lon_deg: origin_gps.lon_deg + lon_offset,
        elevation_m: 0.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    
    // Should be approximately 100m east, 0m north, 0m up
    assert!((enu.east - 100.0).abs() < 1.0, "East = {}, expected ≈ 100", enu.east);
    assert!(enu.north.abs() < 1.0, "North = {}, expected ≈ 0", enu.north);
    assert!(enu.up.abs() < 1.0, "Up = {}, expected ≈ 0", enu.up);
}

#[test]
fn test_ecef_to_enu_100m_north() {
    // Point 100m north of origin
    let origin_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    // ~100m north
    let point_gps = GpsPos {
        lat_deg: -27.4698 + 0.0009, // ~100m north
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    
    // Should be approximately 0m east, 100m north, 0m up
    assert!(enu.east.abs() < 1.0, "East = {}, expected ≈ 0", enu.east);
    assert!((enu.north - 100.0).abs() < 1.0, "North = {}, expected ≈ 100", enu.north);
    assert!(enu.up.abs() < 1.0, "Up = {}, expected ≈ 0", enu.up);
}

#[test]
fn test_ecef_to_enu_50m_up() {
    // Point 50m above origin
    let origin_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    let point_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 50.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    
    // Should be approximately 0m east, 0m north, 50m up
    assert!(enu.east.abs() < 1.0, "East = {}, expected ≈ 0", enu.east);
    assert!(enu.north.abs() < 1.0, "North = {}, expected ≈ 0", enu.north);
    assert!((enu.up - 50.0).abs() < 1.0, "Up = {}, expected ≈ 50", enu.up);
}

#[test]
fn test_enu_round_trip_brisbane() {
    let origin_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    // Point 200m east, 150m north, 25m up
    let point_gps = GpsPos {
        lat_deg: -27.4698 + 0.00135,
        lon_deg: 153.0251 + 0.002,
        elevation_m: 25.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    // ECEF → ENU → ECEF
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    let ecef_back = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
    
    assert!((ecef_back.x - point_ecef.x).abs() < 0.001, 
        "X: {} vs {}", ecef_back.x, point_ecef.x);
    assert!((ecef_back.y - point_ecef.y).abs() < 0.001,
        "Y: {} vs {}", ecef_back.y, point_ecef.y);
    assert!((ecef_back.z - point_ecef.z).abs() < 0.001,
        "Z: {} vs {}", ecef_back.z, point_ecef.z);
}

#[test]
fn test_enu_round_trip_north_pole() {
    let origin_gps = GpsPos {
        lat_deg: 90.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    // Point slightly offset from pole
    let point_gps = GpsPos {
        lat_deg: 89.999,
        lon_deg: 45.0,
        elevation_m: 10.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    // Round-trip test
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    let ecef_back = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
    
    assert!((ecef_back.x - point_ecef.x).abs() < 0.001,
        "X: {} vs {}", ecef_back.x, point_ecef.x);
    assert!((ecef_back.y - point_ecef.y).abs() < 0.001,
        "Y: {} vs {}", ecef_back.y, point_ecef.y);
    assert!((ecef_back.z - point_ecef.z).abs() < 0.001,
        "Z: {} vs {}", ecef_back.z, point_ecef.z);
}

#[test]
fn test_enu_round_trip_equator() {
    let origin_gps = GpsPos {
        lat_deg: 0.0,
        lon_deg: 0.0,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    let point_gps = GpsPos {
        lat_deg: 0.001,
        lon_deg: 0.001,
        elevation_m: 100.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    let ecef_back = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
    
    assert!((ecef_back.x - point_ecef.x).abs() < 0.001,
        "X: {} vs {}", ecef_back.x, point_ecef.x);
    assert!((ecef_back.y - point_ecef.y).abs() < 0.001,
        "Y: {} vs {}", ecef_back.y, point_ecef.y);
    assert!((ecef_back.z - point_ecef.z).abs() < 0.001,
        "Z: {} vs {}", ecef_back.z, point_ecef.z);
}

#[test]
fn test_enu_round_trip_antimeridian() {
    let origin_gps = GpsPos {
        lat_deg: 0.0,
        lon_deg: 180.0,
        elevation_m: 0.0,
    };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    let point_gps = GpsPos {
        lat_deg: 0.001,
        lon_deg: 179.999,
        elevation_m: 50.0,
    };
    let point_ecef = gps_to_ecef(&point_gps);
    
    let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
    let ecef_back = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
    
    assert!((ecef_back.x - point_ecef.x).abs() < 0.001,
        "X: {} vs {}", ecef_back.x, point_ecef.x);
    assert!((ecef_back.y - point_ecef.y).abs() < 0.001,
        "Y: {} vs {}", ecef_back.y, point_ecef.y);
    assert!((ecef_back.z - point_ecef.z).abs() < 0.001,
        "Z: {} vs {}", ecef_back.z, point_ecef.z);
}

// ============================================================================
// Phase 1 Scale Gate Tests - Comprehensive coordinate accuracy validation
// ============================================================================

#[test]
fn test_scale_gate_1m_separation() {
    // Two points 1m apart
    let origin = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let offset = GpsPos { lat_deg: -27.4698 + 0.000008983, lon_deg: 153.0251, elevation_m: 0.0 };
    
    let distance = haversine_distance(&origin, &offset);
    let expected = 1.0;
    let tolerance = 0.001; // 1mm
    
    assert!((distance - expected).abs() < tolerance,
        "1m scale: distance = {:.6}m, expected {:.6}m (±{:.6}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_10m_separation() {
    let origin = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let offset = GpsPos { lat_deg: -27.4698 + 0.000089831, lon_deg: 153.0251, elevation_m: 0.0 };
    
    let distance = haversine_distance(&origin, &offset);
    let expected = 10.0;
    let tolerance = 0.005; // 5mm
    
    assert!((distance - expected).abs() < tolerance,
        "10m scale: distance = {:.6}m, expected {:.6}m (±{:.6}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_100m_separation() {
    let origin = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let offset = GpsPos { lat_deg: -27.4698 + 0.000898311, lon_deg: 153.0251, elevation_m: 0.0 };
    
    let distance = haversine_distance(&origin, &offset);
    let expected = 100.0;
    let tolerance = 0.01; // 10mm
    
    assert!((distance - expected).abs() < tolerance,
        "100m scale: distance = {:.3}m, expected {:.3}m (±{:.3}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_1km_separation() {
    let origin = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let offset = GpsPos { lat_deg: -27.4698 + 0.008983112, lon_deg: 153.0251, elevation_m: 0.0 };
    
    let distance = haversine_distance(&origin, &offset);
    let expected = 1000.0;
    let tolerance = 0.1; // 100mm
    
    assert!((distance - expected).abs() < tolerance,
        "1km scale: distance = {:.3}m, expected {:.3}m (±{:.3}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_10km_separation() {
    // Brisbane CBD to ~10km north
    let cbd = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let offset = GpsPos { lat_deg: -27.4698 + 0.089831117, lon_deg: 153.0251, elevation_m: 0.0 };
    
    let distance = haversine_distance(&cbd, &offset);
    let expected = 10000.0;
    let tolerance = 1.0; // 1m
    
    assert!((distance - expected).abs() < tolerance,
        "10km scale: distance = {:.1}m, expected {:.1}m (±{:.1}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_100km_separation() {
    let origin = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let offset = GpsPos { lat_deg: -27.4698 + 0.898311175, lon_deg: 153.0251, elevation_m: 0.0 };
    
    let distance = haversine_distance(&origin, &offset);
    let expected = 100000.0;
    let tolerance = 10.0; // 10m
    
    assert!((distance - expected).abs() < tolerance,
        "100km scale: distance = {:.1}m, expected {:.1}m (±{:.1}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_1000km_separation() {
    // Brisbane to Sydney (~732km actual)
    let brisbane = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let sydney = GpsPos { lat_deg: -33.8688, lon_deg: 151.2093, elevation_m: 0.0 };
    
    let distance = haversine_distance(&brisbane, &sydney);
    let expected = 733199.0; // Known accurate value from earlier tests
    let tolerance = 100.0; // 100m
    
    assert!((distance - expected).abs() < tolerance,
        "1000km scale: distance = {:.0}m, expected {:.0}m (±{:.0}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_10000km_separation() {
    // Brisbane to London (~16,545km)
    let brisbane = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let london = GpsPos { lat_deg: 51.5074, lon_deg: -0.1278, elevation_m: 0.0 };
    
    let distance = haversine_distance(&brisbane, &london);
    let expected = 16544579.0; // Known accurate value
    let tolerance = 1000.0; // 1km
    
    assert!((distance - expected).abs() < tolerance,
        "10000km scale: distance = {:.0}m, expected {:.0}m (±{:.0}m)",
        distance, expected, tolerance);
}

#[test]
fn test_scale_gate_20000km_antipodal() {
    // Antipodal points: opposite sides of Earth
    let point1 = GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 };
    let point2 = GpsPos { lat_deg: 0.0, lon_deg: 180.0, elevation_m: 0.0 };
    
    let distance = haversine_distance(&point1, &point2);
    let expected = 20037508.0; // Half Earth's circumference
    let min = 19900000.0;
    let max = 20100000.0;
    
    assert!(distance > min && distance < max,
        "20000km scale (antipodal): distance = {:.0}m, expected range [{:.0}m, {:.0}m]",
        distance, min, max);
}

#[test]
fn test_scale_gate_round_trip_all_scales() {
    // Test GPS → ECEF → GPS round-trip at multiple scales
    let test_cases = vec![
        (-27.4698, 153.0251, 0.0, "Origin"),
        (-27.4698 + 0.000009, 153.0251, 0.0, "1m offset"),
        (-27.4698 + 0.00009, 153.0251, 0.0, "10m offset"),
        (-27.4698 + 0.0009, 153.0251, 0.0, "100m offset"),
        (-27.4698 + 0.009, 153.0251, 0.0, "1km offset"),
        (-27.4698 + 0.09, 153.0251, 0.0, "10km offset"),
        (-27.4698 + 0.9, 153.0251, 0.0, "100km offset"),
        (-33.8688, 151.2093, 0.0, "Sydney (732km)"),
        (51.5074, -0.1278, 0.0, "London (16545km)"),
        (0.0, 180.0, 0.0, "Antipodal"),
    ];
    
    for (lat, lon, elev, label) in test_cases {
        let original = GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: elev };
        let ecef = gps_to_ecef(&original);
        let result = ecef_to_gps(&ecef);
        
        let lat_error = (result.lat_deg - original.lat_deg).abs();
        let lon_error = (result.lon_deg - original.lon_deg).abs();
        let elev_error = (result.elevation_m - original.elevation_m).abs();
        
        assert!(lat_error < 0.0000001, "{}: lat error = {}°", label, lat_error);
        assert!(lon_error < 0.0000001, "{}: lon error = {}°", label, lon_error);
        assert!(elev_error < 0.001, "{}: elev error = {}m", label, elev_error);
    }
}

#[test]
fn test_scale_gate_enu_accuracy_within_50km() {
    // ENU should be accurate within ~50km of origin
    let origin_gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    let test_distances = vec![
        (100.0, "100m"),
        (1000.0, "1km"),
        (10000.0, "10km"),
        (50000.0, "50km"),
    ];
    
    for (distance_m, label) in test_distances {
        // Point north of origin at test distance
        let lat_offset = distance_m / 111320.0; // Approximate degrees per metre latitude
        let point_gps = GpsPos {
            lat_deg: origin_gps.lat_deg + lat_offset,
            lon_deg: origin_gps.lon_deg,
            elevation_m: 0.0,
        };
        let point_ecef = gps_to_ecef(&point_gps);
        
        // Convert to ENU and back
        let enu = ecef_to_enu(&point_ecef, &origin_ecef, &origin_gps);
        let ecef_back = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
        
        // Check round-trip accuracy
        let error = ecef_distance(&point_ecef, &ecef_back);
        
        // Tolerance increases with distance (tangent plane approximation breaks down)
        let tolerance = if distance_m <= 10000.0 {
            0.01 // 1cm for distances up to 10km
        } else {
            distance_m * 0.00001 // 0.001% of distance for larger scales
        };
        
        assert!(error < tolerance,
            "{}: ENU round-trip error = {:.6}m (tolerance {:.6}m)",
            label, error, tolerance);
    }
}

#[test]
fn test_scale_gate_enu_limitation_beyond_50km() {
    // Document that ENU accuracy degrades beyond ~50km
    // This is expected due to Earth's curvature - ENU is a tangent plane approximation
    let origin_gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let origin_ecef = gps_to_ecef(&origin_gps);
    
    // Test at 100km - error should be detectable but within reasonable bounds
    let far_point_gps = GpsPos {
        lat_deg: origin_gps.lat_deg + 0.9, // ~100km north
        lon_deg: origin_gps.lon_deg,
        elevation_m: 0.0,
    };
    let far_point_ecef = gps_to_ecef(&far_point_gps);
    
    let enu = ecef_to_enu(&far_point_ecef, &origin_ecef, &origin_gps);
    let ecef_back = enu_to_ecef(&enu, &origin_ecef, &origin_gps);
    
    let error = ecef_distance(&far_point_ecef, &ecef_back);
    
    // At 100km, error should be < 10m (tangent plane approximation holds reasonably)
    assert!(error < 10.0,
        "100km: ENU round-trip error = {:.3}m (should be < 10m)",
        error);
    
    println!("ENU limitation test: 100km distance has {:.3}m round-trip error", error);
    println!("Note: ENU is a local tangent plane - use ECEF for distances > 50km");
}
