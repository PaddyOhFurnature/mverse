use metaverse_core::chunks::{gps_to_chunk_id, chunk_bounds_gps};
use metaverse_core::coordinates::GpsPos;

fn main() {
    // Brisbane coordinates
    let brisbane = GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 100.0,
    };
    
    println!("Input: Brisbane GPS ({:.6}, {:.6})", brisbane.lat_deg, brisbane.lon_deg);
    
    let chunk_id = gps_to_chunk_id(&brisbane, 9);
    println!("Chunk ID: {}", chunk_id);
    
    let bounds = chunk_bounds_gps(&chunk_id).unwrap();
    let (sw, ne) = bounds;
    println!("Bounds: SW({:.6}, {:.6}) NE({:.6}, {:.6})", 
        sw.lat_deg, sw.lon_deg, ne.lat_deg, ne.lon_deg);
    
    // Check if Brisbane is within bounds
    let lat_ok = brisbane.lat_deg >= sw.lat_deg && brisbane.lat_deg <= ne.lat_deg;
    let lon_ok = brisbane.lon_deg >= sw.lon_deg && brisbane.lon_deg <= ne.lon_deg;
    
    println!("Brisbane within bounds? lat={} lon={}", lat_ok, lon_ok);
    
    if !lat_ok || !lon_ok {
        println!("ERROR: Brisbane is NOT within chunk bounds!");
        println!("  Expected lat around -27.47, got {:.6} to {:.6}", sw.lat_deg, ne.lat_deg);
        println!("  Expected lon around 153.03, got {:.6} to {:.6}", sw.lon_deg, ne.lon_deg);
    }
}
