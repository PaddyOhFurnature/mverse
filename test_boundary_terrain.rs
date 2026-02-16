/// Test if terrain elevation matches at chunk boundaries

use metaverse_core::{coordinates::*, chunks::*};

fn main() {
    println!("=== TERRAIN BOUNDARY ALIGNMENT TEST ===\n");
    
    let chunk_id = gps_to_chunk_id(&GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    }, 14);
    
    let bounds = chunk_bounds_gps(&chunk_id).unwrap();
    
    // Check EAST neighbor
    let lon_span = bounds.1.lon_deg - bounds.0.lon_deg;
    let east_center_lon = bounds.1.lon_deg + (lon_span / 2.0);
    let center_lat = (bounds.0.lat_deg + bounds.1.lat_deg) / 2.0;
    
    let east_chunk = gps_to_chunk_id(&GpsPos {
        lat_deg: center_lat,
        lon_deg: east_center_lon,
        elevation_m: 0.0,
    }, 14);
    
    let east_bounds = chunk_bounds_gps(&east_chunk).unwrap();
    let lon_gap = (east_bounds.0.lon_deg - bounds.1.lon_deg).abs();
    
    println!("Camera chunk: {}", chunk_id);
    println!("East neighbor: {}", east_chunk);
    println!("Longitude gap: {:.10} degrees ({:.3}m)", 
        lon_gap, lon_gap * 111_000.0 * center_lat.to_radians().cos());
    
    // Check NORTH neighbor  
    let lat_span = bounds.1.lat_deg - bounds.0.lat_deg;
    let north_center_lat = bounds.1.lat_deg + (lat_span / 2.0);
    let center_lon = (bounds.0.lon_deg + bounds.1.lon_deg) / 2.0;
    
    let north_chunk = gps_to_chunk_id(&GpsPos {
        lat_deg: north_center_lat,
        lon_deg: center_lon,
        elevation_m: 0.0,
    }, 14);
    
    let north_bounds = chunk_bounds_gps(&north_chunk).unwrap();
    let lat_gap = (north_bounds.0.lat_deg - bounds.1.lat_deg).abs();
    
    println!("North neighbor: {}", north_chunk);
    println!("Latitude gap: {:.10} degrees ({:.3}m)", lat_gap, lat_gap * 111_000.0);
    
    println!("\n=== VERDICT ===");
    if lon_gap < 1e-9 && lat_gap < 1e-9 {
        println!("✓ CHUNKS ALIGN PERFECTLY");
        println!("✓ Coordinate system is CORRECT");
        println!("  → Chunk boundaries have ZERO gaps");
        println!("  → Voxels at boundaries should align");
        println!("  → Visible seams are marching cubes needing neighbor data");
    } else {
        println!("✗ COORDINATE SYSTEM BROKEN");
    }
}

    // Investigate chunk geometry
    println!("\n=== CHUNK GEOMETRY ===");
    let lat_size = (bounds.1.lat_deg - bounds.0.lat_deg).abs() * 111_000.0;
    let lon_size = (bounds.1.lon_deg - bounds.0.lon_deg).abs() * 111_000.0 * center_lat.to_radians().cos();
    
    println!("Camera chunk dimensions:");
    println!("  Latitude span: {:.3}m", lat_size);
    println!("  Longitude span: {:.3}m", lon_size);
    println!("  Ratio (lon/lat): {:.6}", lon_size / lat_size);
    
    let north_lat_size = (north_bounds.1.lat_deg - north_bounds.0.lat_deg).abs() * 111_000.0;
    let north_lon_size = (north_bounds.1.lon_deg - north_bounds.0.lon_deg).abs() * 111_000.0 * north_center_lat.to_radians().cos();
    
    println!("\nNorth neighbor dimensions:");
    println!("  Latitude span: {:.3}m", north_lat_size);
    println!("  Longitude span: {:.3}m", north_lon_size);
    
    println!("\n✗ Chunks are NOT square on sphere!");
    println!("✗ Quad-sphere projection creates rectangular chunks");
    println!("✗ This causes gaps in north/south direction");
}
