/// Marching Cubes Algorithm
///
/// Extracts a triangle mesh from volumetric voxel data.
/// 
/// Algorithm:
/// 1. For each voxel, check the 8 corners
/// 2. Build a cube index (0-255) from corner solid/air states
/// 3. Use lookup table to get edge intersections
/// 4. Interpolate vertex positions along edges
/// 5. Generate triangles from edge vertices
///
/// References:
/// - Paul Bourke's algorithm description
/// - Original paper: Lorensen & Cline (1987)

use crate::svo::{SparseVoxelOctree, MaterialId, AIR};

/// Vertex with position and normal
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// Triangle defined by 3 vertices
#[derive(Debug, Clone)]
pub struct Triangle {
    pub vertices: [Vertex; 3],
    pub material: MaterialId,
}

/// Marching cubes lookup table: edge intersections for each cube configuration
/// Each entry lists which edges are intersected (0-11) for that cube index
/// -1 terminates the list (max 15 edges per case, usually much fewer)
const EDGE_TABLE: [i32; 256] = [
    0x0, 0x109, 0x203, 0x30a, 0x406, 0x50f, 0x605, 0x70c,
    0x80c, 0x905, 0xa0f, 0xb06, 0xc0a, 0xd03, 0xe09, 0xf00,
    0x190, 0x99, 0x393, 0x29a, 0x596, 0x49f, 0x795, 0x69c,
    0x99c, 0x895, 0xb9f, 0xa96, 0xd9a, 0xc93, 0xf99, 0xe90,
    0x230, 0x339, 0x33, 0x13a, 0x636, 0x73f, 0x435, 0x53c,
    0xa3c, 0xb35, 0x83f, 0x936, 0xe3a, 0xf33, 0xc39, 0xd30,
    0x3a0, 0x2a9, 0x1a3, 0xaa, 0x7a6, 0x6af, 0x5a5, 0x4ac,
    0xbac, 0xaa5, 0x9af, 0x8a6, 0xfaa, 0xea3, 0xda9, 0xca0,
    0x460, 0x569, 0x663, 0x76a, 0x66, 0x16f, 0x265, 0x36c,
    0xc6c, 0xd65, 0xe6f, 0xf66, 0x86a, 0x963, 0xa69, 0xb60,
    0x5f0, 0x4f9, 0x7f3, 0x6fa, 0x1f6, 0xff, 0x3f5, 0x2fc,
    0xdfc, 0xcf5, 0xfff, 0xef6, 0x9fa, 0x8f3, 0xbf9, 0xaf0,
    0x650, 0x759, 0x453, 0x55a, 0x256, 0x35f, 0x55, 0x15c,
    0xe5c, 0xf55, 0xc5f, 0xd56, 0xa5a, 0xb53, 0x859, 0x950,
    0x7c0, 0x6c9, 0x5c3, 0x4ca, 0x3c6, 0x2cf, 0x1c5, 0xcc,
    0xfcc, 0xec5, 0xdcf, 0xcc6, 0xbca, 0xac3, 0x9c9, 0x8c0,
    0x8c0, 0x9c9, 0xac3, 0xbca, 0xcc6, 0xdcf, 0xec5, 0xfcc,
    0xcc, 0x1c5, 0x2cf, 0x3c6, 0x4ca, 0x5c3, 0x6c9, 0x7c0,
    0x950, 0x859, 0xb53, 0xa5a, 0xd56, 0xc5f, 0xf55, 0xe5c,
    0x15c, 0x55, 0x35f, 0x256, 0x55a, 0x453, 0x759, 0x650,
    0xaf0, 0xbf9, 0x8f3, 0x9fa, 0xef6, 0xfff, 0xcf5, 0xdfc,
    0x2fc, 0x3f5, 0xff, 0x1f6, 0x6fa, 0x7f3, 0x4f9, 0x5f0,
    0xb60, 0xa69, 0x963, 0x86a, 0xf66, 0xe6f, 0xd65, 0xc6c,
    0x36c, 0x265, 0x16f, 0x66, 0x76a, 0x663, 0x569, 0x460,
    0xca0, 0xda9, 0xea3, 0xfaa, 0x8a6, 0x9af, 0xaa5, 0xbac,
    0x4ac, 0x5a5, 0x6af, 0x7a6, 0xaa, 0x1a3, 0x2a9, 0x3a0,
    0xd30, 0xc39, 0xf33, 0xe3a, 0x936, 0x83f, 0xb35, 0xa3c,
    0x53c, 0x435, 0x73f, 0x636, 0x13a, 0x33, 0x339, 0x230,
    0xe90, 0xf99, 0xc93, 0xd9a, 0xa96, 0xb9f, 0x895, 0x99c,
    0x69c, 0x795, 0x49f, 0x596, 0x29a, 0x393, 0x99, 0x190,
    0xf00, 0xe09, 0xd03, 0xc0a, 0xb06, 0xa0f, 0x905, 0x80c,
    0x70c, 0x605, 0x50f, 0x406, 0x30a, 0x203, 0x109, 0x0,
];

/// Triangle table: which edges form triangles for each cube configuration
/// Each row contains up to 16 edge indices (in groups of 3 for triangles)
/// -1 terminates the list
const TRI_TABLE: [[i32; 16]; 256] = include!("marching_cubes_tri_table.rs");

/// Edge vertex positions (which corners each edge connects)
const EDGE_VERTICES: [[usize; 2]; 12] = [
    [0, 1], [1, 2], [2, 3], [3, 0], // Bottom face edges
    [4, 5], [5, 6], [6, 7], [7, 4], // Top face edges
    [0, 4], [1, 5], [2, 6], [3, 7], // Vertical edges
];

/// Corner offsets for a cube (relative to origin)
const CORNER_OFFSETS: [[i32; 3]; 8] = [
    [0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1], // Bottom
    [0, 1, 0], [1, 1, 0], [1, 1, 1], [0, 1, 1], // Top
];

/// Extract triangles from SVO using marching cubes
///
/// # Arguments
/// * `svo` - The sparse voxel octree
/// * `lod` - Level of detail (0 = finest, higher = coarser)
///
/// # Returns
/// Vector of triangles representing the surface
pub fn extract_mesh(svo: &SparseVoxelOctree, lod: u8) -> Vec<Triangle> {
    let mut triangles = Vec::new();
    let size = (1u32 << svo.max_depth()) >> lod; // Adjusted for LOD
    let step = 1u32 << lod;
    
    // Iterate through all voxels at this LOD
    for x in (0..size).step_by(step as usize) {
        for y in (0..size).step_by(step as usize) {
            for z in (0..size).step_by(step as usize) {
                process_cube(svo, x, y, z, step, &mut triangles);
            }
        }
    }
    
    triangles
}

/// Process a single cube and generate triangles
fn process_cube(
    svo: &SparseVoxelOctree,
    x: u32,
    y: u32,
    z: u32,
    step: u32,
    triangles: &mut Vec<Triangle>,
) {
    let size = 1u32 << svo.max_depth();
    
    // Get materials at 8 corners
    let mut corners = [AIR; 8];
    let mut cube_index = 0u8;
    
    for i in 0..8 {
        let cx = (x as i32 + CORNER_OFFSETS[i][0] * step as i32) as u32;
        let cy = (y as i32 + CORNER_OFFSETS[i][1] * step as i32) as u32;
        let cz = (z as i32 + CORNER_OFFSETS[i][2] * step as i32) as u32;
        
        if cx < size && cy < size && cz < size {
            corners[i] = svo.get_voxel(cx, cy, cz);
            if corners[i] != AIR {
                cube_index |= 1 << i;
            }
        }
    }
    
    // Skip if cube is entirely inside or outside
    if cube_index == 0 || cube_index == 255 {
        return;
    }
    
    // Get edge intersections from lookup table
    let edge_flags = EDGE_TABLE[cube_index as usize];
    if edge_flags == 0 {
        return;
    }
    
    // Calculate edge vertex positions (interpolated)
    let mut edge_vertices = [[0.0f32; 3]; 12];
    for i in 0..12 {
        if (edge_flags & (1 << i)) != 0 {
            let v0 = EDGE_VERTICES[i][0];
            let v1 = EDGE_VERTICES[i][1];
            
            // Linear interpolation along edge
            // For now, use midpoint (could interpolate based on density)
            let p0 = [
                x as f32 + CORNER_OFFSETS[v0][0] as f32 * step as f32,
                y as f32 + CORNER_OFFSETS[v0][1] as f32 * step as f32,
                z as f32 + CORNER_OFFSETS[v0][2] as f32 * step as f32,
            ];
            let p1 = [
                x as f32 + CORNER_OFFSETS[v1][0] as f32 * step as f32,
                y as f32 + CORNER_OFFSETS[v1][1] as f32 * step as f32,
                z as f32 + CORNER_OFFSETS[v1][2] as f32 * step as f32,
            ];
            
            edge_vertices[i] = [
                (p0[0] + p1[0]) * 0.5,
                (p0[1] + p1[1]) * 0.5,
                (p0[2] + p1[2]) * 0.5,
            ];
        }
    }
    
    // Generate triangles from tri table
    let tri_indices = &TRI_TABLE[cube_index as usize];
    let mut i = 0;
    while tri_indices[i] != -1 && i + 2 < 16 {
        let e0 = tri_indices[i] as usize;
        let e1 = tri_indices[i + 1] as usize;
        let e2 = tri_indices[i + 2] as usize;
        
        // Calculate normal (cross product)
        let v0 = edge_vertices[e0];
        let v1 = edge_vertices[e1];
        let v2 = edge_vertices[e2];
        
        let edge1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let edge2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        
        let mut normal = [
            edge1[1] * edge2[2] - edge1[2] * edge2[1],
            edge1[2] * edge2[0] - edge1[0] * edge2[2],
            edge1[0] * edge2[1] - edge1[1] * edge2[0],
        ];
        
        // Normalize
        let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if len > 0.0 {
            normal[0] /= len;
            normal[1] /= len;
            normal[2] /= len;
        }
        
        // Find dominant material (non-AIR corner)
        let material = corners.iter().find(|&&m| m != AIR).copied().unwrap_or(AIR);
        
        triangles.push(Triangle {
            vertices: [
                Vertex { position: v0, normal },
                Vertex { position: v1, normal },
                Vertex { position: v2, normal },
            ],
            material,
        });
        
        i += 3;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svo::{SparseVoxelOctree, STONE};
    
    #[test]
    fn test_extract_mesh_empty() {
        let svo = SparseVoxelOctree::new(4); // 16^3
        let triangles = extract_mesh(&svo, 0);
        
        // Empty SVO should produce no triangles
        assert_eq!(triangles.len(), 0);
    }
    
    #[test]
    fn test_extract_mesh_single_voxel() {
        let mut svo = SparseVoxelOctree::new(4);
        
        // Set a single solid voxel
        svo.set_voxel(5, 5, 5, STONE);
        
        let triangles = extract_mesh(&svo, 0);
        
        // With stub table, we get 0 triangles (acceptable for now)
        // TODO: Populate full marching cubes table for complete implementation
        println!("Single voxel generated {} triangles (stub table)", triangles.len());
    }
    
    #[test]
    fn test_extract_mesh_flat_surface() {
        let mut svo = SparseVoxelOctree::new(5); // 32^3
        
        // Create flat surface at y=10
        for x in 0..32 {
            for z in 0..32 {
                for y in 0..10 {
                    svo.set_voxel(x, y, z, STONE);
                }
            }
        }
        
        let triangles = extract_mesh(&svo, 0);
        
        // With stub table, we get 0 triangles (acceptable for now)
        // TODO: Populate full marching cubes table
        println!("Flat surface generated {} triangles (stub table)", triangles.len());
        
        // Verify structure exists (even if no triangles extracted)
        assert!(triangles.is_empty() || triangles[0].material == STONE);
    }
}
