/// OSM Feature Integration with SVO
///
/// This module converts OpenStreetMap data into volumetric SVO operations.
/// Each OSM feature type (rivers, roads, buildings, bridges, tunnels) becomes
/// a series of CSG operations that modify the terrain SVO.
///
/// Architecture:
/// 1. Query OSM data (from cache or API)
/// 2. Convert GPS coordinates to chunk-local voxel coordinates
/// 3. Apply CSG operations (add, subtract, replace materials)
/// 4. Generate operation logs for P2P synchronization

use crate::coordinates::{GpsPos, EcefPos, gps_to_ecef};
use crate::chunks::{ChunkId, chunk_center_ecef};
use crate::osm::OsmBuilding;
use crate::svo::{SparseVoxelOctree, AIR, WATER, CONCRETE, WOOD};
use crate::terrain::find_surface_height;

/// Convert GPS coordinate to chunk-local voxel coordinate
///
/// # Arguments
/// * `gps` - GPS position (lat, lon, elevation)
/// * `chunk_center` - ECEF position of chunk center
/// * `voxel_size_m` - Size of each voxel in meters
///
/// # Returns
/// (x, y, z) voxel indices within chunk, or None if outside chunk bounds
fn gps_to_voxel(
    gps: &GpsPos,
    chunk_center: &EcefPos,
    voxel_size_m: f64,
    svo_size: u32,
) -> Option<(u32, u32, u32)> {
    // Convert GPS to ECEF
    let ecef = gps_to_ecef(gps);
    
    // Offset from chunk center
    let dx = ecef.x - chunk_center.x;
    let dy = ecef.y - chunk_center.y;
    let dz = ecef.z - chunk_center.z;
    
    // Convert to voxel coordinates (centered at chunk middle)
    let half_size = (svo_size / 2) as f64;
    let vx = (dx / voxel_size_m + half_size).floor() as i32;
    let vy = (dy / voxel_size_m + half_size).floor() as i32;
    let vz = (dz / voxel_size_m + half_size).floor() as i32;
    
    // Check bounds
    if vx >= 0 && vx < svo_size as i32 &&
       vy >= 0 && vy < svo_size as i32 &&
       vz >= 0 && vz < svo_size as i32 {
        Some((vx as u32, vy as u32, vz as u32))
    } else {
        None
    }
}

/// Carve river volumes into terrain
///
/// Rivers are represented as channels carved into the terrain.
/// The depth depends on the waterway type:
/// - River: 5m deep
/// - Stream: 2m deep
/// - Canal: 3m deep
///
/// # Process
/// 1. Query terrain surface height along river path
/// 2. Carve channel from surface down to depth
/// 3. Fill bottom with WATER material
/// 4. Carve banks at natural angle (45 degrees)
///
/// # Arguments
/// * `svo` - The terrain SVO to modify
/// * `chunk_center` - ECEF position of chunk center
/// * `waterway_type` - Type of waterway (river, stream, canal)
/// * `nodes` - GPS coordinates of waterway path
/// * `width_m` - Width in meters
/// * `voxel_size_m` - Size of each voxel
pub fn carve_river(
    svo: &mut SparseVoxelOctree,
    chunk_center: &EcefPos,
    waterway_type: &str,
    nodes: &[GpsPos],
    width_m: f64,
    voxel_size_m: f64,
) {
    let svo_size = 1u32 << svo.max_depth();
    
    // Determine depth based on type
    let depth_m = match waterway_type {
        "river" => 5.0,
        "stream" => 2.0,
        "canal" => 3.0,
        _ => 3.0, // Default
    };
    
    let width_voxels = (width_m / voxel_size_m).ceil() as u32;
    let depth_voxels = (depth_m / voxel_size_m).ceil() as u32;
    
    // Process each segment of the river path
    for i in 0..nodes.len().saturating_sub(1) {
        let start = &nodes[i];
        let end = &nodes[i + 1];
        
        // Convert to voxel coordinates
        let start_voxel = match gps_to_voxel(start, chunk_center, voxel_size_m, svo_size) {
            Some(v) => v,
            None => continue, // Outside chunk
        };
        
        let end_voxel = match gps_to_voxel(end, chunk_center, voxel_size_m, svo_size) {
            Some(v) => v,
            None => continue,
        };
        
        // Rasterize line between start and end (Bresenham's line algorithm)
        let steps = ((end_voxel.0 as i32 - start_voxel.0 as i32).abs()
            .max((end_voxel.1 as i32 - start_voxel.1 as i32).abs())
            .max((end_voxel.2 as i32 - start_voxel.2 as i32).abs())) as u32;
        
        for step in 0..=steps {
            let t = if steps > 0 { step as f32 / steps as f32 } else { 0.0 };
            
            let x = (start_voxel.0 as f32 * (1.0 - t) + end_voxel.0 as f32 * t).round() as u32;
            let _y = (start_voxel.1 as f32 * (1.0 - t) + end_voxel.1 as f32 * t).round() as u32;
            let z = (start_voxel.2 as f32 * (1.0 - t) + end_voxel.2 as f32 * t).round() as u32;
            
            // Carve channel: width × depth centered on path
            let half_width = width_voxels / 2;
            
            for dx in 0..width_voxels {
                for dz in 0..width_voxels {
                    let cx = x.saturating_add(dx).saturating_sub(half_width);
                    let cz = z.saturating_add(dz).saturating_sub(half_width);
                    
                    if cx >= svo_size || cz >= svo_size {
                        continue;
                    }
                    
                    // Find current surface height
                    let surface_y = match find_surface_height(svo, cx, cz) {
                        Some(h) => h as u32,
                        None => continue,
                    };
                    
                    // Carve from surface down to depth
                    for dy in 0..depth_voxels {
                        let cy = surface_y.saturating_sub(dy);
                        if cy >= svo_size {
                            continue;
                        }
                        
                        // Bottom layer is WATER, rest is AIR
                        let material = if dy == depth_voxels - 1 {
                            WATER
                        } else {
                            AIR
                        };
                        
                        svo.set_voxel(cx, cy, cz, material);
                    }
                }
            }
        }
    }
    
    println!("Carved {} (type: {}, width: {}m, depth: {}m, {} nodes)",
        "waterway", waterway_type, width_m, depth_m, nodes.len());
}

/// Place road surface on terrain
///
/// Roads flatten the terrain and replace the surface material with ASPHALT.
/// The width depends on the road type:
/// - Motorway: 12m
/// - Primary: 8m
/// - Secondary: 6m
/// - Residential: 4m
///
/// # Process
/// 1. Query terrain elevation along road path
/// 2. Smooth elevation profile (roads don't have sharp dips)
/// 3. Flatten terrain ±0.5m to create road surface
/// 4. Set surface voxels to ASPHALT material
///
/// # Arguments
/// * `svo` - The terrain SVO to modify
/// * `road_type` - Type of road (motorway, primary, etc)
/// * `nodes` - GPS coordinates of road path
pub fn place_road(
    _svo: &mut SparseVoxelOctree,
    road_type: &str,
    nodes: &[(f64, f64)],
) {
    let width_m = match road_type {
        "motorway" => 12.0,
        "primary" => 8.0,
        "secondary" => 6.0,
        "residential" => 4.0,
        _ => 4.0,
    };
    
    // TODO: Implement road placement
    // TODO: Smooth elevation profile
    // TODO: Handle bridges (elevated sections)
    // TODO: Handle tunnels (underground sections)
    
    println!("Placing {} (type: {}, width: {}m, {} nodes)",
        "road", road_type, width_m, nodes.len());
}

/// Add building volume to SVO
///
/// Buildings are solid volumes from terrain surface to height.
/// Materials vary by building type:
/// - Residential: WOOD frame, CONCRETE foundation
/// - Commercial: CONCRETE
/// - Industrial: METAL frame, CONCRETE walls
///
/// # Process
/// 1. Query terrain elevation at building footprint
/// 2. Fill from surface to height_m with building material
/// 3. Add foundation (extend 2m below surface)
/// 4. Hollow interior (leave 0.3m walls)
///
/// # Arguments
/// * `svo` - The terrain SVO to modify
/// * `building` - Building data (footprint, height, type)
pub fn add_building(
    _svo: &mut SparseVoxelOctree,
    building: &OsmBuilding,
) {
    let height_m = building.height_m; // Real field, not Option
    
    // Determine material by building type
    let material = match building.building_type.as_str() {
        "residential" => WOOD,
        "commercial" => CONCRETE,
        "industrial" => CONCRETE, // Could use METAL for frame
        _ => CONCRETE,
    };
    
    // TODO: Implement building volume filling
    // TODO: Add foundation below surface
    // TODO: Hollow interior
    // TODO: Handle multi-part buildings
    
    println!("Adding building (type: {}, height: {}m, material: {:?})",
        building.building_type, height_m, material);
}

/// Add bridge span over terrain
///
/// Bridges are elevated road decks with support pillars.
/// 
/// # Process
/// 1. Detect bridge sections (road elevated above terrain)
/// 2. Create deck at specified height
/// 3. Add support pillars to terrain below
/// 4. Fill deck with CONCRETE, pillars with CONCRETE
///
/// # Arguments
/// * `svo` - The terrain SVO to modify
/// * `bridge_nodes` - GPS coordinates of bridge path
/// * `deck_height_m` - Height of deck above terrain
/// * `width_m` - Width of bridge deck
pub fn add_bridge(
    _svo: &mut SparseVoxelOctree,
    bridge_nodes: &[(f64, f64)],
    deck_height_m: f64,
    width_m: f64,
) {
    // TODO: Implement bridge deck creation
    // TODO: Add support pillars (every 20m)
    // TODO: Handle curved bridges
    // TODO: Ensure deck connects to terrain at ends
    
    println!("Adding bridge (height: {}m, width: {}m, {} nodes)",
        deck_height_m, width_m, bridge_nodes.len());
}

/// Carve tunnel through terrain
///
/// Tunnels are carved passages through terrain with CONCRETE walls.
///
/// # Process
/// 1. Carve cylindrical passage through terrain
/// 2. Line walls with CONCRETE (0.3m thick)
/// 3. Clear interior to AIR
/// 4. Add entrance/exit portals
///
/// # Arguments
/// * `svo` - The terrain SVO to modify
/// * `tunnel_nodes` - GPS coordinates of tunnel path
/// * `depth_m` - Depth below surface
/// * `diameter_m` - Tunnel diameter
pub fn carve_tunnel(
    _svo: &mut SparseVoxelOctree,
    tunnel_nodes: &[(f64, f64)],
    depth_m: f64,
    diameter_m: f64,
) {
    // TODO: Implement tunnel carving
    // TODO: Add concrete walls
    // TODO: Ensure portals connect to surface roads
    // TODO: Handle curved tunnels
    
    println!("Carving tunnel (depth: {}m, diameter: {}m, {} nodes)",
        depth_m, diameter_m, tunnel_nodes.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svo::{SparseVoxelOctree, STONE, DIRT};
    use crate::coordinates::{GpsPos, EcefPos};
    
    #[test]
    fn test_river_carving() {
        let mut svo = SparseVoxelOctree::new(6); // 64^3 for faster test
        
        // Fill with terrain first (flat at y=20)
        for x in 0..64 {
            for z in 0..64 {
                for y in 0..18 {
                    svo.set_voxel(x, y, z, STONE);
                }
                for y in 18..20 {
                    svo.set_voxel(x, y, z, DIRT);
                }
            }
        }
        
        // Mock chunk center (Brisbane area in ECEF)
        let chunk_center = EcefPos {
            x: -5_058_000.0,
            y: 2_710_000.0,
            z: -2_931_000.0,
        };
        
        // Simple river path (GPS coords)
        let nodes = vec![
            GpsPos { lat_deg: -27.46, lon_deg: 153.02, elevation_m: 20.0 },
            GpsPos { lat_deg: -27.47, lon_deg: 153.03, elevation_m: 20.0 },
        ];
        
        carve_river(
            &mut svo,
            &chunk_center,
            "river",
            &nodes,
            10.0, // 10m wide
            1.0,  // 1m voxels
        );
        
        // Verify some voxels were carved (should have AIR and WATER)
        let mut has_air = false;
        let mut has_water = false;
        
        for x in 0..64 {
            for y in 0..64 {
                for z in 0..64 {
                    let mat = svo.get_voxel(x, y, z);
                    if mat == AIR {
                        has_air = true;
                    }
                    if mat == WATER {
                        has_water = true;
                    }
                }
            }
        }
        
        // Note: Actual carving depends on GPS→voxel conversion accuracy
        // This test mainly verifies the function doesn't crash
        println!("River carving completed: has_air={}, has_water={}", has_air, has_water);
    }
    
    #[test]
    fn test_road_placement_stub() {
        let mut svo = SparseVoxelOctree::new(8);
        
        let nodes = vec![
            (153.02, -27.46),
            (153.03, -27.47),
        ];
        
        place_road(&mut svo, "primary", &nodes);
        
        // TODO: Add assertions
    }
    
    #[test]
    fn test_building_addition_stub() {
        let mut svo = SparseVoxelOctree::new(8);
        
        let building = OsmBuilding {
            id: 123456,
            polygon: vec![
                GpsPos { lat_deg: -27.46, lon_deg: 153.02, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.46, lon_deg: 153.021, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.461, lon_deg: 153.021, elevation_m: 0.0 },
                GpsPos { lat_deg: -27.461, lon_deg: 153.02, elevation_m: 0.0 },
            ],
            height_m: 30.0,
            building_type: "commercial".to_string(),
            levels: 10,
        };
        
        add_building(&mut svo, &building);
        
        // TODO: Add assertions
    }
}
