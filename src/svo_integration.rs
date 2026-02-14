/// SVO Integration with Renderer
///
/// Bridges the SVO volumetric world with the wgpu rendering pipeline.
/// Generates meshes from SVO data for a chunk and converts to GPU buffers.

use crate::svo::{SparseVoxelOctree, MaterialId};
use crate::terrain::generate_terrain_from_elevation;
use crate::osm_features::{carve_river, place_road, add_building};
use crate::mesh_generation::{generate_mesh, Mesh};
use crate::materials::{MaterialColors, apply_lighting, Color};
use crate::coordinates::{EcefPos, GpsPos};
use crate::osm::{OsmData, OsmRoad};

/// Vertex format for colored meshes [x, y, z, nx, ny, nz, r, g, b, a]
/// Matches the renderer's Vertex format for compatibility
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColoredVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4], // RGBA
}

impl ColoredVertex {
    /// Create vertex with RGB color (alpha = 1.0)
    pub fn new(position: [f32; 3], normal: [f32; 3], color: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            color: [color[0], color[1], color[2], 1.0],
        }
    }
}

/// Generate chunk mesh from SVO with OSM data
///
/// # Arguments
/// * `osm_data` - OSM buildings, roads, water features
/// * `chunk_center` - ECEF position of chunk center
/// * `chunk_size_m` - Size of chunk in meters
/// * `voxel_size_m` - Size of each voxel in meters
/// * `elevation_fn` - Function to get elevation at (lat, lon)
///
/// # Returns
/// (vertices, indices) ready for GPU upload
pub fn generate_chunk_mesh_from_svo<F>(
    osm_data: &OsmData,
    chunk_center: &EcefPos,
    chunk_size_m: f64,
    voxel_size_m: f64,
    elevation_fn: F,
) -> (Vec<ColoredVertex>, Vec<u32>)
where
    F: Fn(f64, f64) -> f64,
{
    // Calculate SVO depth based on chunk size and voxel size
    let voxels_per_side = (chunk_size_m / voxel_size_m).ceil() as u32;
    let depth = (voxels_per_side as f32).log2().ceil() as u8;
    
    println!("Generating SVO chunk: depth={}, voxels={}^3", depth, 1u32 << depth);
    
    // Create SVO
    let mut svo = SparseVoxelOctree::new(depth);
    
    // Step 1: Generate terrain from elevation data
    // TODO: Implement actual terrain generation with elevation_fn
    // For now, create flat terrain at sea level
    let size = 1u32 << depth;
    let mid_height = size / 2;
    for x in 0..size {
        for z in 0..size {
            for y in 0..mid_height {
                use crate::svo::STONE;
                svo.set_voxel(x, y, z, STONE);
            }
        }
    }
    
    println!("  Terrain generation: {} voxels set", mid_height * size * size);
    
    // Step 2: Apply OSM features via CSG
    // Buildings
    for building in &osm_data.buildings {
        add_building(&mut svo, chunk_center, building, voxel_size_m);
    }
    println!("  Added {} buildings", osm_data.buildings.len());
    
    // Roads
    for road in &osm_data.roads {
        let nodes = road.nodes.clone();
        let road_type_str = match road.road_type {
            crate::osm::RoadType::Motorway => "motorway",
            crate::osm::RoadType::Trunk => "trunk",
            crate::osm::RoadType::Primary => "primary",
            crate::osm::RoadType::Secondary => "secondary",
            crate::osm::RoadType::Tertiary => "tertiary",
            crate::osm::RoadType::Residential => "residential",
            crate::osm::RoadType::Service => "service",
            crate::osm::RoadType::Path => "path",
            crate::osm::RoadType::Cycleway => "cycleway",
            crate::osm::RoadType::Other(_) => "other",
        };
        place_road(&mut svo, chunk_center, road_type_str, &nodes, voxel_size_m);
    }
    println!("  Added {} roads", osm_data.roads.len());
    
    // Water features (rivers, lakes)
    for water in &osm_data.water {
        if !water.polygon.is_empty() {
            use crate::svo::WATER;
            carve_river(&mut svo, chunk_center, "river", &water.polygon, 20.0, voxel_size_m);
        }
    }
    println!("  Added {} water features", osm_data.water.len());
    
    // Step 3: Extract mesh using marching cubes
    let meshes = generate_mesh(&svo, 0); // LOD 0 for now
    println!("  Extracted {} material meshes", meshes.len());
    
    // Step 4: Convert to colored vertices
    let palette = MaterialColors::default_palette();
    let light_dir = [0.3, 0.8, 0.2]; // Light from above and slightly to side
    
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    
    for mesh in &meshes {
        let base_color = palette.get_color(mesh.material);
        
        // Convert mesh vertices [x,y,z,nx,ny,nz] to ColoredVertex
        let offset = all_vertices.len() as u32;
        for chunk in mesh.vertices.chunks(6) {
            if chunk.len() == 6 {
                let position = [chunk[0], chunk[1], chunk[2]];
                let normal = [chunk[3], chunk[4], chunk[5]];
                
                // Apply diffuse lighting
                let dot = normal[0] * light_dir[0] + normal[1] * light_dir[1] + normal[2] * light_dir[2];
                let diffuse = dot.max(0.0);
                let ambient = 0.3;
                let light = ambient + (1.0 - ambient) * diffuse;
                
                let color = [
                    (base_color.r as f32 / 255.0) * light,
                    (base_color.g as f32 / 255.0) * light,
                    (base_color.b as f32 / 255.0) * light,
                ];
                
                all_vertices.push(ColoredVertex::new(position, normal, color));
            }
        }
        
        // Add indices with offset
        for &idx in &mesh.indices {
            all_indices.push(idx + offset);
        }
    }
    
    println!("  Final mesh: {} vertices, {} indices", all_vertices.len(), all_indices.len());
    
    (all_vertices, all_indices)
}

/// Generate simplified mesh for testing (without full SVO pipeline)
///
/// Creates a simple colored mesh from OSM data using old approach but with new vertex format
pub fn generate_test_mesh_from_osm(osm_data: &OsmData) -> (Vec<ColoredVertex>, Vec<u32>) {
    use crate::coordinates::{gps_to_ecef, GpsPos};
    
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Generate simple building boxes with colors
    let building_color = [0.7, 0.7, 0.8]; // Light gray
    
    for building in &osm_data.buildings {
        if building.polygon.len() < 3 {
            continue;
        }
        
        let height = building.height_m as f32;
        let base_idx = vertices.len() as u32;
        
        // Get building center
        let center_lat = building.polygon.iter().map(|p| p.lat_deg).sum::<f64>() / building.polygon.len() as f64;
        let center_lon = building.polygon.iter().map(|p| p.lon_deg).sum::<f64>() / building.polygon.len() as f64;
        let elevation = building.polygon.first().map(|p| p.elevation_m).unwrap_or(0.0);
        
        // Calculate building footprint dimensions from polygon
        let min_lat = building.polygon.iter().map(|p| p.lat_deg).fold(f64::INFINITY, f64::min);
        let max_lat = building.polygon.iter().map(|p| p.lat_deg).fold(f64::NEG_INFINITY, f64::max);
        let min_lon = building.polygon.iter().map(|p| p.lon_deg).fold(f64::INFINITY, f64::min);
        let max_lon = building.polygon.iter().map(|p| p.lon_deg).fold(f64::NEG_INFINITY, f64::max);
        
        // Convert to approximate meters (rough estimate: 1 degree lat ≈ 111km, 1 degree lon ≈ 111km * cos(lat))
        let lat_meters = (max_lat - min_lat).abs() * 111000.0;
        let lon_meters = (max_lon - min_lon).abs() * 111000.0 * center_lat.to_radians().cos().abs();
        
        // Use half of the footprint dimensions (capped at reasonable sizes)
        let half_width = (lon_meters / 2.0).max(2.0).min(50.0) as f32;  // East-West
        let half_depth = (lat_meters / 2.0).max(2.0).min(50.0) as f32;  // North-South
        
        // Convert center to ECEF (base)
        let base_ecef = gps_to_ecef(&GpsPos {
            lat_deg: center_lat,
            lon_deg: center_lon,
            elevation_m: elevation,
        });
        
        // Convert center to ECEF (top)
        let top_ecef = gps_to_ecef(&GpsPos {
            lat_deg: center_lat,
            lon_deg: center_lon,
            elevation_m: elevation + height as f64,
        });
        
        // Create local coordinate system around the building
        let up = glam::Vec3::new(base_ecef.x as f32, base_ecef.y as f32, base_ecef.z as f32).normalize();
        let east = glam::Vec3::new(-base_ecef.y as f32, base_ecef.x as f32, 0.0).normalize();
        let north = up.cross(east).normalize();
        
        let base_pos = glam::Vec3::new(base_ecef.x as f32, base_ecef.y as f32, base_ecef.z as f32);
        let top_pos = glam::Vec3::new(top_ecef.x as f32, top_ecef.y as f32, top_ecef.z as f32);
        
        // 8 vertices for a box in ECEF coordinates with proper proportions
        let box_vertices = [
            // Bottom 4 corners (using calculated dimensions)
            base_pos + north * (-half_depth) + east * (-half_width),
            base_pos + north * (-half_depth) + east * half_width,
            base_pos + north * half_depth + east * half_width,
            base_pos + north * half_depth + east * (-half_width),
            // Top 4 corners
            top_pos + north * (-half_depth) + east * (-half_width),
            top_pos + north * (-half_depth) + east * half_width,
            top_pos + north * half_depth + east * half_width,
            top_pos + north * half_depth + east * (-half_width),
        ];
        
        for pos in &box_vertices {
            vertices.push(ColoredVertex::new(
                [pos.x, pos.y, pos.z],
                [0.0, 1.0, 0.0],
                building_color
            ));
        }
        
        // Add indices for box faces
        let box_indices = [
            0, 1, 2, 0, 2, 3, // Bottom
            4, 6, 5, 4, 7, 6, // Top
            0, 4, 5, 0, 5, 1, // Front
            1, 5, 6, 1, 6, 2, // Right
            2, 6, 7, 2, 7, 3, // Back
            3, 7, 4, 3, 4, 0, // Left
        ];
        
        for idx in &box_indices {
            indices.push(base_idx + idx);
        }
    }
    
    // Generate roads as simple ribbons
    let road_color = [0.3, 0.3, 0.3]; // Dark gray
    
    for road in &osm_data.roads {
        if road.nodes.len() < 2 {
            continue;
        }
        
        // Simple road width based on type
        let width = match road.road_type {
            crate::osm::RoadType::Motorway => 12.0,
            crate::osm::RoadType::Trunk => 10.0,
            crate::osm::RoadType::Primary => 8.0,
            crate::osm::RoadType::Secondary => 6.0,
            crate::osm::RoadType::Tertiary => 5.0,
            crate::osm::RoadType::Residential => 4.0,
            crate::osm::RoadType::Service => 3.0,
            _ => 3.0,
        };
        
        // Convert nodes to ECEF and create road segments
        for i in 0..road.nodes.len() - 1 {
            let node1 = &road.nodes[i];
            let node2 = &road.nodes[i + 1];
            
            let pos1_ecef = gps_to_ecef(&GpsPos {
                lat_deg: node1.lat_deg,
                lon_deg: node1.lon_deg,
                elevation_m: node1.elevation_m + 0.5, // Slightly above ground
            });
            
            let pos2_ecef = gps_to_ecef(&GpsPos {
                lat_deg: node2.lat_deg,
                lon_deg: node2.lon_deg,
                elevation_m: node2.elevation_m + 0.5,
            });
            
            let p1 = glam::Vec3::new(pos1_ecef.x as f32, pos1_ecef.y as f32, pos1_ecef.z as f32);
            let p2 = glam::Vec3::new(pos2_ecef.x as f32, pos2_ecef.y as f32, pos2_ecef.z as f32);
            
            // Create perpendicular vector for road width
            let forward = (p2 - p1).normalize();
            let up = p1.normalize(); // Radial direction from Earth center
            let right = forward.cross(up).normalize();
            
            let half_width = width / 2.0;
            let base_idx = vertices.len() as u32;
            
            // Four corners of road segment
            vertices.push(ColoredVertex::new(
                [(p1 - right * half_width).x, (p1 - right * half_width).y, (p1 - right * half_width).z],
                [up.x, up.y, up.z],
                road_color
            ));
            vertices.push(ColoredVertex::new(
                [(p1 + right * half_width).x, (p1 + right * half_width).y, (p1 + right * half_width).z],
                [up.x, up.y, up.z],
                road_color
            ));
            vertices.push(ColoredVertex::new(
                [(p2 + right * half_width).x, (p2 + right * half_width).y, (p2 + right * half_width).z],
                [up.x, up.y, up.z],
                road_color
            ));
            vertices.push(ColoredVertex::new(
                [(p2 - right * half_width).x, (p2 - right * half_width).y, (p2 - right * half_width).z],
                [up.x, up.y, up.z],
                road_color
            ));
            
            // Two triangles for the segment
            indices.extend_from_slice(&[
                base_idx, base_idx + 1, base_idx + 2,
                base_idx, base_idx + 2, base_idx + 3,
            ]);
        }
    }
    
    // Generate water as simple polygons
    let water_color = [0.2, 0.5, 0.8]; // Blue
    
    for water in &osm_data.water {
        if water.polygon.len() < 3 {
            continue;
        }
        
        // Simple fan triangulation from first vertex
        let base_idx = vertices.len() as u32;
        
        // Convert all polygon points to ECEF
        for point in &water.polygon {
            let pos_ecef = gps_to_ecef(&GpsPos {
                lat_deg: point.lat_deg,
                lon_deg: point.lon_deg,
                elevation_m: point.elevation_m, // At water level
            });
            
            let p = glam::Vec3::new(pos_ecef.x as f32, pos_ecef.y as f32, pos_ecef.z as f32);
            let up = p.normalize();
            
            vertices.push(ColoredVertex::new(
                [p.x, p.y, p.z],
                [up.x, up.y, up.z],
                water_color
            ));
        }
        
        // Fan triangulation: connect all vertices to first vertex
        for i in 1..water.polygon.len() - 1 {
            indices.extend_from_slice(&[
                base_idx,
                base_idx + i as u32,
                base_idx + i as u32 + 1,
            ]);
        }
    }
    
    println!("Generated test mesh: {} buildings, {} roads, {} water features = {} vertices, {} indices",
        osm_data.buildings.len(), osm_data.roads.len(), osm_data.water.len(),
        vertices.len(), indices.len());
    
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::GpsPos;
    use crate::osm::OsmBuilding;
    
    #[test]
    fn test_colored_vertex_size() {
        // Verify vertex format matches renderer::pipeline::Vertex
        assert_eq!(std::mem::size_of::<ColoredVertex>(), 40); // 3+3+4 floats * 4 bytes
    }
    
    #[test]
    fn test_generate_test_mesh() {
        let mut osm_data = OsmData {
            buildings: Vec::new(),
            roads: Vec::new(),
            water: Vec::new(),
            parks: Vec::new(),
        };
        
        // Add a test building
        osm_data.buildings.push(OsmBuilding {
            id: 1,
            polygon: vec![
                GpsPos { lat_deg: -27.5, lon_deg: 153.0, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.5, lon_deg: 153.001, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.501, lon_deg: 153.001, elevation_m: 0.0 },
            ],
            height_m: 10.0,
            building_type: "residential".to_string(),
            levels: 3,
        });
        
        let (vertices, indices) = generate_test_mesh_from_osm(&osm_data);
        
        // Should have vertices and indices
        assert!(vertices.len() > 0);
        assert!(indices.len() > 0);
        
        // Indices should be in multiples of 3 (triangles)
        assert_eq!(indices.len() % 3, 0);
    }
}
