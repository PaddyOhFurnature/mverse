use metaverse_core::chunks::{gps_to_chunk_id, ChunkId};
use metaverse_core::coordinates::{GpsPos, gps_to_ecef, ecef_to_gps};

// Copy of chunk_uv_bounds from chunks.rs
fn chunk_uv_bounds(id: &ChunkId) -> (f64, f64, f64, f64) {
    let mut u_min = -1.0;
    let mut u_max = 1.0;
    let mut v_min = -1.0;
    let mut v_max = 1.0;
    
    for &quadrant in &id.path {
        let u_mid = (u_min + u_max) / 2.0;
        let v_mid = (v_min + v_max) / 2.0;
        
        match quadrant {
            0 => { u_max = u_mid; v_max = v_mid; }
            1 => { u_min = u_mid; v_max = v_mid; }
            2 => { u_max = u_mid; v_min = v_mid; }
            3 => { u_min = u_mid; v_min = v_mid; }
            _ => {}
        }
    }
    
    (u_min, u_max, v_min, v_max)
}

fn main() {
    // Brisbane coordinates
    let brisbane = GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 100.0,
    };
    
    println!("=== Brisbane Chunk Coordinate Debug ===");
    println!("Brisbane GPS: ({:.6}, {:.6})", brisbane.lat_deg, brisbane.lon_deg);
    
    let brisbane_ecef = gps_to_ecef(&brisbane);
    println!("Brisbane ECEF: ({:.1}, {:.1}, {:.1})", 
        brisbane_ecef.x, brisbane_ecef.y, brisbane_ecef.z);
    
    let chunk_id = gps_to_chunk_id(&brisbane, 9);
    println!("\nChunk ID: {}", chunk_id);
    println!("  Face: {}", chunk_id.face);
    println!("  Path: {:?}", chunk_id.path);
    
    let (u_min, u_max, v_min, v_max) = chunk_uv_bounds(&chunk_id);
    println!("\nUV bounds:");
    println!("  u: [{:.6}, {:.6}]", u_min, u_max);
    println!("  v: [{:.6}, {:.6}]", v_min, v_max);
    println!("  u_center: {:.6}", (u_min + u_max) / 2.0);
    println!("  v_center: {:.6}", (v_min + v_max) / 2.0);
}
