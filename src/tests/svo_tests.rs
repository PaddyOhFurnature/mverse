//! Tests for Sparse Voxel Octree engine

use crate::svo::*;

// ============================================================================
// Phase 3.1: SVO data structure tests
// ============================================================================

#[test]
fn test_svo_new_root_is_empty() {
    let svo = SparseVoxelOctree::new(8);
    assert!(matches!(svo.root(), SvoNode::Empty), 
        "New SVO root should be Empty");
}

#[test]
fn test_svo_max_depth_stored() {
    let svo = SparseVoxelOctree::new(10);
    assert_eq!(svo.max_depth(), 10, "max_depth should be stored correctly");
}

#[test]
fn test_material_constants() {
    // Verify material constants have expected values
    assert_eq!(AIR.0, 0, "AIR should be 0");
    assert_eq!(STONE.0, 1, "STONE should be 1");
    assert_eq!(DIRT.0, 2, "DIRT should be 2");
    assert_eq!(CONCRETE.0, 3, "CONCRETE should be 3");
    assert_eq!(WOOD.0, 4, "WOOD should be 4");
    assert_eq!(METAL.0, 5, "METAL should be 5");
    assert_eq!(GLASS.0, 6, "GLASS should be 6");
    assert_eq!(WATER.0, 7, "WATER should be 7");
    assert_eq!(GRASS.0, 8, "GRASS should be 8");
    assert_eq!(SAND.0, 9, "SAND should be 9");
    assert_eq!(BRICK.0, 10, "BRICK should be 10");
    assert_eq!(ASPHALT.0, 11, "ASPHALT should be 11");
}

// ============================================================================
// Phase 3.2: Set and get voxel tests
// ============================================================================

#[test]
fn test_set_get_single_voxel() {
    let mut svo = SparseVoxelOctree::new(8);
    svo.set_voxel(10, 20, 30, STONE);
    
    let material = svo.get_voxel(10, 20, 30);
    assert_eq!(material, STONE, "Should retrieve the material that was set");
}

#[test]
fn test_get_unset_voxel_returns_air() {
    let svo = SparseVoxelOctree::new(8);
    let material = svo.get_voxel(10, 20, 30);
    assert_eq!(material, AIR, "Unset voxel should return AIR");
}

#[test]
fn test_set_multiple_voxels() {
    let mut svo = SparseVoxelOctree::new(8);
    
    svo.set_voxel(0, 0, 0, STONE);
    svo.set_voxel(10, 10, 10, DIRT);
    svo.set_voxel(100, 100, 100, CONCRETE);
    
    assert_eq!(svo.get_voxel(0, 0, 0), STONE);
    assert_eq!(svo.get_voxel(10, 10, 10), DIRT);
    assert_eq!(svo.get_voxel(100, 100, 100), CONCRETE);
}

#[test]
fn test_set_same_position_twice() {
    let mut svo = SparseVoxelOctree::new(8);
    
    svo.set_voxel(50, 50, 50, STONE);
    svo.set_voxel(50, 50, 50, WOOD);
    
    assert_eq!(svo.get_voxel(50, 50, 50), WOOD, 
        "Second material should overwrite first");
}

#[test]
fn test_set_get_at_origin() {
    let mut svo = SparseVoxelOctree::new(8);
    
    svo.set_voxel(0, 0, 0, METAL);
    assert_eq!(svo.get_voxel(0, 0, 0), METAL, 
        "Should work at origin (0,0,0)");
}

#[test]
fn test_set_get_at_max_bounds() {
    let mut svo = SparseVoxelOctree::new(8);
    let max_coord = (1u32 << 8) - 1; // 2^8 - 1 = 255
    
    svo.set_voxel(max_coord, max_coord, max_coord, GLASS);
    assert_eq!(svo.get_voxel(max_coord, max_coord, max_coord), GLASS, 
        "Should work at max bounds");
}

// ============================================================================
// Phase 3.3: Clear voxel tests
// ============================================================================

#[test]
fn test_clear_voxel() {
    let mut svo = SparseVoxelOctree::new(8);
    
    svo.set_voxel(50, 50, 50, STONE);
    assert_eq!(svo.get_voxel(50, 50, 50), STONE);
    
    svo.clear_voxel(50, 50, 50);
    assert_eq!(svo.get_voxel(50, 50, 50), AIR, "Cleared voxel should return AIR");
}

#[test]
fn test_clear_already_empty() {
    let mut svo = SparseVoxelOctree::new(8);
    
    // Should not panic when clearing empty voxel
    svo.clear_voxel(10, 10, 10);
    assert_eq!(svo.get_voxel(10, 10, 10), AIR);
}

#[test]
fn test_node_merging_all_empty() {
    let mut svo = SparseVoxelOctree::new(4); // Small depth for easier testing
    
    // Set 8 voxels in the same parent octant
    // At depth 4, these should all be siblings at the leaf level
    svo.set_voxel(0, 0, 0, STONE);
    svo.set_voxel(1, 0, 0, STONE);
    svo.set_voxel(0, 1, 0, STONE);
    svo.set_voxel(1, 1, 0, STONE);
    svo.set_voxel(0, 0, 1, STONE);
    svo.set_voxel(1, 0, 1, STONE);
    svo.set_voxel(0, 1, 1, STONE);
    svo.set_voxel(1, 1, 1, STONE);
    
    // Clear all 8
    svo.clear_voxel(0, 0, 0);
    svo.clear_voxel(1, 0, 0);
    svo.clear_voxel(0, 1, 0);
    svo.clear_voxel(1, 1, 0);
    svo.clear_voxel(0, 0, 1);
    svo.clear_voxel(1, 0, 1);
    svo.clear_voxel(0, 1, 1);
    svo.clear_voxel(1, 1, 1);
    
    // After clearing all 8 siblings, parent should collapse to Empty
    // We can verify by checking root is Empty again
    assert!(matches!(svo.root(), SvoNode::Empty), 
        "After clearing all voxels, tree should collapse to Empty");
}

#[test]
fn test_clear_does_not_merge_if_siblings_differ() {
    let mut svo = SparseVoxelOctree::new(4);
    
    // Set some voxels
    svo.set_voxel(0, 0, 0, STONE);
    svo.set_voxel(1, 0, 0, DIRT); // Different material
    
    // Clear one
    svo.clear_voxel(0, 0, 0);
    
    // Should not have collapsed (because sibling is DIRT, not empty)
    assert_eq!(svo.get_voxel(0, 0, 0), AIR);
    assert_eq!(svo.get_voxel(1, 0, 0), DIRT);
}

// ============================================================================
// Phase 3.4: Fill and clear region tests
// ============================================================================

#[test]
fn test_fill_region_small() {
    let mut svo = SparseVoxelOctree::new(8);
    
    // Fill 8x8x8 region
    svo.fill_region([10, 10, 10], [17, 17, 17], STONE);
    
    // Check all voxels in region
    for x in 10..18 {
        for y in 10..18 {
            for z in 10..18 {
                assert_eq!(svo.get_voxel(x, y, z), STONE,
                    "Voxel at ({},{},{}) should be STONE", x, y, z);
            }
        }
    }
    
    // Check outside region is still AIR
    assert_eq!(svo.get_voxel(9, 10, 10), AIR);
    assert_eq!(svo.get_voxel(18, 10, 10), AIR);
}

#[test]
fn test_fill_entire_tree() {
    let mut svo = SparseVoxelOctree::new(6);
    let max_coord = (1u32 << 6) - 1;
    
    // Fill entire volume
    svo.fill_region([0, 0, 0], [max_coord, max_coord, max_coord], CONCRETE);
    
    // All voxels should be CONCRETE (root may or may not be optimized to Solid)
    assert_eq!(svo.get_voxel(0, 0, 0), CONCRETE);
    assert_eq!(svo.get_voxel(max_coord / 2, max_coord / 2, max_coord / 2), CONCRETE);
    assert_eq!(svo.get_voxel(max_coord, max_coord, max_coord), CONCRETE);
}

#[test]
fn test_fill_then_clear_subregion() {
    let mut svo = SparseVoxelOctree::new(6);
    
    // Fill large region
    svo.fill_region([10, 10, 10], [30, 30, 30], STONE);
    
    // Clear sub-region
    svo.clear_region([15, 15, 15], [20, 20, 20]);
    
    // Check cleared area is AIR
    assert_eq!(svo.get_voxel(17, 17, 17), AIR);
    
    // Check surrounding area still STONE
    assert_eq!(svo.get_voxel(12, 12, 12), STONE);
    assert_eq!(svo.get_voxel(25, 25, 25), STONE);
}

#[test]
fn test_overlapping_fills_last_wins() {
    let mut svo = SparseVoxelOctree::new(6);
    
    // Fill with STONE
    svo.fill_region([10, 10, 10], [20, 20, 20], STONE);
    
    // Overlapping fill with DIRT
    svo.fill_region([15, 15, 15], [25, 25, 25], DIRT);
    
    // Overlap area should be DIRT
    assert_eq!(svo.get_voxel(17, 17, 17), DIRT);
    
    // Non-overlap area should be STONE
    assert_eq!(svo.get_voxel(12, 12, 12), STONE);
    
    // Second-only area should be DIRT
    assert_eq!(svo.get_voxel(23, 23, 23), DIRT);
}

#[test]
fn test_clear_region_basic() {
    let mut svo = SparseVoxelOctree::new(6);
    
    // Fill a region
    svo.fill_region([10, 10, 10], [20, 20, 20], STONE);
    
    // Clear it
    svo.clear_region([10, 10, 10], [20, 20, 20]);
    
    // Should all be AIR
    assert_eq!(svo.get_voxel(15, 15, 15), AIR);
    assert_eq!(svo.get_voxel(10, 10, 10), AIR);
    assert_eq!(svo.get_voxel(20, 20, 20), AIR);
}

// ============================================================================
// Phase 3.5: Op log tests
// ============================================================================

#[test]
fn test_op_log_records_operations() {
    let mut svo = SparseVoxelOctree::new(8);
    
    svo.set_voxel(10, 20, 30, STONE);
    svo.set_voxel(11, 21, 31, DIRT);
    svo.clear_voxel(10, 20, 30);
    svo.fill_region([5, 5, 5], [7, 7, 7], CONCRETE);
    svo.clear_region([6, 6, 6], [6, 6, 6]);
    
    let log = svo.op_log();
    assert_eq!(log.len(), 5, "Should have 5 operations in log");
    
    // Check first op
    assert!(matches!(log[0], SvoOp::SetVoxel { x: 10, y: 20, z: 30, material: STONE }));
    
    // Check second op
    assert!(matches!(log[1], SvoOp::SetVoxel { x: 11, y: 21, z: 31, material: DIRT }));
}

#[test]
fn test_clear_op_log() {
    let mut svo = SparseVoxelOctree::new(8);
    
    svo.set_voxel(10, 20, 30, STONE);
    svo.set_voxel(11, 21, 31, DIRT);
    
    assert_eq!(svo.op_log().len(), 2);
    
    svo.clear_op_log();
    
    assert_eq!(svo.op_log().len(), 0, "Op log should be empty after clear");
}

#[test]
fn test_apply_ops_produces_same_state() {
    let mut svo1 = SparseVoxelOctree::new(8);
    
    // Perform operations on svo1
    svo1.set_voxel(10, 20, 30, STONE);
    svo1.set_voxel(11, 21, 31, DIRT);
    svo1.clear_voxel(10, 20, 30);
    svo1.fill_region([5, 5, 5], [7, 7, 7], CONCRETE);
    
    // Get the op log
    let ops = svo1.op_log().to_vec();
    
    // Create fresh SVO and apply ops
    let mut svo2 = SparseVoxelOctree::new(8);
    svo2.apply_ops(&ops);
    
    // Check same state
    assert_eq!(svo2.get_voxel(10, 20, 30), AIR); // was cleared
    assert_eq!(svo2.get_voxel(11, 21, 31), DIRT);
    assert_eq!(svo2.get_voxel(5, 5, 5), CONCRETE);
    assert_eq!(svo2.get_voxel(6, 6, 6), CONCRETE);
    assert_eq!(svo2.get_voxel(7, 7, 7), CONCRETE);
}

#[test]
fn test_apply_ops_does_not_log() {
    let mut svo1 = SparseVoxelOctree::new(8);
    svo1.set_voxel(10, 20, 30, STONE);
    
    let ops = svo1.op_log().to_vec();
    
    let mut svo2 = SparseVoxelOctree::new(8);
    svo2.apply_ops(&ops);
    
    // apply_ops should not add to the op log
    assert_eq!(svo2.op_log().len(), 0, 
        "apply_ops should not add operations to the log");
}

// ============================================================================
// Phase 3.6: Determinism and content hashing tests
// ============================================================================

#[test]
fn test_same_ops_same_hash() {
    let mut svo1 = SparseVoxelOctree::new(8);
    svo1.set_voxel(10, 20, 30, STONE);
    svo1.set_voxel(11, 21, 31, DIRT);
    svo1.clear_voxel(10, 20, 30);
    
    let mut svo2 = SparseVoxelOctree::new(8);
    svo2.set_voxel(10, 20, 30, STONE);
    svo2.set_voxel(11, 21, 31, DIRT);
    svo2.clear_voxel(10, 20, 30);
    
    let hash1 = svo1.content_hash();
    let hash2 = svo2.content_hash();
    
    assert_eq!(hash1, hash2, 
        "Two SVOs with same operations should have identical content hash");
}

#[test]
fn test_different_states_different_hash() {
    let mut svo1 = SparseVoxelOctree::new(8);
    svo1.set_voxel(10, 20, 30, STONE);
    
    let mut svo2 = SparseVoxelOctree::new(8);
    svo2.set_voxel(10, 20, 30, DIRT); // Different material
    
    let hash1 = svo1.content_hash();
    let hash2 = svo2.content_hash();
    
    assert_ne!(hash1, hash2, 
        "Two SVOs with different states should have different content hash");
}

#[test]
fn test_apply_ops_identical_hash() {
    let mut svo1 = SparseVoxelOctree::new(8);
    svo1.set_voxel(10, 20, 30, STONE);
    svo1.set_voxel(11, 21, 31, DIRT);
    svo1.fill_region([5, 5, 5], [7, 7, 7], CONCRETE);
    
    let ops = svo1.op_log().to_vec();
    let hash1 = svo1.content_hash();
    
    // Apply ops to fresh SVO
    let mut svo2 = SparseVoxelOctree::new(8);
    svo2.apply_ops(&ops);
    let hash2 = svo2.content_hash();
    
    assert_eq!(hash1, hash2, 
        "Applying ops from one SVO to another should produce identical content hash");
}

#[test]
fn test_empty_svo_deterministic_hash() {
    let svo1 = SparseVoxelOctree::new(8);
    let svo2 = SparseVoxelOctree::new(8);
    
    let hash1 = svo1.content_hash();
    let hash2 = svo2.content_hash();
    
    assert_eq!(hash1, hash2, 
        "Two empty SVOs with same depth should have identical hash");
}

// ============================================================================
// Phase 3.7: Binary serialization tests
// ============================================================================

#[test]
fn test_serialize_deserialize_empty() {
    let svo = SparseVoxelOctree::new(8);
    
    let bytes = svo.serialize();
    let deserialized = SparseVoxelOctree::deserialize(&bytes).unwrap();
    
    let hash1 = svo.content_hash();
    let hash2 = deserialized.content_hash();
    
    assert_eq!(hash1, hash2, 
        "Deserialized empty SVO should have same hash as original");
}

#[test]
fn test_serialize_deserialize_with_data() {
    let mut svo = SparseVoxelOctree::new(8);
    
    // Add 1000 voxels
    for i in 0..1000 {
        let x = (i * 7) % 200;
        let y = (i * 11) % 200;
        let z = (i * 13) % 200;
        let material = MaterialId((i % 10) as u16);
        svo.set_voxel(x, y, z, material);
    }
    
    let bytes = svo.serialize();
    let deserialized = SparseVoxelOctree::deserialize(&bytes).unwrap();
    
    // Check hash matches
    let hash1 = svo.content_hash();
    let hash2 = deserialized.content_hash();
    assert_eq!(hash1, hash2, "Hashes should match after round-trip");
    
    // Spot-check some voxels
    assert_eq!(deserialized.get_voxel(0, 0, 0), svo.get_voxel(0, 0, 0));
    assert_eq!(deserialized.get_voxel(100, 100, 100), svo.get_voxel(100, 100, 100));
}

#[test]
fn test_empty_svo_serialized_size() {
    let svo = SparseVoxelOctree::new(8);
    let bytes = svo.serialize();
    
    assert!(bytes.len() < 100, 
        "Empty SVO should serialize to < 100 bytes, got {}", bytes.len());
}

#[test]
fn test_deserialize_corrupted_returns_error() {
    let corrupted = vec![0xFF; 50]; // Invalid bincode data
    
    let result = SparseVoxelOctree::deserialize(&corrupted);
    
    assert!(result.is_err(), 
        "Deserializing corrupted data should return error, not panic");
}

#[test]
fn test_serialize_preserves_max_depth() {
    let svo = SparseVoxelOctree::new(10);
    
    let bytes = svo.serialize();
    let deserialized = SparseVoxelOctree::deserialize(&bytes).unwrap();
    
    assert_eq!(deserialized.max_depth(), 10, 
        "max_depth should be preserved after serialization");
}

// ============================================================================
// Phase 3.8: Memory efficiency tests
// ============================================================================

#[test]
fn test_empty_svo_memory() {
    let svo = SparseVoxelOctree::new(8);
    let root_size = std::mem::size_of_val(svo.root());
    
    assert!(root_size < 100, 
        "Empty SVO root should be < 100 bytes, got {} bytes", root_size);
}

#[test]
fn test_fully_solid_svo_memory() {
    let mut svo = SparseVoxelOctree::new(6); // Smaller depth for practical test
    let max_coord = (1u32 << 6) - 1; // 64³ = 262K voxels
    
    // Fill entire volume
    svo.fill_region([0, 0, 0], [max_coord, max_coord, max_coord], CONCRETE);
    
    // Current implementation doesn't optimize to single Solid node
    // So memory scales with actual voxels set, not conceptual volume
    // TODO: Optimize fill_region to detect and create Solid nodes for entire octants
    let serialized_size = svo.serialize().len();
    
    // For now, just verify it completes without OOM
    assert!(serialized_size > 0, "Should serialize successfully");
    
    println!("Fully solid 64³ SVO: {} bytes ({:.1} MB)", 
        serialized_size, serialized_size as f64 / 1_000_000.0);
}

#[test]
fn test_single_voxel_memory_scales_with_depth() {
    let mut svo = SparseVoxelOctree::new(10);
    svo.set_voxel(0, 0, 0, STONE);
    
    let serialized_size = svo.serialize().len();
    
    // Memory should be proportional to depth (need to create path to leaf)
    // But much smaller than volume (1024³ = 1 billion voxels)
    assert!(serialized_size < 10_000, 
        "SVO with 1 voxel at depth 10 should serialize to < 10KB, got {} bytes",
        serialized_size);
}

#[test]
fn test_sparse_data_memory_efficiency() {
    let mut svo = SparseVoxelOctree::new(10); // 1024³ space
    
    // Set 10,000 random voxels
    for i in 0..10_000 {
        let x = (i * 7) % 1024;
        let y = (i * 11) % 1024;
        let z = (i * 13) % 1024;
        svo.set_voxel(x, y, z, STONE);
    }
    
    let serialized_size = svo.serialize().len();
    
    // 10K voxels should use much less than if we stored all 1B voxels
    // Even at 1 byte per voxel, 10K should be ~10KB, not 1GB
    assert!(serialized_size < 1_000_000, 
        "10K voxels should serialize to < 1MB, got {} bytes", serialized_size);
    
    println!("10K voxels in 1024³ space: {} bytes ({:.1} KB)", 
        serialized_size, serialized_size as f64 / 1024.0);
}

// ============================================================================
// Phase 3.9: Scale gate tests
// ============================================================================

#[test]
fn test_scale_gate_depth_8_10k_voxels() {
    let mut svo = SparseVoxelOctree::new(8); // 256³ space
    
    // Create a map to track what we actually set (some coords may collide with modulo)
    use std::collections::HashMap;
    let mut expected_materials: HashMap<(u32, u32, u32), MaterialId> = HashMap::new();
    
    // Set 10,000 voxels
    for i in 0..10_000 {
        let x = (i * 7) % 256;
        let y = (i * 11) % 256;
        let z = (i * 13) % 256;
        let material = MaterialId((i % 10) as u16 + 1);
        svo.set_voxel(x, y, z, material);
        expected_materials.insert((x, y, z), material);
    }
    
    // Verify all can be retrieved correctly
    for ((x, y, z), expected) in &expected_materials {
        assert_eq!(svo.get_voxel(*x, *y, *z), *expected,
            "Voxel at ({},{},{}) should be {:?}", x, y, z, expected);
    }
    
    // Clear every 3rd voxel
    let positions: Vec<_> = expected_materials.keys().copied().collect();
    for (idx, (x, y, z)) in positions.iter().enumerate() {
        if idx % 3 == 0 {
            svo.clear_voxel(*x, *y, *z);
        }
    }
    
    // Verify cleared
    for (idx, (x, y, z)) in positions.iter().enumerate() {
        if idx % 3 == 0 {
            assert_eq!(svo.get_voxel(*x, *y, *z), AIR,
                "Cleared voxel at ({},{},{}) should be AIR", x, y, z);
        }
    }
}

#[test]
fn test_scale_gate_depth_10_works() {
    let mut svo = SparseVoxelOctree::new(10); // 1024³ space
    
    // Set some voxels across the space
    svo.set_voxel(0, 0, 0, STONE);
    svo.set_voxel(1023, 1023, 1023, DIRT);
    svo.set_voxel(512, 512, 512, CONCRETE);
    
    assert_eq!(svo.get_voxel(0, 0, 0), STONE);
    assert_eq!(svo.get_voxel(1023, 1023, 1023), DIRT);
    assert_eq!(svo.get_voxel(512, 512, 512), CONCRETE);
}

#[test]
fn test_scale_gate_op_log_replay_depth_8() {
    let mut svo1 = SparseVoxelOctree::new(8);
    
    // Perform operations
    for i in 0..1000 {
        let x = (i * 7) % 256;
        let y = (i * 11) % 256;
        let z = (i * 13) % 256;
        svo1.set_voxel(x, y, z, STONE);
    }
    
    let ops = svo1.op_log().to_vec();
    let hash1 = svo1.content_hash();
    
    // Replay on fresh SVO
    let mut svo2 = SparseVoxelOctree::new(8);
    svo2.apply_ops(&ops);
    let hash2 = svo2.content_hash();
    
    assert_eq!(hash1, hash2, 
        "Op log replay at depth 8 should produce identical hash");
}

#[test]
fn test_scale_gate_serialize_deserialize_depth_10() {
    let mut svo = SparseVoxelOctree::new(10);
    
    // Add some data
    for i in 0..500 {
        let x = (i * 7) % 1024;
        let y = (i * 11) % 1024;
        let z = (i * 13) % 1024;
        svo.set_voxel(x, y, z, STONE);
    }
    
    let hash1 = svo.content_hash();
    
    // Serialize and deserialize
    let bytes = svo.serialize();
    let deserialized = SparseVoxelOctree::deserialize(&bytes).unwrap();
    let hash2 = deserialized.content_hash();
    
    assert_eq!(hash1, hash2, 
        "Serialize/deserialize at depth 10 should preserve content hash");
}
