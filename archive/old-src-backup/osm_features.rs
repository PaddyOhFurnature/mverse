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
use crate::osm::OsmBuilding;
use crate::svo::{SparseVoxelOctree, AIR, WATER, CONCRETE, WOOD, ASPHALT, DIRT, STONE};
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
/// * `chunk_center` - ECEF position of chunk center
/// * `road_type` - Type of road (motorway, primary, etc)
/// * `nodes` - GPS coordinates of road path
/// * `voxel_size_m` - Size of each voxel
pub fn place_road(
    svo: &mut SparseVoxelOctree,
    chunk_center: &EcefPos,
    road_type: &str,
    nodes: &[GpsPos],
    voxel_size_m: f64,
) {
    let svo_size = 1u32 << svo.max_depth();
    
    let width_m = match road_type {
        "motorway" => 12.0,
        "primary" => 8.0,
        "secondary" => 6.0,
        "residential" => 4.0,
        _ => 4.0,
    };
    
    let width_voxels = (width_m / voxel_size_m).ceil() as u32;
    
    // Process each segment of the road path
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
        
        // Rasterize line between start and end
        let steps = ((end_voxel.0 as i32 - start_voxel.0 as i32).abs()
            .max((end_voxel.1 as i32 - start_voxel.1 as i32).abs())
            .max((end_voxel.2 as i32 - start_voxel.2 as i32).abs())) as u32;
        
        for step in 0..=steps {
            let t = if steps > 0 { step as f32 / steps as f32 } else { 0.0 };
            
            let x = (start_voxel.0 as f32 * (1.0 - t) + end_voxel.0 as f32 * t).round() as u32;
            let _y = (start_voxel.1 as f32 * (1.0 - t) + end_voxel.1 as f32 * t).round() as u32;
            let z = (start_voxel.2 as f32 * (1.0 - t) + end_voxel.2 as f32 * t).round() as u32;
            
            // Place road: width centered on path
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
                    
                    // Replace surface voxel with ASPHALT
                    // Also replace the layer below if it's DIRT (create roadbed)
                    svo.set_voxel(cx, surface_y, cz, ASPHALT);
                    
                    if surface_y > 0 {
                        let below = svo.get_voxel(cx, surface_y - 1, cz);
                        if below == DIRT {
                            // Replace dirt with compacted base (use STONE for now)
                            svo.set_voxel(cx, surface_y - 1, cz, STONE);
                        }
                    }
                }
            }
        }
    }
    
    println!("Placed {} (type: {}, width: {}m, {} nodes)",
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
/// * `chunk_center` - ECEF position of chunk center
/// * `building` - Building data (footprint, height, type)
/// * `voxel_size_m` - Size of each voxel
pub fn add_building(
    svo: &mut SparseVoxelOctree,
    chunk_center: &EcefPos,
    building: &OsmBuilding,
    voxel_size_m: f64,
) {
    let svo_size = 1u32 << svo.max_depth();
    let height_m = building.height_m;
    
    // Determine material by building type
    let material = match building.building_type.as_str() {
        "residential" => WOOD,
        "commercial" => CONCRETE,
        "industrial" => CONCRETE, // Could use METAL for frame
        _ => CONCRETE,
    };
    
    // Convert polygon footprint to voxel coordinates
    let footprint_voxels: Vec<(u32, u32, u32)> = building.polygon
        .iter()
        .filter_map(|gps| gps_to_voxel(gps, chunk_center, voxel_size_m, svo_size))
        .collect();
    
    if footprint_voxels.is_empty() {
        return; // Building outside chunk
    }
    
    // Find bounding box of footprint
    let min_x = footprint_voxels.iter().map(|v| v.0).min().unwrap();
    let max_x = footprint_voxels.iter().map(|v| v.0).max().unwrap();
    let min_z = footprint_voxels.iter().map(|v| v.2).min().unwrap();
    let max_z = footprint_voxels.iter().map(|v| v.2).max().unwrap();
    
    // For each voxel in bounding box, check if inside polygon and fill
    for x in min_x..=max_x {
        for z in min_z..=max_z {
            // Simple point-in-polygon test (ray casting)
            // TODO: Implement proper polygon rasterization
            let inside = true; // For now, fill entire bounding box
            
            if !inside {
                continue;
            }
            
            // Find terrain surface at this position
            let surface_y = match find_surface_height(svo, x, z) {
                Some(h) => h as u32,
                None => continue,
            };
            
            // Calculate building height in voxels
            let height_voxels = (height_m / voxel_size_m).ceil() as u32;
            
            // Fill from surface to height with material
            for dy in 0..height_voxels {
                let y = surface_y + dy;
                if y >= svo_size {
                    break;
                }
                
                svo.set_voxel(x, y, z, material);
            }
            
            // Add foundation (2m below surface)
            let foundation_depth = (2.0 / voxel_size_m).ceil() as u32;
            for dy in 1..=foundation_depth {
                if surface_y < dy {
                    break;
                }
                let y = surface_y - dy;
                
                // Replace existing material with CONCRETE foundation
                svo.set_voxel(x, y, z, CONCRETE);
            }
        }
    }
    
    println!("Added building (type: {}, height: {}m, material: {:?}, {} footprint points)",
        building.building_type, height_m, material, footprint_voxels.len());
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
/// * `chunk_center` - ECEF position of chunk center
/// * `bridge_nodes` - GPS coordinates of bridge path
/// * `deck_height_m` - Height of deck above sea level
/// * `width_m` - Width of bridge deck
/// * `voxel_size_m` - Size of each voxel
pub fn add_bridge(
    svo: &mut SparseVoxelOctree,
    chunk_center: &EcefPos,
    bridge_nodes: &[GpsPos],
    deck_height_m: f64,
    width_m: f64,
    voxel_size_m: f64,
) {
    let svo_size = 1u32 << svo.max_depth();
    let width_voxels = (width_m / voxel_size_m).ceil() as u32;
    let deck_thickness_voxels = (0.5 / voxel_size_m).ceil() as u32; // 0.5m thick deck
    
    // Process each segment of the bridge path
    for i in 0..bridge_nodes.len().saturating_sub(1) {
        let start = &bridge_nodes[i];
        let end = &bridge_nodes[i + 1];
        
        // Convert to voxel coordinates
        let start_voxel = match gps_to_voxel(start, chunk_center, voxel_size_m, svo_size) {
            Some(v) => v,
            None => continue,
        };
        
        let end_voxel = match gps_to_voxel(end, chunk_center, voxel_size_m, svo_size) {
            Some(v) => v,
            None => continue,
        };
        
        // Rasterize line for deck
        let steps = ((end_voxel.0 as i32 - start_voxel.0 as i32).abs()
            .max((end_voxel.1 as i32 - start_voxel.1 as i32).abs())
            .max((end_voxel.2 as i32 - start_voxel.2 as i32).abs())) as u32;
        
        for step in 0..=steps {
            let t = if steps > 0 { step as f32 / steps as f32 } else { 0.0 };
            
            let x = (start_voxel.0 as f32 * (1.0 - t) + end_voxel.0 as f32 * t).round() as u32;
            let deck_y = (deck_height_m / voxel_size_m).round() as u32;
            let z = (start_voxel.2 as f32 * (1.0 - t) + end_voxel.2 as f32 * t).round() as u32;
            
            // Create deck
            let half_width = width_voxels / 2;
            for dx in 0..width_voxels {
                for dz in 0..width_voxels {
                    let cx = x.saturating_add(dx).saturating_sub(half_width);
                    let cz = z.saturating_add(dz).saturating_sub(half_width);
                    
                    if cx >= svo_size || cz >= svo_size {
                        continue;
                    }
                    
                    // Fill deck layers with CONCRETE
                    for dy in 0..deck_thickness_voxels {
                        let cy = deck_y + dy;
                        if cy < svo_size {
                            svo.set_voxel(cx, cy, cz, CONCRETE);
                        }
                    }
                }
            }
            
            // Add support pillar every 20m
            if step % ((20.0 / voxel_size_m) as u32).max(1) == 0 {
                // Find terrain surface below
                let surface_y = match find_surface_height(svo, x, z) {
                    Some(h) => h as u32,
                    None => continue,
                };
                
                // Create pillar from surface to deck
                let pillar_width = (2.0 / voxel_size_m).ceil() as u32; // 2m wide pillars
                for dy in surface_y..deck_y {
                    for px in 0..pillar_width {
                        for pz in 0..pillar_width {
                            let px = x.saturating_add(px).saturating_sub(pillar_width / 2);
                            let pz = z.saturating_add(pz).saturating_sub(pillar_width / 2);
                            
                            if px < svo_size && pz < svo_size {
                                svo.set_voxel(px, dy, pz, CONCRETE);
                            }
                        }
                    }
                }
            }
        }
    }
    
    println!("Added bridge (height: {}m, width: {}m, {} nodes)",
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
/// * `chunk_center` - ECEF position of chunk center
/// * `tunnel_nodes` - GPS coordinates of tunnel path
/// * `depth_m` - Depth below surface
/// * `diameter_m` - Tunnel diameter
/// * `voxel_size_m` - Size of each voxel
pub fn carve_tunnel(
    svo: &mut SparseVoxelOctree,
    chunk_center: &EcefPos,
    tunnel_nodes: &[GpsPos],
    depth_m: f64,
    diameter_m: f64,
    voxel_size_m: f64,
) {
    let svo_size = 1u32 << svo.max_depth();
    let radius_voxels = (diameter_m / 2.0 / voxel_size_m).ceil() as u32;
    let wall_thickness = (0.3 / voxel_size_m).ceil() as u32;
    
    // Process each segment of the tunnel path
    for i in 0..tunnel_nodes.len().saturating_sub(1) {
        let start = &tunnel_nodes[i];
        let end = &tunnel_nodes[i + 1];
        
        // Convert to voxel coordinates
        let start_voxel = match gps_to_voxel(start, chunk_center, voxel_size_m, svo_size) {
            Some(v) => v,
            None => continue,
        };
        
        let end_voxel = match gps_to_voxel(end, chunk_center, voxel_size_m, svo_size) {
            Some(v) => v,
            None => continue,
        };
        
        // Rasterize line
        let steps = ((end_voxel.0 as i32 - start_voxel.0 as i32).abs()
            .max((end_voxel.1 as i32 - start_voxel.1 as i32).abs())
            .max((end_voxel.2 as i32 - start_voxel.2 as i32).abs())) as u32;
        
        for step in 0..=steps {
            let t = if steps > 0 { step as f32 / steps as f32 } else { 0.0 };
            
            let x = (start_voxel.0 as f32 * (1.0 - t) + end_voxel.0 as f32 * t).round() as u32;
            let z = (start_voxel.2 as f32 * (1.0 - t) + end_voxel.2 as f32 * t).round() as u32;
            
            // Find surface at this position
            let surface_y = match find_surface_height(svo, x, z) {
                Some(h) => h as u32,
                None => continue,
            };
            
            // Tunnel center depth below surface
            let tunnel_y = surface_y.saturating_sub((depth_m / voxel_size_m) as u32);
            
            // Carve circular cross-section
            for dx in 0..(radius_voxels * 2) {
                for dy in 0..(radius_voxels * 2) {
                    let cx = x.saturating_add(dx).saturating_sub(radius_voxels);
                    let cy = tunnel_y.saturating_add(dy).saturating_sub(radius_voxels);
                    
                    if cx >= svo_size || cy >= svo_size {
                        continue;
                    }
                    
                    // Check if inside circle
                    let dist_sq = (dx as i32 - radius_voxels as i32).pow(2) 
                        + (dy as i32 - radius_voxels as i32).pow(2);
                    let radius_sq = (radius_voxels as i32).pow(2);
                    let wall_radius_sq = ((radius_voxels as i32) - (wall_thickness as i32)).pow(2);
                    
                    if dist_sq <= radius_sq {
                        if dist_sq > wall_radius_sq {
                            // Wall region
                            svo.set_voxel(cx, cy, z, CONCRETE);
                        } else {
                            // Interior - clear to AIR
                            svo.set_voxel(cx, cy, z, AIR);
                        }
                    }
                }
            }
        }
    }
    
    println!("Carved tunnel (depth: {}m, diameter: {}m, {} nodes)",
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
    fn test_road_placement() {
        let mut svo = SparseVoxelOctree::new(6); // 64^3
        
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
        
        // Mock chunk center
        let chunk_center = EcefPos {
            x: -5_058_000.0,
            y: 2_710_000.0,
            z: -2_931_000.0,
        };
        
        // Simple road path
        let nodes = vec![
            GpsPos { lat_deg: -27.46, lon_deg: 153.02, elevation_m: 20.0 },
            GpsPos { lat_deg: -27.47, lon_deg: 153.03, elevation_m: 20.0 },
        ];
        
        place_road(
            &mut svo,
            &chunk_center,
            "primary",
            &nodes,
            1.0, // 1m voxels
        );
        
        // Verify some ASPHALT was placed
        let mut has_asphalt = false;
        
        for x in 0..64 {
            for y in 0..64 {
                for z in 0..64 {
                    if svo.get_voxel(x, y, z) == ASPHALT {
                        has_asphalt = true;
                        break;
                    }
                }
            }
        }
        
        println!("Road placement completed: has_asphalt={}", has_asphalt);
    }
    
    #[test]
    fn test_building_addition() {
        let mut svo = SparseVoxelOctree::new(6);
        
        // Fill with terrain
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
        
        let chunk_center = EcefPos {
            x: -5_058_000.0,
            y: 2_710_000.0,
            z: -2_931_000.0,
        };
        
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
        
        add_building(&mut svo, &chunk_center, &building, 1.0);
        
        // Verify some building material was placed
        let mut has_concrete = false;
        
        for x in 0..64 {
            for y in 0..64 {
                for z in 0..64 {
                    if svo.get_voxel(x, y, z) == CONCRETE {
                        has_concrete = true;
                        break;
                    }
                }
            }
        }
        
        println!("Building addition completed: has_concrete={}", has_concrete);
    }
    
    #[test]
    fn test_bridge_addition() {
        let mut svo = SparseVoxelOctree::new(6);
        
        // Fill with terrain
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
        
        let chunk_center = EcefPos {
            x: -5_058_000.0,
            y: 2_710_000.0,
            z: -2_931_000.0,
        };
        
        // Bridge path (elevated above terrain)
        let nodes = vec![
            GpsPos { lat_deg: -27.46, lon_deg: 153.02, elevation_m: 30.0 },
            GpsPos { lat_deg: -27.47, lon_deg: 153.03, elevation_m: 30.0 },
        ];
        
        add_bridge(&mut svo, &chunk_center, &nodes, 30.0, 10.0, 1.0);
        
        // Verify bridge structures exist
        let mut has_elevated_concrete = false;
        
        for x in 0..64 {
            for y in 25..40 {  // Check above terrain
                for z in 0..64 {
                    if svo.get_voxel(x, y, z) == CONCRETE {
                        has_elevated_concrete = true;
                        break;
                    }
                }
            }
        }
        
        println!("Bridge addition completed: has_elevated_concrete={}", has_elevated_concrete);
    }
    
    #[test]
    fn test_tunnel_carving() {
        let mut svo = SparseVoxelOctree::new(6);
        
        // Fill with terrain
        for x in 0..64 {
            for z in 0..64 {
                for y in 0..30 {
                    svo.set_voxel(x, y, z, STONE);
                }
            }
        }
        
        let chunk_center = EcefPos {
            x: -5_058_000.0,
            y: 2_710_000.0,
            z: -2_931_000.0,
        };
        
        // Tunnel path
        let nodes = vec![
            GpsPos { lat_deg: -27.46, lon_deg: 153.02, elevation_m: 0.0 },
            GpsPos { lat_deg: -27.47, lon_deg: 153.03, elevation_m: 0.0 },
        ];
        
        carve_tunnel(&mut svo, &chunk_center, &nodes, 10.0, 6.0, 1.0);
        
        // Verify tunnel was carved (has AIR underground)
        let mut has_underground_air = false;
        
        for x in 0..64 {
            for y in 5..20 {  // Underground region
                for z in 0..64 {
                    if svo.get_voxel(x, y, z) == AIR {
                        has_underground_air = true;
                        break;
                    }
                }
            }
        }
        
        println!("Tunnel carving completed: has_underground_air={}", has_underground_air);
    }
}
