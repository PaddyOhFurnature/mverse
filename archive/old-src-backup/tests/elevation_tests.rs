use crate::elevation::{parse_hgt, parse_hgt_filename, get_elevation, SrtmResolution};

#[test]
fn test_parse_small_synthetic_hgt() {
    // Create a 3×3 synthetic tile (normally SRTM has 1201 or 3601, but we test with small size)
    // We'll manually adjust the test to use proper SRTM3 size
    
    // SRTM3: 1201 × 1201 samples = 1,442,401 samples × 2 bytes = 2,884,802 bytes
    let sample_count = 1201 * 1201;
    let mut bytes = Vec::with_capacity(sample_count * 2);
    
    // Fill with test data: elevation = row * 1000 + col
    for row in 0..1201 {
        for col in 0..1201 {
            let elevation = if row == 0 && col == 0 {
                100i16 // NW corner
            } else if row == 0 && col == 1200 {
                200i16 // NE corner
            } else if row == 1200 && col == 0 {
                300i16 // SW corner
            } else if row == 1200 && col == 1200 {
                400i16 // SE corner
            } else {
                50i16 // Fill
            };
            
            // Big-endian encoding
            bytes.push((elevation >> 8) as u8);
            bytes.push((elevation & 0xFF) as u8);
        }
    }
    
    let tile = parse_hgt("N37W122.hgt", &bytes).unwrap();
    
    assert_eq!(tile.sw_lat, 37);
    assert_eq!(tile.sw_lon, -122);
    assert_eq!(tile.resolution, SrtmResolution::Srtm3);
    assert_eq!(tile.elevations.len(), sample_count);
    
    // Check corners (grid origin is NW, so first sample is NW corner)
    assert_eq!(tile.elevations[0], 100); // NW corner
    assert_eq!(tile.elevations[1200], 200); // NE corner
    assert_eq!(tile.elevations[1200 * 1201], 300); // SW corner
    assert_eq!(tile.elevations[1201 * 1201 - 1], 400); // SE corner
}

#[test]
fn test_filename_parsing_north_west() {
    let (lat, lon) = parse_hgt_filename("N37W122.hgt").unwrap();
    assert_eq!(lat, 37);
    assert_eq!(lon, -122);
}

#[test]
fn test_filename_parsing_south_east() {
    let (lat, lon) = parse_hgt_filename("S27E153.hgt").unwrap();
    assert_eq!(lat, -27);
    assert_eq!(lon, 153);
}

#[test]
fn test_filename_parsing_without_extension() {
    let (lat, lon) = parse_hgt_filename("N00E000").unwrap();
    assert_eq!(lat, 0);
    assert_eq!(lon, 0);
}

#[test]
fn test_invalid_file_size_returns_error() {
    let bytes = vec![0u8; 1000]; // Invalid size
    let result = parse_hgt("N37W122.hgt", &bytes);
    assert!(result.is_err());
}

#[test]
fn test_void_values_preserved() {
    // SRTM3 size with void value (-32768)
    let sample_count = 1201 * 1201;
    let mut bytes = Vec::with_capacity(sample_count * 2);
    
    for i in 0..sample_count {
        let elevation = if i == 100 {
            -32768i16 // Void value
        } else {
            50i16
        };
        
        bytes.push((elevation >> 8) as u8);
        bytes.push((elevation & 0xFF) as u8);
    }
    
    let tile = parse_hgt("N00E000.hgt", &bytes).unwrap();
    
    assert_eq!(tile.elevations[100], -32768); // Void preserved
    assert_eq!(tile.elevations[0], 50); // Normal value
}

#[test]
fn test_srtm1_resolution_detection() {
    // SRTM1: 3601 × 3601 samples
    let sample_count = 3601 * 3601;
    let bytes = vec![0u8; sample_count * 2];
    
    let tile = parse_hgt("N37W122.hgt", &bytes).unwrap();
    
    assert_eq!(tile.resolution, SrtmResolution::Srtm1);
    assert_eq!(tile.elevations.len(), sample_count);
}

// Phase 4.6 tests - Elevation query

#[test]
fn test_query_exact_grid_point() {
    // Create SRTM3 with uniform elevation of 150m
    let sample_count = 1201 * 1201;
    let mut bytes = Vec::with_capacity(sample_count * 2);
    
    for _ in 0..sample_count {
        let elevation = 150i16;
        bytes.push((elevation >> 8) as u8);
        bytes.push((elevation & 0xFF) as u8);
    }
    
    let tile = parse_hgt("N37W122.hgt", &bytes).unwrap();
    
    // Query anywhere in the tile - should return 150m
    let elev = get_elevation(&tile, 37.5, -121.5).unwrap();
    assert!((elev - 150.0).abs() < 0.1);
}

#[test]
fn test_query_interpolated_value() {
    // Create SRTM3 tile with known corner values
    let sample_count = 1201 * 1201;
    let mut bytes = Vec::with_capacity(sample_count * 2);
    
    // Simple pattern: all 100 except specific test points
    for row in 0..1201 {
        for col in 0..1201 {
            let elevation = if row == 0 && col == 0 {
                100i16 // NW
            } else if row == 0 && col == 1 {
                200i16 // Point east of NW
            } else if row == 1 && col == 0 {
                300i16 // Point south of NW
            } else if row == 1 && col == 1 {
                400i16 // Diagonal from NW
            } else {
                100i16
            };
            
            bytes.push((elevation >> 8) as u8);
            bytes.push((elevation & 0xFF) as u8);
        }
    }
    
    let tile = parse_hgt("N00E000.hgt", &bytes).unwrap();
    
    // Query a point between 4 samples
    // Samples: (0,0)=100, (0,1)=200, (1,0)=300, (1,1)=400
    // Grid spacing = 1.0 / 1200 degrees
    let spacing = 1.0 / 1200.0;
    let lat = 1.0 - spacing * 0.5; // Halfway between row 0 and row 1
    let lon = 0.0 + spacing * 0.5; // Halfway between col 0 and col 1
    
    let elev = get_elevation(&tile, lat, lon).unwrap();
    
    // Bilinear interpolation at center should average all 4 corners
    let expected = (100.0 + 200.0 + 300.0 + 400.0) / 4.0;
    assert!((elev - expected).abs() < 1.0, "Expected ~{}, got {}", expected, elev);
}

#[test]
fn test_query_outside_tile_returns_none() {
    let sample_count = 1201 * 1201;
    let bytes = vec![0u8; sample_count * 2];
    let tile = parse_hgt("N37W122.hgt", &bytes).unwrap();
    
    // Outside tile bounds
    assert!(get_elevation(&tile, 36.5, -122.0).is_none()); // Too far south
    assert!(get_elevation(&tile, 38.5, -122.0).is_none()); // Too far north
    assert!(get_elevation(&tile, 37.5, -123.0).is_none()); // Too far west
    assert!(get_elevation(&tile, 37.5, -121.0).is_none()); // Too far east
}

#[test]
fn test_query_with_void_sample_returns_none() {
    let sample_count = 1201 * 1201;
    let mut bytes = Vec::with_capacity(sample_count * 2);
    
    for row in 0..1201 {
        for col in 0..1201 {
            let elevation = if row == 0 && col == 0 {
                -32768i16 // Void at NW corner
            } else {
                100i16
            };
            
            bytes.push((elevation >> 8) as u8);
            bytes.push((elevation & 0xFF) as u8);
        }
    }
    
    let tile = parse_hgt("N00E000.hgt", &bytes).unwrap();
    
    // Query near the void sample - should return None
    let spacing = 1.0 / 1200.0;
    let lat = 1.0 - spacing * 0.5; // Near row 0
    let lon = 0.0 + spacing * 0.5; // Near col 0
    
    assert!(get_elevation(&tile, lat, lon).is_none());
}
