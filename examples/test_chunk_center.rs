use metaverse_core::coordinates::*;
use metaverse_core::chunks::*;

fn main() {
    // Brisbane
    let brisbane_gps = GpsPos {
        lat_deg: -27.4705,
        lon_deg: 153.0260,
        elevation_m: 0.0,
    };
    let brisbane_ecef = gps_to_ecef(&brisbane_gps);
    
    println!("Brisbane:");
    println!("  GPS: ({:.6}, {:.6})", brisbane_gps.lat_deg, brisbane_gps.lon_deg);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", brisbane_ecef.x, brisbane_ecef.y, brisbane_ecef.z);
    
    // Get chunk ID
    let chunk_id = gps_to_chunk_id(&brisbane_gps, 14);
    println!("\nChunk ID: {}", chunk_id);
    
    // Get chunk bounds (GPS)
    let bounds = chunk_bounds_gps(&chunk_id);
    println!("\nChunk Bounds (GPS):");
    println!("  SW: ({:.6}, {:.6})", bounds.0.lat_deg, bounds.0.lon_deg);
    println!("  NE: ({:.6}, {:.6})", bounds.1.lat_deg, bounds.1.lon_deg);
    
    // Calculate center from GPS bounds
    let center_from_gps = GpsPos {
        lat_deg: (bounds.0.lat_deg + bounds.1.lat_deg) / 2.0,
        lon_deg: (bounds.0.lon_deg + bounds.1.lon_deg) / 2.0,
        elevation_m: 0.0,
    };
    let center_from_gps_ecef = gps_to_ecef(&center_from_gps);
    println!("\nCenter from GPS average:");
    println!("  GPS: ({:.6}, {:.6})", center_from_gps.lat_deg, center_from_gps.lon_deg);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", center_from_gps_ecef.x, center_from_gps_ecef.y, center_from_gps_ecef.z);
    
    // Get center from chunk_center_ecef
    let center_from_func = chunk_center_ecef(&chunk_id);
    let center_from_func_gps = ecef_to_gps(&center_from_func);
    println!("\nCenter from chunk_center_ecef:");
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", center_from_func.x, center_from_func.y, center_from_func.z);
    println!("  GPS: ({:.6}, {:.6})", center_from_func_gps.lat_deg, center_from_func_gps.lon_deg);
    
    // Calculate distance
    let dx = center_from_gps_ecef.x - center_from_func.x;
    let dy = center_from_gps_ecef.y - center_from_func.y;
    let dz = center_from_gps_ecef.z - center_from_func.z;
    let dist = (dx*dx + dy*dy + dz*dz).sqrt();
    
    println!("\nDifference:");
    println!("  Distance: {:.1}m", dist);
    
    if dist > 1000.0 {
        println!("\n⚠ ERROR: Centers are {}km apart!", dist / 1000.0);
    }
}
