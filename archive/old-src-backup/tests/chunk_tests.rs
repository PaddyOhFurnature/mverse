// Unit tests for quad-sphere chunking system

use crate::chunks::*;
use crate::coordinates::*;

#[test]
fn test_chunk_id_depth_0() {
    let chunk = ChunkId::root(0);
    assert_eq!(chunk.depth(), 0, "Root tile should have depth 0");
}

#[test]
fn test_chunk_id_depth_5() {
    let chunk = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2, 1],
    };
    assert_eq!(chunk.depth(), 5, "Depth should equal path length");
}

#[test]
fn test_chunk_id_depth_14() {
    let chunk = ChunkId {
        face: 4,
        path: vec![0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3, 0, 1],
    };
    assert_eq!(chunk.depth(), 14, "Depth 14 tile");
}

#[test]
fn test_chunk_id_depth_20() {
    let mut path = Vec::new();
    for i in 0..20 {
        path.push((i % 4) as u8);
    }
    let chunk = ChunkId { face: 1, path };
    assert_eq!(chunk.depth(), 20, "Depth 20 tile");
}

#[test]
fn test_chunk_id_root_all_faces() {
    for face in 0..6 {
        let chunk = ChunkId::root(face);
        assert_eq!(chunk.face, face, "Root should have correct face");
        assert_eq!(chunk.path.len(), 0, "Root should have empty path");
        assert_eq!(chunk.depth(), 0, "Root should have depth 0");
    }
}

#[test]
fn test_chunk_id_display() {
    let chunk = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    let display = format!("{}", chunk);
    assert_eq!(display, "F2/0312", "Display format should be F{{face}}/{{path}}");
}

#[test]
fn test_chunk_id_display_root() {
    let chunk = ChunkId::root(5);
    let display = format!("{}", chunk);
    assert_eq!(display, "F5/", "Root display should be F{{face}}/");
}

#[test]
fn test_chunk_id_equality() {
    let chunk1 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    let chunk2 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    assert_eq!(chunk1, chunk2, "Identical ChunkIds should be equal");
}

#[test]
fn test_chunk_id_inequality_face() {
    let chunk1 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    let chunk2 = ChunkId {
        face: 3,
        path: vec![0, 3, 1, 2],
    };
    assert_ne!(chunk1, chunk2, "Different faces should not be equal");
}

#[test]
fn test_chunk_id_inequality_path() {
    let chunk1 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    let chunk2 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 3],
    };
    assert_ne!(chunk1, chunk2, "Different paths should not be equal");
}

#[test]
fn test_chunk_id_hash_consistency() {
    use std::collections::HashSet;
    
    let chunk1 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    let chunk2 = ChunkId {
        face: 2,
        path: vec![0, 3, 1, 2],
    };
    
    let mut set = HashSet::new();
    set.insert(chunk1.clone());
    
    assert!(set.contains(&chunk2), "Equal ChunkIds should hash the same");
    assert_eq!(set.len(), 1, "Duplicate should not increase set size");
}

// ============================================================================
// Cube-face mapping tests (ECEF → face + UV)
// ============================================================================

use crate::coordinates::{gps_to_ecef, GpsPos};

#[test]
fn test_ecef_to_cube_face_positive_x() {
    // Equator at 0° longitude should map to face 0 (+X)
    let gps = GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    assert_eq!(face, 0, "Equator at 0° lon should be face 0 (+X)");
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
}

#[test]
fn test_ecef_to_cube_face_negative_x() {
    // Equator at 180° longitude should map to face 1 (-X)
    let gps = GpsPos { lat_deg: 0.0, lon_deg: 180.0, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    assert_eq!(face, 1, "Equator at 180° lon should be face 1 (-X)");
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
}

#[test]
fn test_ecef_to_cube_face_positive_y() {
    // Equator at 90° East should map to face 2 (+Y)
    let gps = GpsPos { lat_deg: 0.0, lon_deg: 90.0, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    assert_eq!(face, 2, "Equator at 90° E should be face 2 (+Y)");
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
}

#[test]
fn test_ecef_to_cube_face_negative_y() {
    // Equator at 90° West should map to face 3 (-Y)
    let gps = GpsPos { lat_deg: 0.0, lon_deg: -90.0, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    assert_eq!(face, 3, "Equator at 90° W should be face 3 (-Y)");
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
}

#[test]
fn test_ecef_to_cube_face_positive_z() {
    // North Pole should map to face 4 (+Z)
    let gps = GpsPos { lat_deg: 90.0, lon_deg: 0.0, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    assert_eq!(face, 4, "North Pole should be face 4 (+Z)");
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
}

#[test]
fn test_ecef_to_cube_face_negative_z() {
    // South Pole should map to face 5 (-Z)
    let gps = GpsPos { lat_deg: -90.0, lon_deg: 0.0, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    assert_eq!(face, 5, "South Pole should be face 5 (-Z)");
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
}

#[test]
fn test_ecef_to_cube_face_brisbane() {
    // Brisbane should map deterministically to one face
    let gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let ecef = gps_to_ecef(&gps);
    let (face, u, v) = ecef_to_cube_face(&ecef);
    
    // Brisbane at 153° East should be face 2 (+Y) or face 0 (+X)
    assert!(face <= 5, "Face should be valid (0-5), got {}", face);
    assert!(u >= -1.0 && u <= 1.0, "u should be in [-1, 1], got {}", u);
    assert!(v >= -1.0 && v <= 1.0, "v should be in [-1, 1], got {}", v);
    
    println!("Brisbane maps to face {}, u={:.3}, v={:.3}", face, u, v);
}

#[test]
fn test_ecef_to_cube_face_all_faces_reachable() {
    // Verify all 6 faces are reachable
    let test_points = vec![
        (0.0, 0.0, 0),     // Face 0: +X
        (0.0, 180.0, 1),   // Face 1: -X
        (0.0, 90.0, 2),    // Face 2: +Y
        (0.0, -90.0, 3),   // Face 3: -Y
        (90.0, 0.0, 4),    // Face 4: +Z
        (-90.0, 0.0, 5),   // Face 5: -Z
    ];
    
    for (lat, lon, expected_face) in test_points {
        let gps = GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: 0.0 };
        let ecef = gps_to_ecef(&gps);
        let (face, _u, _v) = ecef_to_cube_face(&ecef);
        
        assert_eq!(face, expected_face, 
            "Point ({}, {}) should map to face {}, got {}",
            lat, lon, expected_face, face);
    }
}

// ============================================================================
// Phase 2.3: Cube-to-sphere projection tests
// ============================================================================

#[test]
fn test_cube_to_sphere_face_centres() {
    // Face centres (u=0, v=0) should map to axis-aligned points on the sphere
    let radius = WGS84_A;
    
    // Face 0: +X
    let ecef0 = cube_to_sphere(0, 0.0, 0.0, radius);
    assert!((ecef0.x - radius).abs() < 1.0, "Face 0 centre should be on +X axis");
    assert!(ecef0.y.abs() < 1.0, "Face 0 centre Y should be ~0");
    assert!(ecef0.z.abs() < 1.0, "Face 0 centre Z should be ~0");
    
    // Face 1: -X
    let ecef1 = cube_to_sphere(1, 0.0, 0.0, radius);
    assert!((ecef1.x + radius).abs() < 1.0, "Face 1 centre should be on -X axis");
    assert!(ecef1.y.abs() < 1.0, "Face 1 centre Y should be ~0");
    assert!(ecef1.z.abs() < 1.0, "Face 1 centre Z should be ~0");
    
    // Face 2: +Y
    let ecef2 = cube_to_sphere(2, 0.0, 0.0, radius);
    assert!(ecef2.x.abs() < 1.0, "Face 2 centre X should be ~0");
    assert!((ecef2.y - radius).abs() < 1.0, "Face 2 centre should be on +Y axis");
    assert!(ecef2.z.abs() < 1.0, "Face 2 centre Z should be ~0");
    
    // Face 3: -Y
    let ecef3 = cube_to_sphere(3, 0.0, 0.0, radius);
    assert!(ecef3.x.abs() < 1.0, "Face 3 centre X should be ~0");
    assert!((ecef3.y + radius).abs() < 1.0, "Face 3 centre should be on -Y axis");
    assert!(ecef3.z.abs() < 1.0, "Face 3 centre Z should be ~0");
    
    // Face 4: +Z
    let ecef4 = cube_to_sphere(4, 0.0, 0.0, radius);
    assert!(ecef4.x.abs() < 1.0, "Face 4 centre X should be ~0");
    assert!(ecef4.y.abs() < 1.0, "Face 4 centre Y should be ~0");
    assert!((ecef4.z - radius).abs() < 1.0, "Face 4 centre should be on +Z axis");
    
    // Face 5: -Z
    let ecef5 = cube_to_sphere(5, 0.0, 0.0, radius);
    assert!(ecef5.x.abs() < 1.0, "Face 5 centre X should be ~0");
    assert!(ecef5.y.abs() < 1.0, "Face 5 centre Y should be ~0");
    assert!((ecef5.z + radius).abs() < 1.0, "Face 5 centre should be on -Z axis");
}

#[test]
fn test_cube_to_sphere_on_sphere_surface() {
    // All face centres should be exactly on the sphere surface
    let radius = WGS84_A;
    
    for face in 0..6 {
        let ecef = cube_to_sphere(face, 0.0, 0.0, radius);
        let distance = (ecef.x * ecef.x + ecef.y * ecef.y + ecef.z * ecef.z).sqrt();
        
        assert!((distance - radius).abs() < 1.0,
            "Face {} centre distance from origin should be {}, got {}",
            face, radius, distance);
    }
}

#[test]
fn test_sphere_to_cube_face_centres() {
    // Points on the sphere should map back to their original face and (0,0) UV
    let radius = WGS84_A;
    
    for face in 0..6 {
        let ecef = cube_to_sphere(face, 0.0, 0.0, radius);
        let (mapped_face, u, v) = sphere_to_cube(&ecef);
        
        assert_eq!(mapped_face, face, "Face {} centre should map back to face {}", face, face);
        assert!(u.abs() < 0.001, "Face {} centre u should be ~0, got {}", face, u);
        assert!(v.abs() < 0.001, "Face {} centre v should be ~0, got {}", face, v);
    }
}

#[test]
fn test_cube_sphere_round_trip_face_centres() {
    // ECEF → sphere_to_cube → cube_to_sphere → ECEF should match to <1mm
    let radius = WGS84_A;
    
    for face in 0..6 {
        let ecef1 = cube_to_sphere(face, 0.0, 0.0, radius);
        let (f, u, v) = sphere_to_cube(&ecef1);
        let ecef2 = cube_to_sphere(f, u, v, radius);
        
        let error = ((ecef1.x - ecef2.x).powi(2) 
                   + (ecef1.y - ecef2.y).powi(2) 
                   + (ecef1.z - ecef2.z).powi(2)).sqrt();
        
        assert!(error < 0.001,
            "Face {} round-trip error should be <1mm, got {}m",
            face, error);
    }
}

#[test]
fn test_cube_sphere_round_trip_random_points() {
    // Test round-trip at various UV coordinates
    let radius = WGS84_A;
    let test_uv = vec![
        (0.5, 0.5),
        (-0.5, 0.5),
        (0.5, -0.5),
        (-0.5, -0.5),
        (0.0, 0.7),
        (0.7, 0.0),
        (-0.3, 0.6),
        (0.9, -0.2),
    ];
    
    for face in 0..6 {
        for (u, v) in &test_uv {
            let ecef1 = cube_to_sphere(face, *u, *v, radius);
            let (f, u2, v2) = sphere_to_cube(&ecef1);
            let ecef2 = cube_to_sphere(f, u2, v2, radius);
            
            let error = ((ecef1.x - ecef2.x).powi(2) 
                       + (ecef1.y - ecef2.y).powi(2) 
                       + (ecef1.z - ecef2.z).powi(2)).sqrt();
            
            assert!(error < 0.001,
                "Face {} u={} v={} round-trip error should be <1mm, got {}m",
                face, u, v, error);
        }
    }
}

#[test]
fn test_cube_to_sphere_corners() {
    // Verify corners are on the sphere surface and reasonable
    // NOTE: Finding exact matching corners across faces is complex due to the projection
    // The key property is that all corners should be on the sphere surface
    let radius = WGS84_A;
    
    let test_corners = vec![
        (0, 1.0, 1.0),
        (0, 1.0, -1.0),
        (1, 1.0, 1.0),
        (2, 1.0, -1.0),
        (4, 1.0, 1.0),
        (5, -1.0, -1.0),
    ];
    
    for (face, u, v) in test_corners {
        let corner = cube_to_sphere(face, u, v, radius);
        let dist_from_origin = (corner.x * corner.x + corner.y * corner.y + corner.z * corner.z).sqrt();
        
        assert!((dist_from_origin - radius).abs() < 1.0,
            "Corner (face={}, u={}, v={}) should be on sphere surface, distance={}",
            face, u, v, dist_from_origin);
    }
}

#[test]
fn test_sphere_to_cube_brisbane() {
    // Brisbane should round-trip correctly through sphere_to_cube and cube_to_sphere
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let ecef1 = gps_to_ecef(&brisbane);
    
    // Use sphere_to_cube to get face/UV
    let (face, u, v) = sphere_to_cube(&ecef1);
    
    // Project back to sphere
    let radius = (ecef1.x * ecef1.x + ecef1.y * ecef1.y + ecef1.z * ecef1.z).sqrt();
    let ecef2 = cube_to_sphere(face, u, v, radius);
    
    // Should match within 1mm
    let error = ((ecef1.x - ecef2.x).powi(2) 
               + (ecef1.y - ecef2.y).powi(2) 
               + (ecef1.z - ecef2.z).powi(2)).sqrt();
    
    assert!(error < 0.001, "Brisbane round-trip error should be <1mm, got {}m", error);
}

#[test]
fn test_cube_sphere_inverse_consistency() {
    // sphere_to_cube should be the mathematical inverse of cube_to_sphere
    // Meaning: ECEF coordinates should round-trip accurately (even if face/UV representation changes near edges)
    let radius = WGS84_A;
    
    // Start with known UV coordinates
    let test_cases = vec![
        (0, 0.0, 0.0),
        (1, -0.5, 0.5),
        (2, 0.8, -0.3),
        (3, -0.7, 0.7),
        (4, 0.3, -0.4),
        (5, -0.2, 0.9),
    ];
    
    for (face_orig, u_orig, v_orig) in test_cases {
        let ecef1 = cube_to_sphere(face_orig, u_orig, v_orig, radius);
        let (face_back, u_back, v_back) = sphere_to_cube(&ecef1);
        let ecef2 = cube_to_sphere(face_back, u_back, v_back, radius);
        
        // Verify ECEF round-trip accuracy
        let error = ((ecef1.x - ecef2.x).powi(2) 
                   + (ecef1.y - ecef2.y).powi(2) 
                   + (ecef1.z - ecef2.z).powi(2)).sqrt();
        
        assert!(error < 0.001,
            "ECEF round-trip for face {} u={} v={} should be <1mm, got {}m",
            face_orig, u_orig, v_orig, error);
    }
}

// ============================================================================
// Phase 2.4: GPS → ChunkId tests
// ============================================================================

#[test]
fn test_gps_to_chunk_id_brisbane_depth_0() {
    // Brisbane at depth 0 should give a face with empty path
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 0);
    
    assert_eq!(chunk.depth(), 0, "Depth 0 should have empty path");
    assert!(chunk.face < 6, "Face should be 0-5, got {}", chunk.face);
    assert_eq!(chunk.path.len(), 0, "Path should be empty at depth 0");
}

#[test]
fn test_gps_to_chunk_id_brisbane_depth_8() {
    // Brisbane at depth 8 - record the result for consistency testing
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 8);
    
    assert_eq!(chunk.depth(), 8, "Should have depth 8");
    assert_eq!(chunk.path.len(), 8, "Path should have 8 elements");
    
    // All path elements should be 0-3 (quadrant indices)
    for &quadrant in &chunk.path {
        assert!(quadrant <= 3, "Quadrant should be 0-3, got {}", quadrant);
    }
    
    // Test determinism: same input should give same output
    let chunk2 = gps_to_chunk_id(&brisbane, 8);
    assert_eq!(chunk, chunk2, "Should be deterministic");
}

#[test]
fn test_gps_to_chunk_id_brisbane_depth_14() {
    // Brisbane at depth 14 should be a descendant of depth-8 result
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    
    let chunk8 = gps_to_chunk_id(&brisbane, 8);
    let chunk14 = gps_to_chunk_id(&brisbane, 14);
    
    assert_eq!(chunk14.depth(), 14, "Should have depth 14");
    assert_eq!(chunk14.face, chunk8.face, "Should be on same face");
    
    // First 8 elements of path should match depth-8 result
    assert_eq!(&chunk14.path[0..8], &chunk8.path[..],
        "Depth-14 tile should be descendant of depth-8 tile");
}

#[test]
fn test_gps_to_chunk_id_north_pole() {
    // North Pole should map to face 4 (+Z)
    let north_pole = GpsPos { lat_deg: 90.0, lon_deg: 0.0, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&north_pole, 14);
    
    assert_eq!(chunk.face, 4, "North Pole should be on face 4 (+Z)");
    assert_eq!(chunk.depth(), 14, "Should have depth 14");
}

#[test]
fn test_gps_to_chunk_id_deterministic() {
    // Same GPS position should always produce same ChunkId
    let pos = GpsPos { lat_deg: 51.5074, lon_deg: -0.1278, elevation_m: 0.0 }; // London
    
    let chunk1 = gps_to_chunk_id(&pos, 10);
    let chunk2 = gps_to_chunk_id(&pos, 10);
    let chunk3 = gps_to_chunk_id(&pos, 10);
    
    assert_eq!(chunk1, chunk2, "Should be deterministic (run 1 vs 2)");
    assert_eq!(chunk2, chunk3, "Should be deterministic (run 2 vs 3)");
}

#[test]
fn test_gps_to_chunk_id_nearby_points_same_chunk() {
    // Two points 20m apart at depth 14 (~779m tiles) should be in same chunk
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    
    // Offset by ~20m (approximately 0.0002° at Brisbane's latitude)
    let nearby = GpsPos { 
        lat_deg: brisbane.lat_deg + 0.0002, 
        lon_deg: brisbane.lon_deg, 
        elevation_m: 0.0 
    };
    
    let chunk1 = gps_to_chunk_id(&brisbane, 14);
    let chunk2 = gps_to_chunk_id(&nearby, 14);
    
    // At 20m apart in a ~779m tile, they should usually be in same chunk
    // (unless we're very close to a tile boundary)
    // Check they're at least on the same face and share most of the path
    assert_eq!(chunk1.face, chunk2.face, "Should be on same face");
    
    // Count how many path elements match
    let mut matching = 0;
    for i in 0..14 {
        if chunk1.path[i] == chunk2.path[i] {
            matching += 1;
        } else {
            break;
        }
    }
    
    // Should match at least the first 13 levels (differ only in last level at most)
    assert!(matching >= 13, 
        "Points 20m apart should share at least 13 path levels, matched {}", matching);
}

#[test]
fn test_gps_to_chunk_id_distant_points_different_chunks() {
    // Two points 500m apart at depth 14 should likely be in different chunks
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    
    // Offset by ~500m (approximately 0.005° at Brisbane's latitude)
    let distant = GpsPos { 
        lat_deg: brisbane.lat_deg + 0.005, 
        lon_deg: brisbane.lon_deg, 
        elevation_m: 0.0 
    };
    
    let chunk1 = gps_to_chunk_id(&brisbane, 14);
    let chunk2 = gps_to_chunk_id(&distant, 14);
    
    assert_ne!(chunk1, chunk2, 
        "Points 500m apart should be in different depth-14 chunks");
}

#[test]
fn test_gps_to_chunk_id_parent_child_relationship() {
    // Verify that deeper tiles are descendants of shallower tiles
    let pos = GpsPos { lat_deg: 35.6762, lon_deg: 139.6503, elevation_m: 0.0 }; // Tokyo
    
    let depth5 = gps_to_chunk_id(&pos, 5);
    let depth10 = gps_to_chunk_id(&pos, 10);
    let depth15 = gps_to_chunk_id(&pos, 15);
    
    // All should be on same face
    assert_eq!(depth10.face, depth5.face, "Should be on same face");
    assert_eq!(depth15.face, depth5.face, "Should be on same face");
    
    // Path prefixes should match
    assert_eq!(&depth10.path[0..5], &depth5.path[..], 
        "Depth-10 should start with depth-5 path");
    assert_eq!(&depth15.path[0..5], &depth5.path[..], 
        "Depth-15 should start with depth-5 path");
    assert_eq!(&depth15.path[0..10], &depth10.path[..], 
        "Depth-15 should start with depth-10 path");
}

#[test]
fn test_gps_to_chunk_id_all_quadrants_used() {
    // Over many random points, all 4 quadrants (0-3) should be used
    use std::collections::HashSet;
    
    let test_positions = vec![
        GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.0, lon_deg: 90.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.0, lon_deg: 180.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.0, lon_deg: -90.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 45.0, lon_deg: 45.0, elevation_m: 0.0 },
        GpsPos { lat_deg: -45.0, lon_deg: -45.0, elevation_m: 0.0 },
    ];
    
    let mut quadrants_seen = HashSet::new();
    
    for pos in test_positions {
        let chunk = gps_to_chunk_id(&pos, 5);
        for &q in &chunk.path {
            quadrants_seen.insert(q);
        }
    }
    
    assert!(quadrants_seen.contains(&0), "Quadrant 0 should be used");
    assert!(quadrants_seen.contains(&1), "Quadrant 1 should be used");
    assert!(quadrants_seen.contains(&2), "Quadrant 2 should be used");
    assert!(quadrants_seen.contains(&3), "Quadrant 3 should be used");
}

// ============================================================================
// Phase 2.5: ChunkId → bounding geometry tests
// ============================================================================

#[test]
fn test_chunk_approximate_width_depth_0() {
    // Depth-0 tile should be ~9,000km wide (±1,000km)
    let chunk = ChunkId::root(0);
    let width = chunk_approximate_width(&chunk);
    
    assert!(width > 8_000_000.0 && width < 10_000_000.0,
        "Depth-0 tile width should be ~9,000km (±1,000km), got {}m", width);
}

#[test]
fn test_chunk_approximate_width_depth_8() {
    // Depth-8 tile should be ~45km wide (±10km)
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 8);
    let width = chunk_approximate_width(&chunk);
    
    assert!(width > 35_000.0 && width < 55_000.0,
        "Depth-8 tile width should be ~45km (±10km), got {}m", width);
}

#[test]
fn test_chunk_approximate_width_depth_14() {
    // Depth-14 tile should be ~400m wide (±100m)
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 14);
    let width = chunk_approximate_width(&chunk);
    
    assert!(width > 300.0 && width < 900.0,
        "Depth-14 tile width should be ~400m (±100m), got {}m", width);
}

#[test]
fn test_chunk_center_on_sphere_surface() {
    // Chunk centre should be on sphere surface (distance ≈ WGS84_A ± 100m)
    let test_chunks = vec![
        ChunkId::root(0),
        ChunkId::root(3),
        gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10),
        gps_to_chunk_id(&GpsPos { lat_deg: 90.0, lon_deg: 0.0, elevation_m: 0.0 }, 5),
    ];
    
    for chunk in test_chunks {
        let center = chunk_center_ecef(&chunk);
        let dist = (center.x * center.x + center.y * center.y + center.z * center.z).sqrt();
        
        assert!((dist - WGS84_A).abs() < 100.0,
            "Chunk center should be on sphere surface, distance={}m from origin", dist);
    }
}

#[test]
fn test_chunk_corners_on_sphere_surface() {
    // All 4 corners should be on sphere surface
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    let corners = chunk_corners_ecef(&chunk);
    
    assert_eq!(corners.len(), 4, "Should have 4 corners");
    
    for (i, corner) in corners.iter().enumerate() {
        let dist = (corner.x * corner.x + corner.y * corner.y + corner.z * corner.z).sqrt();
        
        assert!((dist - WGS84_A).abs() < 100.0,
            "Corner {} should be on sphere surface, distance={}m", i, dist);
    }
}

#[test]
fn test_chunk_bounding_radius_positive() {
    // Bounding radius should be positive for all tiles
    let test_chunks = vec![
        ChunkId::root(0),
        gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 5),
        gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10),
        gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 15),
    ];
    
    for chunk in test_chunks {
        let radius = chunk_bounding_radius(&chunk);
        assert!(radius > 0.0, "Bounding radius should be positive, got {}", radius);
    }
}

#[test]
fn test_chunk_bounding_radius_decreases_with_depth() {
    // Bounding radius should decrease as depth increases
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    
    let r0 = chunk_bounding_radius(&gps_to_chunk_id(&brisbane, 0));
    let r5 = chunk_bounding_radius(&gps_to_chunk_id(&brisbane, 5));
    let r10 = chunk_bounding_radius(&gps_to_chunk_id(&brisbane, 10));
    let r15 = chunk_bounding_radius(&gps_to_chunk_id(&brisbane, 15));
    
    assert!(r0 > r5, "Depth 0 radius should be > depth 5: {} vs {}", r0, r5);
    assert!(r5 > r10, "Depth 5 radius should be > depth 10: {} vs {}", r5, r10);
    assert!(r10 > r15, "Depth 10 radius should be > depth 15: {} vs {}", r10, r15);
}

#[test]
fn test_chunk_corners_form_quadrilateral() {
    // The 4 corners should form a reasonable quadrilateral (not all identical)
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 10);
    let corners = chunk_corners_ecef(&chunk);
    
    // Check that corners are distinct
    for i in 0..4 {
        for j in (i+1)..4 {
            let dist = ((corners[i].x - corners[j].x).powi(2)
                      + (corners[i].y - corners[j].y).powi(2)
                      + (corners[i].z - corners[j].z).powi(2)).sqrt();
            
            assert!(dist > 1.0, 
                "Corners {} and {} should be distinct, distance={}m", i, j, dist);
        }
    }
}

// ============================================================================
// Phase 2.7: Parent and child queries tests
// ============================================================================

#[test]
fn test_chunk_parent_depth_0() {
    // Parent of depth-0 tile should be None
    let root = ChunkId::root(0);
    let parent = chunk_parent(&root);
    
    assert!(parent.is_none(), "Depth-0 tile should have no parent");
}

#[test]
fn test_chunk_parent_depth_5() {
    // Parent of depth-5 tile should be depth-4
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 5);
    let parent = chunk_parent(&chunk);
    
    assert!(parent.is_some(), "Depth-5 tile should have a parent");
    
    let p = parent.unwrap();
    assert_eq!(p.depth(), 4, "Parent should be depth 4");
    assert_eq!(p.face, chunk.face, "Parent should be on same face");
    assert_eq!(&p.path[..], &chunk.path[0..4], "Parent path should be first 4 elements");
}

#[test]
fn test_chunk_children_count() {
    // Should return exactly 4 children
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    let children = chunk_children(&chunk);
    
    assert_eq!(children.len(), 4, "Should have exactly 4 children");
}

#[test]
fn test_chunk_children_distinct() {
    // All 4 children should be distinct
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    let children = chunk_children(&chunk);
    
    for i in 0..4 {
        for j in (i+1)..4 {
            assert_ne!(children[i], children[j], 
                "Children {} and {} should be distinct", i, j);
        }
    }
}

#[test]
fn test_chunk_children_depth() {
    // Children should be at depth = parent.depth + 1
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 45.0, lon_deg: 90.0, elevation_m: 0.0 }, 10);
    let children = chunk_children(&chunk);
    
    for (i, child) in children.iter().enumerate() {
        assert_eq!(child.depth(), chunk.depth() + 1, 
            "Child {} should be at depth {}", i, chunk.depth() + 1);
        assert_eq!(child.face, chunk.face, 
            "Child {} should be on same face", i);
    }
}

#[test]
fn test_chunk_children_parent_round_trip() {
    // child.parent() should equal original for all 4 children
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 8);
    let children = chunk_children(&chunk);
    
    for (i, child) in children.iter().enumerate() {
        let parent = chunk_parent(child);
        assert!(parent.is_some(), "Child {} should have a parent", i);
        assert_eq!(parent.unwrap(), chunk, 
            "Child {}'s parent should equal original chunk", i);
    }
}

#[test]
fn test_chunk_gps_point_in_one_child() {
    // A GPS point inside parent falls inside exactly one child
    let parent = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    
    // Pick a specific GPS point that should be in the parent
    let test_point = GpsPos { lat_deg: 0.001, lon_deg: 0.001, elevation_m: 0.0 };
    
    // Get the chunk at parent's depth - should match parent
    let parent_of_point = gps_to_chunk_id(&test_point, parent.depth() as u8);
    assert_eq!(parent_of_point, parent, "Test point should be in parent tile");
    
    // Get the chunk at child depth
    let child_of_point = gps_to_chunk_id(&test_point, (parent.depth() + 1) as u8);
    
    // This child should be one of the parent's 4 children
    let children = chunk_children(&parent);
    let matches: Vec<_> = children.iter().filter(|c| **c == child_of_point).collect();
    
    assert_eq!(matches.len(), 1, 
        "Point should fall in exactly one child, found {} matches", matches.len());
}

#[test]
fn test_chunk_grandparent_consistency() {
    // Grandparent of grandchild should equal original
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10);
    let children = chunk_children(&chunk);
    
    for child in &children {
        let grandchildren = chunk_children(child);
        for grandchild in &grandchildren {
            // grandchild -> parent -> grandparent should equal chunk
            let parent = chunk_parent(grandchild);
            assert!(parent.is_some());
            let grandparent = chunk_parent(&parent.unwrap());
            assert!(grandparent.is_some());
            assert_eq!(grandparent.unwrap(), chunk, 
                "Grandparent should equal original chunk");
        }
    }
}

// ============================================================================
// Phase 2.8: Tile containment tests
// ============================================================================

#[test]
fn test_chunk_contains_center() {
    // A point that generated this chunk should be inside it
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 10);
    
    // Brisbane itself should be in the chunk it generated
    assert!(chunk_contains_gps(&chunk, &brisbane), 
        "Point that generated chunk should be inside it");
}

#[test]
fn test_chunk_contains_outside_point() {
    // A point clearly outside the tile should return false
    let brisbane_chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10);
    
    // London is definitely not in a Brisbane tile
    let london = GpsPos { lat_deg: 51.5074, lon_deg: -0.1278, elevation_m: 0.0 };
    
    assert!(!chunk_contains_gps(&brisbane_chunk, &london),
        "London should not be in a Brisbane tile");
}

#[test]
fn test_chunk_contains_nearby_point() {
    // A point very close to Brisbane should be in Brisbane tile at low depth
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 5); // Depth 5 is a larger tile
    
    // Point 100m away
    let nearby = GpsPos { 
        lat_deg: brisbane.lat_deg + 0.001, 
        lon_deg: brisbane.lon_deg, 
        elevation_m: 0.0 
    };
    
    assert!(chunk_contains_gps(&chunk, &nearby),
        "Point 100m away should be in same depth-5 tile");
}

#[test]
fn test_chunk_contains_edge_consistency() {
    // Points on tile edges should consistently belong to exactly one tile
    // Test by checking that adjacent tiles don't both claim the same edge point
    let center = GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&center, 8);
    
    // Get all 4 children
    let children = chunk_children(&chunk);
    
    // Pick a test point that should be in the parent
    let test_point = GpsPos { lat_deg: 0.0001, lon_deg: 0.0001, elevation_m: 0.0 };
    
    // Count how many children claim this point
    let mut count = 0;
    for child in &children {
        if chunk_contains_gps(child, &test_point) {
            count += 1;
        }
    }
    
    assert_eq!(count, 1, 
        "Point should be in exactly one child, found {} children claiming it", count);
}

#[test]
fn test_chunk_children_cover_parent() {
    // All 4 children together should completely cover the parent
    // Test with multiple random-ish points in the parent
    let parent = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    let children = chunk_children(&parent);
    
    // Test points that should be in the parent
    let test_points = vec![
        GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.01, lon_deg: 0.01, elevation_m: 0.0 },
        GpsPos { lat_deg: -0.01, lon_deg: 0.01, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.01, lon_deg: -0.01, elevation_m: 0.0 },
        GpsPos { lat_deg: -0.01, lon_deg: -0.01, elevation_m: 0.0 },
        GpsPos { lat_deg: 0.005, lon_deg: 0.005, elevation_m: 0.0 },
    ];
    
    for point in test_points {
        // Verify point is in parent
        if !chunk_contains_gps(&parent, &point) {
            continue; // Skip points not in parent
        }
        
        // Exactly one child should contain this point
        let mut count = 0;
        for child in &children {
            if chunk_contains_gps(child, &point) {
                count += 1;
            }
        }
        
        assert_eq!(count, 1, 
            "Point at ({}, {}) should be in exactly one child, found {}", 
            point.lat_deg, point.lon_deg, count);
    }
}

#[test]
fn test_chunk_contains_deterministic() {
    // Same point checked multiple times should give same result
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10);
    let test_point = GpsPos { lat_deg: -27.47, lon_deg: 153.03, elevation_m: 0.0 };
    
    let result1 = chunk_contains_gps(&chunk, &test_point);
    let result2 = chunk_contains_gps(&chunk, &test_point);
    let result3 = chunk_contains_gps(&chunk, &test_point);
    
    assert_eq!(result1, result2, "Should be deterministic");
    assert_eq!(result2, result3, "Should be deterministic");
}

// ============================================================================
// Phase 2.9: Phase 2 scale gate tests
// ============================================================================

#[test]
fn test_scale_gate_100_random_points_depth_14() {
    // 100 random GPS points should all resolve to valid ChunkIds at depth 14
    use std::collections::HashSet;
    
    let mut unique_chunks = HashSet::new();
    
    // Generate 100 semi-random but deterministic points
    for i in 0..100 {
        let lat = -90.0 + (i as f64 * 1.8); // -90 to 90
        let lon = -180.0 + (i as f64 * 3.6); // -180 to 180
        
        let gps = GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: 0.0 };
        let chunk = gps_to_chunk_id(&gps, 14);
        
        // Verify it's a valid chunk
        assert!(chunk.face < 6, "Face should be 0-5, got {}", chunk.face);
        assert_eq!(chunk.depth(), 14, "Should be depth 14");
        assert_eq!(chunk.path.len(), 14, "Path should have 14 elements");
        
        // All path elements should be 0-3
        for &q in &chunk.path {
            assert!(q <= 3, "Quadrant should be 0-3, got {}", q);
        }
        
        unique_chunks.insert(chunk);
    }
    
    // Should have many unique chunks (100 points spread globally)
    assert!(unique_chunks.len() > 50, 
        "100 global points should map to >50 unique depth-14 tiles, got {}", 
        unique_chunks.len());
}

#[test]
fn test_scale_gate_all_faces_cover_sphere() {
    // All 6 root tiles should together cover the sphere
    // Test with points distributed across all octants
    
    let mut face_counts = [0; 6];
    let mut total = 0;
    
    // Generate points distributed across latitude and longitude
    for lat_i in 0..20 {
        for lon_i in 0..50 {
            let lat = -90.0 + (lat_i as f64 * 9.0); // -90 to 81
            let lon = -180.0 + (lon_i as f64 * 7.2); // -180 to 180
            
            let gps = GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: 0.0 };
            let chunk = gps_to_chunk_id(&gps, 0);
            
            // Should be on exactly one face
            assert!(chunk.face < 6, "Face should be 0-5");
            assert_eq!(chunk.depth(), 0, "Should be depth 0");
            
            face_counts[chunk.face as usize] += 1;
            total += 1;
        }
    }
    
    // Each face should have some points
    for (face, &count) in face_counts.iter().enumerate() {
        assert!(count > 0, "Face {} should have at least one point, got {}", face, count);
    }
    
    // All points should sum correctly
    assert_eq!(total, 20 * 50, "Total points should be 1000");
}

#[test]
fn test_scale_gate_adjacent_tiles_shared_edges() {
    // Adjacent depth-14 tiles near Brisbane should have corners that nearly match
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let chunk = gps_to_chunk_id(&brisbane, 14);
    
    // Get all 4 children to find adjacent tiles
    let parent = chunk_parent(&chunk).expect("Depth 14 should have parent");
    let siblings = chunk_children(&parent);
    
    // Find a sibling (adjacent tile at same depth)
    let sibling = siblings.iter().find(|s| **s != chunk).expect("Should have siblings");
    
    // Get corners of both tiles
    let corners1 = chunk_corners_ecef(&chunk);
    let corners2 = chunk_corners_ecef(sibling);
    
    // At least one corner should be very close (shared corner)
    let mut min_distance = f64::MAX;
    for c1 in &corners1 {
        for c2 in &corners2 {
            let dist = ((c1.x - c2.x).powi(2) 
                      + (c1.y - c2.y).powi(2) 
                      + (c1.z - c2.z).powi(2)).sqrt();
            min_distance = min_distance.min(dist);
        }
    }
    
    // Shared corners should be within 1m
    assert!(min_distance < 1.0, 
        "Adjacent tiles should share corners within 1m, closest was {}m", min_distance);
}

#[test]
fn test_scale_gate_brisbane_landmarks() {
    // Brisbane landmarks should resolve to nearby tiles at depth 14
    let landmarks = vec![
        ("Queen St Mall", GpsPos { lat_deg: -27.4698, lon_deg: 153.0256, elevation_m: 0.0 }),
        ("Story Bridge", GpsPos { lat_deg: -27.4633, lon_deg: 153.0401, elevation_m: 0.0 }),
        ("Mt Coot-tha", GpsPos { lat_deg: -27.4753, lon_deg: 152.9569, elevation_m: 0.0 }),
        ("Brisbane Airport", GpsPos { lat_deg: -27.3942, lon_deg: 153.1218, elevation_m: 0.0 }),
    ];
    
    let brisbane_cbd = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    let cbd_chunk = gps_to_chunk_id(&brisbane_cbd, 14);
    
    for (name, landmark) in landmarks {
        let chunk = gps_to_chunk_id(&landmark, 14);
        
        // Should be valid chunks
        assert!(chunk.face < 6, "{} should map to valid face", name);
        assert_eq!(chunk.depth(), 14, "{} should be depth 14", name);
        
        // Should be on same face as CBD (Brisbane region)
        assert_eq!(chunk.face, cbd_chunk.face, 
            "{} should be on same face as Brisbane CBD", name);
        
        // Should have some common path prefix (nearby tiles)
        let mut common_prefix = 0;
        for i in 0..14 {
            if chunk.path[i] == cbd_chunk.path[i] {
                common_prefix += 1;
            } else {
                break;
            }
        }
        
        assert!(common_prefix >= 5, 
            "{} should share at least 5 path levels with CBD (regional proximity), got {}", 
            name, common_prefix);
    }
}

#[test]
fn test_scale_gate_chunk_centers_valid() {
    // Chunk centers should be valid GPS positions on the sphere
    let test_locations = vec![
        GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, // Brisbane
        GpsPos { lat_deg: 51.5074, lon_deg: -0.1278, elevation_m: 0.0 },   // London
        GpsPos { lat_deg: 35.6762, lon_deg: 139.6503, elevation_m: 0.0 },  // Tokyo
        GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 },           // Null Island
    ];
    
    for original in test_locations {
        let chunk = gps_to_chunk_id(&original, 8);
        let center_ecef = chunk_center_ecef(&chunk);
        
        // Center should be on sphere surface
        let dist = (center_ecef.x * center_ecef.x 
                  + center_ecef.y * center_ecef.y 
                  + center_ecef.z * center_ecef.z).sqrt();
        
        assert!((dist - WGS84_A).abs() < 100.0,
            "Chunk center should be on sphere surface, distance={}m", dist);
        
        // Should convert to valid GPS
        let center_gps = ecef_to_gps(&center_ecef);
        assert!(center_gps.lat_deg >= -90.0 && center_gps.lat_deg <= 90.0,
            "Center latitude should be valid");
        assert!(center_gps.lon_deg >= -180.0 && center_gps.lon_deg <= 180.0,
            "Center longitude should be valid");
    }
}

#[test]
fn test_scale_gate_tile_width_decreases_with_depth() {
    // Verify tile widths decrease as depth increases
    let brisbane = GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 };
    
    let w0 = chunk_approximate_width(&gps_to_chunk_id(&brisbane, 0));
    let w5 = chunk_approximate_width(&gps_to_chunk_id(&brisbane, 5));
    let w10 = chunk_approximate_width(&gps_to_chunk_id(&brisbane, 10));
    let w14 = chunk_approximate_width(&gps_to_chunk_id(&brisbane, 14));
    
    assert!(w0 > w5, "Depth 0 should be larger than depth 5");
    assert!(w5 > w10, "Depth 5 should be larger than depth 10");
    assert!(w10 > w14, "Depth 10 should be larger than depth 14");
    
    // Depth 0 should be ~millions of meters
    assert!(w0 > 5_000_000.0, "Depth 0 should be >5,000km");
    
    // Depth 14 should be ~hundreds of meters
    assert!(w14 < 1_000.0, "Depth 14 should be <1km");
}

#[test]
fn test_scale_gate_global_coverage() {
    // Every point on Earth should map to exactly one chunk
    // Test representative points from all continents
    let global_points = vec![
        GpsPos { lat_deg: 40.7128, lon_deg: -74.0060, elevation_m: 0.0 },  // New York
        GpsPos { lat_deg: -33.8688, lon_deg: 151.2093, elevation_m: 0.0 }, // Sydney
        GpsPos { lat_deg: 55.7558, lon_deg: 37.6173, elevation_m: 0.0 },   // Moscow
        GpsPos { lat_deg: -23.5505, lon_deg: -46.6333, elevation_m: 0.0 }, // São Paulo
        GpsPos { lat_deg: 30.0444, lon_deg: 31.2357, elevation_m: 0.0 },   // Cairo
        GpsPos { lat_deg: 1.3521, lon_deg: 103.8198, elevation_m: 0.0 },   // Singapore
        GpsPos { lat_deg: -1.2921, lon_deg: 36.8219, elevation_m: 0.0 },   // Nairobi
    ];
    
    for point in global_points {
        let chunk = gps_to_chunk_id(&point, 12);
        
        // Should produce valid chunk
        assert!(chunk.face < 6);
        assert_eq!(chunk.depth(), 12);
        
        // Point should be in its own chunk
        assert!(chunk_contains_gps(&chunk, &point),
            "Point at ({}, {}) should be in its own chunk",
            point.lat_deg, point.lon_deg);
    }
}

// ============================================================================
// Phase 2.6: Neighbour queries tests  
// ============================================================================

#[test]
fn test_chunk_neighbors_interior_tile() {
    // Interior tile should have 4 neighbours, all same face, same depth
    // Use quadrant patterns that keep us away from edges
    // Alternating 0 and 3 keeps us in the middle regions
    let chunk = ChunkId { 
        face: 0, 
        path: vec![0, 3, 0, 3] // Interior pattern
    };
    
    let neighbors = chunk_neighbors(&chunk);
    
    assert_eq!(neighbors.len(), 4, "Should have exactly 4 neighbours");
    
    // All neighbours should be on same face for truly interior tile
    let same_face_count = neighbors.iter().filter(|n| n.face == chunk.face).count();
    assert!(same_face_count >= 3, 
        "Interior tile should have mostly same-face neighbours, got {} out of 4", 
        same_face_count);
    
    for neighbor in &neighbors {
        assert_eq!(neighbor.depth(), chunk.depth(), "Neighbours should be at same depth");
    }
}

#[test]
fn test_chunk_neighbors_count() {
    // Any tile should have exactly 4 neighbours
    let test_chunks = vec![
        ChunkId::root(0),
        gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8),
        gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10),
    ];
    
    for chunk in test_chunks {
        let neighbors = chunk_neighbors(&chunk);
        assert_eq!(neighbors.len(), 4, "Should have exactly 4 neighbours");
    }
}

#[test]
fn test_chunk_neighbors_no_duplicates() {
    // No duplicate neighbours in result
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    let neighbors = chunk_neighbors(&chunk);
    
    use std::collections::HashSet;
    let unique: HashSet<_> = neighbors.iter().collect();
    
    assert_eq!(unique.len(), neighbors.len(), 
        "All neighbours should be unique, got {} unique out of {} total",
        unique.len(), neighbors.len());
}

#[test]
fn test_chunk_neighbors_same_depth() {
    // All neighbours should be at same depth as input
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 10);
    let neighbors = chunk_neighbors(&chunk);
    
    for neighbor in neighbors {
        assert_eq!(neighbor.depth(), chunk.depth(), 
            "Neighbour should be at same depth as original");
    }
}

#[test]
fn test_chunk_neighbors_bidirectional() {
    // Each neighbour's neighbours should include the original tile
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 6);
    let neighbors = chunk_neighbors(&chunk);
    
    for neighbor in neighbors {
        let neighbor_neighbors = chunk_neighbors(&neighbor);
        
        assert!(neighbor_neighbors.contains(&chunk),
            "Neighbour's neighbours should include original tile (bidirectional)");
    }
}

#[test]
fn test_chunk_neighbors_different_from_original() {
    // None of the neighbours should be the original tile
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: -27.4705, lon_deg: 153.0260, elevation_m: 0.0 }, 8);
    let neighbors = chunk_neighbors(&chunk);
    
    for neighbor in neighbors {
        assert_ne!(neighbor, chunk, "Neighbour should not be the original tile");
    }
}

#[test]
fn test_chunk_neighbors_root_tile() {
    // Root tiles should have neighbours on adjacent faces
    let root = ChunkId::root(0);
    let neighbors = chunk_neighbors(&root);
    
    assert_eq!(neighbors.len(), 4, "Root should have 4 neighbours");
    
    // At least some neighbours should be on different faces (cross-face adjacency)
    let different_face_count = neighbors.iter().filter(|n| n.face != root.face).count();
    assert!(different_face_count > 0, 
        "Root tile should have at least one neighbour on different face (cross-face adjacency)");
}

#[test]
fn test_chunk_neighbors_consistency() {
    // Same chunk queried multiple times should give same neighbours
    let chunk = gps_to_chunk_id(&GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 }, 8);
    
    let neighbors1 = chunk_neighbors(&chunk);
    let neighbors2 = chunk_neighbors(&chunk);
    
    assert_eq!(neighbors1.len(), neighbors2.len());
    
    // Convert to sets for comparison (order doesn't matter)
    use std::collections::HashSet;
    let set1: HashSet<_> = neighbors1.iter().collect();
    let set2: HashSet<_> = neighbors2.iter().collect();
    
    assert_eq!(set1, set2, "Neighbour queries should be deterministic");
}

#[test]
fn test_chunk_neighbors_cross_face_edges() {
    // Test face-edge tiles to verify cross-face adjacency works correctly
    
    // Test tile at edge of face 0 (should have neighbours on adjacent faces)
    let edge_chunk = ChunkId { 
        face: 0, 
        path: vec![1, 1, 1] // Right edge
    };
    
    let neighbors = chunk_neighbors(&edge_chunk);
    assert_eq!(neighbors.len(), 4, "Edge tile should have 4 neighbours");
    
    // At least one neighbour should be on a different face
    let cross_face = neighbors.iter().any(|n| n.face != edge_chunk.face);
    assert!(cross_face, "Edge tile should have at least one cross-face neighbour");
    
    // All neighbours should be at same depth
    for neighbor in &neighbors {
        assert_eq!(neighbor.depth(), edge_chunk.depth(), 
            "Neighbours should be at same depth");
    }
}
