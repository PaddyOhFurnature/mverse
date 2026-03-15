//! Marching cubes algorithm for voxel mesh extraction
//!
//! Converts binary voxel grid into smooth triangle mesh.
//! Uses lookup tables for 256 cube configurations.

use crate::materials::MaterialId;
use crate::mesh::{Mesh, Vertex};
use crate::voxel::{Octree, VoxelCoord};
use glam::Vec3;

/// Cube corner numbering (binary position encoding):
/// ```
///     7-------6
///    /|      /|
///   4-------5 |
///   | |     | |
///   | 3-----|-2
///   |/      |/
///   0-------1
/// ```
const CORNER_OFFSETS: [Vec3; 8] = [
    Vec3::new(0.0, 0.0, 0.0), // 0: (0,0,0)
    Vec3::new(1.0, 0.0, 0.0), // 1: (1,0,0)
    Vec3::new(1.0, 0.0, 1.0), // 2: (1,0,1)
    Vec3::new(0.0, 0.0, 1.0), // 3: (0,0,1)
    Vec3::new(0.0, 1.0, 0.0), // 4: (0,1,0)
    Vec3::new(1.0, 1.0, 0.0), // 5: (1,1,0)
    Vec3::new(1.0, 1.0, 1.0), // 6: (1,1,1)
    Vec3::new(0.0, 1.0, 1.0), // 7: (0,1,1)
];

/// Edge connections (which two corners each edge connects)
const EDGE_CONNECTIONS: [(usize, usize); 12] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 0), // Bottom face
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 4), // Top face
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7), // Vertical edges
];

/// Triangle table: lists which edges form triangles for each configuration
/// Each row is 16 values (up to 5 triangles × 3 edges, -1 terminated)
/// Rows indexed by cube configuration (0-255)
pub const TRIANGLE_TABLE: [[i8; 16]; 256] = [
    [
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ],
    [0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 8, 3, 9, 8, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 1, 2, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 2, 10, 0, 2, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 8, 3, 2, 10, 8, 10, 9, 8, -1, -1, -1, -1, -1, -1, -1],
    [3, 11, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 11, 2, 8, 11, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 11, 2, 1, 9, 11, 9, 8, 11, -1, -1, -1, -1, -1, -1, -1],
    [3, 10, 1, 11, 10, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 10, 1, 0, 8, 10, 8, 11, 10, -1, -1, -1, -1, -1, -1, -1],
    [3, 9, 0, 3, 11, 9, 11, 10, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 8, 10, 10, 8, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 7, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 3, 0, 7, 3, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 1, 9, 4, 7, 1, 7, 3, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 4, 7, 3, 0, 4, 1, 2, 10, -1, -1, -1, -1, -1, -1, -1],
    [9, 2, 10, 9, 0, 2, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1],
    [2, 10, 9, 2, 9, 7, 2, 7, 3, 7, 9, 4, -1, -1, -1, -1],
    [8, 4, 7, 3, 11, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 4, 7, 11, 2, 4, 2, 0, 4, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 1, 8, 4, 7, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1],
    [4, 7, 11, 9, 4, 11, 9, 11, 2, 9, 2, 1, -1, -1, -1, -1],
    [3, 10, 1, 3, 11, 10, 7, 8, 4, -1, -1, -1, -1, -1, -1, -1],
    [1, 11, 10, 1, 4, 11, 1, 0, 4, 7, 11, 4, -1, -1, -1, -1],
    [4, 7, 8, 9, 0, 11, 9, 11, 10, 11, 0, 3, -1, -1, -1, -1],
    [4, 7, 11, 4, 11, 9, 9, 11, 10, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, 0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 5, 4, 1, 5, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 5, 4, 8, 3, 5, 3, 1, 5, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 9, 5, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 1, 2, 10, 4, 9, 5, -1, -1, -1, -1, -1, -1, -1],
    [5, 2, 10, 5, 4, 2, 4, 0, 2, -1, -1, -1, -1, -1, -1, -1],
    [2, 10, 5, 3, 2, 5, 3, 5, 4, 3, 4, 8, -1, -1, -1, -1],
    [9, 5, 4, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 11, 2, 0, 8, 11, 4, 9, 5, -1, -1, -1, -1, -1, -1, -1],
    [0, 5, 4, 0, 1, 5, 2, 3, 11, -1, -1, -1, -1, -1, -1, -1],
    [2, 1, 5, 2, 5, 8, 2, 8, 11, 4, 8, 5, -1, -1, -1, -1],
    [10, 3, 11, 10, 1, 3, 9, 5, 4, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 5, 0, 8, 1, 8, 10, 1, 8, 11, 10, -1, -1, -1, -1],
    [5, 4, 0, 5, 0, 11, 5, 11, 10, 11, 0, 3, -1, -1, -1, -1],
    [5, 4, 8, 5, 8, 10, 10, 8, 11, -1, -1, -1, -1, -1, -1, -1],
    [9, 7, 8, 5, 7, 9, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 3, 0, 9, 5, 3, 5, 7, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 7, 8, 0, 1, 7, 1, 5, 7, -1, -1, -1, -1, -1, -1, -1],
    [1, 5, 3, 3, 5, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 7, 8, 9, 5, 7, 10, 1, 2, -1, -1, -1, -1, -1, -1, -1],
    [10, 1, 2, 9, 5, 0, 5, 3, 0, 5, 7, 3, -1, -1, -1, -1],
    [8, 0, 2, 8, 2, 5, 8, 5, 7, 10, 5, 2, -1, -1, -1, -1],
    [2, 10, 5, 2, 5, 3, 3, 5, 7, -1, -1, -1, -1, -1, -1, -1],
    [7, 9, 5, 7, 8, 9, 3, 11, 2, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 7, 9, 7, 2, 9, 2, 0, 2, 7, 11, -1, -1, -1, -1],
    [2, 3, 11, 0, 1, 8, 1, 7, 8, 1, 5, 7, -1, -1, -1, -1],
    [11, 2, 1, 11, 1, 7, 7, 1, 5, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 8, 8, 5, 7, 10, 1, 3, 10, 3, 11, -1, -1, -1, -1],
    [5, 7, 0, 5, 0, 9, 7, 11, 0, 1, 0, 10, 11, 10, 0, -1],
    [11, 10, 0, 11, 0, 3, 10, 5, 0, 8, 0, 7, 5, 7, 0, -1],
    [11, 10, 5, 7, 11, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [10, 6, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 1, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 8, 3, 1, 9, 8, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 5, 2, 6, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 5, 1, 2, 6, 3, 0, 8, -1, -1, -1, -1, -1, -1, -1],
    [9, 6, 5, 9, 0, 6, 0, 2, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 9, 8, 5, 8, 2, 5, 2, 6, 3, 2, 8, -1, -1, -1, -1],
    [2, 3, 11, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 0, 8, 11, 2, 0, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 2, 3, 11, 5, 10, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 1, 9, 2, 9, 11, 2, 9, 8, 11, -1, -1, -1, -1],
    [6, 3, 11, 6, 5, 3, 5, 1, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 11, 0, 11, 5, 0, 5, 1, 5, 11, 6, -1, -1, -1, -1],
    [3, 11, 6, 0, 3, 6, 0, 6, 5, 0, 5, 9, -1, -1, -1, -1],
    [6, 5, 9, 6, 9, 11, 11, 9, 8, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 4, 7, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 3, 0, 4, 7, 3, 6, 5, 10, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 5, 10, 6, 8, 4, 7, -1, -1, -1, -1, -1, -1, -1],
    [10, 6, 5, 1, 9, 7, 1, 7, 3, 7, 9, 4, -1, -1, -1, -1],
    [6, 1, 2, 6, 5, 1, 4, 7, 8, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 5, 5, 2, 6, 3, 0, 4, 3, 4, 7, -1, -1, -1, -1],
    [8, 4, 7, 9, 0, 5, 0, 6, 5, 0, 2, 6, -1, -1, -1, -1],
    [7, 3, 9, 7, 9, 4, 3, 2, 9, 5, 9, 6, 2, 6, 9, -1],
    [3, 11, 2, 7, 8, 4, 10, 6, 5, -1, -1, -1, -1, -1, -1, -1],
    [5, 10, 6, 4, 7, 2, 4, 2, 0, 2, 7, 11, -1, -1, -1, -1],
    [0, 1, 9, 4, 7, 8, 2, 3, 11, 5, 10, 6, -1, -1, -1, -1],
    [9, 2, 1, 9, 11, 2, 9, 4, 11, 7, 11, 4, 5, 10, 6, -1],
    [8, 4, 7, 3, 11, 5, 3, 5, 1, 5, 11, 6, -1, -1, -1, -1],
    [5, 1, 11, 5, 11, 6, 1, 0, 11, 7, 11, 4, 0, 4, 11, -1],
    [0, 5, 9, 0, 6, 5, 0, 3, 6, 11, 6, 3, 8, 4, 7, -1],
    [6, 5, 9, 6, 9, 11, 4, 7, 9, 7, 11, 9, -1, -1, -1, -1],
    [10, 4, 9, 6, 4, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 10, 6, 4, 9, 10, 0, 8, 3, -1, -1, -1, -1, -1, -1, -1],
    [10, 0, 1, 10, 6, 0, 6, 4, 0, -1, -1, -1, -1, -1, -1, -1],
    [8, 3, 1, 8, 1, 6, 8, 6, 4, 6, 1, 10, -1, -1, -1, -1],
    [1, 4, 9, 1, 2, 4, 2, 6, 4, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 1, 2, 9, 2, 4, 9, 2, 6, 4, -1, -1, -1, -1],
    [0, 2, 4, 4, 2, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 3, 2, 8, 2, 4, 4, 2, 6, -1, -1, -1, -1, -1, -1, -1],
    [10, 4, 9, 10, 6, 4, 11, 2, 3, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 2, 2, 8, 11, 4, 9, 10, 4, 10, 6, -1, -1, -1, -1],
    [3, 11, 2, 0, 1, 6, 0, 6, 4, 6, 1, 10, -1, -1, -1, -1],
    [6, 4, 1, 6, 1, 10, 4, 8, 1, 2, 1, 11, 8, 11, 1, -1],
    [9, 6, 4, 9, 3, 6, 9, 1, 3, 11, 6, 3, -1, -1, -1, -1],
    [8, 11, 1, 8, 1, 0, 11, 6, 1, 9, 1, 4, 6, 4, 1, -1],
    [3, 11, 6, 3, 6, 0, 0, 6, 4, -1, -1, -1, -1, -1, -1, -1],
    [6, 4, 8, 11, 6, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 10, 6, 7, 8, 10, 8, 9, 10, -1, -1, -1, -1, -1, -1, -1],
    [0, 7, 3, 0, 10, 7, 0, 9, 10, 6, 7, 10, -1, -1, -1, -1],
    [10, 6, 7, 1, 10, 7, 1, 7, 8, 1, 8, 0, -1, -1, -1, -1],
    [10, 6, 7, 10, 7, 1, 1, 7, 3, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 6, 1, 6, 8, 1, 8, 9, 8, 6, 7, -1, -1, -1, -1],
    [2, 6, 9, 2, 9, 1, 6, 7, 9, 0, 9, 3, 7, 3, 9, -1],
    [7, 8, 0, 7, 0, 6, 6, 0, 2, -1, -1, -1, -1, -1, -1, -1],
    [7, 3, 2, 6, 7, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 11, 10, 6, 8, 10, 8, 9, 8, 6, 7, -1, -1, -1, -1],
    [2, 0, 7, 2, 7, 11, 0, 9, 7, 6, 7, 10, 9, 10, 7, -1],
    [1, 8, 0, 1, 7, 8, 1, 10, 7, 6, 7, 10, 2, 3, 11, -1],
    [11, 2, 1, 11, 1, 7, 10, 6, 1, 6, 7, 1, -1, -1, -1, -1],
    [8, 9, 6, 8, 6, 7, 9, 1, 6, 11, 6, 3, 1, 3, 6, -1],
    [0, 9, 1, 11, 6, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 8, 0, 7, 0, 6, 3, 11, 0, 11, 6, 0, -1, -1, -1, -1],
    [7, 11, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 6, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 8, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 9, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 1, 9, 8, 3, 1, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1],
    [10, 1, 2, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 3, 0, 8, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1],
    [2, 9, 0, 2, 10, 9, 6, 11, 7, -1, -1, -1, -1, -1, -1, -1],
    [6, 11, 7, 2, 10, 3, 10, 8, 3, 10, 9, 8, -1, -1, -1, -1],
    [7, 2, 3, 6, 2, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [7, 0, 8, 7, 6, 0, 6, 2, 0, -1, -1, -1, -1, -1, -1, -1],
    [2, 7, 6, 2, 3, 7, 0, 1, 9, -1, -1, -1, -1, -1, -1, -1],
    [1, 6, 2, 1, 8, 6, 1, 9, 8, 8, 7, 6, -1, -1, -1, -1],
    [10, 7, 6, 10, 1, 7, 1, 3, 7, -1, -1, -1, -1, -1, -1, -1],
    [10, 7, 6, 1, 7, 10, 1, 8, 7, 1, 0, 8, -1, -1, -1, -1],
    [0, 3, 7, 0, 7, 10, 0, 10, 9, 6, 10, 7, -1, -1, -1, -1],
    [7, 6, 10, 7, 10, 8, 8, 10, 9, -1, -1, -1, -1, -1, -1, -1],
    [6, 8, 4, 11, 8, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 6, 11, 3, 0, 6, 0, 4, 6, -1, -1, -1, -1, -1, -1, -1],
    [8, 6, 11, 8, 4, 6, 9, 0, 1, -1, -1, -1, -1, -1, -1, -1],
    [9, 4, 6, 9, 6, 3, 9, 3, 1, 11, 3, 6, -1, -1, -1, -1],
    [6, 8, 4, 6, 11, 8, 2, 10, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 3, 0, 11, 0, 6, 11, 0, 4, 6, -1, -1, -1, -1],
    [4, 11, 8, 4, 6, 11, 0, 2, 9, 2, 10, 9, -1, -1, -1, -1],
    [10, 9, 3, 10, 3, 2, 9, 4, 3, 11, 3, 6, 4, 6, 3, -1],
    [8, 2, 3, 8, 4, 2, 4, 6, 2, -1, -1, -1, -1, -1, -1, -1],
    [0, 4, 2, 4, 6, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 9, 0, 2, 3, 4, 2, 4, 6, 4, 3, 8, -1, -1, -1, -1],
    [1, 9, 4, 1, 4, 2, 2, 4, 6, -1, -1, -1, -1, -1, -1, -1],
    [8, 1, 3, 8, 6, 1, 8, 4, 6, 6, 10, 1, -1, -1, -1, -1],
    [10, 1, 0, 10, 0, 6, 6, 0, 4, -1, -1, -1, -1, -1, -1, -1],
    [4, 6, 3, 4, 3, 8, 6, 10, 3, 0, 3, 9, 10, 9, 3, -1],
    [10, 9, 4, 6, 10, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 5, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 4, 9, 5, 11, 7, 6, -1, -1, -1, -1, -1, -1, -1],
    [5, 0, 1, 5, 4, 0, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1],
    [11, 7, 6, 8, 3, 4, 3, 5, 4, 3, 1, 5, -1, -1, -1, -1],
    [9, 5, 4, 10, 1, 2, 7, 6, 11, -1, -1, -1, -1, -1, -1, -1],
    [6, 11, 7, 1, 2, 10, 0, 8, 3, 4, 9, 5, -1, -1, -1, -1],
    [7, 6, 11, 5, 4, 10, 4, 2, 10, 4, 0, 2, -1, -1, -1, -1],
    [3, 4, 8, 3, 5, 4, 3, 2, 5, 10, 5, 2, 11, 7, 6, -1],
    [7, 2, 3, 7, 6, 2, 5, 4, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 5, 4, 0, 8, 6, 0, 6, 2, 6, 8, 7, -1, -1, -1, -1],
    [3, 6, 2, 3, 7, 6, 1, 5, 0, 5, 4, 0, -1, -1, -1, -1],
    [6, 2, 8, 6, 8, 7, 2, 1, 8, 4, 8, 5, 1, 5, 8, -1],
    [9, 5, 4, 10, 1, 6, 1, 7, 6, 1, 3, 7, -1, -1, -1, -1],
    [1, 6, 10, 1, 7, 6, 1, 0, 7, 8, 7, 0, 9, 5, 4, -1],
    [4, 0, 10, 4, 10, 5, 0, 3, 10, 6, 10, 7, 3, 7, 10, -1],
    [7, 6, 10, 7, 10, 8, 5, 4, 10, 4, 8, 10, -1, -1, -1, -1],
    [6, 9, 5, 6, 11, 9, 11, 8, 9, -1, -1, -1, -1, -1, -1, -1],
    [3, 6, 11, 0, 6, 3, 0, 5, 6, 0, 9, 5, -1, -1, -1, -1],
    [0, 11, 8, 0, 5, 11, 0, 1, 5, 5, 6, 11, -1, -1, -1, -1],
    [6, 11, 3, 6, 3, 5, 5, 3, 1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 10, 9, 5, 11, 9, 11, 8, 11, 5, 6, -1, -1, -1, -1],
    [0, 11, 3, 0, 6, 11, 0, 9, 6, 5, 6, 9, 1, 2, 10, -1],
    [11, 8, 5, 11, 5, 6, 8, 0, 5, 10, 5, 2, 0, 2, 5, -1],
    [6, 11, 3, 6, 3, 5, 2, 10, 3, 10, 5, 3, -1, -1, -1, -1],
    [5, 8, 9, 5, 2, 8, 5, 6, 2, 3, 8, 2, -1, -1, -1, -1],
    [9, 5, 6, 9, 6, 0, 0, 6, 2, -1, -1, -1, -1, -1, -1, -1],
    [1, 5, 8, 1, 8, 0, 5, 6, 8, 3, 8, 2, 6, 2, 8, -1],
    [1, 5, 6, 2, 1, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 3, 6, 1, 6, 10, 3, 8, 6, 5, 6, 9, 8, 9, 6, -1],
    [10, 1, 0, 10, 0, 6, 9, 5, 0, 5, 6, 0, -1, -1, -1, -1],
    [0, 3, 8, 5, 6, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [10, 5, 6, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 5, 10, 7, 5, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [11, 5, 10, 11, 7, 5, 8, 3, 0, -1, -1, -1, -1, -1, -1, -1],
    [5, 11, 7, 5, 10, 11, 1, 9, 0, -1, -1, -1, -1, -1, -1, -1],
    [10, 7, 5, 10, 11, 7, 9, 8, 1, 8, 3, 1, -1, -1, -1, -1],
    [11, 1, 2, 11, 7, 1, 7, 5, 1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 1, 2, 7, 1, 7, 5, 7, 2, 11, -1, -1, -1, -1],
    [9, 7, 5, 9, 2, 7, 9, 0, 2, 2, 11, 7, -1, -1, -1, -1],
    [7, 5, 2, 7, 2, 11, 5, 9, 2, 3, 2, 8, 9, 8, 2, -1],
    [2, 5, 10, 2, 3, 5, 3, 7, 5, -1, -1, -1, -1, -1, -1, -1],
    [8, 2, 0, 8, 5, 2, 8, 7, 5, 10, 2, 5, -1, -1, -1, -1],
    [9, 0, 1, 5, 10, 3, 5, 3, 7, 3, 10, 2, -1, -1, -1, -1],
    [9, 8, 2, 9, 2, 1, 8, 7, 2, 10, 2, 5, 7, 5, 2, -1],
    [1, 3, 5, 3, 7, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 7, 0, 7, 1, 1, 7, 5, -1, -1, -1, -1, -1, -1, -1],
    [9, 0, 3, 9, 3, 5, 5, 3, 7, -1, -1, -1, -1, -1, -1, -1],
    [9, 8, 7, 5, 9, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [5, 8, 4, 5, 10, 8, 10, 11, 8, -1, -1, -1, -1, -1, -1, -1],
    [5, 0, 4, 5, 11, 0, 5, 10, 11, 11, 3, 0, -1, -1, -1, -1],
    [0, 1, 9, 8, 4, 10, 8, 10, 11, 10, 4, 5, -1, -1, -1, -1],
    [10, 11, 4, 10, 4, 5, 11, 3, 4, 9, 4, 1, 3, 1, 4, -1],
    [2, 5, 1, 2, 8, 5, 2, 11, 8, 4, 5, 8, -1, -1, -1, -1],
    [0, 4, 11, 0, 11, 3, 4, 5, 11, 2, 11, 1, 5, 1, 11, -1],
    [0, 2, 5, 0, 5, 9, 2, 11, 5, 4, 5, 8, 11, 8, 5, -1],
    [9, 4, 5, 2, 11, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 5, 10, 3, 5, 2, 3, 4, 5, 3, 8, 4, -1, -1, -1, -1],
    [5, 10, 2, 5, 2, 4, 4, 2, 0, -1, -1, -1, -1, -1, -1, -1],
    [3, 10, 2, 3, 5, 10, 3, 8, 5, 4, 5, 8, 0, 1, 9, -1],
    [5, 10, 2, 5, 2, 4, 1, 9, 2, 9, 4, 2, -1, -1, -1, -1],
    [8, 4, 5, 8, 5, 3, 3, 5, 1, -1, -1, -1, -1, -1, -1, -1],
    [0, 4, 5, 1, 0, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [8, 4, 5, 8, 5, 3, 9, 0, 5, 0, 3, 5, -1, -1, -1, -1],
    [9, 4, 5, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 11, 7, 4, 9, 11, 9, 10, 11, -1, -1, -1, -1, -1, -1, -1],
    [0, 8, 3, 4, 9, 7, 9, 11, 7, 9, 10, 11, -1, -1, -1, -1],
    [1, 10, 11, 1, 11, 4, 1, 4, 0, 7, 4, 11, -1, -1, -1, -1],
    [3, 1, 4, 3, 4, 8, 1, 10, 4, 7, 4, 11, 10, 11, 4, -1],
    [4, 11, 7, 9, 11, 4, 9, 2, 11, 9, 1, 2, -1, -1, -1, -1],
    [9, 7, 4, 9, 11, 7, 9, 1, 11, 2, 11, 1, 0, 8, 3, -1],
    [11, 7, 4, 11, 4, 2, 2, 4, 0, -1, -1, -1, -1, -1, -1, -1],
    [11, 7, 4, 11, 4, 2, 8, 3, 4, 3, 2, 4, -1, -1, -1, -1],
    [2, 9, 10, 2, 7, 9, 2, 3, 7, 7, 4, 9, -1, -1, -1, -1],
    [9, 10, 7, 9, 7, 4, 10, 2, 7, 8, 7, 0, 2, 0, 7, -1],
    [3, 7, 10, 3, 10, 2, 7, 4, 10, 1, 10, 0, 4, 0, 10, -1],
    [1, 10, 2, 8, 7, 4, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 1, 4, 1, 7, 7, 1, 3, -1, -1, -1, -1, -1, -1, -1],
    [4, 9, 1, 4, 1, 7, 0, 8, 1, 8, 7, 1, -1, -1, -1, -1],
    [4, 0, 3, 7, 4, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [4, 8, 7, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [9, 10, 8, 10, 11, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 9, 3, 9, 11, 11, 9, 10, -1, -1, -1, -1, -1, -1, -1],
    [0, 1, 10, 0, 10, 8, 8, 10, 11, -1, -1, -1, -1, -1, -1, -1],
    [3, 1, 10, 11, 3, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 2, 11, 1, 11, 9, 9, 11, 8, -1, -1, -1, -1, -1, -1, -1],
    [3, 0, 9, 3, 9, 11, 1, 2, 9, 2, 11, 9, -1, -1, -1, -1],
    [0, 2, 11, 8, 0, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [3, 2, 11, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 8, 2, 8, 10, 10, 8, 9, -1, -1, -1, -1, -1, -1, -1],
    [9, 10, 2, 0, 9, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [2, 3, 8, 2, 8, 10, 0, 1, 8, 1, 10, 8, -1, -1, -1, -1],
    [1, 10, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [1, 3, 8, 9, 1, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 9, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [0, 3, 8, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    ],
];

/// Edge table: which edges are intersected for each cube configuration (256 entries)
/// Bit N set means edge N has a vertex
pub const EDGE_TABLE: [u16; 256] = [
    0x000, 0x109, 0x203, 0x30a, 0x406, 0x50f, 0x605, 0x70c, 0x80c, 0x905, 0xa0f, 0xb06, 0xc0a,
    0xd03, 0xe09, 0xf00, 0x190, 0x099, 0x393, 0x29a, 0x596, 0x49f, 0x795, 0x69c, 0x99c, 0x895,
    0xb9f, 0xa96, 0xd9a, 0xc93, 0xf99, 0xe90, 0x230, 0x339, 0x033, 0x13a, 0x636, 0x73f, 0x435,
    0x53c, 0xa3c, 0xb35, 0x83f, 0x936, 0xe3a, 0xf33, 0xc39, 0xd30, 0x3a0, 0x2a9, 0x1a3, 0x0aa,
    0x7a6, 0x6af, 0x5a5, 0x4ac, 0xbac, 0xaa5, 0x9af, 0x8a6, 0xfaa, 0xea3, 0xda9, 0xca0, 0x460,
    0x569, 0x663, 0x76a, 0x066, 0x16f, 0x265, 0x36c, 0xc6c, 0xd65, 0xe6f, 0xf66, 0x86a, 0x963,
    0xa69, 0xb60, 0x5f0, 0x4f9, 0x7f3, 0x6fa, 0x1f6, 0x0ff, 0x3f5, 0x2fc, 0xdfc, 0xcf5, 0xfff,
    0xef6, 0x9fa, 0x8f3, 0xbf9, 0xaf0, 0x650, 0x759, 0x453, 0x55a, 0x256, 0x35f, 0x055, 0x15c,
    0xe5c, 0xf55, 0xc5f, 0xd56, 0xa5a, 0xb53, 0x859, 0x950, 0x7c0, 0x6c9, 0x5c3, 0x4ca, 0x3c6,
    0x2cf, 0x1c5, 0x0cc, 0xfcc, 0xec5, 0xdcf, 0xcc6, 0xbca, 0xac3, 0x9c9, 0x8c0, 0x8c0, 0x9c9,
    0xac3, 0xbca, 0xcc6, 0xdcf, 0xec5, 0xfcc, 0x0cc, 0x1c5, 0x2cf, 0x3c6, 0x4ca, 0x5c3, 0x6c9,
    0x7c0, 0x950, 0x859, 0xb53, 0xa5a, 0xd56, 0xc5f, 0xf55, 0xe5c, 0x15c, 0x055, 0x35f, 0x256,
    0x55a, 0x453, 0x759, 0x650, 0xaf0, 0xbf9, 0x8f3, 0x9fa, 0xef6, 0xfff, 0xcf5, 0xdfc, 0x2fc,
    0x3f5, 0x0ff, 0x1f6, 0x6fa, 0x7f3, 0x4f9, 0x5f0, 0xb60, 0xa69, 0x963, 0x86a, 0xf66, 0xe6f,
    0xd65, 0xc6c, 0x36c, 0x265, 0x16f, 0x066, 0x76a, 0x663, 0x569, 0x460, 0xca0, 0xda9, 0xea3,
    0xfaa, 0x8a6, 0x9af, 0xaa5, 0xbac, 0x4ac, 0x5a5, 0x6af, 0x7a6, 0x0aa, 0x1a3, 0x2a9, 0x3a0,
    0xd30, 0xc39, 0xf33, 0xe3a, 0x936, 0x83f, 0xb35, 0xa3c, 0x53c, 0x435, 0x73f, 0x636, 0x13a,
    0x033, 0x339, 0x230, 0xe90, 0xf99, 0xc93, 0xd9a, 0xa96, 0xb9f, 0x895, 0x99c, 0x69c, 0x795,
    0x49f, 0x596, 0x29a, 0x393, 0x099, 0x190, 0xf00, 0xe09, 0xd03, 0xc0a, 0xb06, 0xa0f, 0x905,
    0x80c, 0x70c, 0x605, 0x50f, 0x406, 0x30a, 0x203, 0x109, 0x000,
];

/// Get cube configuration index from 8 corner states
/// Returns 0-255 based on which corners are solid
pub fn cube_index(corners: &[bool; 8]) -> usize {
    let mut index = 0usize;
    for i in 0..8 {
        if corners[i] {
            index |= 1 << i;
        }
    }
    index
}

/// Interpolate vertex position between two cube corners
/// t = 0.5 for simple midpoint (we don't have density values yet)
fn interpolate_vertex(p1: Vec3, p2: Vec3, _t: f32) -> Vec3 {
    // Simple midpoint interpolation
    // TODO: Use actual density values when available
    (p1 + p2) * 0.5
}

/// Extract mesh from a single cube using marching cubes algorithm
///
/// # Arguments
/// * `cube_pos` - Position of cube minimum corner (in world space)
/// * `corners` - Boolean array of 8 corner states (true = solid, false = air)
///
/// # Returns
/// A Mesh containing triangles for this cube, or empty mesh if fully empty/solid
pub fn extract_cube_mesh(cube_pos: Vec3, corners: &[bool; 8], scale: f32) -> Mesh {
    let index = cube_index(corners);

    // Get which edges have vertices
    let edges = EDGE_TABLE[index];

    // If 0, cube is entirely inside/outside surface
    if edges == 0 {
        return Mesh::new();
    }

    // Calculate vertex positions for each edge that's intersected
    let mut edge_vertices = [Vec3::ZERO; 12];
    for edge_idx in 0..12 {
        if (edges & (1 << edge_idx)) != 0 {
            let (c1, c2) = EDGE_CONNECTIONS[edge_idx];
            let p1 = cube_pos + CORNER_OFFSETS[c1] * scale;
            let p2 = cube_pos + CORNER_OFFSETS[c2] * scale;
            edge_vertices[edge_idx] = interpolate_vertex(p1, p2, 0.5);
        }
    }

    // Build triangles from triangle table
    let mut mesh = Mesh::new();

    let tri_table = TRIANGLE_TABLE[index];
    let mut tri_idx = 0;

    while tri_table[tri_idx] != -1 {
        // Read 3 edges forming a triangle
        let e1 = tri_table[tri_idx] as usize;
        let e2 = tri_table[tri_idx + 1] as usize;
        let e3 = tri_table[tri_idx + 2] as usize;

        let v1 = edge_vertices[e1];
        let v2 = edge_vertices[e2];
        let v3 = edge_vertices[e3];

        // Calculate normal from triangle
        let edge1 = v2 - v1;
        let edge2 = v3 - v1;
        let normal = edge1.cross(edge2).normalize();

        // Add vertices and get their indices
        let i1 = mesh.add_vertex(Vertex::new(v1, normal));
        let i2 = mesh.add_vertex(Vertex::new(v2, normal));
        let i3 = mesh.add_vertex(Vertex::new(v3, normal));

        // Add triangle
        mesh.add_triangle(crate::mesh::Triangle::new(i1, i2, i3));

        tri_idx += 3;
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cube_index_empty() {
        let corners = [false; 8];
        assert_eq!(cube_index(&corners), 0);
    }

    #[test]
    fn test_cube_index_full() {
        let corners = [true; 8];
        assert_eq!(cube_index(&corners), 255);
    }

    #[test]
    fn test_cube_index_single_corner() {
        let mut corners = [false; 8];
        corners[0] = true;
        assert_eq!(cube_index(&corners), 1);

        corners = [false; 8];
        corners[7] = true;
        assert_eq!(cube_index(&corners), 128);
    }

    #[test]
    fn test_interpolate_vertex() {
        let p1 = Vec3::new(0.0, 0.0, 0.0);
        let p2 = Vec3::new(2.0, 2.0, 2.0);
        let result = interpolate_vertex(p1, p2, 0.5);
        assert_eq!(result, Vec3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn test_edge_table_populated() {
        // Edge table should have non-zero entries
        assert_eq!(EDGE_TABLE[0], 0x000); // No edges
        assert_eq!(EDGE_TABLE[255], 0x000); // No edges
        assert_ne!(EDGE_TABLE[1], 0x000); // Has edges
    }

    #[test]
    fn test_extract_empty_cube() {
        // All corners AIR = no mesh
        let corners = [false; 8];
        let mesh = extract_cube_mesh(Vec3::ZERO, &corners, 1.0);
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);
    }

    #[test]
    fn test_extract_full_cube() {
        // All corners SOLID = no mesh (entirely inside surface)
        let corners = [true; 8];
        let mesh = extract_cube_mesh(Vec3::ZERO, &corners, 1.0);
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);
    }

    #[test]
    fn test_extract_single_corner() {
        // Single solid corner = small pyramid
        let mut corners = [false; 8];
        corners[0] = true;

        let mesh = extract_cube_mesh(Vec3::ZERO, &corners, 1.0);

        // Should produce at least one triangle
        assert!(
            mesh.vertex_count() >= 3,
            "Should have at least 3 vertices for a triangle"
        );
        assert!(
            mesh.triangle_count() >= 1,
            "Should have at least 1 triangle"
        );

        // All vertices should be within cube bounds
        for v in &mesh.vertices {
            let pos = v.position;
            assert!(pos.x >= -0.1 && pos.x <= 1.1, "X out of bounds: {}", pos.x);
            assert!(pos.y >= -0.1 && pos.y <= 1.1, "Y out of bounds: {}", pos.y);
            assert!(pos.z >= -0.1 && pos.z <= 1.1, "Z out of bounds: {}", pos.z);
        }
    }

    #[test]
    fn test_extract_half_cube() {
        // Bottom half solid, top half air = horizontal plane
        let corners = [true, true, true, true, false, false, false, false];

        let mesh = extract_cube_mesh(Vec3::ZERO, &corners, 1.0);
        assert!(mesh.vertex_count() >= 3);
        assert!(mesh.triangle_count() >= 1);

        // Vertices should be near y=0.5 (midpoint between bottom and top)
        for v in &mesh.vertices {
            let pos = v.position;
            // Allow some tolerance
            assert!(
                pos.y >= 0.4 && pos.y <= 0.6,
                "Y should be near midpoint: {}",
                pos.y
            );
        }
    }
}

/// Extract mesh from octree region with FloatingOrigin transform
///
/// Samples octree in a cube around `center` of size `2^depth` meters.
/// Transforms vertices from ECEF absolute coordinates to camera-relative (FloatingOrigin).
///
/// For depth=7: samples 128×128×128 cube = 2,097,152 voxels
pub fn extract_octree_mesh(octree: &Octree, center: &VoxelCoord, depth: u8) -> Mesh {
    let mut mesh = Mesh::new();
    let size = 1 << depth; // 2^depth
    let half = size / 2;

    // Iterate over all cubes in the region
    for x in (-half)..half {
        for y in (-half)..half {
            for z in (-half)..half {
                let voxel_pos = VoxelCoord::new(center.x + x, center.y + y, center.z + z);

                // Sample 8 corners of this cube
                let mut corners = [false; 8];
                let offsets = [
                    (0, 0, 0),
                    (1, 0, 0),
                    (1, 0, 1),
                    (0, 0, 1),
                    (0, 1, 0),
                    (1, 1, 0),
                    (1, 1, 1),
                    (0, 1, 1),
                ];

                for (i, (dx, dy, dz)) in offsets.iter().enumerate() {
                    let corner_pos =
                        VoxelCoord::new(voxel_pos.x + dx, voxel_pos.y + dy, voxel_pos.z + dz);
                    let mat = octree.get_voxel(corner_pos);
                    corners[i] = mat != MaterialId::AIR && mat != MaterialId::WATER;
                }

                // Use simple integer offsets for marching cubes
                // Marching cubes will interpolate vertices within the cube
                let cube_mesh =
                    extract_cube_mesh(Vec3::new(x as f32, y as f32, z as f32), &corners, 1.0);

                mesh.merge(&cube_mesh);
            }
        }
    }

    mesh
}

/// Extract mesh for exact voxel bounds (for chunks)
///
/// Unlike extract_octree_mesh which uses center+depth, this extracts
/// mesh for an exact voxel range. Useful for chunk-based terrain where
/// each chunk has specific bounds.
///
/// Vertices are positioned relative to chunk center (centered at origin).
/// Returns both the mesh and the chunk center voxel coordinate.
pub fn extract_chunk_mesh(
    octree: &Octree,
    min_voxel: &VoxelCoord,
    max_voxel: &VoxelCoord,
    step: usize,
) -> (Mesh, VoxelCoord) {
    let mut mesh = Mesh::new();

    // Calculate chunk center - max_voxel is exclusive, so true max is max-1
    // For min=0, max=100: range is 0..99 (inclusive), center should be 49.5
    // Using integer math: (0+100-1)/2 = 99/2 = 49 (not 50!)
    let center = VoxelCoord::new(
        min_voxel.x + (max_voxel.x - min_voxel.x - 1) / 2,
        min_voxel.y + (max_voxel.y - min_voxel.y - 1) / 2,
        min_voxel.z + (max_voxel.z - min_voxel.z - 1) / 2,
    );

    // Iterate over all cubes in the chunk bounds, stepping by `step` for LOD.
    let s = step.max(1) as i64;
    let sf = s as f32;

    let mut voxel_x = min_voxel.x;
    while voxel_x < max_voxel.x {
        let mut voxel_y = min_voxel.y;
        while voxel_y < max_voxel.y {
            let mut voxel_z = min_voxel.z;
            while voxel_z < max_voxel.z {
                let voxel_pos = VoxelCoord::new(voxel_x, voxel_y, voxel_z);

                // Sample 8 corners of this (step-sized) cube,
                // clamping to chunk bounds to avoid out-of-range reads.
                let mut corners = [false; 8];
                let offsets: [(i64, i64, i64); 8] = [
                    (0, 0, 0),
                    (s, 0, 0),
                    (s, 0, s),
                    (0, 0, s),
                    (0, s, 0),
                    (s, s, 0),
                    (s, s, s),
                    (0, s, s),
                ];

                for (i, (dx, dy, dz)) in offsets.iter().enumerate() {
                    let corner_pos = VoxelCoord::new(
                        (voxel_pos.x + dx).min(max_voxel.x - 1),
                        (voxel_pos.y + dy).min(max_voxel.y - 1),
                        (voxel_pos.z + dz).min(max_voxel.z - 1),
                    );
                    let mat = octree.get_voxel(corner_pos);
                    corners[i] = mat != MaterialId::AIR && mat != MaterialId::WATER;
                }

                // Position relative to chunk center (mesh centered at origin)
                let local_x = (voxel_x - center.x) as f32;
                let local_y = (voxel_y - center.y) as f32;
                let local_z = (voxel_z - center.z) as f32;

                let cube_mesh =
                    extract_cube_mesh(Vec3::new(local_x, local_y, local_z), &corners, sf);

                mesh.merge(&cube_mesh);
                voxel_z += s;
            }
            voxel_y += s;
        }
        voxel_x += s;
    }

    (mesh, center)
}

/// Extract a flat water surface mesh from the top face of WATER voxels.
/// Vertices use the water color in the normal slot for OSM shader rendering.
/// Positions are relative to chunk center (same as extract_chunk_mesh).
pub fn extract_water_surface_mesh(
    octree: &Octree,
    min_voxel: &VoxelCoord,
    max_voxel: &VoxelCoord,
) -> Mesh {
    use crate::mesh::Triangle;

    let mut mesh = Mesh::new();

    // Royal blue water color (stored in normal slot for OSM flat-colour shader)
    let water_color = Vec3::new(65.0 / 255.0, 105.0 / 255.0, 225.0 / 255.0);

    let center = VoxelCoord::new(
        min_voxel.x + (max_voxel.x - min_voxel.x - 1) / 2,
        min_voxel.y + (max_voxel.y - min_voxel.y - 1) / 2,
        min_voxel.z + (max_voxel.z - min_voxel.z - 1) / 2,
    );

    for voxel_x in min_voxel.x..max_voxel.x {
        for voxel_y in min_voxel.y..max_voxel.y {
            for voxel_z in min_voxel.z..max_voxel.z {
                let pos = VoxelCoord::new(voxel_x, voxel_y, voxel_z);
                let above = VoxelCoord::new(voxel_x, voxel_y + 1, voxel_z);

                // Top face of a WATER voxel exposed to AIR
                if octree.get_voxel(pos) == MaterialId::WATER
                    && octree.get_voxel(above) == MaterialId::AIR
                {
                    let lx = (voxel_x - center.x) as f32;
                    let ly = (voxel_y - center.y) as f32 + 1.0; // top of voxel
                    let lz = (voxel_z - center.z) as f32;

                    let v0 = mesh.add_vertex(Vertex::new(Vec3::new(lx, ly, lz), water_color));
                    let v1 = mesh.add_vertex(Vertex::new(Vec3::new(lx + 1.0, ly, lz), water_color));
                    let v2 = mesh
                        .add_vertex(Vertex::new(Vec3::new(lx + 1.0, ly, lz + 1.0), water_color));
                    let v3 = mesh.add_vertex(Vertex::new(Vec3::new(lx, ly, lz + 1.0), water_color));

                    mesh.add_triangle(Triangle::new(v0, v1, v2));
                    mesh.add_triangle(Triangle::new(v0, v2, v3));
                }
            }
        }
    }

    mesh
}

/// Smooth marching cubes mesh extraction using a density field.
///
/// Unlike `extract_chunk_mesh` which snaps vertices to voxel midpoints (creating the
/// visible "grey grid" stairstepping), this function:
/// 1. Computes continuous density `d(x,y,z) = surface_y_f(x,z) - y + noise` from the
///    surface cache (which stores sub-voxel surface heights from SRTM interpolation).
/// 2. Places edge vertices at the true zero-crossing (linear interpolation by density ratio).
/// 3. Uses density gradient normals instead of per-face normals → smooth shading.
///
/// The noise layers add natural-looking surface detail on top of the SRTM base:
/// - Medium FBM (wavelength ~143m, ±0.4 voxel): gentle hill undulation
/// - Small FBM (wavelength ~12m, ±0.12 voxel): surface grain / roughness
/// Smooth marching cubes mesh extraction with optional neighbour surface caches.
///
/// `neighbor_x` — surface cache of the chunk at (chunk_id.x + 1, same y, same z).
/// `neighbor_z` — surface cache of the chunk at (same x, same y, chunk_id.z + 1).
///
/// When a neighbour cache is provided the shared boundary grid points use its exact
/// surface_y values, eliminating the seam entirely.  When a neighbour is absent (not
/// yet loaded) we fall back to clamping the last interior column — a minor flat shelf
/// that disappears once that neighbour loads.
pub fn extract_chunk_mesh_smooth(
    octree: &Octree,
    surface_cache: &crate::terrain::SurfaceCache,
    min_voxel: &VoxelCoord,
    max_voxel: &VoxelCoord,
    neighbor_x: Option<&crate::terrain::SurfaceCache>,
    neighbor_z: Option<&crate::terrain::SurfaceCache>,
    // −Y neighbour: used for all-air bottom columns so both chunks agree on density
    // at wy = min_voxel.y (shared boundary with the chunk below).
    neighbor_y_lower: Option<&crate::terrain::SurfaceCache>,
    // +Y neighbour: used for all-solid top columns so both chunks agree on density
    // at wy = max_voxel.y (shared boundary with the chunk above).
    neighbor_y_upper: Option<&crate::terrain::SurfaceCache>,
    // Voxel step size for LOD. step=1 (LOD 0/1) gives 1m cells; step=2 (LOD 2)
    // gives 2m cells; step=4 (LOD 3) gives 4m cells. Boundary stitching still uses
    // the neighbor's 1m surface_cache so seams remain correct at all LOD levels.
    step: usize,
) -> (Mesh, VoxelCoord) {
    use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

    let step = step.max(1);

    // Medium-scale FBM: adds gentle undulation over ~143m wavelengths.
    // Disabled at step≥2 — noise is smaller than the cell size at coarse LOD.
    let fbm_med: Fbm<Perlin> = Fbm::<Perlin>::new(42)
        .set_octaves(4)
        .set_frequency(0.007)
        .set_lacunarity(2.0)
        .set_persistence(0.5);
    // Fine-scale FBM: surface grain at ~12m wavelength
    let fbm_fine: Fbm<Perlin> = Fbm::<Perlin>::new(137)
        .set_octaves(2)
        .set_frequency(0.08)
        .set_lacunarity(2.0)
        .set_persistence(0.5);
    let apply_noise = false; // disabled: FBM noise obscures SRTM surface; re-enable for polish pass

    let center = VoxelCoord::new(
        min_voxel.x + (max_voxel.x - min_voxel.x - 1) / 2,
        min_voxel.y + (max_voxel.y - min_voxel.y - 1) / 2,
        min_voxel.z + (max_voxel.z - min_voxel.z - 1) / 2,
    );

    // Grid dimensions — one sample point per cell corner, step-spaced.
    // At step=1: gx = CHUNK_SIZE+1; at step=2: gx = CHUNK_SIZE/2+1, etc.
    let chunk_x = (max_voxel.x - min_voxel.x) as usize;
    let chunk_y = (max_voxel.y - min_voxel.y) as usize;
    let chunk_z = (max_voxel.z - min_voxel.z) as usize;
    let gx = chunk_x / step + 1;
    let gy = chunk_y / step + 1;
    let gz = chunk_z / step + 1;

    // Stride: density_grid[xi * gy * gz + yi * gz + zi]
    let g_stride = gy * gz;
    let g_idx = |xi: usize, yi: usize, zi: usize| xi * g_stride + yi * gz + zi;

    // Pre-compute density for all grid points in a single pass.
    //
    // The density grid has one entry per step-spaced column corner.  The last
    // column on each axis (xi=gx-1, zi=gz-1) corresponds to the chunk boundary
    // (wx=max_voxel.x, wz=max_voxel.z) and is fetched from the neighbor surface
    // cache so both chunks produce the same density at shared edges.
    //
    // If the neighbour cache is not yet loaded we fall back to the last interior
    // column (a minor flat shelf that disappears once the neighbour loads).
    let mut density_grid = vec![0.0f32; gx * gy * gz];
    // Per-grid-point material color (linear RGB, derived from the surface voxel material).
    // Boundary columns default to grass; interior columns sample the octree at surface_y.
    let default_color = Vec3::new(0.30, 0.52, 0.18); // grass green
    let mut color_grid = vec![default_color; gx * gy * gz];
    let xi_last = gx - 1;
    let zi_last = gz - 1;
    let no_surface = min_voxel.y as f64 - 1.0; // sentinel → all air

    // Look up surface_y from this chunk's cache (interior only).
    let own =
        |cx: i64, cz: i64| -> f64 { surface_cache.get(&(cx, cz)).copied().unwrap_or(no_surface) };

    for xi in 0..gx {
        let wx = min_voxel.x + (xi * step) as i64;
        let x_bnd = xi == xi_last;
        for zi in 0..gz {
            let wz = min_voxel.z + (zi * step) as i64;
            let z_bnd = zi == zi_last;

            let surface_y = match (x_bnd, z_bnd) {
                // Interior point: use this chunk's surface cache directly.
                (false, false) => own(wx, wz),

                // X-boundary: use the +X neighbour's first column.
                (true, false) => neighbor_x
                    .and_then(|nx| nx.get(&(max_voxel.x, wz)).copied())
                    .unwrap_or_else(|| own(max_voxel.x - 1, wz)), // clamp fallback

                // Z-boundary: use the +Z neighbour's first column.
                (false, true) => neighbor_z
                    .and_then(|nz| nz.get(&(wx, max_voxel.z)).copied())
                    .unwrap_or_else(|| own(wx, max_voxel.z - 1)), // clamp fallback

                // Corner: average both neighbour first columns (or clamp if missing).
                (true, true) => {
                    let sx = neighbor_x
                        .and_then(|nx| nx.get(&(max_voxel.x, max_voxel.z)).copied())
                        .unwrap_or_else(|| own(max_voxel.x - 1, max_voxel.z - 1));
                    let sz = neighbor_z
                        .and_then(|nz| nz.get(&(max_voxel.x, max_voxel.z)).copied())
                        .unwrap_or_else(|| own(max_voxel.x - 1, max_voxel.z - 1));
                    (sx + sz) * 0.5
                }
            };

            // Y-boundary override — ensures density at the shared row
            // (wy = max_voxel.y = min_voxel.y of the chunk above, and vice-versa)
            // is IDENTICAL in both chunks, eliminating horizontal seams.
            let ny_key = match (x_bnd, z_bnd) {
                (false, false) => (wx, wz),
                (true, false) => (max_voxel.x, wz),
                (false, true) => (wx, max_voxel.z),
                (true, true) => (max_voxel.x, max_voxel.z),
            };
            let surface_y = if surface_y >= max_voxel.y as f64 {
                neighbor_y_upper
                    .and_then(|ny| ny.get(&ny_key).copied())
                    .unwrap_or(surface_y)
            } else if surface_y < min_voxel.y as f64 {
                neighbor_y_lower
                    .and_then(|ny| ny.get(&ny_key).copied())
                    .unwrap_or(surface_y)
            } else {
                surface_y
            };

            // Noise is sampled at actual world coordinates so it stays consistent
            // across LOD levels. At step>1 we skip fine-grained noise (sub-cell detail
            // would be aliased anyway).
            let med = if apply_noise {
                fbm_med.get([wx as f64, wz as f64]) as f32 * 0.4
            } else {
                0.0
            };

            // Sample material color at surface voxel for interior columns only.
            // Boundary columns use the default grass color since the voxel data
            // belongs to the neighbour chunk's octree.
            let col_color = if !x_bnd && !z_bnd {
                let sy = surface_y.round() as i64;
                let sy_clamped = sy.clamp(min_voxel.y, max_voxel.y - 1);
                let mat = octree.get_voxel(crate::voxel::VoxelCoord::new(wx, sy_clamped, wz));
                let c = crate::materials::MaterialId::properties(mat).color;
                Vec3::new(
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                )
            } else {
                default_color
            };

            for yi in 0..gy {
                let wy = min_voxel.y + (yi * step) as i64;
                let fine = if apply_noise {
                    fbm_fine.get([wx as f64, wy as f64, wz as f64]) as f32 * 0.12
                } else {
                    0.0
                };
                density_grid[g_idx(xi, yi, zi)] =
                    (surface_y - wy as f64 + med as f64 + fine as f64) as f32;
                color_grid[g_idx(xi, yi, zi)] = col_color;
            }
        }
    }

    // Corner offset layout (matches CORNER_OFFSETS / EDGE_CONNECTIONS tables above)
    const OFFSETS: [(usize, usize, usize); 8] = [
        (0, 0, 0),
        (1, 0, 0),
        (1, 0, 1),
        (0, 0, 1),
        (0, 1, 0),
        (1, 1, 0),
        (1, 1, 1),
        (0, 1, 1),
    ];

    let mut mesh = Mesh::new();

    // Iterate over cells (gx-1) × (gy-1) × (gz-1); each cell is step×step×step voxels.
    let stepf = step as f64;
    for xi0 in 0..gx - 1 {
        let voxel_x = min_voxel.x + (xi0 * step) as i64;
        for yi0 in 0..gy - 1 {
            let voxel_y = min_voxel.y + (yi0 * step) as i64;
            for zi0 in 0..gz - 1 {
                let voxel_z = min_voxel.z + (zi0 * step) as i64;

                // Sample density at 8 corners from the pre-computed grid
                let mut d = [0.0f32; 8];
                let mut cube_idx = 0usize;
                for (i, (dx, dy, dz)) in OFFSETS.iter().enumerate() {
                    let di = density_grid[g_idx(xi0 + dx, yi0 + dy, zi0 + dz)];
                    d[i] = di;
                    // Convention: density ≥ 0 → solid (bit set in cube index)
                    if di >= 0.0 {
                        cube_idx |= 1 << i;
                    }
                }

                let edges = EDGE_TABLE[cube_idx];
                if edges == 0 {
                    continue;
                } // fully solid or fully air

                // Compute interpolated vertex position + gradient normal for each active edge
                let mut edge_verts = [Vec3::ZERO; 12];
                let mut edge_norms = [Vec3::new(0.0, 1.0, 0.0); 12];
                let mut edge_colors = [default_color; 12];

                for edge_idx in 0..12usize {
                    if (edges & (1 << edge_idx)) == 0 {
                        continue;
                    }

                    let (c1, c2) = EDGE_CONNECTIONS[edge_idx];
                    let (dx1, dy1, dz1) = OFFSETS[c1];
                    let (dx2, dy2, dz2) = OFFSETS[c2];

                    let d1 = d[c1];
                    let d2 = d[c2];
                    // Linear interpolation to the zero crossing
                    let t = if (d1 - d2).abs() > 1e-5 {
                        (d1 / (d1 - d2)).clamp(0.01, 0.99) as f64
                    } else {
                        0.5
                    };

                    // World voxel position of vertex (fractional), scaled by step
                    let wx_v =
                        voxel_x as f64 + (dx1 as f64 + t * (dx2 as f64 - dx1 as f64)) * stepf;
                    let wy_v =
                        voxel_y as f64 + (dy1 as f64 + t * (dy2 as f64 - dy1 as f64)) * stepf;
                    let wz_v =
                        voxel_z as f64 + (dz1 as f64 + t * (dz2 as f64 - dz1 as f64)) * stepf;

                    // Local position relative to chunk center (what the GPU sees)
                    edge_verts[edge_idx] = Vec3::new(
                        (wx_v - center.x as f64) as f32,
                        (wy_v - center.y as f64) as f32,
                        (wz_v - center.z as f64) as f32,
                    );

                    // Gradient normal via central differences on the density grid.
                    // Grid indices are in cell-space (divide world offset by step).
                    let xi_v = ((wx_v - min_voxel.x as f64) / stepf).round() as i64;
                    let yi_v = ((wy_v - min_voxel.y as f64) / stepf).round() as i64;
                    let zi_v = ((wz_v - min_voxel.z as f64) / stepf).round() as i64;

                    let gxp = (xi_v + 1).clamp(0, gx as i64 - 1) as usize;
                    let gxn = (xi_v - 1).clamp(0, gx as i64 - 1) as usize;
                    let gyp = (yi_v + 1).clamp(0, gy as i64 - 1) as usize;
                    let gyn = (yi_v - 1).clamp(0, gy as i64 - 1) as usize;
                    let gzp = (zi_v + 1).clamp(0, gz as i64 - 1) as usize;
                    let gzn = (zi_v - 1).clamp(0, gz as i64 - 1) as usize;
                    let yi_c = yi_v.clamp(0, gy as i64 - 1) as usize;
                    let xi_c = xi_v.clamp(0, gx as i64 - 1) as usize;
                    let zi_c = zi_v.clamp(0, gz as i64 - 1) as usize;

                    let ngx =
                        density_grid[g_idx(gxp, yi_c, zi_c)] - density_grid[g_idx(gxn, yi_c, zi_c)];
                    let ngy =
                        density_grid[g_idx(xi_c, gyp, zi_c)] - density_grid[g_idx(xi_c, gyn, zi_c)];
                    let ngz =
                        density_grid[g_idx(xi_c, yi_c, gzp)] - density_grid[g_idx(xi_c, yi_c, gzn)];

                    // Gradient points "into" the solid; negate for outward-facing normal
                    let n = Vec3::new(-ngx, -ngy, -ngz);
                    let len = n.dot(n).sqrt();
                    edge_norms[edge_idx] = if len > 1e-4 {
                        n / len
                    } else {
                        Vec3::new(0.0, 1.0, 0.0)
                    };

                    // Color: use the solid side of the edge (d >= 0 → solid)
                    let (c1, c2) = EDGE_CONNECTIONS[edge_idx];
                    edge_colors[edge_idx] = if d[c1] >= 0.0 {
                        let (dx1, dy1, dz1) = OFFSETS[c1];
                        color_grid[g_idx(xi0 + dx1, yi0 + dy1, zi0 + dz1)]
                    } else {
                        let (dx2, dy2, dz2) = OFFSETS[c2];
                        color_grid[g_idx(xi0 + dx2, yi0 + dy2, zi0 + dz2)]
                    };
                }

                // Build triangles from the marching cubes triangle table
                let tri_table = TRIANGLE_TABLE[cube_idx];
                let mut ti = 0;
                while tri_table[ti] != -1 {
                    let e1 = tri_table[ti] as usize;
                    let e2 = tri_table[ti + 1] as usize;
                    let e3 = tri_table[ti + 2] as usize;

                    let i1 = mesh.add_vertex(Vertex::with_color(
                        edge_verts[e1],
                        edge_norms[e1],
                        edge_colors[e1],
                    ));
                    let i2 = mesh.add_vertex(Vertex::with_color(
                        edge_verts[e2],
                        edge_norms[e2],
                        edge_colors[e2],
                    ));
                    let i3 = mesh.add_vertex(Vertex::with_color(
                        edge_verts[e3],
                        edge_norms[e3],
                        edge_colors[e3],
                    ));
                    mesh.add_triangle(crate::mesh::Triangle::new(i1, i2, i3));

                    ti += 3;
                }
            }
        }
    }

    (mesh, center)
}
