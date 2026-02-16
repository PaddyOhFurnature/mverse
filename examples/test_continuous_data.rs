//! Test continuous query data
//! Simple test to verify what data comes back from continuous queries

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::svo::AIR;

// Test location: Kangaroo Point, Brisbane (-27.479769°, 153.033586°)
const KANGAROO_POINT: [f64; 3] = [-5046877.97, 2567787.42, -2925481.59];

fn main() {
    println!("=== Testing Continuous Query Data ===\n");
    
    // Initialize continuous world
    println!("1. Creating ContinuousWorld at Kangaroo Point...");
    let mut world = match ContinuousWorld::new(KANGAROO_POINT, 100.0) {
        Ok(w) => {
            println!("   ✓ Created (100m radius test area)\n");
            w
        }
        Err(e) => {
            eprintln!("   ✗ Failed: {}", e);
            return;
        }
    };
    
    // Load data
    println!("2. Loading elevation and OSM data...");
    if let Err(e) = world.load_elevation_data() {
        println!("   ⚠ Elevation load failed: {} (expected if no SRTM tiles)", e);
    } else {
        println!("   ✓ Elevation data loaded");
    }
    
    if let Err(e) = world.load_osm_features() {
        println!("   ⚠ OSM load failed: {} (will fetch if needed)", e);
    } else {
        println!("   ✓ OSM features loaded");
    }
    println!();
    
    // Query blocks
    println!("3. Querying blocks in 20m radius...");
    let query = AABB::from_center(KANGAROO_POINT, 20.0);
    let blocks = world.query_range(query);
    println!("   ✓ Got {} blocks\n", blocks.len());
    
    if blocks.is_empty() {
        println!("⚠ WARNING: No blocks returned!");
        return;
    }
    
    // Analyze blocks
    println!("4. Analyzing block contents...");
    let mut total_non_air = 0;
    let mut material_counts = std::collections::HashMap::new();
    
    for (i, block) in blocks.iter().enumerate() {
        let mut block_non_air = 0;
        
        // Count voxels by material
        for voxel in block.voxels.iter() {
            if *voxel != AIR {
                block_non_air += 1;
                *material_counts.entry(*voxel).or_insert(0) += 1;
            }
        }
        
        total_non_air += block_non_air;
        
        if block_non_air > 0 {
            println!("   Block {}: {} non-air voxels ({:.1}% filled)", 
                i, block_non_air, block_non_air as f64 / 512.0 * 100.0);
        }
    }
    
    println!();
    println!("5. Summary:");
    println!("   Total blocks: {}", blocks.len());
    println!("   Total voxels: {}", blocks.len() * 512);
    println!("   Non-air voxels: {} ({:.2}%)", total_non_air, 
        total_non_air as f64 / (blocks.len() * 512) as f64 * 100.0);
    
    println!("\n6. Material breakdown:");
    let mut materials: Vec<_> = material_counts.iter().collect();
    materials.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    
    for (material, count) in materials.iter().take(10) {
        println!("   Material {:3?}: {:6} voxels ({:.2}%)", 
            **material, **count, **count as f64 / total_non_air as f64 * 100.0);
    }
    
    // Validation
    println!("\n7. Validation:");
    if total_non_air == 0 {
        println!("   ❌ FAIL: No terrain generated!");
        println!("   This means procedural generation is not working.");
    } else if total_non_air < 100 {
        println!("   ⚠ WARN: Very little terrain ({} voxels)", total_non_air);
        println!("   Generation may be working but producing sparse data.");
    } else {
        println!("   ✅ PASS: Terrain generated ({} voxels)", total_non_air);
        println!("   Procedural generation appears to be working!");
    }
}
