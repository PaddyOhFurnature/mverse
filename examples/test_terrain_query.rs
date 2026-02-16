//! Test querying terrain blocks

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::{AIR, GRASS, DIRT, STONE, ASPHALT};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Terrain Query Test ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    let mut world = ContinuousWorld::new(center, 100.0).expect("Failed to create world");
    
    println!("Querying 50m radius at ground level...");
    let query = AABB::from_center(center, 50.0);
    let blocks = world.query_range(query);
    
    println!("Got {} blocks\n", blocks.len());
    
    let mut grass = 0; let mut dirt = 0; let mut stone = 0; let mut asphalt = 0; let mut other = 0; let mut air = 0;
    
    for block in &blocks {
        for voxel in block.voxels.iter() {
            if *voxel == AIR { air += 1; }
            else if *voxel == GRASS { grass += 1; }
            else if *voxel == DIRT { dirt += 1; }
            else if *voxel == STONE { stone += 1; }
            else if *voxel == ASPHALT { asphalt += 1; }
            else { other += 1; }
        }
    }
    
    println!("Material counts:");
    println!("  AIR: {}", air);
    println!("  GRASS: {}", grass);
    println!("  DIRT: {}", dirt);
    println!("  STONE: {}", stone);
    println!("  ASPHALT: {}", asphalt);
    println!("  OTHER: {}", other);
    
    if grass > 0 || dirt > 0 || stone > 0 {
        println!("\n✓ TERRAIN FOUND!");
    } else {
        println!("\n❌ NO TERRAIN - only roads");
    }
}
