//! Test if terrain generation is actually working

use metaverse_core::continuous_world::ContinuousWorld;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use metaverse_core::svo::AIR;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Terrain Generation Debug ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    let center = [center_ecef.x, center_ecef.y, center_ecef.z];
    
    println!("Creating world with 50m pre-generation...");
    let mut world = ContinuousWorld::new(center, 50.0)?;
    
    // Sample blocks at different distances
    let test_points = vec![
        ("Ground level", 0.0, 0.0, 0.0),
        ("10m east", 10.0, 0.0, 0.0),
        ("20m east", 20.0, 0.0, 0.0),
        ("30m east", 30.0, 0.0, 0.0),
        ("40m east", 40.0, 0.0, 0.0),
    ];
    
    for (label, dx, dy, dz) in test_points {
        let pos = [center[0] + dx, center[1] + dy, center[2] + dz];
        let blocks_with_lod = world.query_lod(pos, 10.0);
        
        // Find closest block
        if let Some((block, _)) = blocks_with_lod.first() {
            let non_air = block.voxels.iter().filter(|&&v| v != AIR).count();
            println!("{:20} {} voxels ({} blocks nearby)", 
                label, non_air, blocks_with_lod.len());
            
            // Show voxel types
            let mut types = std::collections::HashMap::new();
            for v in block.voxels.iter() {
                *types.entry(*v).or_insert(0) += 1;
            }
            println!("  Voxel types: {:?}", types);
        } else {
            println!("{:20} NO BLOCKS!", label);
        }
    }
    
    Ok(())
}
