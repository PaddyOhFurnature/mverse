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
}
