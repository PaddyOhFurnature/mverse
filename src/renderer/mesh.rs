//! Mesh generation utilities
//!
//! Functions for generating geometric meshes (spheres, cubes, etc.)

use crate::renderer::pipeline::Vertex;
use crate::coordinates::WGS84_A;
use glam::Vec3;

/// Generate a UV sphere mesh
///
/// Creates a sphere by subdividing latitude/longitude lines.
/// - `radius`: Sphere radius in meters
/// - `lat_divisions`: Number of latitude lines (more = rounder)
/// - `lon_divisions`: Number of longitude lines (more = rounder)
///
/// Returns (vertices, indices) where indices form triangles.
pub fn generate_uv_sphere(radius: f32, lat_divisions: u32, lon_divisions: u32) -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Generate vertices
    for lat in 0..=lat_divisions {
        let theta = lat as f32 * std::f32::consts::PI / lat_divisions as f32;
        let sin_theta = theta.sin();
        let cos_theta = theta.cos();
        
        for lon in 0..=lon_divisions {
            let phi = lon as f32 * 2.0 * std::f32::consts::PI / lon_divisions as f32;
            let sin_phi = phi.sin();
            let cos_phi = phi.cos();
            
            // Position on unit sphere, then scale by radius
            let x = sin_theta * cos_phi;
            let y = sin_theta * sin_phi;
            let z = cos_theta;
            
            let position = [x * radius, y * radius, z * radius];
            let normal = [x, y, z]; // Unit sphere normal
            let color = [1.0, 1.0, 1.0, 1.0]; // White
            
            vertices.push(Vertex {
                position,
                normal,
                color,
            });
        }
    }
    
    // Generate indices for triangles
    for lat in 0..lat_divisions {
        for lon in 0..lon_divisions {
            let first = lat * (lon_divisions + 1) + lon;
            let second = first + lon_divisions + 1;
            
            // Two triangles per quad
            indices.push(first);
            indices.push(second);
            indices.push(first + 1);
            
            indices.push(second);
            indices.push(second + 1);
            indices.push(first + 1);
        }
    }
    
    (vertices, indices)
}

/// Generate an icosphere mesh
///
/// Creates a sphere by subdividing an icosahedron.
/// - `radius`: Sphere radius in meters
/// - `subdivisions`: Number of subdivision levels (0 = 20 faces, 1 = 80, 2 = 320, etc.)
///
/// Returns (vertices, indices) where indices form triangles.
pub fn generate_icosphere(radius: f32, subdivisions: u32) -> (Vec<Vertex>, Vec<u32>) {
    // Golden ratio
    let phi = (1.0 + 5.0_f32.sqrt()) / 2.0;
    
    // Initial icosahedron vertices (12 vertices)
    let mut positions = vec![
        [-1.0,  phi,  0.0],
        [ 1.0,  phi,  0.0],
        [-1.0, -phi,  0.0],
        [ 1.0, -phi,  0.0],
        [ 0.0, -1.0,  phi],
        [ 0.0,  1.0,  phi],
        [ 0.0, -1.0, -phi],
        [ 0.0,  1.0, -phi],
        [ phi,  0.0, -1.0],
        [ phi,  0.0,  1.0],
        [-phi,  0.0, -1.0],
        [-phi,  0.0,  1.0],
    ];
    
    // Normalize initial vertices
    for pos in &mut positions {
        let len = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt();
        pos[0] /= len;
        pos[1] /= len;
        pos[2] /= len;
    }
    
    // Initial icosahedron indices (20 faces)
    let mut indices = vec![
        0, 11, 5,
        0, 5, 1,
        0, 1, 7,
        0, 7, 10,
        0, 10, 11,
        1, 5, 9,
        5, 11, 4,
        11, 10, 2,
        10, 7, 6,
        7, 1, 8,
        3, 9, 4,
        3, 4, 2,
        3, 2, 6,
        3, 6, 8,
        3, 8, 9,
        4, 9, 5,
        2, 4, 11,
        6, 2, 10,
        8, 6, 7,
        9, 8, 1,
    ];
    
    // Subdivide triangles
    for _ in 0..subdivisions {
        let mut new_indices = Vec::new();
        
        for tri in indices.chunks(3) {
            let v0 = tri[0] as usize;
            let v1 = tri[1] as usize;
            let v2 = tri[2] as usize;
            
            // Calculate midpoints
            let m01 = midpoint_normalized(&positions[v0], &positions[v1]);
            let m12 = midpoint_normalized(&positions[v1], &positions[v2]);
            let m20 = midpoint_normalized(&positions[v2], &positions[v0]);
            
            // Add new vertices
            let i01 = positions.len() as u32;
            positions.push(m01);
            let i12 = positions.len() as u32;
            positions.push(m12);
            let i20 = positions.len() as u32;
            positions.push(m20);
            
            // Create 4 new triangles
            new_indices.extend_from_slice(&[v0 as u32, i01, i20]);
            new_indices.extend_from_slice(&[v1 as u32, i12, i01]);
            new_indices.extend_from_slice(&[v2 as u32, i20, i12]);
            new_indices.extend_from_slice(&[i01, i12, i20]);
        }
        
        indices = new_indices;
    }
    
    // Convert to Vertex format
    let vertices: Vec<Vertex> = positions
        .iter()
        .map(|pos| {
            // Scale by radius
            let position = [pos[0] * radius, pos[1] * radius, pos[2] * radius];
            let normal = *pos; // Normalized position is the normal for a sphere
            let color = [1.0, 1.0, 1.0, 1.0]; // White
            
            Vertex {
                position,
                normal,
                color,
            }
        })
        .collect();
    
    (vertices, indices)
}

/// Calculate midpoint of two points and normalize
fn midpoint_normalized(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    let mid = [
        (a[0] + b[0]) / 2.0,
        (a[1] + b[1]) / 2.0,
        (a[2] + b[2]) / 2.0,
    ];
    
    let len = (mid[0] * mid[0] + mid[1] * mid[1] + mid[2] * mid[2]).sqrt();
    [mid[0] / len, mid[1] / len, mid[2] / len]
}

/// Generate Earth sphere at WGS84 radius
///
/// Uses icosphere for better triangle distribution than UV sphere.
/// Subdivision level 3 gives ~1280 triangles, which is good for visualization.
pub fn generate_earth_sphere() -> (Vec<Vertex>, Vec<u32>) {
    let radius = WGS84_A as f32; // ~6,378,137 meters
    generate_icosphere(radius, 3)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_uv_sphere_generation() {
        let (vertices, indices) = generate_uv_sphere(1.0, 10, 20);
        
        // Should have (lat_divisions+1) * (lon_divisions+1) vertices
        assert_eq!(vertices.len(), 11 * 21);
        
        // Should have lat_divisions * lon_divisions * 6 indices (2 triangles per quad)
        assert_eq!(indices.len(), 10 * 20 * 6);
        
        // All indices should be valid
        for &idx in &indices {
            assert!((idx as usize) < vertices.len());
        }
    }
    
    #[test]
    fn test_icosphere_generation() {
        let (vertices, indices) = generate_icosphere(1.0, 0);
        
        // Icosahedron has 12 vertices and 20 faces (60 indices)
        assert_eq!(vertices.len(), 12);
        assert_eq!(indices.len(), 60);
        
        // All vertices should be on unit sphere
        for vertex in &vertices {
            let len_sq = vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1]
                + vertex.position[2] * vertex.position[2];
            let len = len_sq.sqrt();
            assert!((len - 1.0).abs() < 0.01); // Within 1% of radius
        }
    }
    
    #[test]
    fn test_icosphere_subdivision() {
        let (v0, i0) = generate_icosphere(1.0, 0);
        let (v1, i1) = generate_icosphere(1.0, 1);
        let (v2, i2) = generate_icosphere(1.0, 2);
        
        // Each subdivision multiplies triangles by 4
        assert!(v1.len() > v0.len());
        assert!(i1.len() == i0.len() * 4);
        assert!(i2.len() == i1.len() * 4);
    }
    
    #[test]
    fn test_earth_sphere() {
        let (vertices, indices) = generate_earth_sphere();
        
        // Should have reasonable poly count (~1000+ triangles)
        assert!(indices.len() / 3 > 1000);
        assert!(indices.len() / 3 < 2000);
        
        // All vertices should be near WGS84_A radius
        for vertex in &vertices {
            let len_sq = vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1]
                + vertex.position[2] * vertex.position[2];
            let len = len_sq.sqrt();
            let expected = WGS84_A as f32;
            assert!((len - expected).abs() < expected * 0.01); // Within 1%
        }
    }
    
    #[test]
    fn test_sphere_normals() {
        let (vertices, _) = generate_icosphere(100.0, 2);
        
        // All normals should be unit length
        for vertex in &vertices {
            let len_sq = vertex.normal[0] * vertex.normal[0]
                + vertex.normal[1] * vertex.normal[1]
                + vertex.normal[2] * vertex.normal[2];
            let len = len_sq.sqrt();
            assert!((len - 1.0).abs() < 0.01); // Within 1%
        }
    }
}
