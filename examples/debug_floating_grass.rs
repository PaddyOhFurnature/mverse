//! Debug why blocks are full of grass

use metaverse_core::procedural_generator::ProceduralGenerator;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos, ecef_to_gps};
use metaverse_core::svo::{AIR, GRASS};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Floating Grass Debug ===\n");
    
    let mut gen = ProceduralGenerator::new()?;
    
    // Test block 40m east of center (the one that was all grass)
    let center_gps = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&center_gps);
    
    // Block 40m east
    let block_min = [center_ecef.x + 40.0, center_ecef.y, center_ecef.z];
    
    let block_gps = ecef_to_gps(&crate::coordinates::EcefPos {
        x: block_min[0],
        y: block_min[1],
        z: block_min[2],
    });
    
    println!("Block position:");
    println!("  GPS: ({:.6}, {:.6}, {:.1}m)", block_gps.lat_deg, block_gps.lon_deg, block_gps.elevation_m);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", block_min[0], block_min[1], block_min[2]);
    
    let block = gen.generate_block(block_min);
    
    let grass_count = block.voxels.iter().filter(|&&v| v == GRASS).count();
    let air_count = block.voxels.iter().filter(|&&v| v == AIR).count();
    
    println!("\nBlock contents:");
    println!("  GRASS: {}/512", grass_count);
    println!("  AIR: {}/512", air_count);
    println!("  Other: {}/512", 512 - grass_count - air_count);
    
    if grass_count == 512 {
        println!("\n❌ PROBLEM: Entire block is grass!");
        println!("This block is likely above ground but being filled incorrectly.");
    }
    
    Ok(())
}
