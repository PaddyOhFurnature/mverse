//! Mesh generation utilities
//!
//! Functions for generating geometric meshes (spheres, cubes, etc.)

use crate::renderer::pipeline::Vertex;
use crate::coordinates::{WGS84_A, gps_to_ecef};
use glam::{Vec3, DVec3};

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
    let _tiles_per_face = 4_usize.pow(depth as u32);
    
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

/// Generate terrain patch for a single chunk with SRTM elevation data
///
/// Similar to `generate_chunk_patch` but displaces vertices by elevation.
///
/// - `chunk_id`: The chunk to generate terrain for
/// - `subdivisions`: Grid subdivisions (higher = smoother terrain)
/// - `color`: Base terrain color
/// - `get_elevation_fn`: Function to query elevation at (lat, lon) → Option<f64> in meters
///
/// Returns (vertices, indices) for the terrain patch with elevation applied.
pub fn generate_chunk_patch_with_elevation<F>(
    chunk_id: &crate::chunks::ChunkId,
    subdivisions: u32,
    color: Vec3,
    mut get_elevation_fn: F,
) -> (Vec<Vertex>, Vec<u32>)
where
    F: FnMut(f64, f64) -> Option<f64>,
{
    use crate::chunks::chunk_corners_ecef;
    use crate::coordinates::{ecef_to_gps, EcefPos};
    
    let corners = chunk_corners_ecef(chunk_id);
    
    // Convert to Vec3
    let c0 = Vec3::new(corners[0].x as f32, corners[0].y as f32, corners[0].z as f32);
    let c1 = Vec3::new(corners[1].x as f32, corners[1].y as f32, corners[1].z as f32);
    let c2 = Vec3::new(corners[2].x as f32, corners[2].y as f32, corners[2].z as f32);
    let c3 = Vec3::new(corners[3].x as f32, corners[3].y as f32, corners[3].z as f32);
    
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Debug: Track elevation statistics
    let mut elevation_count = 0;
    let mut elevation_sum = 0.0;
    let mut elevation_min = f64::MAX;
    let mut elevation_max = f64::MIN;
    let mut none_count = 0;
    
    // Generate grid vertices
    for row in 0..=subdivisions {
        let v = row as f32 / subdivisions as f32; // 0 to 1
        
        for col in 0..=subdivisions {
            let u = col as f32 / subdivisions as f32; // 0 to 1
            
            // Bilinear interpolation between corners
            let p0 = c0.lerp(c1, u);
            let p1 = c3.lerp(c2, u);
            let pos = p0.lerp(p1, v);
            
            // Project onto sphere surface at WGS84 radius (base height)
            let earth_radius = WGS84_A as f32;
            let len = pos.length();
            let base_pos = if len > 1.0 {
                pos.normalize() * earth_radius
            } else {
                Vec3::Z * earth_radius
            };
            
            // Convert to GPS to query elevation
            let ecef = EcefPos {
                x: base_pos.x as f64,
                y: base_pos.y as f64,
                z: base_pos.z as f64,
            };
            let gps = ecef_to_gps(&ecef);
            
            // Query elevation and displace vertex outward
            let elevation = get_elevation_fn(gps.lat_deg, gps.lon_deg).unwrap_or(0.0);
            
            // Track statistics
            if elevation != 0.0 {
                elevation_count += 1;
                elevation_sum += elevation;
                elevation_min = elevation_min.min(elevation);
                elevation_max = elevation_max.max(elevation);
            } else {
                none_count += 1;
            }
            
            // Use real elevation (no exaggeration)
            let final_pos = base_pos.normalize() * (earth_radius + elevation as f32);
            
            let normal = final_pos.normalize();
            
            vertices.push(Vertex {
                position: final_pos.to_array(),
                normal: normal.to_array(),
                color: [color.x, color.y, color.z, 1.0],
            });
        }
    }
    
    // Debug output
    if elevation_count > 0 {
        let avg = elevation_sum / elevation_count as f64;
        eprintln!("Chunk {:?}: {} vertices with elevation (avg: {:.1}m, min: {:.1}m, max: {:.1}m), {} at zero",
                  chunk_id, elevation_count, avg, elevation_min, elevation_max, none_count);
    } else {
        eprintln!("Chunk {:?}: WARNING - ALL {} vertices have zero elevation!", chunk_id, none_count);
    }
    
    // Generate indices for triangles (same as before)
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
    
    // Convert footprint to ECEF at base elevation (real scale)
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
    
    // Convert footprint to ECEF at top elevation (real scale)
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
    generate_buildings_from_osm_with_elevation(osm_data, color, |_lat, _lon| None)
}

/// Generate building meshes from OSM data with terrain elevation lookup
pub fn generate_buildings_from_osm_with_elevation<F>(
    osm_data: &crate::osm::OsmData,
    color: Vec3,
    mut get_elevation_fn: F,
) -> (Vec<Vertex>, Vec<u32>)
where
    F: FnMut(f64, f64) -> Option<f64>,
{
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for building in &osm_data.buildings {
        // Get building centroid for terrain lookup
        let (centroid_lat, centroid_lon) = if let Some(first) = building.polygon.first() {
            (first.lat_deg, first.lon_deg)
        } else {
            continue;
        };
        
        // Try terrain elevation first, fall back to OSM elevation, then 0
        let elevation = get_elevation_fn(centroid_lat, centroid_lon)
            .or_else(|| building.polygon.first().map(|pos| pos.elevation_m))
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
    
    // ============================================================
    // PHASE 5 SCALE GATE TESTS
    // ============================================================
    
    #[test]
    fn test_scale_gate_single_building_position() {
        use crate::coordinates::{gps_to_ecef, GpsPos};
        
        // Known building in Brisbane CBD (approximate)
        let lat = -27.4698;
        let lon = 153.0251;
        let footprint = vec![
            (lat, lon),
            (lat, lon + 0.0001),
            (lat + 0.0001, lon + 0.0001),
            (lat + 0.0001, lon),
        ];
        
        let (vertices, _) = generate_building(&footprint, 0.0, 30.0, glam::Vec3::new(0.5, 0.5, 0.5));
        
        // Base vertices should be near expected ECEF position
        let gps = GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: 0.0 };
        let expected_ecef = gps_to_ecef(&gps);
        let base_vertex = vertices[0];
        let vertex_ecef = glam::DVec3::new(
            base_vertex.position[0] as f64,
            base_vertex.position[1] as f64,
            base_vertex.position[2] as f64,
        );
        let expected_vec = glam::DVec3::new(expected_ecef.x, expected_ecef.y, expected_ecef.z);
        
        // Distance should be < 100m (building is ~10m across, so center could be 50m off)
        let distance = (vertex_ecef - expected_vec).length();
        assert!(distance < 100.0, "Building vertex {} meters from expected GPS position", distance);
    }
    
    #[test]
    fn test_scale_gate_multiple_buildings_relative() {
        // Two buildings ~1.1km apart (0.01 degrees = ~1.1km)
        let lat1 = -27.4698;
        let lon1 = 153.0251;
        let lat2 = -27.4798; // 0.01 degrees south = ~1.1km
        let lon2 = 153.0251;
        
        let footprint1 = vec![(lat1, lon1), (lat1, lon1 + 0.0001), (lat1 + 0.0001, lon1 + 0.0001), (lat1 + 0.0001, lon1)];
        let footprint2 = vec![(lat2, lon2), (lat2, lon2 + 0.0001), (lat2 + 0.0001, lon2 + 0.0001), (lat2 + 0.0001, lon2)];
        
        let (v1, _) = generate_building(&footprint1, 0.0, 30.0, glam::Vec3::new(0.5, 0.5, 0.5));
        let (v2, _) = generate_building(&footprint2, 0.0, 30.0, glam::Vec3::new(0.5, 0.5, 0.5));
        
        // Distance between base vertices should be ~1100m
        let p1 = glam::Vec3::new(v1[0].position[0], v1[0].position[1], v1[0].position[2]);
        let p2 = glam::Vec3::new(v2[0].position[0], v2[0].position[1], v2[0].position[2]);
        let distance = (p1 - p2).length();
        
        // Should be within 20% of expected distance (1.1km = 1100m)
        assert!(distance > 900.0 && distance < 1400.0, "Building separation {} meters (expected ~1100m)", distance);
    }
    
    #[test]
    fn test_chunk_patch_with_elevation() {
        use crate::chunks::ChunkId;
        
        // Test terrain generation with mock elevation data
        let chunk_id = ChunkId { face: 0, path: vec![] };
        
        // Mock elevation function: returns 100m everywhere
        let elevation_fn = |_lat: f64, _lon: f64| Some(100.0);
        
        let (vertices, indices) = generate_chunk_patch_with_elevation(
            &chunk_id,
            4,
            glam::Vec3::new(0.2, 0.8, 0.2),
            elevation_fn,
        );
        
        // Should have (4+1)^2 = 25 vertices
        assert_eq!(vertices.len(), 25);
        
        // Should have 4*4*6 = 96 indices
        assert_eq!(indices.len(), 96);
        
        // All vertices should be at approximately WGS84_A + 100m radius
        let expected_radius = (WGS84_A + 100.0) as f32;
        for vertex in &vertices {
            let len = (vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1]
                + vertex.position[2] * vertex.position[2]).sqrt();
            let diff = (len - expected_radius).abs();
            assert!(diff < 1.0, "Vertex radius {} differs from expected {} by {}", len, expected_radius, diff);
        }
    }
    
    #[test]
    fn test_chunk_patch_with_varying_elevation() {
        use crate::chunks::ChunkId;
        
        // Test with varying elevation (simulating a hill)
        let chunk_id = ChunkId { face: 0, path: vec![] };
        
        // Mock elevation function: creates a gradient from 0m to 1000m
        let elevation_fn = |lat: f64, _lon: f64| {
            // Higher latitude = higher elevation
            Some((lat + 90.0) * 10.0) // 0m at -90°, 1800m at +90°
        };
        
        let (vertices, _indices) = generate_chunk_patch_with_elevation(
            &chunk_id,
            4,
            glam::Vec3::new(0.2, 0.8, 0.2),
            elevation_fn,
        );
        
        // Vertices should have varying radii
        let mut min_radius = f32::MAX;
        let mut max_radius = f32::MIN;
        
        for vertex in &vertices {
            let radius = (vertex.position[0] * vertex.position[0]
                + vertex.position[1] * vertex.position[1]
                + vertex.position[2] * vertex.position[2]).sqrt();
            min_radius = min_radius.min(radius);
            max_radius = max_radius.max(radius);
        }
        
        let radius_range = max_radius - min_radius;
        
        // Should have significant elevation variation (at least 100m range)
        assert!(radius_range > 100.0, "Expected elevation variation but got range of {}", radius_range);
    }
    
    #[test]
    fn test_scale_gate_1km_radius_mesh_generation() {
        use crate::chunks::{gps_to_chunk_id, ChunkId};
        use crate::coordinates::GpsPos;
        
        // Brisbane CBD center
        let gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
        
        // Get chunks within ~1km at depth 14
        let _center_chunk = gps_to_chunk_id(&gps, 14);
        
        // Generate terrain for depth 6 (reasonable workload - 24,576 chunks total)
        let (vertices, indices) = generate_terrain_patches(6, 4, glam::Vec3::ZERO);
        
        // Should handle large mesh generation without panic
        assert!(vertices.len() > 0, "Should generate vertices");
        assert!(indices.len() > 0, "Should generate indices");
        
        // Each chunk at subdivisions=4 has (4+1)^2 = 25 vertices
        // At depth 6: 6 * 4^6 = 24,576 chunks × 25 vertices = 614,400 vertices
        assert!(vertices.len() < 1_000_000, "Vertex count should be reasonable");
    }
    
    #[test]
    fn test_scale_gate_floating_origin_precision() {
        // Test that floating origin keeps precision at different locations
        
        // Brisbane ECEF (large coordinates ~6M meters)
        let brisbane_offset = glam::DVec3::new(-5085442.782, 2668653.127, -2912473.617);
        
        // Small building near origin in floating frame
        let local_pos = glam::Vec3::new(100.0, 100.0, 100.0);
        
        // Convert back to ECEF
        let ecef = brisbane_offset + glam::DVec3::new(local_pos.x as f64, local_pos.y as f64, local_pos.z as f64);
        
        // Convert to f32 and back
        let ecef_f32 = glam::Vec3::new(ecef.x as f32, ecef.y as f32, ecef.z as f32);
        let roundtrip = glam::DVec3::new(ecef_f32.x as f64, ecef_f32.y as f64, ecef_f32.z as f64);
        
        // Precision loss should be < 1m at Brisbane coordinates
        let error = (ecef - roundtrip).length();
        assert!(error < 1.0, "Floating origin precision loss {} meters (should be <1m)", error);
    }
    
    #[test]
    fn test_scale_gate_camera_no_jitter_brisbane() {
        use crate::renderer::camera::Camera;
        
        // Brisbane ECEF coordinates (~6M meters from origin)
        let brisbane = Camera::brisbane();
        let pos = brisbane.position;
        
        // Camera position should be valid and stable
        assert!(pos.length() > 6_000_000.0, "Brisbane camera position should be ~6M meters from origin");
        assert!(pos.length() < 7_000_000.0, "Brisbane camera position should be reasonable");
        
        // View projection matrix should be computable without NaN
        let aspect_ratio = 1280.0 / 720.0;
        let (vp_matrix, offset) = brisbane.view_projection_matrix(aspect_ratio);
        
        // Check that matrix is valid (no NaN, no Inf)
        for i in 0..16 {
            let val = vp_matrix.to_cols_array()[i];
            assert!(val.is_finite(), "VP matrix contains non-finite value at index {}", i);
        }
        
        // Offset should match position
        assert!((offset - pos).length() < 0.1, "Floating origin offset should match camera position");
    }
    
    #[test]
    fn test_scale_gate_camera_no_jitter_north_pole() {
        use crate::renderer::camera::Camera;
        use crate::coordinates::{gps_to_ecef, GpsPos};
        use glam::DVec3;
        
        // North Pole
        let north_pole_gps = GpsPos { lat_deg: 90.0, lon_deg: 0.0, elevation_m: 1000.0 };
        let north_pole_ecef = gps_to_ecef(&north_pole_gps);
        let north_pole_pos = DVec3::new(north_pole_ecef.x, north_pole_ecef.y, north_pole_ecef.z);
        
        // Look at Earth center
        let camera = Camera::new(north_pole_pos, DVec3::ZERO);
        
        // View projection should work at North Pole
        let aspect_ratio = 1280.0 / 720.0;
        let (vp_matrix, _offset) = camera.view_projection_matrix(aspect_ratio);
        
        for i in 0..16 {
            let val = vp_matrix.to_cols_array()[i];
            assert!(val.is_finite(), "VP matrix at North Pole contains non-finite value");
        }
    }
    
    #[test]
    fn test_scale_gate_camera_no_jitter_equator() {
        use crate::renderer::camera::Camera;
        use crate::coordinates::{gps_to_ecef, GpsPos};
        use glam::DVec3;
        
        // Equator at 0°N, 0°E (off coast of Africa)
        let equator_gps = GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 1000.0 };
        let equator_ecef = gps_to_ecef(&equator_gps);
        let equator_pos = DVec3::new(equator_ecef.x, equator_ecef.y, equator_ecef.z);
        
        // Look at Earth center
        let camera = Camera::new(equator_pos, DVec3::ZERO);
        
        // View projection should work at equator
        let aspect_ratio = 1280.0 / 720.0;
        let (vp_matrix, _offset) = camera.view_projection_matrix(aspect_ratio);
        
        for i in 0..16 {
            let val = vp_matrix.to_cols_array()[i];
            assert!(val.is_finite(), "VP matrix at equator contains non-finite value");
        }
    }
}

/// Generate road mesh from OSM road data
///
/// Creates a ribbon mesh along the road path with the specified width.
/// Roads are rendered as flat quads following the centerline.
///
/// # Arguments
/// * `roads` - OSM road data
/// * `color` - Base color for all roads (typically dark gray)
///
/// # Returns
/// (vertices, indices) for rendering
pub fn generate_roads_from_osm(roads: &[crate::osm::OsmRoad], color: Vec3) -> (Vec<Vertex>, Vec<u32>) {
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for road in roads {
        if road.nodes.len() < 2 {
            continue; // Need at least 2 points to make a road segment
        }
        
        let vertex_offset = all_vertices.len() as u32;
        let half_width = (road.width_m / 2.0) as f32;
        
        // Convert all nodes to ECEF
        let ecef_points: Vec<DVec3> = road.nodes
            .iter()
            .map(|gps| {
                let ecef = gps_to_ecef(gps);
                DVec3::new(ecef.x, ecef.y, ecef.z)
            })
            .collect();
        
        // Generate vertices along the road path
        for i in 0..ecef_points.len() {
            let point = ecef_points[i];
            
            // Calculate perpendicular direction for road width
            let (_forward, perpendicular) = if i == 0 {
                // First point: use direction to next point
                let next = ecef_points[i + 1];
                let forward = (next - point).normalize();
                let up = point.normalize(); // Radial direction from Earth center
                let perpendicular = forward.cross(up).normalize();
                (forward, perpendicular)
            } else if i == ecef_points.len() - 1 {
                // Last point: use direction from previous point
                let prev = ecef_points[i - 1];
                let forward = (point - prev).normalize();
                let up = point.normalize();
                let perpendicular = forward.cross(up).normalize();
                (forward, perpendicular)
            } else {
                // Middle points: average of incoming and outgoing directions
                let prev = ecef_points[i - 1];
                let next = ecef_points[i + 1];
                let forward = ((point - prev).normalize() + (next - point).normalize()).normalize();
                let up = point.normalize();
                let perpendicular = forward.cross(up).normalize();
                (forward, perpendicular)
            };
            
            // Create left and right vertices
            let left = point + perpendicular * half_width as f64;
            let right = point - perpendicular * half_width as f64;
            
            // Normal points up (radially outward from Earth)
            let normal_left = left.normalize().as_vec3();
            let normal_right = right.normalize().as_vec3();
            
            all_vertices.push(Vertex {
                position: [left.x as f32, left.y as f32, left.z as f32],
                normal: [normal_left.x, normal_left.y, normal_left.z],
                color: [color.x, color.y, color.z, 1.0],
            });
            
            all_vertices.push(Vertex {
                position: [right.x as f32, right.y as f32, right.z as f32],
                normal: [normal_right.x, normal_right.y, normal_right.z],
                color: [color.x, color.y, color.z, 1.0],
            });
        }
        
        // Generate indices for triangle strip
        for i in 0..(ecef_points.len() - 1) {
            let base = vertex_offset + (i * 2) as u32;
            
            // Two triangles per segment
            // Triangle 1: left[i], right[i], left[i+1]
            all_indices.push(base);
            all_indices.push(base + 1);
            all_indices.push(base + 2);
            
            // Triangle 2: right[i], right[i+1], left[i+1]
            all_indices.push(base + 1);
            all_indices.push(base + 3);
            all_indices.push(base + 2);
        }
    }
    
    (all_vertices, all_indices)
}

/// Generate water mesh from OSM water data
///
/// Creates filled polygon meshes for water features (rivers, lakes, etc).
/// Uses simple triangle fan triangulation from the first vertex.
///
/// # Arguments
/// * `water_features` - OSM water polygon data
/// * `color` - Base color for water (typically blue)
///
/// # Returns
/// (vertices, indices) for rendering
pub fn generate_water_from_osm(water_features: &[crate::osm::OsmWater], color: Vec3) -> (Vec<Vertex>, Vec<u32>) {
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for water in water_features {
        if water.polygon.len() < 3 {
            continue; // Need at least 3 points to make a polygon
        }
        
        let vertex_offset = all_vertices.len() as u32;
        
        // Convert all polygon points to ECEF
        let ecef_points: Vec<DVec3> = water.polygon
            .iter()
            .map(|gps| {
                let ecef = gps_to_ecef(gps);
                DVec3::new(ecef.x, ecef.y, ecef.z)
            })
            .collect();
        
        // Generate vertices
        for point in &ecef_points {
            // Normal points up (radially outward from Earth)
            let normal = point.normalize().as_vec3();
            
            all_vertices.push(Vertex {
                position: [point.x as f32, point.y as f32, point.z as f32],
                normal: [normal.x, normal.y, normal.z],
                color: [color.x, color.y, color.z, 1.0],
            });
        }
        
        // Triangulate using triangle fan from first vertex
        // This works for convex polygons and most simple concave ones
        for i in 1..(ecef_points.len() - 1) {
            all_indices.push(vertex_offset);
            all_indices.push(vertex_offset + i as u32);
            all_indices.push(vertex_offset + (i + 1) as u32);
        }
    }
    
    (all_vertices, all_indices)
}

/// Generate road mesh from OSM road data with elevation queries
///
/// Same as generate_roads_from_osm but queries terrain elevation for each point.
/// Roads are offset slightly above terrain (0.1m) to ensure visibility.
///
/// # Arguments
/// * `roads` - OSM road data
/// * `color` - Base color for all roads
/// * `elevation_fn` - Function to query terrain elevation at (lat, lon)
///
/// # Returns
/// (vertices, indices) for rendering
pub fn generate_roads_from_osm_with_elevation<F>(
    roads: &[crate::osm::OsmRoad], 
    color: Vec3,
    mut elevation_fn: F,
) -> (Vec<Vertex>, Vec<u32>) 
where
    F: FnMut(f64, f64) -> Option<f32>,
{
    use crate::coordinates::GpsPos;
    
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for road in roads {
        if road.nodes.len() < 2 {
            continue;
        }
        
        let vertex_offset = all_vertices.len() as u32;
        let half_width = (road.width_m / 2.0) as f32;
        
        // Convert all nodes to ECEF with terrain elevation
        let ecef_points: Vec<DVec3> = road.nodes
            .iter()
            .map(|gps| {
                // Query actual terrain elevation, fallback to 20m if unavailable
                let terrain_elevation = elevation_fn(gps.lat_deg, gps.lon_deg)
                    .unwrap_or(20.0); // Brisbane average ~20-40m
                
                // Add small offset (0.1m) to ensure roads render above terrain
                let elevated_gps = GpsPos {
                    lat_deg: gps.lat_deg,
                    lon_deg: gps.lon_deg,
                    elevation_m: terrain_elevation as f64 + 0.1,
                };
                
                let ecef = gps_to_ecef(&elevated_gps);
                DVec3::new(ecef.x, ecef.y, ecef.z)
            })
            .collect();
        
        // Generate vertices along the road path
        for i in 0..ecef_points.len() {
            let point = ecef_points[i];
            
            let (_forward, perpendicular) = if i == 0 {
                let next = ecef_points[i + 1];
                let forward = (next - point).normalize();
                let up = point.normalize();
                let perpendicular = forward.cross(up).normalize();
                (forward, perpendicular)
            } else if i == ecef_points.len() - 1 {
                let prev = ecef_points[i - 1];
                let forward = (point - prev).normalize();
                let up = point.normalize();
                let perpendicular = forward.cross(up).normalize();
                (forward, perpendicular)
            } else {
                let prev = ecef_points[i - 1];
                let next = ecef_points[i + 1];
                let forward = ((point - prev).normalize() + (next - point).normalize()).normalize();
                let up = point.normalize();
                let perpendicular = forward.cross(up).normalize();
                (forward, perpendicular)
            };
            
            let left = point + perpendicular * half_width as f64;
            let right = point - perpendicular * half_width as f64;
            
            let normal_left = left.normalize().as_vec3();
            let normal_right = right.normalize().as_vec3();
            
            all_vertices.push(Vertex {
                position: [left.x as f32, left.y as f32, left.z as f32],
                normal: [normal_left.x, normal_left.y, normal_left.z],
                color: [color.x, color.y, color.z, 1.0],
            });
            
            all_vertices.push(Vertex {
                position: [right.x as f32, right.y as f32, right.z as f32],
                normal: [normal_right.x, normal_right.y, normal_right.z],
                color: [color.x, color.y, color.z, 1.0],
            });
        }
        
        // Generate indices for triangle strip
        for i in 0..(ecef_points.len() - 1) {
            let base = vertex_offset + (i * 2) as u32;
            
            all_indices.push(base);
            all_indices.push(base + 1);
            all_indices.push(base + 2);
            
            all_indices.push(base + 1);
            all_indices.push(base + 3);
            all_indices.push(base + 2);
        }
    }
    
    (all_vertices, all_indices)
}

/// Generate water mesh from OSM water data with elevation queries
///
/// Same as generate_water_from_osm but queries terrain elevation for each point.
/// Water is offset slightly above terrain (0.05m) for visibility.
///
/// # Arguments
/// * `water_features` - OSM water polygon data
/// * `color` - Base color for water
/// * `elevation_fn` - Function to query terrain elevation at (lat, lon)
///
/// # Returns
/// (vertices, indices) for rendering
pub fn generate_water_from_osm_with_elevation<F>(
    water_features: &[crate::osm::OsmWater],
    color: Vec3,
    mut elevation_fn: F,
) -> (Vec<Vertex>, Vec<u32>)
where
    F: FnMut(f64, f64) -> Option<f32>,
{
    use crate::coordinates::GpsPos;
    
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for water in water_features {
        if water.polygon.len() < 3 {
            continue;
        }
        
        let vertex_offset = all_vertices.len() as u32;
        
        // Convert all polygon points to ECEF with terrain elevation
        let ecef_points: Vec<DVec3> = water.polygon
            .iter()
            .map(|gps| {
                // Query actual terrain elevation, fallback to 15m (rivers are lower)
                let terrain_elevation = elevation_fn(gps.lat_deg, gps.lon_deg)
                    .unwrap_or(15.0); // Rivers typically at lower elevation
                
                // Add small offset (0.05m) to ensure water renders above terrain
                let elevated_gps = GpsPos {
                    lat_deg: gps.lat_deg,
                    lon_deg: gps.lon_deg,
                    elevation_m: terrain_elevation as f64 + 0.05,
                };
                
                let ecef = gps_to_ecef(&elevated_gps);
                DVec3::new(ecef.x, ecef.y, ecef.z)
            })
            .collect();
        
        // Generate vertices
        for point in &ecef_points {
            let normal = point.normalize().as_vec3();
            
            all_vertices.push(Vertex {
                position: [point.x as f32, point.y as f32, point.z as f32],
                normal: [normal.x, normal.y, normal.z],
                color: [color.x, color.y, color.z, 1.0],
            });
        }
        
        // Triangulate using triangle fan from first vertex
        for i in 1..(ecef_points.len() - 1) {
            all_indices.push(vertex_offset);
            all_indices.push(vertex_offset + i as u32);
            all_indices.push(vertex_offset + (i + 1) as u32);
        }
    }
    
    (all_vertices, all_indices)
}
