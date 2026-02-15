use metaverse_core::chunks::{ecef_to_cube_face, cube_to_sphere};
use metaverse_core::coordinates::{GpsPos, gps_to_ecef, ecef_to_gps, WGS84_A};

fn main() {
    let brisbane = GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 0.0,  // Use 0 for ground level
    };
    
    println!("=== Round-trip test ===");
    println!("Brisbane GPS: ({:.6}, {:.6})", brisbane.lat_deg, brisbane.lon_deg);
    
    let ecef_original = gps_to_ecef(&brisbane);
    println!("Original ECEF: ({:.1}, {:.1}, {:.1})", 
        ecef_original.x, ecef_original.y, ecef_original.z);
    
    let (face, u, v) = ecef_to_cube_face(&ecef_original);
    println!("\nCube face: {}, UV: ({:.6}, {:.6})", face, u, v);
    
    let ecef_reconstructed = cube_to_sphere(face, u, v, WGS84_A);
    println!("\nReconstructed ECEF: ({:.1}, {:.1}, {:.1})", 
        ecef_reconstructed.x, ecef_reconstructed.y, ecef_reconstructed.z);
    
    let gps_reconstructed = ecef_to_gps(&ecef_reconstructed);
    println!("Reconstructed GPS: ({:.6}, {:.6})", 
        gps_reconstructed.lat_deg, gps_reconstructed.lon_deg);
    
    let lat_error = (gps_reconstructed.lat_deg - brisbane.lat_deg).abs();
    let lon_error = (gps_reconstructed.lon_deg - brisbane.lon_deg).abs();
    println!("\nError: lat={:.6}°, lon={:.6}°", lat_error, lon_error);
    
    if lat_error > 0.001 || lon_error > 0.001 {
        println!("ERROR: Round-trip mismatch!");
    } else {
        println!("✓ Round-trip successful");
    }
}
