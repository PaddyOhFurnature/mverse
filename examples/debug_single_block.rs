//! Debug a single block generation in detail

use metaverse_core::procedural_generator::{ProceduralGenerator, GeneratorConfig};
use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};
use metaverse_core::svo::{AIR, GRASS, DIRT, STONE, ASPHALT};
use std::path::PathBuf;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Single Block Debug ===\n");
    
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("metaverse");
    
    // Center at Kangaroo Point at ground level  
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    
    let config = GeneratorConfig {
        srtm_cache_path: cache_base.join("srtm"),
        osm_cache_path: cache_base.join("osm"),
        area_center: center_ecef,
        area_radius: 100.0,
    };
    
    let generator = ProceduralGenerator::new(config).expect("Failed");
    
    // Generate block at exactly ground level (0-8m)
    println!("Block ECEF min: ({:.1}, {:.1}, {:.1})",
        center_ecef.x, center_ecef.y, center_ecef.z);
    let block_gps = ecef_to_gps(&EcefPos {
        x: center_ecef.x,
        y: center_ecef.y,
        z: center_ecef.z,
    });
    println!("Block GPS (corner): ({:.6}°, {:.6}°, {:.1}m)",
        block_gps.lat_deg, block_gps.lon_deg, block_gps.elevation_m);
    println!("Block size: 8m (so covers {:.1}m to {:.1}m elevation)",
        block_gps.elevation_m, block_gps.elevation_m + 8.0);
    println!("Ground level: 5.0m (fallback)");
    println!();
    
    let block = generator.generate_block([center_ecef.x, center_ecef.y, center_ecef.z]);
    
    // Count materials
    let mut grass = 0; let mut dirt = 0; let mut stone = 0; let mut asphalt = 0; let mut air = 0; let mut other = 0;
    
    for voxel in block.voxels.iter() {
        if *voxel == AIR { air += 1; }
        else if *voxel == GRASS { grass += 1; }
        else if *voxel == DIRT { dirt += 1; }
        else if *voxel == STONE { stone += 1; }
        else if *voxel == ASPHALT { asphalt += 1; }
        else { other += 1; }
    }
    
    println!("Block voxel counts:");
    println!("  AIR: {}", air);
    println!("  GRASS: {}", grass);
    println!("  DIRT: {}", dirt);
    println!("  STONE: {}", stone);
    println!("  ASPHALT: {}", asphalt);
    println!("  OTHER: {}", other);
    
    if grass + dirt + stone > 0 {
        println!("\n✓ Terrain voxels generated!");
    } else {
        println!("\n❌ No terrain voxels!");
        println!("\nAnalysis:");
        println!("  Block spans: {:.1}m to {:.1}m elevation",
            block_gps.elevation_m, block_gps.elevation_m + 8.0);
        println!("  Ground: 5.0m");
        println!("  Expected: voxels below 5m should be terrain");
        
        if block_gps.elevation_m > 5.0 {
            println!("  Problem: Block entirely above ground!");
        } else if block_gps.elevation_m + 8.0 < 5.0 {
            println!("  Problem: Block entirely below ground!");
        } else {
            println!("  Problem: Block spans ground but no terrain?");
            println!("  Check: Is get_ground_elevation() returning Some(5.0)?");
            println!("  Check: Is comparison logic working?");
        }
    }
}
