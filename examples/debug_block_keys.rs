//! Debug block key generation vs storage

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Block Key Debug ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    println!("Center ECEF: ({:.6}, {:.6}, {:.6})", center[0], center[1], center[2]);
    println!("Block size: 8.0m\n");
    
    // Create world (this pre-generates 7803 terrain blocks)
    println!("Creating world...");
    let mut world = ContinuousWorld::new(center, 100.0).expect("Failed");
    println!();
    
    // Now query a single block at center
    // The key generation in query_range should produce the same ECEF as what was generated
    
    // Manually calculate what block key SHOULD be for the center point
    let block_size = 8.0;
    let key_x = (center[0] / block_size).floor() * block_size;
    let key_y = (center[1] / block_size).floor() * block_size;
    let key_z = (center[2] / block_size).floor() * block_size;
    
    println!("Expected block key for center:");
    println!("  ECEF: ({:.6}, {:.6}, {:.6})", key_x, key_y, key_z);
    
    // Now query and see what we actually get
    use metaverse_core::spatial_index::AABB;
    let tiny_query = AABB {
        min: [key_x, key_y, key_z],
        max: [key_x + block_size, key_y + block_size, key_z + block_size],
    };
    
    let blocks = world.query_range(tiny_query);
    println!("\nQuery returned {} blocks", blocks.len());
    
    if blocks.len() > 0 {
        for (i, block) in blocks.iter().take(3).enumerate() {
            println!("  Block {}: ECEF ({:.6}, {:.6}, {:.6})",
                i, block.ecef_min[0], block.ecef_min[1], block.ecef_min[2]);
        }
    } else {
        println!("  ❌ NO BLOCKS FOUND!");
        println!("\nThis means the block key calculation is wrong.");
    }
}
