/// Placeholder meshes for chunks during loading
///
/// Shows a low-poly wireframe cube while terrain generates in background.
/// Prevents visual "pop-in" and gives player feedback that chunks are loading.

use crate::chunk::ChunkId;
use crate::mesh::{Mesh, Vertex};
use glam::Vec3;
use rapier3d::prelude::*;

/// Placeholder mesh state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderStyle {
    /// No placeholder (instant pop-in when loaded)
    None,
    
    /// Wireframe cube outline
    Wireframe,
    
    /// Semi-transparent solid cube
    Translucent,
    
    /// Low-poly terrain approximation (future)
    LowPoly,
}

impl Default for PlaceholderStyle {
    fn default() -> Self {
        PlaceholderStyle::Wireframe
    }
}

/// Generate placeholder mesh for a chunk
///
/// Creates a simple visual indicator while terrain loads in background.
/// Low vertex count (< 100 vertices) for minimal overhead.
pub fn generate_placeholder_mesh(chunk_id: &ChunkId, style: PlaceholderStyle) -> Option<Mesh> {
    match style {
        PlaceholderStyle::None => None,
        PlaceholderStyle::Wireframe => Some(generate_wireframe_cube(chunk_id)),
        PlaceholderStyle::Translucent => Some(generate_translucent_cube(chunk_id)),
        PlaceholderStyle::LowPoly => {
            // Future: Generate low-poly terrain approximation
            // For now, fall back to wireframe
            Some(generate_wireframe_cube(chunk_id))
        }
    }
}

/// Generate wireframe cube (8 vertices, 12 edges as lines)
///
/// Renders as line segments, minimal GPU overhead.
/// Clearly indicates "this chunk is loading" without obscuring view.
fn generate_wireframe_cube(chunk_id: &ChunkId) -> Mesh {
    use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};
    use crate::voxel::VoxelCoord;
    
    let mut vertices = Vec::with_capacity(8);
    
    // Get chunk bounds in voxel space
    let min = chunk_id.min_voxel();
    
    // Define 8 corners of the chunk
    let corners = [
        min,                                                          // 0: min corner
        VoxelCoord::new(min.x + CHUNK_SIZE_X, min.y, min.z),        // 1: +X
        VoxelCoord::new(min.x + CHUNK_SIZE_X, min.y, min.z + CHUNK_SIZE_Z),  // 2: +X+Z
        VoxelCoord::new(min.x, min.y, min.z + CHUNK_SIZE_Z),        // 3: +Z
        VoxelCoord::new(min.x, min.y + CHUNK_SIZE_Y, min.z),        // 4: +Y
        VoxelCoord::new(min.x + CHUNK_SIZE_X, min.y + CHUNK_SIZE_Y, min.z),  // 5: +X+Y
        VoxelCoord::new(min.x + CHUNK_SIZE_X, min.y + CHUNK_SIZE_Y, min.z + CHUNK_SIZE_Z),  // 6: all
        VoxelCoord::new(min.x, min.y + CHUNK_SIZE_Y, min.z + CHUNK_SIZE_Z),  // 7: +Y+Z
    ];
    
    // Convert voxel corners to ECEF, then to local coordinates
    let center = chunk_id.center_ecef();
    
    for corner_voxel in &corners {
        let corner_ecef = corner_voxel.to_ecef();
        let local_pos = Vec3::new(
            (corner_ecef.x - center.x) as f32,
            (corner_ecef.y - center.y) as f32,
            (corner_ecef.z - center.z) as f32,
        );
        
        // Normal doesn't matter for wireframe, use up vector
        let vertex = Vertex::new(local_pos, Vec3::Y);
        vertices.push(vertex);
    }
    
    // For wireframe rendering, we'd normally use line primitives
    // But Mesh uses triangles, so we'll create degenerate triangles (lines)
    // Renderer can check for degenerate triangles and render as lines
    
    // For now, return mesh with vertices only
    // Triangles will be empty (renderer must handle this case)
    Mesh {
        vertices,
        triangles: Vec::new(),
    }
}

/// Generate translucent solid cube (12 triangles)
///
/// Semi-transparent cube, less visually intrusive than wireframe.
fn generate_translucent_cube(_chunk_id: &ChunkId) -> Mesh {
    // TODO: Implement translucent cube with actual triangles
    // For now, return empty mesh (will be skipped by renderer)
    Mesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
    }
}

/// Generate placeholder collider for a chunk
///
/// Simple box collider, prevents player from falling through loading chunks.
/// Essential for maintaining physics simulation during chunk streaming.
pub fn generate_placeholder_collider(_chunk_id: &ChunkId) -> Collider {
    use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};
    
    // Calculate chunk bounds
    let half_x = (CHUNK_SIZE_X as f32) / 2.0;
    let half_y = (CHUNK_SIZE_Y as f32) / 2.0;
    let half_z = (CHUNK_SIZE_Z as f32) / 2.0;
    
    // Create box collider centered at chunk center
    ColliderBuilder::cuboid(half_x, half_y, half_z)
        .friction(0.5)
        .restitution(0.0)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wireframe_cube_generation() {
        let chunk_id = ChunkId::new(0, 0, 0);
        let mesh = generate_placeholder_mesh(&chunk_id, PlaceholderStyle::Wireframe);
        
        assert!(mesh.is_some());
        let mesh = mesh.unwrap();
        
        // Should have 8 corners
        assert_eq!(mesh.vertices.len(), 8);
        
        // Wireframe cube has no triangles (uses line rendering)
        assert_eq!(mesh.triangles.len(), 0);
    }
    
    #[test]
    fn test_placeholder_style_default() {
        assert_eq!(PlaceholderStyle::default(), PlaceholderStyle::Wireframe);
    }
    
    #[test]
    fn test_no_placeholder() {
        let chunk_id = ChunkId::new(5, 3, 7);
        let mesh = generate_placeholder_mesh(&chunk_id, PlaceholderStyle::None);
        assert!(mesh.is_none());
    }
    
    #[test]
    fn test_placeholder_collider() {
        let chunk_id = ChunkId::new(0, 0, 0);
        let collider = generate_placeholder_collider(&chunk_id);
        
        // Collider should exist
        assert!(collider.shape().as_cuboid().is_some());
    }
}
