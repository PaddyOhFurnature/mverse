//! Debug intersection tests
//! Check if roads/water/buildings are detected as intersecting blocks

use metaverse_core::procedural_generator::{ProceduralGenerator, GeneratorConfig};
use metaverse_core::coordinates::EcefPos;
use metaverse_core::svo::AIR;

// Test location: Kangaroo Point, Brisbane (-27.479769°, 153.033586°)
const KANGAROO_POINT: [f64; 3] = [-5046877.97, 2567787.42, -2925481.59];

fn main() {
    println!("=== Debugging Intersection Tests ===\n");
    
    let cache_dir = dirs::cache_dir().unwrap().join("metaverse");
    
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
    
    let generator = ProceduralGenerator::new(config).unwrap();
    generator.load_osm_features().unwrap();
    
    println!("Testing blocks in 7x7x7 grid around center...\n");
    let mut total_intersections = 0;
    
    for dx in -3..=3 {
        for dy in -3..=3 {
            for dz in -3..=3 {
                let test_pos = [
                    KANGAROO_POINT[0] + (dx as f64 * 8.0),
                    KANGAROO_POINT[1] + (dy as f64 * 8.0),
                    KANGAROO_POINT[2] + (dz as f64 * 8.0),
                ];
                
                let block = generator.generate_block(test_pos);
                
                let mut non_air = 0;
                for voxel in block.voxels.iter() {
                    if *voxel != AIR {
                        non_air += 1;
                    }
                }
                
                if non_air > 0 {
                    println!("Block at offset ({:2}, {:2}, {:2}) has {} non-air voxels", 
                        dx, dy, dz, non_air);
                    total_intersections += 1;
                }
            }
        }
    }
    
    println!("\nTotal blocks with voxels: {} / 343", total_intersections);
    
    if total_intersections == 0 {
        println!("\n❌ PROBLEM: No blocks have voxels!");
        println!("Checking if OSM features are in range...");
    } else {
        println!("\n✅ Generation working - {} blocks have content!", total_intersections);
    }
}
