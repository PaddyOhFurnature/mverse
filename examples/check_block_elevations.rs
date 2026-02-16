//! Check what elevations blocks are generated at

use metaverse_core::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos, EcefPos};

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() {
    println!("=== Block Elevation Check ===\n");
    
    let gps_center = GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 };
    let center_ecef = gps_to_ecef(&gps_center);
    
    println!("Center:");
    println!("  GPS: ({:.6}°, {:.6}°, {:.1}m)", gps_center.lat_deg, gps_center.lon_deg, gps_center.elevation_m);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", center_ecef.x, center_ecef.y, center_ecef.z);
    println!();
    
    let block_size = 8.0;
    let z_levels = vec![-2, -1, 0, 1];
    
    println!("Block Z levels:");
    for &z_offset in &z_levels {
        let block_z_ecef = center_ecef.z + (z_offset as f64 * block_size);
        
        // Convert block corner to GPS to see elevation
        let block_corner = EcefPos {
            x: center_ecef.x,
            y: center_ecef.y,
            z: block_z_ecef,
        };
        let block_gps = ecef_to_gps(&block_corner);
        
        println!("  Z offset {}: ECEF z={:.1}, GPS elevation {:.1}m to {:.1}m",
            z_offset, block_z_ecef, block_gps.elevation_m, block_gps.elevation_m + block_size);
    }
    
    println!("\nGround level (fallback): 5.0m");
    println!("\nFor terrain to generate:");
    println!("  Block must span elevation 5.0m");
    println!("  i.e., block.elev_min < 5.0 AND block.elev_max > 5.0");
}
