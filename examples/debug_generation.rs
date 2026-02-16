//! Debug procedural generation
//! Check what's happening during voxel generation

use metaverse_core::procedural_generator::{ProceduralGenerator, GeneratorConfig};
use metaverse_core::coordinates::EcefPos;
use metaverse_core::svo::AIR;

// Test location: Kangaroo Point, Brisbane (-27.479769°, 153.033586°)
const KANGAROO_POINT: [f64; 3] = [-5046877.97, 2567787.42, -2925481.59];

fn main() {
    println!("=== Debugging Procedural Generation ===\n");
    
    let cache_dir = dirs::cache_dir()
        .unwrap()
        .join("metaverse");
    
    let config = GeneratorConfig {
        srtm_cache_path: cache_dir.join("srtm"),
        osm_cache_path: cache_dir.join("osm"),
        area_center: EcefPos {
            x: KANGAROO_POINT[0],
            y: KANGAROO_POINT[1],
            z: KANGAROO_POINT[2],
        },
        area_radius: 100.0,
    };
    
    println!("1. Creating generator...");
    let generator = match ProceduralGenerator::new(config) {
        Ok(g) => {
            println!("   ✓ Generator created\n");
            g
        }
        Err(e) => {
            eprintln!("   ✗ Failed: {}", e);
            return;
        }
    };
    
    println!("2. Loading OSM features...");
    if let Err(e) = generator.load_osm_features() {
        println!("   ⚠ Failed: {}", e);
        return;
    }
    println!();
    
    println!("3. Generating block at center...");
    let block = generator.generate_block(KANGAROO_POINT);
    
    let mut non_air = 0;
    for voxel in block.voxels.iter() {
        if *voxel != AIR {
            non_air += 1;
        }
    }
    
    println!("   Block position: ({:.1}, {:.1}, {:.1})", 
        block.ecef_min[0], block.ecef_min[1], block.ecef_min[2]);
    println!("   Block size: {}m", block.size);
    println!("   Non-air voxels: {}/512 ({:.1}%)", non_air, non_air as f64 / 512.0 * 100.0);
    
    if non_air == 0 {
        println!("\n   ❌ PROBLEM: No voxels generated!");
        println!("   This suggests:");
        println!("   - Roads don't intersect this block, OR");
        println!("   - Voxelization logic has a bug");
    } else {
        println!("\n   ✅ Voxels generated successfully!");
    }
}
