//! Scale stress test - progressively test larger terrain regions

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource},
    marching_cubes::extract_octree_mesh,
    terrain::TerrainGenerator,
    voxel::{Octree, VoxelCoord},
};
use std::time::Instant;

fn main() {
    println!("=== Terrain Scale Stress Test ===\n");
    
    let nas = NasFileSource::new();
    if nas.is_none() {
        eprintln!("ERROR: NAS SRTM file required for stress testing");
        return;
    }
    let nas = nas.unwrap();
    
    let origin = GPS::new(-27.4775, 153.0355, 0.0);
    
    // Test progressively larger scales
    let test_sizes = vec![
        120,   // Baseline (proven working)
        250,   // 2× linear
        500,   // 4× linear
        1000,  // ~8× linear
        2000,  // 16× linear
        5000,  // 40× linear
    ];
    
    for size in test_sizes {
        println!("\n{}", "=".repeat(60));
        println!("Testing {}m × {}m region", size, size);
        println!("{}\n", "=".repeat(60));
        
        let mut pipeline = ElevationPipeline::new();
        pipeline.add_source(Box::new(NasFileSource::new().unwrap()));
        
        let mut generator = TerrainGenerator::new(pipeline);
        let mut octree = Octree::new();
        
        // Terrain generation
        println!("Generating terrain...");
        let gen_start = Instant::now();
        
        match generator.generate_region(&mut octree, &origin, size as f64) {
            Ok(_) => {
                let gen_time = gen_start.elapsed();
                println!("✓ Generation: {:.2}s", gen_time.as_secs_f32());
                
                // Calculate expected voxels
                let columns = size * size;
                let avg_voxels_per_column = 300; // ~300 voxels per column (bedrock to sky)
                let total_voxels = columns * avg_voxels_per_column;
                
                println!("  Columns: {}", columns);
                println!("  Est. voxels: ~{:.1}M", total_voxels as f64 / 1_000_000.0);
                
                // Mesh extraction
                println!("\nExtracting mesh...");
                let mesh_start = Instant::now();
                
                let origin_ecef = origin.to_ecef();
                let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
                
                // Calculate depth needed to cover region
                let depth = ((size as f32).log2().ceil() as u8).max(7);
                println!("  Using depth {} ({}×{}×{} cube)", depth, 1<<depth, 1<<depth, 1<<depth);
                
                let mesh = extract_octree_mesh(&octree, &origin_voxel, depth);
                let mesh_time = mesh_start.elapsed();
                
                println!("✓ Mesh extraction: {:.2}s", mesh_time.as_secs_f32());
                println!("  Vertices: {}", mesh.vertex_count());
                println!("  Triangles: {}", mesh.triangle_count());
                
                let total_time = gen_start.elapsed();
                println!("\n✓ TOTAL: {:.2}s", total_time.as_secs_f32());
                
                // Memory estimate
                let mesh_mb = (mesh.vertex_count() * 32) as f64 / (1024.0 * 1024.0);
                println!("  Mesh memory: ~{:.1} MB", mesh_mb);
                
                // Check if we should continue
                if total_time.as_secs() > 120 {
                    println!("\n⚠ Took >2 minutes, stopping here");
                    break;
                }
            }
            Err(e) => {
                println!("✗ FAILED: {}", e);
                break;
            }
        }
    }
    
    println!("\n{}", "=".repeat(60));
    println!("Scale test complete!");
}
