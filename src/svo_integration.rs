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
    // Default: use large radius to get all data (for testing/screenshots)
    generate_mesh_from_osm_filtered(osm_data, None, f64::INFINITY)
}

/// Generate mesh from OSM data with distance filtering
///
/// Only includes geometry within `max_distance_m` from `camera_pos`.
/// Uses GPU buffer limit as hard stop, not arbitrary building count.
///
/// # Arguments
/// * `osm_data` - OSM data to render
/// * `camera_pos` - Camera position in GPS coords (for distance filtering). None = no filtering
/// * `max_distance_m` - Maximum distance from camera to include geometry
pub fn generate_mesh_from_osm_filtered(
    osm_data: &OsmData,
    camera_pos: Option<&GpsPos>,
    max_distance_m: f64,
) -> (Vec<ColoredVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    let building_color = glam::Vec3::new(0.7, 0.7, 0.8);
    let road_color = glam::Vec3::new(0.3, 0.3, 0.3);
    let water_color = glam::Vec3::new(0.2, 0.5, 0.8);
    
    // Convert camera position to ECEF for distance calculations
    let camera_ecef = camera_pos.map(|gps| {
        let ecef = gps_to_ecef(gps);
        glam::DVec3::new(ecef.x, ecef.y, ecef.z)
    });
    
    // Generate buildings using ACTUAL polygons from OSM
    // NO arbitrary limit - only GPU buffer capacity matters
    let mut buildings_added = 0;
    let mut buildings_skipped_distance = 0;
    let mut buildings_skipped_buffer = 0;
    
    for building in &osm_data.buildings {
        if building.polygon.len() < 3 {
            continue;
        }
        
        // Distance filtering (if camera position provided)
        if let Some(cam_ecef) = camera_ecef {
            // Use building center for distance check
            let center_gps = building.polygon.first().unwrap();
            let building_ecef = gps_to_ecef(center_gps);
            let building_pos = glam::DVec3::new(building_ecef.x, building_ecef.y, building_ecef.z);
            
            let distance = (building_pos - cam_ecef).length();
            if distance > max_distance_m {
                buildings_skipped_distance += 1;
                continue;
            }
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
        
        // Skip if this would push us over GPU limits (268MB max per buffer)
        let new_vertex_size = (vertices.len() + bldg_verts.len()) * 40; // 40 bytes per vertex
        if new_vertex_size > 200_000_000 { // 200MB safety margin
            buildings_skipped_buffer += 1;
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
    
    // Generate roads as 3D volumes (30cm thick boxes with 6 faces)
    // NO arbitrary segment limit - only GPU buffer capacity matters
    let mut road_segments_added = 0;
    let mut road_segments_skipped_distance = 0;
    let mut road_segments_skipped_buffer = 0;
    
    for road in &osm_data.roads {
        if road.nodes.len() < 2 {
            continue;
        }
        
        let width = road.width_m as f32;
        let thickness = 0.3; // 30cm road thickness
        
        for i in 0..road.nodes.len() - 1 {
            let node1 = &road.nodes[i];
            let node2 = &road.nodes[i + 1];
            
            // Distance filtering (if camera position provided)
            if let Some(cam_ecef) = camera_ecef {
                // Use segment midpoint for distance check
                let mid_lat = (node1.lat_deg + node2.lat_deg) / 2.0;
                let mid_lon = (node1.lon_deg + node2.lon_deg) / 2.0;
                let mid_elev = (node1.elevation_m + node2.elevation_m) / 2.0;
                
                let mid_ecef = gps_to_ecef(&GpsPos {
                    lat_deg: mid_lat,
                    lon_deg: mid_lon,
                    elevation_m: mid_elev,
                });
                let mid_pos = glam::DVec3::new(mid_ecef.x, mid_ecef.y, mid_ecef.z);
                
                let distance = (mid_pos - cam_ecef).length();
                if distance > max_distance_m {
                    road_segments_skipped_distance += 1;
                    continue;
                }
            }
            
            // Check GPU buffer limit before adding
            let new_vertex_size = (vertices.len() + 36) * 40; // 36 vertices per road segment
            if new_vertex_size > 200_000_000 {
                road_segments_skipped_buffer += 1;
                continue; // Skip this segment but keep trying (maybe later roads are closer)
            }
            
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
            
            // Generate 3D road volume (not flat ribbon!)
            let road_verts = generate_road_volume(p1, p2, width, thickness, road_color);
            
            let offset = vertices.len() as u32;
            vertices.extend(road_verts);
            
            // 36 vertices form 12 triangles (6 faces × 2 triangles)
            for tri in 0..12 {
                indices.push(offset + tri * 3);
                indices.push(offset + tri * 3 + 1);
                indices.push(offset + tri * 3 + 2);
            }
            
            road_segments_added += 1;
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
    
    // Logging
    let total_buildings = osm_data.buildings.len();
    let total_roads = osm_data.roads.len();
    let total_water = osm_data.water.len();
    
    if camera_pos.is_some() && max_distance_m < f64::INFINITY {
        println!("Generated mesh with distance filtering ({}m radius):", max_distance_m);
        println!("  Buildings: {} rendered, {} skipped (distance), {} skipped (buffer), {} total",
            buildings_added, buildings_skipped_distance, buildings_skipped_buffer, total_buildings);
        println!("  Roads: {} segments, {} skipped (distance), {} skipped (buffer)",
            road_segments_added, road_segments_skipped_distance, road_segments_skipped_buffer);
        println!("  Water: {} features", total_water);
    } else {
        println!("Generated mesh (no distance filtering):");
        println!("  Buildings: {} rendered, {} skipped (buffer), {} total",
            buildings_added, buildings_skipped_buffer, total_buildings);
        println!("  Roads: {} segments, {} skipped (buffer)",
            road_segments_added, road_segments_skipped_buffer);
        println!("  Water: {} features", total_water);
    }
    println!("  Result: {} vertices, {} indices ({:.1}MB vertex buffer)",
        vertices.len(), indices.len(), (vertices.len() * 40) as f32 / 1_000_000.0);
    
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
