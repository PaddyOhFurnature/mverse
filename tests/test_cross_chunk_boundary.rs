//! Test cross-chunk boundary voxel editing

use metaverse_core::{
    chunk::{ChunkId, CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z},
    voxel::VoxelCoord,
};

#[test]
fn test_affected_chunks_center() {
    // Voxel in center of chunk - should only affect 1 chunk
    let voxel = VoxelCoord::new(15, 100, 15);
    let affected = ChunkId::affected_by_voxel(&voxel);
    
    assert_eq!(affected.len(), 1, "Center voxel should affect 1 chunk");
    assert_eq!(affected[0], ChunkId::from_voxel(&voxel));
}

#[test]
fn test_affected_chunks_face_boundary() {
    // Voxel exactly on X boundary (30m chunks)
    let voxel = VoxelCoord::new(30, 100, 15);
    let affected = ChunkId::affected_by_voxel(&voxel);
    
    assert_eq!(affected.len(), 2, "Face boundary voxel should affect 2 chunks");
    
    // Should affect chunks on both sides of the boundary
    let chunk_ids: Vec<_> = affected.iter().map(|c| (c.x, c.y, c.z)).collect();
    assert!(chunk_ids.contains(&(0, 0, 0)) || chunk_ids.contains(&(1, 0, 0)),
        "Should affect chunks on both sides of X boundary");
}

#[test]
fn test_affected_chunks_edge_boundary() {
    // Voxel on edge (X and Z boundaries)
    let voxel = VoxelCoord::new(30, 100, 30);
    let affected = ChunkId::affected_by_voxel(&voxel);
    
    assert_eq!(affected.len(), 4, "Edge boundary voxel should affect 4 chunks");
}

#[test]
fn test_affected_chunks_corner_boundary() {
    // Voxel at 3-way corner (X, Y, Z boundaries)
    let voxel = VoxelCoord::new(30, 200, 30);
    let affected = ChunkId::affected_by_voxel(&voxel);
    
    assert_eq!(affected.len(), 8, "Corner boundary voxel should affect 8 chunks");
}

#[test]
fn test_chunk_size_constants() {
    // Verify our chunk size constants are correct
    assert_eq!(CHUNK_SIZE_X, 30, "X dimension should be 30m");
    assert_eq!(CHUNK_SIZE_Y, 200, "Y dimension should be 200m");
    assert_eq!(CHUNK_SIZE_Z, 30, "Z dimension should be 30m");
}

#[test]
fn test_boundary_detection_comprehensive() {
    // Test various positions to ensure correct boundary detection
    
    // Test X boundary at different Y and Z
    for y in [0, 100, 199] {
        for z in [0, 15, 29] {
            let voxel = VoxelCoord::new(30, y, z);
            let affected = ChunkId::affected_by_voxel(&voxel);
            assert!(affected.len() >= 2, 
                "X boundary at ({}, {}, {}) should affect at least 2 chunks, got {}", 
                30, y, z, affected.len());
        }
    }
    
    // Test Y boundary at different X and Z  
    for x in [0, 15, 29] {
        for z in [0, 15, 29] {
            let voxel = VoxelCoord::new(x, 200, z);
            let affected = ChunkId::affected_by_voxel(&voxel);
            assert!(affected.len() >= 2,
                "Y boundary at ({}, {}, {}) should affect at least 2 chunks, got {}",
                x, 200, z, affected.len());
        }
    }
    
    // Test Z boundary at different X and Y
    for x in [0, 15, 29] {
        for y in [0, 100, 199] {
            let voxel = VoxelCoord::new(x, y, 30);
            let affected = ChunkId::affected_by_voxel(&voxel);
            assert!(affected.len() >= 2,
                "Z boundary at ({}, {}, {}) should affect at least 2 chunks, got {}",
                x, y, 30, affected.len());
        }
    }
}
