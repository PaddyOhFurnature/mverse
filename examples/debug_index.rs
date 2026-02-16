//! Debug spatial index queries

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::{AIR, GRASS};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Spatial Index Debug ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    println!("Creating world (pre-generating terrain)...");
    let mut world = ContinuousWorld::new(center, 100.0).expect("Failed");
    
    // Query the exact center block
    println!("\nQuerying CENTER block only:");
    let tiny_query = AABB::from_center(center, 4.0); // Just 8m box
    let blocks = world.query_range(tiny_query);
    println!("  Got {} blocks", blocks.len());
    
    if blocks.len() > 0 {
        let block = &blocks[0];
        let mut grass = 0;
        let mut air = 0;
        for voxel in block.voxels.iter() {
            if *voxel == AIR { air += 1; }
            else if *voxel == GRASS { grass += 1; }
        }
        println!("  Block[0]: {} AIR, {} GRASS", air, grass);
    } else {
        println!("  ❌ NO BLOCKS FOUND!");
    }
}
