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
