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

/// Generate line segments for quad-sphere tile outlines
///
/// Creates lines along tile edges projected onto the sphere surface.
/// - `depth`: Chunk depth level (0 = 6 tiles, 1 = 24, 2 = 96, etc.)
/// - `face_colors`: Optional array of 6 colors for each cube face (if None, uses default colors)
///
/// Returns (vertices, indices) where indices form line segments (pairs of vertices).
pub fn generate_tile_outlines(depth: u8, face_colors: Option<[Vec3; 6]>) -> (Vec<Vertex>, Vec<u32>) {
    use crate::chunks::{ChunkId, chunk_corners_ecef};
    
    let colors = face_colors.unwrap_or([
        Vec3::new(1.0, 0.0, 0.0), // Face 0: Red
        Vec3::new(0.0, 1.0, 0.0), // Face 1: Green
        Vec3::new(0.0, 0.0, 1.0), // Face 2: Blue
        Vec3::new(1.0, 1.0, 0.0), // Face 3: Yellow
        Vec3::new(1.0, 0.0, 1.0), // Face 4: Magenta
        Vec3::new(0.0, 1.0, 1.0), // Face 5: Cyan
    ]);
    
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Generate all chunk IDs at this depth
    let tiles_per_face = 4_usize.pow(depth as u32);
    
    for face in 0..6 {
        let face_color = colors[face as usize];
        
        // Generate all possible paths iteratively
        let all_paths = generate_all_paths(depth);
        
        for path in all_paths {
            let chunk_id = ChunkId { face, path };
            let corners = chunk_corners_ecef(&chunk_id);
            
            // Convert ECEF corners to f32 Vec3
            let c0 = Vec3::new(corners[0].x as f32, corners[0].y as f32, corners[0].z as f32);
            let c1 = Vec3::new(corners[1].x as f32, corners[1].y as f32, corners[1].z as f32);
            let c2 = Vec3::new(corners[2].x as f32, corners[2].y as f32, corners[2].z as f32);
            let c3 = Vec3::new(corners[3].x as f32, corners[3].y as f32, corners[3].z as f32);
            
            // Add vertices for the 4 corners
            let base_idx = vertices.len() as u32;
            for &pos in &[c0, c1, c2, c3] {
                let normal = pos.normalize();
                vertices.push(Vertex {
                    position: pos.to_array(),
                    normal: normal.to_array(),
                    color: [face_color.x, face_color.y, face_color.z, 1.0],
                });
            }
            
            // Add line segments for the 4 edges
            // Edge order: 0->1, 1->2, 2->3, 3->0
            indices.extend_from_slice(&[
                base_idx, base_idx + 1,
                base_idx + 1, base_idx + 2,
                base_idx + 2, base_idx + 3,
                base_idx + 3, base_idx,
            ]);
        }
    }
    
    (vertices, indices)
}

/// Generate all possible quadtree paths at given depth iteratively
fn generate_all_paths(depth: u8) -> Vec<Vec<u8>> {
    if depth == 0 {
        return vec![Vec::new()];
    }
    
    let mut paths = vec![Vec::new()];
    
    for _ in 0..depth {
        let mut new_paths = Vec::new();
        for path in paths {
            for child in 0..4 {
                let mut new_path = path.clone();
                new_path.push(child);
                new_paths.push(new_path);
            }
        }
        paths = new_paths;
    }
    
    paths
}

/// Generate a terrain patch for a chunk
///
/// Creates a subdivided quad mesh on the sphere surface for the given chunk.
/// The quad is subdivided into a grid to follow the sphere's curvature smoothly.
/// - `chunk_id`: The chunk to generate a patch for
/// - `subdivisions`: Number of grid divisions per side (e.g., 16 = 16×16 = 256 quads)
/// - `color`: Color for the patch (e.g., green for terrain)
///
/// Returns (vertices, indices) for the patch mesh.
pub fn generate_chunk_patch(chunk_id: &crate::chunks::ChunkId, subdivisions: u32, color: Vec3) -> (Vec<Vertex>, Vec<u32>) {
    use crate::chunks::chunk_corners_ecef;
    
    let corners = chunk_corners_ecef(chunk_id);
    
    // Convert to Vec3
    let c0 = Vec3::new(corners[0].x as f32, corners[0].y as f32, corners[0].z as f32);
    let c1 = Vec3::new(corners[1].x as f32, corners[1].y as f32, corners[1].z as f32);
    let c2 = Vec3::new(corners[2].x as f32, corners[2].y as f32, corners[2].z as f32);
    let c3 = Vec3::new(corners[3].x as f32, corners[3].y as f32, corners[3].z as f32);
    
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Generate grid vertices
    for row in 0..=subdivisions {
        let v = row as f32 / subdivisions as f32; // 0 to 1
        
        for col in 0..=subdivisions {
            let u = col as f32 / subdivisions as f32; // 0 to 1
            
            // Bilinear interpolation between corners
            let p0 = c0.lerp(c1, u);
            let p1 = c3.lerp(c2, u);
            let pos = p0.lerp(p1, v);
            
            // Project onto sphere surface at constant radius
            // Handle case where interpolation passes near origin
            let earth_radius = WGS84_A as f32;
            let len = pos.length();
            let projected = if len > 1.0 {
                pos.normalize() * earth_radius
            } else {
                // Fallback: if too close to origin, just use the up direction
                Vec3::Z * earth_radius
            };
            
            let normal = projected.normalize();
            
            vertices.push(Vertex {
                position: projected.to_array(),
                normal: normal.to_array(),
                color: [color.x, color.y, color.z, 1.0],
            });
        }
    }
    
    // Generate indices for triangles
    for row in 0..subdivisions {
        for col in 0..subdivisions {
            let base = row * (subdivisions + 1) + col;
            let next_row = base + subdivisions + 1;
            
            // Two triangles per quad
            indices.push(base);
            indices.push(next_row);
            indices.push(base + 1);
            
            indices.push(base + 1);
            indices.push(next_row);
            indices.push(next_row + 1);
        }
    }
    
    (vertices, indices)
}

/// Generate terrain patches for multiple chunks
///
/// - `depth`: Chunk depth level
/// - `subdivisions`: Grid subdivisions per chunk
/// - `color`: Color for all patches
///
/// Returns combined (vertices, indices) for all patches.
pub fn generate_terrain_patches(depth: u8, subdivisions: u32, color: Vec3) -> (Vec<Vertex>, Vec<u32>) {
    use crate::chunks::ChunkId;
    
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    let all_paths = generate_all_paths(depth);
    
    for face in 0..6 {
        for path in &all_paths {
            let chunk_id = ChunkId { face, path: path.clone() };
            let (vertices, indices) = generate_chunk_patch(&chunk_id, subdivisions, color);
            
            // Offset indices by current vertex count
            let vertex_offset = all_vertices.len() as u32;
            all_vertices.extend(vertices);
            all_indices.extend(indices.iter().map(|&i| i + vertex_offset));
        }
    }
    
    (all_vertices, all_indices)
}

/// Generate a building mesh from OSM data
///
/// Creates an extruded prism from a building footprint.
/// - `footprint`: GPS coordinates of building corners (lat, lon)
/// - `elevation_m`: Base elevation above WGS84 ellipsoid
/// - `height_m`: Building height
/// - `color`: Color for the building
///
/// Returns (vertices, indices) for the building mesh.
pub fn generate_building(
    footprint: &[(f64, f64)],
    elevation_m: f64,
    height_m: f64,
    color: Vec3,
) -> (Vec<Vertex>, Vec<u32>) {
    use crate::coordinates::{gps_to_ecef, GpsPos};
    
    if footprint.len() < 3 {
        return (Vec::new(), Vec::new());
    }
    
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Convert footprint to ECEF at base elevation
    let base_positions: Vec<Vec3> = footprint
        .iter()
        .map(|&(lat, lon)| {
            let gps = GpsPos {
                lat_deg: lat,
                lon_deg: lon,
                elevation_m,
            };
            let ecef = gps_to_ecef(&gps);
            Vec3::new(ecef.x as f32, ecef.y as f32, ecef.z as f32)
        })
        .collect();
    
    // Convert footprint to ECEF at top elevation
    let top_positions: Vec<Vec3> = footprint
        .iter()
        .map(|&(lat, lon)| {
            let gps = GpsPos {
                lat_deg: lat,
                lon_deg: lon,
                elevation_m: elevation_m + height_m,
            };
            let ecef = gps_to_ecef(&gps);
            Vec3::new(ecef.x as f32, ecef.y as f32, ecef.z as f32)
        })
        .collect();
    
    let n = footprint.len();
    
    // Add base vertices
    for pos in &base_positions {
        let normal = pos.normalize();
        vertices.push(Vertex {
            position: pos.to_array(),
            normal: normal.to_array(),
            color: [color.x, color.y, color.z, 1.0],
        });
    }
    
    // Add top vertices
    for pos in &top_positions {
        let normal = pos.normalize();
        vertices.push(Vertex {
            position: pos.to_array(),
            normal: normal.to_array(),
            color: [color.x, color.y, color.z, 1.0],
        });
    }
    
    // Generate wall faces (sides)
    for i in 0..n {
        let next = (i + 1) % n;
        let base_i = i as u32;
        let base_next = next as u32;
        let top_i = (n + i) as u32;
        let top_next = (n + next) as u32;
        
        // Two triangles per wall face
        indices.extend_from_slice(&[
            base_i, top_i, base_next,
            base_next, top_i, top_next,
        ]);
    }
    
    // Generate base face (triangulate as fan from first vertex)
    if n >= 3 {
        for i in 1..(n - 1) {
            indices.extend_from_slice(&[0, i as u32 + 1, i as u32]);
        }
    }
    
    // Generate top face (triangulate as fan from first vertex)
    if n >= 3 {
        let top_offset = n as u32;
        for i in 1..(n - 1) {
            indices.extend_from_slice(&[
                top_offset,
                top_offset + i as u32,
                top_offset + i as u32 + 1,
            ]);
        }
    }
    
    (vertices, indices)
}

/// Generate meshes for multiple buildings from OSM data
///
/// Returns combined (vertices, indices) for all buildings.
pub fn generate_buildings_from_osm(osm_data: &crate::osm::OsmData, color: Vec3) -> (Vec<Vertex>, Vec<u32>) {
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for building in &osm_data.buildings {
        // Use building centroid elevation as base
        let elevation = building.polygon.first()
            .map(|pos| pos.elevation_m)
            .unwrap_or(0.0);
        
        let footprint: Vec<(f64, f64)> = building.polygon
            .iter()
            .map(|pos| (pos.lat_deg, pos.lon_deg))
            .collect();
        
        let (vertices, indices) = generate_building(
            &footprint,
            elevation,
            building.height_m,
            color,
        );
        
        // Offset indices by current vertex count
        let vertex_offset = all_vertices.len() as u32;
        all_vertices.extend(vertices);
        all_indices.extend(indices.iter().map(|&i| i + vertex_offset));
    }
    
    (all_vertices, all_indices)
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
    
    #[test]
    fn test_chunk_patch_generation() {
        use crate::chunks::ChunkId;
        
        // Generate a patch for depth-0 chunk on face 0
        let chunk_id = ChunkId { face: 0, path: vec![] };
        let (vertices, indices) = generate_chunk_patch(&chunk_id, 4, glam::Vec3::new(0.0, 1.0, 0.0));
        
        // Should have (subdivisions+1)^2 vertices
        assert_eq!(vertices.len(), 5 * 5);
        
        // Should have subdivisions^2 * 6 indices (2 triangles per quad)
        assert_eq!(indices.len(), 4 * 4 * 6);
        
        // All vertices should be at Earth radius (we project onto constant radius sphere)
        let expected_radius = WGS84_A as f32;
        for (i, vertex) in vertices.iter().enumerate() {
            let len = (vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1]
                + vertex.position[2] * vertex.position[2]).sqrt();
            let diff = (len - expected_radius).abs();
            if i == 0 || diff > 1.0 {
                println!("Vertex {}: len={}, expected={}, diff={}", i, len, expected_radius, diff);
            }
            // Should be exact since we normalize and multiply by exact radius
            assert!(diff < 10.0, "Vertex {} radius {} differs from expected {} by {}", i, len, expected_radius, diff);
        }
    }
    
    #[test]
    fn test_terrain_patches_multiple_chunks() {
        let (vertices, indices) = generate_terrain_patches(0, 4, glam::Vec3::new(0.0, 1.0, 0.0));
        
        // Depth 0 = 6 chunks, each with (4+1)^2 = 25 vertices
        assert_eq!(vertices.len(), 6 * 25);
        
        // Each chunk has 4*4*6 = 96 indices
        assert_eq!(indices.len(), 6 * 96);
        
        // All indices should be valid
        for &idx in &indices {
            assert!((idx as usize) < vertices.len());
        }
    }
    
    #[test]
    fn test_building_generation() {
        // Simple square building
        let footprint = vec![
            (-27.47, 153.02), // SW corner
            (-27.47, 153.03), // SE corner
            (-27.46, 153.03), // NE corner
            (-27.46, 153.02), // NW corner
        ];
        
        let (vertices, indices) = generate_building(&footprint, 0.0, 30.0, glam::Vec3::new(0.5, 0.5, 0.5));
        
        // Should have 8 vertices (4 base + 4 top)
        assert_eq!(vertices.len(), 8);
        
        // Should have indices for walls (4 walls × 6 indices) + base (2 triangles × 3) + top (2 triangles × 3)
        // = 24 + 6 + 6 = 36
        assert_eq!(indices.len(), 36);
        
        // All indices should be valid
        for &idx in &indices {
            assert!((idx as usize) < vertices.len());
        }
    }
}
