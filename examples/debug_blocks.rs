//! Debug block generation - print actual block positions and voxel counts

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};
use metaverse_core::svo::AIR;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Debug Block Generation ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    let mut world = match ContinuousWorld::new(center, 100.0) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Failed to create world: {}", e);
            return;
        }
    };
    
    println!("Query 100m radius...");
    let query = AABB::from_center(center, 100.0);
    let blocks = world.query_range(query);
    println!("Got {} blocks\n", blocks.len());
    
    let mut blocks_with_voxels = Vec::new();
    
    for block in &blocks {
        let mut non_air_count = 0;
        for voxel in block.voxels.iter() {
            if *voxel != AIR {
                non_air_count += 1;
            }
        }
        
        if non_air_count > 0 {
            blocks_with_voxels.push((block, non_air_count));
        }
    }
    
    println!("Found {} blocks with voxels\n", blocks_with_voxels.len());
    
    if blocks_with_voxels.is_empty() {
        println!("❌ NO VOXELS GENERATED!");
        return;
    }
    
    println!("Block positions (GPS coordinates):");
    for (i, (block, count)) in blocks_with_voxels.iter().enumerate().take(10) {
        let block_center_ecef = EcefPos {
            x: block.ecef_min[0] + block.size / 2.0,
            y: block.ecef_min[1] + block.size / 2.0,
            z: block.ecef_min[2] + block.size / 2.0,
        };
        let gps = ecef_to_gps(&block_center_ecef);
        
        println!("  Block {}: ({:.6}°, {:.6}°, {:.1}m) - {} voxels",
            i+1, gps.lat_deg, gps.lon_deg, gps.elevation_m, count);
    }
    
    // Calculate bounding box of all blocks with voxels
    if blocks_with_voxels.len() > 0 {
        let mut min_lat = f64::MAX;
        let mut max_lat = f64::MIN;
        let mut min_lon = f64::MAX;
        let mut max_lon = f64::MIN;
        
        for (block, _) in &blocks_with_voxels {
            let gps = ecef_to_gps(&EcefPos {
                x: block.ecef_min[0],
                y: block.ecef_min[1],
                z: block.ecef_min[2],
            });
            min_lat = min_lat.min(gps.lat_deg);
            max_lat = max_lat.max(gps.lat_deg);
            min_lon = min_lon.min(gps.lon_deg);
            max_lon = max_lon.max(gps.lon_deg);
        }
        
        println!("\nBounding box of blocks with voxels:");
        println!("  Lat: {:.6}° to {:.6}° (span: {:.6}°)",
            min_lat, max_lat, max_lat - min_lat);
        println!("  Lon: {:.6}° to {:.6}° (span: {:.6}°)",
            min_lon, max_lon, max_lon - min_lon);
        
        let center_lat = (min_lat + max_lat) / 2.0;
        let center_lon = (min_lon + max_lon) / 2.0;
        println!("  Center: ({:.6}°, {:.6}°)", center_lat, center_lon);
        println!("  Test center: ({:.6}°, {:.6}°)", TEST_LAT, TEST_LON);
        
        let lat_diff = (center_lat - TEST_LAT).abs();
        let lon_diff = (center_lon - TEST_LON).abs();
        println!("  Offset: {:.6}° lat, {:.6}° lon", lat_diff, lon_diff);
        
        if lat_diff > 0.001 || lon_diff > 0.001 {
            println!("\n⚠️  WARNING: Block positions significantly offset from test center!");
            println!("  This suggests coordinate transformation issue.");
        }
    }
}
