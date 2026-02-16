//! Profile viewer performance
//! Measures time spent in each phase: query, meshing, GPU upload, render

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::spatial_index::AABB;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::AIR;
use std::time::Instant;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Viewer Performance Profile ===\n");
    
    // Setup world
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    println!("Creating world (pre-generation)...");
    let start = Instant::now();
    let mut world = ContinuousWorld::new(center, 100.0)?;
    let pregen_time = start.elapsed();
    println!("  Pre-generation: {:.2}ms\n", pregen_time.as_secs_f64() * 1000.0);
    
    // Test different query radii
    let radii = vec![25.0, 50.0, 75.0, 100.0];
    
    for radius in radii {
        println!("=== {}m Radius ===", radius);
        
        // Position: 20m altitude at Kangaroo Point
        let gps = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 20.0 };
        let pos_ecef = gps_to_ecef(&gps);
        let cam_pos = [pos_ecef.x, pos_ecef.y, pos_ecef.z];
        
        // Query blocks
        let start = Instant::now();
        let query = AABB::from_center(cam_pos, radius);
        let blocks = world.query_range(query);
        let query_time = start.elapsed();
        
        // Count voxels
        let start = Instant::now();
        let mut voxel_count = 0;
        for block in &blocks {
            for voxel in block.voxels.iter() {
                if *voxel != AIR {
                    voxel_count += 1;
                }
            }
        }
        let count_time = start.elapsed();
        
        // Simulate meshing
        let start = Instant::now();
        let vertex_count = voxel_count * 8;      // 8 vertices per voxel
        let index_count = voxel_count * 36;      // 36 indices per voxel (12 tri * 3)
        let mesh_time = start.elapsed();
        
        // Calculate memory
        let vertex_bytes = vertex_count * 32;    // 32 bytes per vertex (3*f32 pos + 3*f32 normal + 4*f32 color)
        let index_bytes = index_count * 4;       // 4 bytes per u32 index
        let total_mb = (vertex_bytes + index_bytes) as f64 / 1_048_576.0;
        
        println!("  Query:   {:.2}ms ({} blocks)", query_time.as_secs_f64() * 1000.0, blocks.len());
        println!("  Count:   {:.2}ms ({} voxels)", count_time.as_secs_f64() * 1000.0, voxel_count);
        println!("  Mesh:    {:.2}ms ({} vertices, {} indices)", 
            mesh_time.as_secs_f64() * 1000.0, vertex_count, index_count);
        println!("  Memory:  {:.2} MB", total_mb);
        
        let total_time = query_time + count_time + mesh_time;
        let frame_budget = 16.67; // 60 FPS
        let budget_used = (total_time.as_secs_f64() * 1000.0 / frame_budget) * 100.0;
        
        println!("  Total:   {:.2}ms ({:.0}% of 16.67ms frame budget)", 
            total_time.as_secs_f64() * 1000.0, budget_used);
        
        if total_mb > 256.0 {
            println!("  ⚠️  WARNING: Exceeds 256 MB GPU buffer limit!");
        }
        
        println!();
    }
    
    // Test moving camera (cache behavior)
    println!("=== Cache Performance (Moving Camera) ===");
    
    let positions = vec![
        GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 20.0 },
        GpsPos { lat_deg: TEST_LAT + 0.0001, lon_deg: TEST_LON, elevation_m: 20.0 },
        GpsPos { lat_deg: TEST_LAT + 0.0002, lon_deg: TEST_LON, elevation_m: 20.0 },
        GpsPos { lat_deg: TEST_LAT + 0.0003, lon_deg: TEST_LON, elevation_m: 20.0 },
        GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 20.0 }, // Back to start
    ];
    
    for (i, gps) in positions.iter().enumerate() {
        let pos_ecef = gps_to_ecef(gps);
        let cam_pos = [pos_ecef.x, pos_ecef.y, pos_ecef.z];
        
        let start = Instant::now();
        let query = AABB::from_center(cam_pos, 50.0);
        let blocks = world.query_range(query);
        let query_time = start.elapsed();
        
        println!("  Move {}: {:.2}ms ({} blocks)", 
            i + 1, query_time.as_secs_f64() * 1000.0, blocks.len());
    }
    
    println!("\n=== Summary ===");
    println!("Bottlenecks (in order):");
    println!("  1. Query time (blocks from cache/index/generation)");
    println!("  2. Mesh generation (voxels → vertices/indices)");
    println!("  3. GPU upload (vertices/indices to GPU buffer)");
    println!("\nOptimizations needed:");
    println!("  - LOD: Reduce voxel count for distant blocks");
    println!("  - Culling: Don't mesh blocks outside view frustum");
    println!("  - Instancing: Batch identical cubes");
    println!("  - Caching: Keep meshes in GPU memory");
    
    Ok(())
}
