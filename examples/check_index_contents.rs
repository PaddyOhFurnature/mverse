//! Check if blocks in spatial index actually have terrain

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::{AIR, GRASS};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Index Contents Check ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    // Create world WITHOUT pre-generation
    use metaverse_core::procedural_generator::{ProceduralGenerator, GeneratorConfig};
    use metaverse_core::spatial_index::SpatialIndex;
    use std::path::PathBuf;
    
    let cache_base = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("metaverse");
    
    let config = GeneratorConfig {
        srtm_cache_path: cache_base.join("srtm"),
        osm_cache_path: cache_base.join("osm"),
        area_center: metaverse_core::coordinates::EcefPos {
            x: center[0],
            y: center[1],
            z: center[2],
        },
        area_radius: 100.0,
    };
    
    let generator = ProceduralGenerator::new(config).expect("Failed");
    
    // Generate ONE block manually
    println!("Generating single block at center...");
    let block_size = 8.0;
    let key_x = (center[0] / block_size).floor() * block_size;
    let key_y = (center[1] / block_size).floor() * block_size;
    let key_z = (center[2] / block_size).floor() * block_size;
    
    let block = generator.generate_block([key_x, key_y, key_z]);
    
    let mut grass = 0;
    let mut air = 0;
    for voxel in block.voxels.iter() {
        if *voxel == AIR { air += 1; }
        else if *voxel == GRASS { grass += 1; }
    }
    println!("  Generated block: {} AIR, {} GRASS", air, grass);
    
    // Now insert into index
    let bounds = AABB::from_center(center, 100.0);
    let mut index = SpatialIndex::new(bounds);
    index.insert(block.clone());
    println!("  ✓ Inserted into index\n");
    
    // Query it back
    println!("Querying from index...");
    let query = AABB {
        min: [key_x, key_y, key_z],
        max: [key_x + block_size, key_y + block_size, key_z + block_size],
    };
    let retrieved = index.query_range(query);
    
    println!("  Retrieved {} blocks", retrieved.len());
    if retrieved.len() > 0 {
        let ret_block = &retrieved[0];
        let mut ret_grass = 0;
        let mut ret_air = 0;
        for voxel in ret_block.voxels.iter() {
            if *voxel == AIR { ret_air += 1; }
            else if *voxel == GRASS { ret_grass += 1; }
        }
        println!("  Retrieved block: {} AIR, {} GRASS", ret_air, ret_grass);
        
        if ret_grass == grass {
            println!("\n✓ Index stores and retrieves blocks correctly!");
        } else {
            println!("\n❌ Data mismatch!");
        }
    } else {
        println!("\n❌ No blocks retrieved!");
    }
}
