//! Test terrain generation specifically

use metaverse_core::procedural_generator::{ProceduralGenerator, GeneratorConfig};
use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};
use metaverse_core::svo::AIR;
use std::path::PathBuf;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Terrain Generation Test ===\n");
    
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("metaverse");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    
    let config = GeneratorConfig {
        srtm_cache_path: cache_base.join("srtm"),
        osm_cache_path: cache_base.join("osm"),
        area_center: center_ecef,
        area_radius: 100.0,
    };
    
    let generator = ProceduralGenerator::new(config).expect("Failed to create generator");
    
    // Generate a block at ground level (0-8m altitude)
    println!("Generating block at ground level...");
    let block_ecef = [center_ecef.x, center_ecef.y, center_ecef.z];
    let block = generator.generate_block(block_ecef);
    
    // Count voxels by type
    let mut air_count = 0;
    let mut non_air_count = 0;
    
    for voxel in block.voxels.iter() {
        if *voxel == AIR {
            air_count += 1;
        } else {
            non_air_count += 1;
        }
    }
    
    println!("\nBlock stats:");
    println!("  AIR voxels: {}", air_count);
    println!("  Non-AIR voxels: {}", non_air_count);
    println!("  Total: {}", air_count + non_air_count);
    
    if non_air_count == 0 {
        println!("\n❌ NO TERRAIN GENERATED!");
        println!("\nDebugging info:");
        println!("  Block ECEF min: ({:.1}, {:.1}, {:.1})", 
            block_ecef[0], block_ecef[1], block_ecef[2]);
        
        let block_gps = ecef_to_gps(&EcefPos { 
            x: block_ecef[0], 
            y: block_ecef[1], 
            z: block_ecef[2] 
        });
        println!("  Block GPS: ({:.6}°, {:.6}°, {:.1}m)",
            block_gps.lat_deg, block_gps.lon_deg, block_gps.elevation_m);
        
        // Check voxel at block center
        let voxel_center_ecef = EcefPos {
            x: block_ecef[0] + 4.0,
            y: block_ecef[1] + 4.0,
            z: block_ecef[2] + 4.0,
        };
        let voxel_gps = ecef_to_gps(&voxel_center_ecef);
        println!("  Voxel center GPS: ({:.6}°, {:.6}°, {:.1}m)",
            voxel_gps.lat_deg, voxel_gps.lon_deg, voxel_gps.elevation_m);
        println!("  Ground level (fallback): 5.0m");
        
        if voxel_gps.elevation_m < 5.0 {
            println!("  → Voxel SHOULD be filled (below ground)");
        } else {
            println!("  → Voxel correctly AIR (above ground)");
        }
    } else {
        println!("\n✓ Terrain generated successfully!");
    }
}
