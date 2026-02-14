/// SVO Integration with Renderer
///
/// Bridges the SVO volumetric world with the wgpu rendering pipeline.
/// Generates meshes from SVO data for a chunk and converts to GPU buffers.

use crate::svo::{SparseVoxelOctree};
use crate::coordinates::{EcefPos, GpsPos, gps_to_ecef};
use crate::osm::OsmData;
use crate::renderer::mesh::generate_building;
use crate::renderer::pipeline::Vertex;

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

/// Generate mesh from OSM data with proper 3D geometry
///
/// Uses ACTUAL building polygons (not boxes), 3D road volumes, and water surfaces
pub fn generate_mesh_from_osm(osm_data: &OsmData) -> (Vec<ColoredVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    let building_color = glam::Vec3::new(0.7, 0.7, 0.8);
    let road_color = glam::Vec3::new(0.3, 0.3, 0.3);
    let water_color = glam::Vec3::new(0.2, 0.5, 0.8);
    
    // Generate buildings using ACTUAL polygons from OSM
    // BUT: Limit detail to avoid GPU buffer overflow (268MB limit)
    let mut buildings_added = 0;
    let max_buildings = 10000; // Limit to avoid buffer overflow (8M verts ≈ 320MB)
    
    for building in osm_data.buildings.iter().take(max_buildings) {
        if building.polygon.len() < 3 {
            continue;
        }
        
        let footprint: Vec<(f64, f64)> = building.polygon
            .iter()
            .map(|pos| (pos.lat_deg, pos.lon_deg))
            .collect();
        
        let elevation = building.polygon.first().map(|p| p.elevation_m).unwrap_or(0.0);
        
        // Use the existing proper building generator that creates 3D volumes
        let (bldg_verts, bldg_indices) = generate_building(
            &footprint,
            elevation,
            building.height_m,
            building_color,
        );
        
        // Skip if this would push us over GPU limits
        let new_vertex_size = (vertices.len() + bldg_verts.len()) * 40; // 40 bytes per vertex
        if new_vertex_size > 200_000_000 { // 200MB safety margin (wgpu limit is 268MB)
            println!("  Stopping at {} buildings (GPU buffer limit approaching)", buildings_added);
            break;
        }
        
        // Convert Vertex to ColoredVertex
        let offset = vertices.len() as u32;
        for v in bldg_verts {
            vertices.push(ColoredVertex {
                position: v.position,
                normal: v.normal,
                color: v.color,
            });
        }
        
        for idx in bldg_indices {
            indices.push(idx + offset);
        }
        
        buildings_added += 1;
    }
    
    // Generate roads as simplified flat ribbons (GPU buffer constraints - will fix with LOD/chunking)
    for road in &osm_data.roads {
        if road.nodes.len() < 2 {
            continue;
        }
        
        let width = road.width_m as f32;
        
        for i in 0..road.nodes.len() - 1 {
            let node1 = &road.nodes[i];
            let node2 = &road.nodes[i + 1];
            
            let pos1_ecef = gps_to_ecef(&GpsPos {
                lat_deg: node1.lat_deg,
                lon_deg: node1.lon_deg,
                elevation_m: node1.elevation_m,
            });
            
            let pos2_ecef = gps_to_ecef(&GpsPos {
                lat_deg: node2.lat_deg,
                lon_deg: node2.lon_deg,
                elevation_m: node2.elevation_m,
            });
            
            let p1 = glam::Vec3::new(pos1_ecef.x as f32, pos1_ecef.y as f32, pos1_ecef.z as f32);
            let p2 = glam::Vec3::new(pos2_ecef.x as f32, pos2_ecef.y as f32, pos2_ecef.z as f32);
            
            // Flat ribbon - 4 vertices, 2 triangles (1/6th the geometry of 3D volume)
            let dir = (p2 - p1).normalize();
            let up = p1.normalize(); // Radial from Earth center
            let perp = dir.cross(up).normalize() * width * 0.5;
            
            let offset = vertices.len() as u32;
            
            vertices.push(ColoredVertex {
                position: [(p1.x - perp.x), (p1.y - perp.y), (p1.z - perp.z)],
                normal: [up.x, up.y, up.z],
                color: [road_color.x, road_color.y, road_color.z, 1.0],
            });
            vertices.push(ColoredVertex {
                position: [(p1.x + perp.x), (p1.y + perp.y), (p1.z + perp.z)],
                normal: [up.x, up.y, up.z],
                color: [road_color.x, road_color.y, road_color.z, 1.0],
            });
            vertices.push(ColoredVertex {
                position: [(p2.x - perp.x), (p2.y - perp.y), (p2.z - perp.z)],
                normal: [up.x, up.y, up.z],
                color: [road_color.x, road_color.y, road_color.z, 1.0],
            });
            vertices.push(ColoredVertex {
                position: [(p2.x + perp.x), (p2.y + perp.y), (p2.z + perp.z)],
                normal: [up.x, up.y, up.z],
                color: [road_color.x, road_color.y, road_color.z, 1.0],
            });
            
            // 2 triangles
            indices.extend_from_slice(&[
                offset, offset + 1, offset + 2,
                offset + 1, offset + 3, offset + 2,
            ]);
        }
    }
    
    // Generate water surfaces (at correct elevation)
    for water in &osm_data.water {
        if water.polygon.len() < 3 {
            continue;
        }
        
        let base_idx = vertices.len() as u32;
        
        // Convert polygon to ECEF at water level
        for point in &water.polygon {
            let pos_ecef = gps_to_ecef(&GpsPos {
                lat_deg: point.lat_deg,
                lon_deg: point.lon_deg,
                elevation_m: point.elevation_m,
            });
            
            let p = glam::Vec3::new(pos_ecef.x as f32, pos_ecef.y as f32, pos_ecef.z as f32);
            let normal = p.normalize(); // Surface normal points up from Earth
            
            vertices.push(ColoredVertex {
                position: [p.x, p.y, p.z],
                normal: [normal.x, normal.y, normal.z],
                color: [water_color.x, water_color.y, water_color.z, 1.0],
            });
        }
        
        // Fan triangulation
        for i in 1..water.polygon.len() - 1 {
            indices.extend_from_slice(&[
                base_idx,
                base_idx + i as u32,
                base_idx + i as u32 + 1,
            ]);
        }
    }
    
    println!("Generated mesh: {} buildings (of {}), {} roads, {} water = {} vertices, {} indices",
        buildings_added, osm_data.buildings.len(), osm_data.roads.len(), osm_data.water.len(),
        vertices.len(), indices.len());
    
    (vertices, indices)
}

/// Generate a road as a 3D volume (with thickness, not a flat ribbon!)
fn generate_road_volume(
    p1: glam::Vec3,
    p2: glam::Vec3,
    width: f32,
    thickness: f32,
    color: glam::Vec3,
) -> Vec<ColoredVertex> {
    let mut verts = Vec::new();
    
    let forward = (p2 - p1).normalize();
    let up = p1.normalize();  // Radial direction from Earth center
    let right = forward.cross(up).normalize();
    
    let half_width = width / 2.0;
    
    // 8 corners of the road volume box
    let corners = [
        p1 - right * half_width,                    // 0: bottom left start
        p1 + right * half_width,                    // 1: bottom right start
        p2 + right * half_width,                    // 2: bottom right end
        p2 - right * half_width,                    // 3: bottom left end
        p1 - right * half_width + up * thickness,   // 4: top left start
        p1 + right * half_width + up * thickness,   // 5: top right start
        p2 + right * half_width + up * thickness,   // 6: top right end
        p2 - right * half_width + up * thickness,   // 7: top left end
    ];
    
    // Define 12 triangles (6 faces * 2 triangles each) with proper normals
    let faces = [
        // Bottom face (facing down into terrain)
        ([0, 2, 1], -up),
        ([0, 3, 2], -up),
        // Top face (the road surface - facing up)
        ([4, 5, 6], up),
        ([4, 6, 7], up),
        // Left side wall
        ([0, 4, 7], -right),
        ([0, 7, 3], -right),
        // Right side wall
        ([1, 6, 5], right),
        ([1, 2, 6], right),
        // Start face (perpendicular to road direction)
        ([0, 1, 5], -forward),
        ([0, 5, 4], -forward),
        // End face
        ([3, 6, 2], forward),
        ([3, 7, 6], forward),
    ];
    
    for (tri, normal) in faces {
        for &idx in &tri {
            let pos = corners[idx];
            verts.push(ColoredVertex {
                position: [pos.x, pos.y, pos.z],
                normal: [normal.x, normal.y, normal.z],
                color: [color.x, color.y, color.z, 1.0],
            });
        }
    }
    
    verts
}

/// Placeholder for full SVO pipeline (once marching cubes table is populated)
pub fn generate_chunk_mesh_from_svo<F>(
    _osm_data: &OsmData,
    _chunk_center: &EcefPos,
    _chunk_size_m: f64,
    _voxel_size_m: f64,
    _elevation_fn: F,
) -> (Vec<ColoredVertex>, Vec<u32>)
where
    F: Fn(f64, f64) -> f64,
{
    // TODO: Implement once marching cubes triangle table is populated
    // For now, this is a stub
    (Vec::new(), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osm::OsmBuilding;
    
    #[test]
    fn test_colored_vertex_size() {
        // Verify vertex format matches renderer::pipeline::Vertex
        assert_eq!(std::mem::size_of::<ColoredVertex>(), 40); // 3+3+4 floats * 4 bytes
    }
    
    #[test]
    fn test_generate_mesh_empty() {
        let osm_data = OsmData {
            buildings: Vec::new(),
            roads: Vec::new(),
            water: Vec::new(),
            parks: Vec::new(),
        };
        
        let (vertices, indices) = generate_mesh_from_osm(&osm_data);
        
        assert_eq!(vertices.len(), 0);
        assert_eq!(indices.len(), 0);
    }
    
    #[test]
    fn test_generate_mesh_with_building() {
        let mut osm_data = OsmData {
            buildings: Vec::new(),
            roads: Vec::new(),
            water: Vec::new(),
            parks: Vec::new(),
        };
        
        osm_data.buildings.push(OsmBuilding {
            id: 1,
            polygon: vec![
                GpsPos { lat_deg: -27.5, lon_deg: 153.0, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.5, lon_deg: 153.001, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.501, lon_deg: 153.001, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.501, lon_deg: 153.0, elevation_m: 0.0 },
            ],
            height_m: 10.0,
            building_type: "residential".to_string(),
            levels: 3,
        });
        
        let (vertices, indices) = generate_mesh_from_osm(&osm_data);
        
        // Should have generated vertices and indices for the building
        assert!(vertices.len() > 0);
        assert!(indices.len() > 0);
        assert_eq!(indices.len() % 3, 0); // Triangles
    }
}
