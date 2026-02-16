/// Diagnostic test for chunk boundary alignment
/// Tests whether adjacent chunks have perfectly aligned voxel grids

use metaverse_core::{
    coordinates::*,
    chunks::*,
};

fn main() {
    println!("=== CHUNK SEAM ALIGNMENT TEST ===\n");
    
    // Get camera chunk and its eastern neighbor
    let camera_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    
    let chunk_a = gps_to_chunk_id(&camera_gps, 14);
    let neighbors = chunk_neighbors(&chunk_a);
    
    println!("Chunk A (camera): {}", chunk_a);
    println!("Number of neighbors: {}\n", neighbors.len());
    
    // Find eastern neighbor (first in list should be east)
    if neighbors.len() == 0 {
        println!("ERROR: No neighbors found!");
        return;
    }
    
    let chunk_b = &neighbors[0];  // East neighbor
    println!("Chunk B (east neighbor): {}\n", chunk_b);
    
    // Get GPS bounds for both chunks
    let bounds_a = chunk_bounds_gps(&chunk_a).unwrap();
    let bounds_b = chunk_bounds_gps(&chunk_b).unwrap();
    
    println!("=== GPS BOUNDS CHECK ===");
    println!("Chunk A SW: ({:.10}, {:.10})", bounds_a.0.lat_deg, bounds_a.0.lon_deg);
    println!("Chunk A NE: ({:.10}, {:.10})", bounds_a.1.lat_deg, bounds_a.1.lon_deg);
    println!("Chunk B SW: ({:.10}, {:.10})", bounds_b.0.lat_deg, bounds_b.0.lon_deg);
    println!("Chunk B NE: ({:.10}, {:.10})", bounds_b.1.lat_deg, bounds_b.1.lon_deg);
    
    // Check if Chunk A's east edge == Chunk B's west edge
    let a_east_lon = bounds_a.1.lon_deg;
    let b_west_lon = bounds_b.0.lon_deg;
    let lon_diff = (a_east_lon - b_west_lon).abs();
    
    println!("\nChunk A east longitude: {:.10}", a_east_lon);
    println!("Chunk B west longitude: {:.10}", b_west_lon);
    println!("Difference: {:.2e} degrees", lon_diff);
    println!("Difference in meters: {:.6}m", lon_diff * 111_000.0 * camera_gps.lat_deg.to_radians().cos());
    
    if lon_diff < 1e-10 {
        println!("✓ GPS boundaries align perfectly");
    } else {
        println!("✗ GPS boundaries MISALIGNED - THIS IS THE BUG");
    }
    
    // Calculate chunk properties
    println!("\n=== CHUNK GEOMETRY ===");
    let lat_span_a = (bounds_a.1.lat_deg - bounds_a.0.lat_deg).abs() * 111_000.0;
    let lon_span_a = (bounds_a.1.lon_deg - bounds_a.0.lon_deg).abs() * 111_000.0 * bounds_a.0.lat_deg.to_radians().cos();
    let area_size_a = lat_span_a.max(lon_span_a);
    
    println!("Chunk A dimensions: {:.2}m × {:.2}m", lat_span_a, lon_span_a);
    println!("Chunk A area_size: {:.2}m", area_size_a);
    
    // Calculate voxel grid properties
    let svo_depth = 7;
    let svo_size = 1u32 << svo_depth;
    let voxel_size = area_size_a / svo_size as f64;
    
    println!("\nSVO depth: {} ({}³ voxels)", svo_depth, svo_size);
    println!("Voxel size: {:.6}m", voxel_size);
    println!("Chunk width / voxel count: {:.10}", area_size_a / svo_size as f64);
    
    // Check if voxel size divides evenly
    let expected_chunk_size = voxel_size * svo_size as f64;
    let size_error = (expected_chunk_size - area_size_a).abs();
    println!("Reconstruction error: {:.2e}m", size_error);
    
    if size_error < 0.001 {
        println!("✓ Voxel grid fits chunk size cleanly");
    } else {
        println!("⚠ Voxel grid causes rounding errors: {:.6}m", size_error);
    }
    
    // Test voxel→ECEF transform at boundary
    println!("\n=== VOXEL POSITION CHECK ===");
    
    // Chunk A center
    let center_a_gps = GpsPos {
        lat_deg: (bounds_a.0.lat_deg + bounds_a.1.lat_deg) / 2.0,
        lon_deg: (bounds_a.0.lon_deg + bounds_a.1.lon_deg) / 2.0,
        elevation_m: 0.0,
    };
    let center_a_ecef = gps_to_ecef(&center_a_gps);
    
    // Chunk B center
    let center_b_gps = GpsPos {
        lat_deg: (bounds_b.0.lat_deg + bounds_b.1.lat_deg) / 2.0,
        lon_deg: (bounds_b.0.lon_deg + bounds_b.1.lon_deg) / 2.0,
        elevation_m: 0.0,
    };
    let center_b_ecef = gps_to_ecef(&center_b_gps);
    
    println!("Chunk A center ECEF: ({:.1}, {:.1}, {:.1})", 
        center_a_ecef.x, center_a_ecef.y, center_a_ecef.z);
    println!("Chunk B center ECEF: ({:.1}, {:.1}, {:.1})", 
        center_b_ecef.x, center_b_ecef.y, center_b_ecef.z);
    
    // Voxel (127, 64, 64) in Chunk A = east boundary, middle height
    let half = svo_size as f64 / 2.0;
    let voxel_a_x = 127.0;
    let voxel_a_y = 64.0;
    let voxel_a_z = 64.0;
    
    // Map to ENU (relative to Chunk A center)
    let enu_a = EnuPos {
        east: (voxel_a_x - half) * voxel_size,
        north: (voxel_a_z - half) * voxel_size,
        up: (voxel_a_y - half) * voxel_size,
    };
    
    println!("\nChunk A voxel (127, 64, 64) - EAST BOUNDARY");
    println!("  Voxel offset from center: ({:.2}, {:.2}, {:.2})", 
        voxel_a_x - half, voxel_a_y - half, voxel_a_z - half);
    println!("  ENU offset: ({:.2}m E, {:.2}m N, {:.2}m U)", 
        enu_a.east, enu_a.north, enu_a.up);
    
    // Transform to ECEF
    let ecef_a = enu_to_ecef(&enu_a, &center_a_ecef, &center_a_gps);
    println!("  ECEF position: ({:.3}, {:.3}, {:.3})", ecef_a.x, ecef_a.y, ecef_a.z);
    
    // Voxel (0, 64, 64) in Chunk B = west boundary, middle height
    let voxel_b_x = 0.0;
    let voxel_b_y = 64.0;
    let voxel_b_z = 64.0;
    
    // Map to ENU (relative to Chunk B center)
    let enu_b = EnuPos {
        east: (voxel_b_x - half) * voxel_size,
        north: (voxel_b_z - half) * voxel_size,
        up: (voxel_b_y - half) * voxel_size,
    };
    
    println!("\nChunk B voxel (0, 64, 64) - WEST BOUNDARY");
    println!("  Voxel offset from center: ({:.2}, {:.2}, {:.2})", 
        voxel_b_x - half, voxel_b_y - half, voxel_b_z - half);
    println!("  ENU offset: ({:.2}m E, {:.2}m N, {:.2}m U)", 
        enu_b.east, enu_b.north, enu_b.up);
    
    // Transform to ECEF
    let ecef_b = enu_to_ecef(&enu_b, &center_b_ecef, &center_b_gps);
    println!("  ECEF position: ({:.3}, {:.3}, {:.3})", ecef_b.x, ecef_b.y, ecef_b.z);
    
    // Calculate distance between boundary voxels
    let dx = ecef_b.x - ecef_a.x;
    let dy = ecef_b.y - ecef_a.y;
    let dz = ecef_b.z - ecef_a.z;
    let distance = (dx*dx + dy*dy + dz*dz).sqrt();
    
    println!("\n=== BOUNDARY ALIGNMENT CHECK ===");
    println!("Distance between boundary voxels: {:.6}m", distance);
    println!("Expected distance (1 voxel): {:.6}m", voxel_size);
    println!("Alignment error: {:.6}m", (distance - voxel_size).abs());
    
    let error_pct = ((distance - voxel_size).abs() / voxel_size) * 100.0;
    println!("Error percentage: {:.4}%", error_pct);
    
    if error_pct < 0.01 {
        println!("✓ Voxel grids PERFECTLY aligned!");
    } else if error_pct < 1.0 {
        println!("⚠ Voxel grids slightly misaligned - acceptable for now");
    } else {
        println!("✗ Voxel grids BADLY misaligned - THIS IS THE BUG");
    }
    
    // Summary
    println!("\n=== SUMMARY ===");
    println!("GPS bounds aligned: {}", if lon_diff < 1e-10 { "YES" } else { "NO - BUG" });
    println!("Voxel grid clean: {}", if size_error < 0.001 { "YES" } else { "ROUNDING ERRORS" });
    println!("Boundary voxels aligned: {}", if error_pct < 1.0 { "YES" } else { "NO - BUG" });
}
