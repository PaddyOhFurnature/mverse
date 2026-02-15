use metaverse_core::coordinates::*;
use metaverse_core::chunks::*;

fn main() {
    let camera_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 500.0,
    };
    
    let camera_ecef = gps_to_ecef(&camera_gps);
    println!("Camera GPS: ({:.6}, {:.6}, {:.1}m)", camera_gps.lat_deg, camera_gps.lon_deg, camera_gps.elevation_m);
    println!("Camera ECEF: ({:.1}, {:.1}, {:.1})", camera_ecef.x, camera_ecef.y, camera_ecef.z);
    
    let chunk_id = gps_to_chunk_id(&camera_gps, 14);
    println!("\nCamera chunk: {}", chunk_id);
    
    let bounds = chunk_bounds_gps(&chunk_id).unwrap();
    println!("Chunk bounds: SW({:.6}, {:.6}) NE({:.6}, {:.6})",
        bounds.0.lat_deg, bounds.0.lon_deg, bounds.1.lat_deg, bounds.1.lon_deg);
    
    let center_gps = GpsPos {
        lat_deg: (bounds.0.lat_deg + bounds.1.lat_deg) / 2.0,
        lon_deg: (bounds.0.lon_deg + bounds.1.lon_deg) / 2.0,
        elevation_m: 0.0,
    };
    let center_ecef = gps_to_ecef(&center_gps);
    println!("Chunk center GPS: ({:.6}, {:.6})", center_gps.lat_deg, center_gps.lon_deg);
    println!("Chunk center ECEF: ({:.1}, {:.1}, {:.1})", center_ecef.x, center_ecef.y, center_ecef.z);
    
    let dx = camera_ecef.x - center_ecef.x;
    let dy = camera_ecef.y - center_ecef.y;
    let dz = camera_ecef.z - center_ecef.z;
    let dist = (dx*dx + dy*dy + dz*dz).sqrt();
    println!("\nDistance camera to chunk center: {:.1}m", dist);
}
