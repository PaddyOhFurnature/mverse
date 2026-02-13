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
