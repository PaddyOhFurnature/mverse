//! Check coordinate conversion
//! Verify OSM coordinates convert correctly to ECEF

use metaverse_core::coordinates::{gps_to_ecef, GpsPos};

fn main() {
    println!("=== Checking Coordinate Conversion ===\n");
    
    // Test center (Kangaroo Point)
    let center = GpsPos {
        lat_deg: -27.479769,
        lon_deg: 153.033586,
        elevation_m: 0.0,
    };
    let center_ecef = gps_to_ecef(&center);
    println!("Test center GPS: ({:.6}°, {:.6}°)", center.lat_deg, center.lon_deg);
    println!("Test center ECEF: ({:.1}, {:.1}, {:.1})", center_ecef.x, center_ecef.y, center_ecef.z);
    println!();
    
    // First road node
    let road_node = GpsPos {
        lat_deg: -27.480618,
        lon_deg: 153.033429,
        elevation_m: 0.0,
    };
    let road_ecef = gps_to_ecef(&road_node);
    println!("Road node GPS: ({:.6}°, {:.6}°)", road_node.lat_deg, road_node.lon_deg);
    println!("Road node ECEF: ({:.1}, {:.1}, {:.1})", road_ecef.x, road_ecef.y, road_ecef.z);
    println!();
    
    // Distance
    let dx = road_ecef.x - center_ecef.x;
    let dy = road_ecef.y - center_ecef.y;
    let dz = road_ecef.z - center_ecef.z;
    let dist = (dx*dx + dy*dy + dz*dz).sqrt();
    
    println!("Distance: {:.1}m", dist);
    println!("Delta: ({:.1}, {:.1}, {:.1})", dx, dy, dz);
    println!();
    
    if dist > 50.0 {
        println!("⚠ WARNING: Road is {}m away - outside our 50m test grid!", dist);
        println!("This explains why no voxels are generated.");
    } else {
        println!("✓ Road is within test range");
    }
}
