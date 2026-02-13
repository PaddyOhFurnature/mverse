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
