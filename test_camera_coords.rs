// Quick test to see what ECEF coordinates we're generating
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};

fn main() {
    let test_gps = GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 50.0,
    };
    
    let ecef = gps_to_ecef(&test_gps);
    println!("Queen Street Mall @ 50m altitude:");
    println!("  GPS: ({:.6}, {:.6}, {:.1}m)", test_gps.lat_deg, test_gps.lon_deg, test_gps.elevation_m);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", ecef.x, ecef.y, ecef.z);
    println!("  Distance from origin: {:.1}m", (ecef.x*ecef.x + ecef.y*ecef.y + ecef.z*ecef.z).sqrt());
    
    // Ground level
    let ground = GpsPos { lat_deg: -27.469800, lon_deg: 153.025100, elevation_m: 0.0 };
    let ecef_ground = gps_to_ecef(&ground);
    println!("\nGround level:");
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", ecef_ground.x, ecef_ground.y, ecef_ground.z);
    
    // Test building nearby
    let building_gps = GpsPos { lat_deg: -27.470, lon_deg: 153.025, elevation_m: 0.0 };
    let building_ecef = gps_to_ecef(&building_gps);
    println!("\nNearby building:");
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", building_ecef.x, building_ecef.y, building_ecef.z);
    
    // Distance between camera and ground
    let dx = ecef.x - ecef_ground.x;
    let dy = ecef.y - ecef_ground.y;
    let dz = ecef.z - ecef_ground.z;
    let dist = (dx*dx + dy*dy + dz*dz).sqrt();
    println!("\nCamera 50m above ground - actual ECEF distance: {:.1}m", dist);
}
