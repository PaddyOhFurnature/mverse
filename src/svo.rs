//! Sparse Voxel Octree (SVO) Engine
//!
//! The SVO stores volumetric data for building and destroying terrain.
//! Each chunk can have a separate SVO for its local volume.
//!
//! Key properties:
//! - Memory-efficient: empty and solid regions stored as single nodes
//! - Deterministic: operations produce consistent results
//! - Op log: all mutations recorded for CRDT synchronization
//! - Serializable: can be saved/loaded from disk or network

use sha2::{Sha256, Digest};

/// Material identifier (16-bit allows 65,536 material types)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u16);

/// Material constants for common types
pub const AIR: MaterialId = MaterialId(0);
pub const STONE: MaterialId = MaterialId(1);
pub const DIRT: MaterialId = MaterialId(2);
pub const CONCRETE: MaterialId = MaterialId(3);
pub const WOOD: MaterialId = MaterialId(4);
pub const METAL: MaterialId = MaterialId(5);
pub const GLASS: MaterialId = MaterialId(6);
pub const WATER: MaterialId = MaterialId(7);
pub const GRASS: MaterialId = MaterialId(8);
pub const SAND: MaterialId = MaterialId(9);
pub const BRICK: MaterialId = MaterialId(10);
pub const ASPHALT: MaterialId = MaterialId(11);

/// SVO node - recursive octree structure
#[derive(Debug, Clone, PartialEq)]
pub enum SvoNode {
    /// No material (all AIR)
    Empty,
    /// Uniformly filled with a material
    Solid(MaterialId),
    /// Subdivided into 8 child octants
    /// Children ordered: [---,+--,-+-,++-,--+,+-+,-++,+++]
    /// (x,y,z bits: 0=negative half, 1=positive half)
    Branch(Box<[SvoNode; 8]>),
}

/// Operation log entry for CRDT synchronization
#[derive(Debug, Clone, PartialEq)]
pub enum SvoOp {
    SetVoxel { x: u32, y: u32, z: u32, material: MaterialId },
    ClearVoxel { x: u32, y: u32, z: u32 },
    FillRegion { min: [u32; 3], max: [u32; 3], material: MaterialId },
    ClearRegion { min: [u32; 3], max: [u32; 3] },
}

/// Sparse Voxel Octree - volumetric data structure
pub struct SparseVoxelOctree {
    root: SvoNode,
    max_depth: u8,
    op_log: Vec<SvoOp>,
}

impl SparseVoxelOctree {
    /// Creates a new empty SVO with specified maximum depth
    ///
    /// # Arguments
    /// * `max_depth` - Maximum subdivision depth (depth 8 = 256³ voxels)
    ///
    /// # Returns
    /// A new SVO with all voxels set to AIR
    pub fn new(max_depth: u8) -> Self {
        Self {
            root: SvoNode::Empty,
            max_depth,
            op_log: Vec::new(),
        }
    }

    /// Returns the maximum depth of this SVO
    pub fn max_depth(&self) -> u8 {
        self.max_depth
    }

    /// Returns the root node (for testing purposes)
    pub fn root(&self) -> &SvoNode {
        &self.root
    }

    /// Sets a voxel at the specified position to the given material
    ///
    /// Subdivides nodes as needed to reach the target depth.
    ///
    /// # Arguments
    /// * `x`, `y`, `z` - Voxel coordinates (must be < 2^max_depth)
    /// * `material` - Material to set
    pub fn set_voxel(&mut self, x: u32, y: u32, z: u32, material: MaterialId) {
        let size = 1u32 << self.max_depth;
        assert!(x < size && y < size && z < size, 
            "Voxel coordinates ({},{},{}) out of bounds (max: {})", x, y, z, size);
        
        // Log the operation
        self.op_log.push(SvoOp::SetVoxel { x, y, z, material });
        
        Self::set_voxel_recursive(&mut self.root, x, y, z, material, 0, self.max_depth, size);
    }

    /// Gets the material at the specified voxel position
    ///
    /// Returns AIR if the position has not been set.
    ///
    /// # Arguments
    /// * `x`, `y`, `z` - Voxel coordinates
    ///
    /// # Returns
    /// The material at this position, or AIR if empty
    pub fn get_voxel(&self, x: u32, y: u32, z: u32) -> MaterialId {
        let size = 1u32 << self.max_depth;
        if x >= size || y >= size || z >= size {
            return AIR;
        }
        
        Self::get_voxel_recursive(&self.root, x, y, z, 0, self.max_depth, size)
    }

    /// Clears a voxel (sets it to AIR)
    ///
    /// If clearing makes all 8 siblings Empty, the parent collapses to Empty.
    ///
    /// # Arguments
    /// * `x`, `y`, `z` - Voxel coordinates
    pub fn clear_voxel(&mut self, x: u32, y: u32, z: u32) {
        let size = 1u32 << self.max_depth;
        if x >= size || y >= size || z >= size {
            return;
        }
        
        // Log the operation
        self.op_log.push(SvoOp::ClearVoxel { x, y, z });
        
        Self::clear_voxel_recursive(&mut self.root, x, y, z, 0, self.max_depth, size);
    }

    /// Fills a rectangular region with the specified material
    ///
    /// Coordinates are inclusive: [min, max] on each axis.
    /// If a region covers an entire octant, it's set to Solid (optimization).
    ///
    /// # Arguments
    /// * `min` - Minimum corner [x, y, z]
    /// * `max` - Maximum corner [x, y, z] (inclusive)
    /// * `material` - Material to fill with
    pub fn fill_region(&mut self, min: [u32; 3], max: [u32; 3], material: MaterialId) {
        // Log the operation
        self.op_log.push(SvoOp::FillRegion { min, max, material });
        
        let size = 1u32 << self.max_depth;
        
        // Simple implementation: iterate and set each voxel
        // TODO: Optimize to detect when entire octants are covered
        for x in min[0]..=max[0] {
            if x >= size { break; }
            for y in min[1]..=max[1] {
                if y >= size { break; }
                for z in min[2]..=max[2] {
                    if z >= size { break; }
                    self.set_voxel_internal(x, y, z, material);
                }
            }
        }
    }

    /// Clears a rectangular region (sets all voxels to AIR)
    ///
    /// Coordinates are inclusive: [min, max] on each axis.
    ///
    /// # Arguments
    /// * `min` - Minimum corner [x, y, z]
    /// * `max` - Maximum corner [x, y, z] (inclusive)
    pub fn clear_region(&mut self, min: [u32; 3], max: [u32; 3]) {
        // Log the operation
        self.op_log.push(SvoOp::ClearRegion { min, max });
        
        let size = 1u32 << self.max_depth;
        
        for x in min[0]..=max[0] {
            if x >= size { break; }
            for y in min[1]..=max[1] {
                if y >= size { break; }
                for z in min[2]..=max[2] {
                    if z >= size { break; }
                    self.clear_voxel_internal(x, y, z);
                }
            }
        }
    }

    /// Internal set_voxel without logging (for use by fill_region)
    fn set_voxel_internal(&mut self, x: u32, y: u32, z: u32, material: MaterialId) {
        let size = 1u32 << self.max_depth;
        Self::set_voxel_recursive(&mut self.root, x, y, z, material, 0, self.max_depth, size);
    }

    /// Internal clear_voxel without logging (for use by clear_region)
    fn clear_voxel_internal(&mut self, x: u32, y: u32, z: u32) {
        let size = 1u32 << self.max_depth;
        Self::clear_voxel_recursive(&mut self.root, x, y, z, 0, self.max_depth, size);
    }

    /// Returns the operation log
    pub fn op_log(&self) -> &[SvoOp] {
        &self.op_log
    }

    /// Clears the operation log
    pub fn clear_op_log(&mut self) {
        self.op_log.clear();
    }

    /// Applies a sequence of operations to this SVO
    ///
    /// Operations are applied in order. This does NOT add to the op log.
    ///
    /// # Arguments
    /// * `ops` - Slice of operations to apply
    pub fn apply_ops(&mut self, ops: &[SvoOp]) {
        for op in ops {
            match *op {
                SvoOp::SetVoxel { x, y, z, material } => {
                    self.set_voxel_internal(x, y, z, material);
                }
                SvoOp::ClearVoxel { x, y, z } => {
                    self.clear_voxel_internal(x, y, z);
                }
                SvoOp::FillRegion { min, max, material } => {
                    let size = 1u32 << self.max_depth;
                    for x in min[0]..=max[0] {
                        if x >= size { break; }
                        for y in min[1]..=max[1] {
                            if y >= size { break; }
                            for z in min[2]..=max[2] {
                                if z >= size { break; }
                                self.set_voxel_internal(x, y, z, material);
                            }
                        }
                    }
                }
                SvoOp::ClearRegion { min, max } => {
                    let size = 1u32 << self.max_depth;
                    for x in min[0]..=max[0] {
                        if x >= size { break; }
                        for y in min[1]..=max[1] {
                            if y >= size { break; }
                            for z in min[2]..=max[2] {
                                if z >= size { break; }
                                self.clear_voxel_internal(x, y, z);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Recursive helper for set_voxel
    fn set_voxel_recursive(
        node: &mut SvoNode,
        x: u32, y: u32, z: u32,
        material: MaterialId,
        depth: u8,
        max_depth: u8,
        size: u32,
    ) {
        // At target depth, just set to Solid
        if depth == max_depth {
            *node = SvoNode::Solid(material);
            return;
        }

        // Subdivide if needed
        match node {
            SvoNode::Empty => {
                // Need to create Branch
                *node = SvoNode::Branch(Box::new([
                    SvoNode::Empty, SvoNode::Empty, SvoNode::Empty, SvoNode::Empty,
                    SvoNode::Empty, SvoNode::Empty, SvoNode::Empty, SvoNode::Empty,
                ]));
            }
            SvoNode::Solid(old_material) => {
                // Need to expand Solid into Branch filled with same material
                let filled = SvoNode::Solid(*old_material);
                *node = SvoNode::Branch(Box::new([
                    filled.clone(), filled.clone(), filled.clone(), filled.clone(),
                    filled.clone(), filled.clone(), filled.clone(), filled.clone(),
                ]));
            }
            SvoNode::Branch(_) => {
                // Already a branch, continue
            }
        }

        // Determine which octant
        let half_size = size / 2;
        let octant = Self::compute_octant(x, y, z, half_size);

        // Recurse into child
        if let SvoNode::Branch(children) = node {
            let child_x = if x >= half_size { x - half_size } else { x };
            let child_y = if y >= half_size { y - half_size } else { y };
            let child_z = if z >= half_size { z - half_size } else { z };
            
            Self::set_voxel_recursive(
                &mut children[octant],
                child_x, child_y, child_z,
                material,
                depth + 1,
                max_depth,
                half_size,
            );
        }
    }

    /// Recursive helper for get_voxel
    fn get_voxel_recursive(
        node: &SvoNode,
        x: u32, y: u32, z: u32,
        depth: u8,
        max_depth: u8,
        size: u32,
    ) -> MaterialId {
        match node {
            SvoNode::Empty => AIR,
            SvoNode::Solid(material) => *material,
            SvoNode::Branch(children) => {
                if depth == max_depth {
                    return AIR; // Shouldn't happen, but safe default
                }
                
                let half_size = size / 2;
                let octant = Self::compute_octant(x, y, z, half_size);
                
                let child_x = if x >= half_size { x - half_size } else { x };
                let child_y = if y >= half_size { y - half_size } else { y };
                let child_z = if z >= half_size { z - half_size } else { z };
                
                Self::get_voxel_recursive(
                    &children[octant],
                    child_x, child_y, child_z,
                    depth + 1,
                    max_depth,
                    half_size,
                )
            }
        }
    }

    /// Computes octant index (0-7) based on which half of each axis
    fn compute_octant(x: u32, y: u32, z: u32, half_size: u32) -> usize {
        let x_bit = if x >= half_size { 1 } else { 0 };
        let y_bit = if y >= half_size { 1 } else { 0 };
        let z_bit = if z >= half_size { 1 } else { 0 };
        
        // Octant encoding: z*4 + y*2 + x
        (z_bit << 2) | (y_bit << 1) | x_bit
    }

    /// Recursive helper for clear_voxel
    fn clear_voxel_recursive(
        node: &mut SvoNode,
        x: u32, y: u32, z: u32,
        depth: u8,
        max_depth: u8,
        size: u32,
    ) {
        match node {
            SvoNode::Empty => {
                // Already empty, nothing to do
                return;
            }
            SvoNode::Solid(_) => {
                // If we're at target depth, just set to Empty
                if depth == max_depth {
                    *node = SvoNode::Empty;
                    return;
                }
                
                // Need to subdivide solid, then clear
                let material = if let SvoNode::Solid(m) = node { *m } else { unreachable!() };
                *node = SvoNode::Branch(Box::new([
                    SvoNode::Solid(material), SvoNode::Solid(material), 
                    SvoNode::Solid(material), SvoNode::Solid(material),
                    SvoNode::Solid(material), SvoNode::Solid(material), 
                    SvoNode::Solid(material), SvoNode::Solid(material),
                ]));
            }
            SvoNode::Branch(_) => {
                // Continue recursion
            }
        }

        // At this point, node must be Branch
        let half_size = size / 2;
        let octant = Self::compute_octant(x, y, z, half_size);
        
        if let SvoNode::Branch(children) = node {
            let child_x = if x >= half_size { x - half_size } else { x };
            let child_y = if y >= half_size { y - half_size } else { y };
            let child_z = if z >= half_size { z - half_size } else { z };
            
            Self::clear_voxel_recursive(
                &mut children[octant],
                child_x, child_y, child_z,
                depth + 1,
                max_depth,
                half_size,
            );
            
            // After clearing, check if all children are Empty (node merging)
            let all_empty = children.iter().all(|child| matches!(child, SvoNode::Empty));
            if all_empty {
                *node = SvoNode::Empty;
            }
        }
    }

    /// Computes SHA-256 hash of the tree state
    ///
    /// This hash is deterministic and depends only on the tree structure,
    /// not on the operation log. Useful for verifying consistency across
    /// network synchronization.
    ///
    /// # Returns
    /// 32-byte SHA-256 hash of the serialized tree state
    pub fn content_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        
        // Hash max_depth
        hasher.update(&self.max_depth.to_le_bytes());
        
        // Hash tree structure
        Self::hash_node(&self.root, &mut hasher);
        
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Recursively hashes a node
    fn hash_node(node: &SvoNode, hasher: &mut Sha256) {
        match node {
            SvoNode::Empty => {
                hasher.update(&[0u8]); // Tag for Empty
            }
            SvoNode::Solid(material) => {
                hasher.update(&[1u8]); // Tag for Solid
                hasher.update(&material.0.to_le_bytes());
            }
            SvoNode::Branch(children) => {
                hasher.update(&[2u8]); // Tag for Branch
                for child in children.iter() {
                    Self::hash_node(child, hasher);
                }
            }
        }
    }
}
