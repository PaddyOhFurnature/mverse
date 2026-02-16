//! Check if retrieved blocks have terrain

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::{AIR, GRASS, DIRT, STONE};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Retrieved Block Debug ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    let mut world = ContinuousWorld::new(center, 100.0).expect("Failed");
    
    // Query center block
    let block_size = 8.0;
    let key_x = (center[0] / block_size).floor() * block_size;
    let key_y = (center[1] / block_size).floor() * block_size;
    let key_z = (center[2] / block_size).floor() * block_size;
    
    let query = AABB {
        min: [key_x, key_y, key_z],
        max: [key_x + block_size, key_y + block_size, key_z + block_size],
    };
    
    let blocks = world.query_range(query);
    println!("Retrieved {} blocks\n", blocks.len());
    
    if blocks.len() > 0 {
        let block = &blocks[0];
        let mut air = 0;
        let mut grass = 0;
        let mut dirt = 0;
        let mut stone = 0;
        let mut other = 0;
        
        for voxel in block.voxels.iter() {
            if *voxel == AIR { air += 1; }
            else if *voxel == GRASS { grass += 1; }
            else if *voxel == DIRT { dirt += 1; }
            else if *voxel == STONE { stone += 1; }
            else { other += 1; }
        }
        
        println!("Block voxel breakdown:");
        println!("  AIR: {}", air);
        println!("  GRASS: {}", grass);
        println!("  DIRT: {}", dirt);
        println!("  STONE: {}", stone);
        println!("  OTHER: {}", other);
        println!("  Total: {}", air + grass + dirt + stone + other);
        
        if grass > 0 || dirt > 0 || stone > 0 {
            println!("\n✓ TERRAIN IN RETRIEVED BLOCK!");
        } else {
            println!("\n❌ NO TERRAIN - block was regenerated without terrain");
        }
    }
}
