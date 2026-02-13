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
}
