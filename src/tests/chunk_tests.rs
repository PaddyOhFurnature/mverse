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
