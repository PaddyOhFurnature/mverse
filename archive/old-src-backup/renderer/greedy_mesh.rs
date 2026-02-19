//! Greedy Meshing Algorithm
//!
//! Optimizes voxel rendering by merging adjacent same-material faces into large quads.
//! Based on: https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/
//!
//! **Algorithm:**
//! 1. For each of 6 face directions (±X, ±Y, ±Z):
//!    - Sweep through slices perpendicular to that direction
//!    - Build 2D mask of visible faces (material boundaries)
//!    - Greedily merge rectangular regions of same material
//!    - Emit one quad per merged region
//!
//! **Benefits:**
//! - Reduces triangles from millions to thousands
//! - 10-100× reduction for flat/regular terrain
//! - Massive FPS improvement

use crate::svo::{MaterialId, AIR};
use crate::renderer::pipeline::Vertex;

/// Represents a rectangular quad in a 2D slice
#[derive(Debug, Clone)]
struct Quad {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    material: MaterialId,
}

/// Greedy mesh a single 8×8×8 voxel block
///
/// Returns (vertices, indices) for rendering the block with minimal triangles.
/// Only generates faces at material boundaries (including AIR).
pub fn greedy_mesh_block(
    voxels: &[MaterialId; 512],
    block_offset: [f64; 3],
) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Process each of 6 face directions
    for axis in 0..3 {
        for direction in [false, true] {
            process_axis(&mut vertices, &mut indices, voxels, block_offset, axis, direction);
        }
    }
    
    (vertices, indices)
}

/// Process one axis direction (e.g., +X faces)
fn process_axis(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    voxels: &[MaterialId; 512],
    block_offset: [f64; 3],
    axis: usize, // 0=X, 1=Y, 2=Z
    positive: bool, // true=+axis, false=-axis
) {
    // For each slice perpendicular to axis
    for slice in 0..8 {
        // Build mask of visible faces in this slice
        let mask = extract_face_mask(voxels, axis, positive, slice);
        
        // Greedily merge mask into quads
        let quads = greedy_merge_slice(&mask);
        
        // Convert quads to vertices/indices
        for quad in quads {
            add_quad_mesh(vertices, indices, &quad, block_offset, axis, positive, slice);
        }
    }
}

/// Extract 2D mask of visible faces for one slice
///
/// Returns 8×8 grid where Some(material) means a face should be rendered
fn extract_face_mask(
    voxels: &[MaterialId; 512],
    axis: usize,
    positive: bool,
    slice: usize,
) -> [[Option<MaterialId>; 8]; 8] {
    let mut mask = [[None; 8]; 8];
    
    for j in 0..8 {
        for i in 0..8 {
            // Convert 2D (i,j) in slice to 3D (x,y,z)
            let (x, y, z) = slice_coords_to_3d(i, j, slice, axis);
            
            let voxel = voxels[voxel_index(x, y, z)];
            if voxel == AIR {
                continue; // No face for air voxels
            }
            
            // Check neighbor in face direction
            let (nx, ny, nz) = if positive {
                // Looking in positive direction (+X, +Y, or +Z)
                match axis {
                    0 => (x + 1, y, z), // +X direction
                    1 => (x, y + 1, z), // +Y direction
                    _ => (x, y, z + 1), // +Z direction
                }
            } else {
                // Looking in negative direction (-X, -Y, or -Z)
                if (axis == 0 && x == 0) || (axis == 1 && y == 0) || (axis == 2 && z == 0) {
                    // At boundary, assume neighbor is AIR (render face)
                    mask[j][i] = Some(voxel);
                    continue;
                }
                match axis {
                    0 => (x - 1, y, z),
                    1 => (x, y - 1, z),
                    _ => (x, y, z - 1),
                }
            };
            
            // Check if neighbor is out of bounds or different material
            if nx >= 8 || ny >= 8 || nz >= 8 {
                // At boundary, assume neighbor is AIR
                mask[j][i] = Some(voxel);
            } else {
                let neighbor = voxels[voxel_index(nx, ny, nz)];
                if neighbor != voxel {
                    // Different material = render face
                    mask[j][i] = Some(voxel);
                }
            }
        }
    }
    
    mask
}

/// Greedily merge 2D mask into rectangular quads
fn greedy_merge_slice(mask: &[[Option<MaterialId>; 8]; 8]) -> Vec<Quad> {
    let mut quads = Vec::new();
    let mut processed = [[false; 8]; 8];
    
    for j in 0..8 {
        for i in 0..8 {
            if processed[j][i] || mask[j][i].is_none() {
                continue;
            }
            
            let material = mask[j][i].unwrap();
            
            // Extend width (i direction) as far as possible
            let mut width = 1;
            while i + width < 8 
                && !processed[j][i + width]
                && mask[j][i + width] == Some(material)
            {
                width += 1;
            }
            
            // Extend height (j direction) as far as possible
            let mut height = 1;
            'height_loop: while j + height < 8 {
                // Check if entire row can be added
                for di in 0..width {
                    if processed[j + height][i + di] 
                        || mask[j + height][i + di] != Some(material)
                    {
                        break 'height_loop;
                    }
                }
                height += 1;
            }
            
            // Mark region as processed
            for dj in 0..height {
                for di in 0..width {
                    processed[j + dj][i + di] = true;
                }
            }
            
            // Create quad
            quads.push(Quad {
                x: i,
                y: j,
                width,
                height,
                material,
            });
        }
    }
    
    quads
}

/// Add quad mesh to vertices/indices
fn add_quad_mesh(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    quad: &Quad,
    block_offset: [f64; 3],
    axis: usize,
    positive: bool,
    slice: usize,
) {
    let base_idx = vertices.len() as u32;
    
    // Material color
    let color = material_color(quad.material);
    
    // Calculate normal vector
    let normal = calculate_normal(axis, positive);
    
    // Calculate 4 corners of quad in 3D space
    let corners = quad_corners(quad, block_offset, axis, positive, slice);
    
    // Add 4 vertices
    for corner in &corners {
        vertices.push(Vertex {
            position: *corner,
            normal,
            color,
        });
    }
    
    // Add 2 triangles (6 indices) for quad
    // CRITICAL FIX: Flip winding order to render correct face
    // Original was 0-1-2, 0-2-3 which rendered backsides
    // Now: 0-2-1, 0-3-2 to flip triangles
    indices.extend_from_slice(&[
        base_idx,
        base_idx + 2,  // FLIPPED: was 1
        base_idx + 1,  // FLIPPED: was 2
        base_idx,
        base_idx + 3,  // FLIPPED: was 2
        base_idx + 2,  // FLIPPED: was 3
    ]);
}

/// Calculate 4 corners of quad in 3D space
fn quad_corners(
    quad: &Quad,
    block_offset: [f64; 3],
    axis: usize,
    positive: bool,
    slice: usize,
) -> [[f32; 3]; 4] {
    let mut corners = [[0.0f32; 3]; 4];
    
    // Determine which coordinate is constant (slice position)
    let slice_pos = slice as f32 + if positive { 1.0 } else { 0.0 };
    
    // Map 2D quad coords to 3D based on axis
    for corner_idx in 0..4 {
        let (di, dj) = match corner_idx {
            0 => (0.0, 0.0),                                    // Bottom-left
            1 => (quad.width as f32, 0.0),                      // Bottom-right
            2 => (quad.width as f32, quad.height as f32),       // Top-right
            3 => (0.0, quad.height as f32),                     // Top-left
            _ => unreachable!(),
        };
        
        let i = quad.x as f32 + di;
        let j = quad.y as f32 + dj;
        
        // Convert slice 2D coords back to 3D
        let (x, y, z) = match axis {
            0 => (slice_pos, i, j),  // X-axis: slice in X, (i,j) = (Y,Z)
            1 => (i, slice_pos, j),  // Y-axis: slice in Y, (i,j) = (X,Z)
            _ => (i, j, slice_pos),  // Z-axis: slice in Z, (i,j) = (X,Y)
        };
        
        corners[corner_idx] = [
            (block_offset[0] + x as f64) as f32,
            (block_offset[1] + y as f64) as f32,
            (block_offset[2] + z as f64) as f32,
        ];
    }
    
    corners
}

/// Calculate normal vector for face
fn calculate_normal(axis: usize, positive: bool) -> [f32; 3] {
    // FLIPPED: Invert sign because we flipped winding order
    let sign = if positive { -1.0 } else { 1.0 };  // WAS: positive=1.0, negative=-1.0
    match axis {
        0 => [sign, 0.0, 0.0], // X-axis
        1 => [0.0, sign, 0.0], // Y-axis
        _ => [0.0, 0.0, sign], // Z-axis
    }
}

/// Map material ID to RGB color
fn material_color(material: MaterialId) -> [f32; 4] {
    use crate::svo::{STONE, DIRT, GRASS, WATER, CONCRETE, ASPHALT};
    
    match material {
        AIR => [0.0, 0.0, 0.0, 0.0],           // Transparent
        STONE => [0.5, 0.5, 0.5, 1.0],         // Gray stone
        DIRT => [0.6, 0.4, 0.2, 1.0],          // Brown dirt
        CONCRETE => [0.7, 0.7, 0.7, 1.0],      // Light gray concrete
        WATER => [0.2, 0.4, 0.8, 1.0],         // Blue water
        GRASS => [0.2, 0.8, 0.2, 1.0],         // Green grass
        ASPHALT => [0.3, 0.3, 0.3, 1.0],       // Dark gray asphalt
        MaterialId(9) => [0.9, 0.8, 0.6, 1.0], // SAND
        MaterialId(4) => [0.6, 0.3, 0.1, 1.0], // WOOD
        MaterialId(10) => [0.7, 0.3, 0.2, 1.0],// BRICK
        _ => [0.8, 0.2, 0.8, 1.0],             // Magenta for unknown
    }
}

// Helper functions

/// Convert 3D coords to 1D voxel index
#[inline]
fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    z * 64 + y * 8 + x
}

/// Convert 2D slice coords to 3D voxel coords
#[inline]
fn slice_coords_to_3d(i: usize, j: usize, slice: usize, axis: usize) -> (usize, usize, usize) {
    match axis {
        0 => (slice, i, j), // X-axis: slice in X, (i,j) = (Y,Z)
        1 => (i, slice, j), // Y-axis: slice in Y, (i,j) = (X,Z)
        _ => (i, j, slice), // Z-axis: slice in Z, (i,j) = (X,Y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svo::{GRASS, DIRT};
    
    /// Test: Empty block should produce no geometry
    #[test]
    fn test_empty_block() {
        let voxels = [AIR; 512];
        let (vertices, indices) = greedy_mesh_block(&voxels, [0.0, 0.0, 0.0]);
        assert_eq!(vertices.len(), 0);
        assert_eq!(indices.len(), 0);
    }
    
    /// Test: Single voxel should produce 6 faces (cube)
    #[test]
    fn test_single_voxel() {
        let mut voxels = [AIR; 512];
        voxels[voxel_index(4, 4, 4)] = GRASS;
        
        let (vertices, indices) = greedy_mesh_block(&voxels, [0.0, 0.0, 0.0]);
        
        // 6 faces × 4 vertices = 24 vertices
        // 6 faces × 6 indices (2 triangles) = 36 indices
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
    }
    
    /// Test: Flat plane should produce 2 quads (top and bottom)
    #[test]
    fn test_flat_plane() {
        let mut voxels = [AIR; 512];
        
        // Fill bottom layer (z=0) with grass
        for x in 0..8 {
            for y in 0..8 {
                voxels[voxel_index(x, y, 0)] = GRASS;
            }
        }
        
        let (vertices, indices) = greedy_mesh_block(&voxels, [0.0, 0.0, 0.0]);
        
        // Should merge into:
        // - 1 quad on top (8×8)
        // - 1 quad on bottom (8×8)
        // - 4 quads on sides (8×1 each)
        // Total: 6 quads = 24 vertices, 36 indices
        assert_eq!(vertices.len(), 24, "Expected 6 quads (24 vertices)");
        assert_eq!(indices.len(), 36, "Expected 6 quads (36 indices)");
    }
    
    /// Test: Two different materials should create boundary face
    #[test]
    fn test_material_boundary() {
        let mut voxels = [AIR; 512];
        
        // Bottom half: dirt
        for x in 0..8 {
            for y in 0..8 {
                for z in 0..4 {
                    voxels[voxel_index(x, y, z)] = DIRT;
                }
            }
        }
        
        // Top half: grass
        for x in 0..8 {
            for y in 0..8 {
                for z in 4..8 {
                    voxels[voxel_index(x, y, z)] = GRASS;
                }
            }
        }
        
        let (vertices, _) = greedy_mesh_block(&voxels, [0.0, 0.0, 0.0]);
        
        // Should have faces at boundary between dirt and grass
        // Plus external faces on all sides
        assert!(vertices.len() > 0, "Should generate boundary faces");
    }
}
