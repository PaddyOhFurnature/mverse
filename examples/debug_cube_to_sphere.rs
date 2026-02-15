use metaverse_core::chunks::{gps_to_chunk_id, ChunkId};
use metaverse_core::coordinates::{GpsPos, gps_to_ecef, ecef_to_gps, WGS84_A};

// Need to access cube_to_sphere which is in chunks.rs but might not be public
// Let me check if we can use chunk_corners_ecef directly
use metaverse_core::chunks::chunk_corners_ecef;

fn main() {
    let brisbane = GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 100.0,
    };
    
    println!("Brisbane GPS: ({:.6}, {:.6})", brisbane.lat_deg, brisbane.lon_deg);
    let brisbane_ecef = gps_to_ecef(&brisbane);
    println!("Brisbane ECEF: ({:.1}, {:.1}, {:.1})", 
        brisbane_ecef.x, brisbane_ecef.y, brisbane_ecef.z);
    
    let chunk_id = gps_to_chunk_id(&brisbane, 9);
    println!("\nChunk ID: {}", chunk_id);
    
    let corners = chunk_corners_ecef(&chunk_id);
    println!("\nChunk corners ECEF:");
    for (i, corner) in corners.iter().enumerate() {
        let gps = ecef_to_gps(corner);
        println!("  Corner {}: ECEF({:.1}, {:.1}, {:.1}) → GPS({:.6}, {:.6})",
            i, corner.x, corner.y, corner.z, gps.lat_deg, gps.lon_deg);
    }
    
    // Check if Brisbane is within the bounding box
    let gps_corners: Vec<_> = corners.iter().map(ecef_to_gps).collect();
    let min_lat = gps_corners.iter().map(|p| p.lat_deg).fold(f64::INFINITY, f64::min);
    let max_lat = gps_corners.iter().map(|p| p.lat_deg).fold(f64::NEG_INFINITY, f64::max);
    let min_lon = gps_corners.iter().map(|p| p.lon_deg).fold(f64::INFINITY, f64::min);
    let max_lon = gps_corners.iter().map(|p| p.lon_deg).fold(f64::NEG_INFINITY, f64::max);
    
    println!("\nBounding box:");
    println!("  Lat: [{:.6}, {:.6}]", min_lat, max_lat);
    println!("  Lon: [{:.6}, {:.6}]", min_lon, max_lon);
    
    let lat_ok = brisbane.lat_deg >= min_lat && brisbane.lat_deg <= max_lat;
    let lon_ok = brisbane.lon_deg >= min_lon && brisbane.lon_deg <= max_lon;
    println!("\nBrisbane within bounds? lat={} lon={}", lat_ok, lon_ok);
}
