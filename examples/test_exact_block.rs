//! Test generation of exact block that should have terrain

use metaverse_core::procedural_generator::{ProceduralGenerator, GeneratorConfig};
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::{AIR, GRASS, DIRT, STONE};
use std::path::PathBuf;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Test Exact Block ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("metaverse");
    
    let config = GeneratorConfig {
        srtm_cache_path: cache_base.join("srtm"),
        osm_cache_path: cache_base.join("osm"),
        area_center: center_ecef,
        area_radius: 100.0,
    };
    
    let generator = ProceduralGenerator::new(config).expect("Failed");
    
    // Generate block at Z offset -1 (should span ground level)
    let block_size = 8.0;
    let z_offset = -1;
    let block_z = center_ecef.z + (z_offset as f64 * block_size);
    
    println!("Generating block:");
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", center_ecef.x, center_ecef.y, block_z);
    println!("  Should span: 3.7m to 11.7m elevation");
    println!("  Ground: 5.0m");
    println!();
    
    let block = generator.generate_block([center_ecef.x, center_ecef.y, block_z]);
    
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
    
    println!("Results:");
    println!("  AIR: {}", air);
    println!("  GRASS: {}", grass);
    println!("  DIRT: {}", dirt);
    println!("  STONE: {}", stone);
    println!("  OTHER: {}", other);
    
    if grass + dirt + stone > 0 {
        println!("\n✓ TERRAIN GENERATED!");
    } else {
        println!("\n❌ NO TERRAIN!");
        println!("Check: Is get_ground_elevation() returning Some(5.0)?");
    }
}
