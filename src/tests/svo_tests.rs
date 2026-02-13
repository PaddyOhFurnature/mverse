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
