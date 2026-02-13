// Unit tests for quad-sphere chunking system

use crate::chunks::*;

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
