//! Debug elevation data at test location

use metaverse_core::coordinates::{gps_to_ecef, GpsPos, ecef_to_gps, EcefPos};
use metaverse_core::elevation::get_elevation;

const TEST_LAT: f64 = -27.479769;
const TEST_LON: f64 = 153.033586;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Elevation Data Debug ===\n");
    
    // Test at center
    println!("Ground level (center):");
    let elev_center = get_elevation(TEST_LAT, TEST_LON);
    println!("  GPS: ({:.6}, {:.6})", TEST_LAT, TEST_LON);
    println!("  Elevation: {:?}", elev_center);
    
    // Test 40m east
    let center_ecef = gps_to_ecef(&GpsPos { lat_deg: TEST_LAT, lon_deg: TEST_LON, elevation_m: 0.0 });
    let east_40m = EcefPos {
        x: center_ecef.x + 40.0,
        y: center_ecef.y,
        z: center_ecef.z,
    };
    let east_gps = ecef_to_gps(&east_40m);
    println!("\n40m east:");
    println!("  GPS: ({:.6}, {:.6})", east_gps.lat_deg, east_gps.lon_deg);
    let elev_east = get_elevation(east_gps.lat_deg, east_gps.lon_deg);
    println!("  Elevation: {:?}", elev_east);
    
    // Test voxels at different heights
    println!("\nVoxel altitude tests (40m east location):");
    for height in [0.0, 10.0, 20.0, 30.0, 40.0, 50.0] {
        let voxel = EcefPos {
            x: east_40m.x,
            y: east_40m.y,
            z: east_40m.z + height,
        };
        let voxel_gps = ecef_to_gps(&voxel);
        let is_below_ground = if let Some(ground) = elev_east {
            voxel_gps.elevation_m < ground
        } else {
            false
        };
        println!("  Height +{:2}m: voxel_alt={:.1}m, below_ground={}", 
            height, voxel_gps.elevation_m, is_below_ground);
    }
    
    Ok(())
}
