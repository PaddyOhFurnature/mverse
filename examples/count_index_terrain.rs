//! Count how many blocks in index actually have terrain

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::{AIR, GRASS, DIRT, STONE};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Count Terrain in Index ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    let mut world = ContinuousWorld::new(center, 100.0).expect("Failed");
    
    // Query ENTIRE bounds to get ALL blocks
    let query = AABB {
        min: [center[0] - 200.0, center[1] - 200.0, center[2] - 200.0],
        max: [center[0] + 200.0, center[1] + 200.0, center[2] + 200.0],
    };
    
    println!("Querying all blocks in 200m radius...");
    let blocks = world.query_range(query);
    println!("Got {} blocks\n", blocks.len());
    
    let mut blocks_with_terrain = 0;
    let mut total_grass = 0;
    let mut total_dirt = 0;
    let mut total_stone = 0;
    
    for block in &blocks {
        let mut has_terrain = false;
        for voxel in block.voxels.iter() {
            if *voxel == GRASS { total_grass += 1; has_terrain = true; }
            else if *voxel == DIRT { total_dirt += 1; has_terrain = true; }
            else if *voxel == STONE { total_stone += 1; has_terrain = true; }
        }
        if has_terrain {
            blocks_with_terrain += 1;
        }
    }
    
    println!("Results:");
    println!("  Blocks with terrain: {}", blocks_with_terrain);
    println!("  Total GRASS: {}", total_grass);
    println!("  Total DIRT: {}", total_dirt);
    println!("  Total STONE: {}", total_stone);
    
    if blocks_with_terrain > 0 {
        println!("\n✓ TERRAIN EXISTS IN INDEX!");
    } else {
        println!("\n❌ NO TERRAIN IN ANY BLOCKS!");
        println!("Problem: Blocks generated without terrain voxels.");
    }
}
