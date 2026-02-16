//! Test LOD system

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== LOD System Test ===\n");
    
    // Create world
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    println!("Creating world with pre-generation...");
    let mut world = ContinuousWorld::new(center, 100.0)?;
    
    // Test camera at 20m altitude
    let cam_gps = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 20.0 };
    let cam_ecef = gps_to_ecef(&cam_gps);
    let cam_pos = [cam_ecef.x, cam_ecef.y, cam_ecef.z];
    
    // Query with LOD (100m radius)
    println!("\nQuerying 100m radius with LOD...");
    let blocks_with_lod = world.query_lod(cam_pos, 100.0);
    
    // Count by LOD level
    let mut lod_counts = [0; 4];
    let mut voxel_counts = [0; 4];
    
    for (block, lod) in &blocks_with_lod {
        lod_counts[*lod as usize] += 1;
        
        // Count non-AIR voxels
        let non_air = block.voxels.iter().filter(|&&v| v != metaverse_core::svo::AIR).count();
        voxel_counts[*lod as usize] += non_air;
    }
    
    println!("\nResults:");
    println!("  Total blocks: {}", blocks_with_lod.len());
    println!("  LOD 0 (0-25m,  1m voxels): {} blocks, {} voxels", lod_counts[0], voxel_counts[0]);
    println!("  LOD 1 (25-50m, 2m voxels): {} blocks, {} voxels", lod_counts[1], voxel_counts[1]);
    println!("  LOD 2 (50-100m, 4m voxels): {} blocks, {} voxels", lod_counts[2], voxel_counts[2]);
    println!("  LOD 3 (100m+, 8m voxels): {} blocks, {} voxels", lod_counts[3], voxel_counts[3]);
    
    // Calculate effective voxel count with LOD sampling
    let effective_voxels = voxel_counts[0]                  // LOD 0: every voxel
                         + voxel_counts[1] / 8              // LOD 1: 1/8 voxels
                         + voxel_counts[2] / 64             // LOD 2: 1/64 voxels
                         + voxel_counts[3] / 512;           // LOD 3: 1/512 voxels
    
    println!("\nVoxel reduction:");
    println!("  Without LOD: {} voxels total", voxel_counts.iter().sum::<usize>());
    println!("  With LOD: {} voxels rendered", effective_voxels);
    println!("  Reduction: {:.1}x", voxel_counts.iter().sum::<usize>() as f64 / effective_voxels as f64);
    
    Ok(())
}
