use metaverse_core::coordinates::{GpsPos, gps_to_ecef, ecef_to_gps, EcefPos, WGS84_A};

fn cube_to_sphere_test(face: u8, u: f64, v: f64, radius: f64, perm: usize) -> EcefPos {
    // Snyder's equal-area projection formulas
    let x_prime = u * (1.0 - v * v / 2.0).sqrt();
    let y_prime = v * (1.0 - u * u / 2.0).sqrt();
    let z_prime = (0.0_f64.max(1.0 - u * u / 2.0 - v * v / 2.0)).sqrt();
    
    // Test different permutations for face 1
    let (x, y, z) = if face == 1 {
        match perm {
            0 => (-z_prime, -x_prime, y_prime),   // Current
            1 => (-z_prime, x_prime, -y_prime),   // Original
            2 => (-z_prime, x_prime, y_prime),
            3 => (-z_prime, -x_prime, -y_prime),
            4 => (z_prime, -x_prime, y_prime),
            5 => (z_prime, x_prime, -y_prime),
            6 => (z_prime, x_prime, y_prime),
            7 => (z_prime, -x_prime, -y_prime),
            _ => panic!(),
        }
    } else {
        (-z_prime, -x_prime, y_prime)
    };
    
    let magnitude = (x * x + y * y + z * z).sqrt();
    EcefPos {
        x: x / magnitude * radius,
        y: y / magnitude * radius,
        z: z / magnitude * radius,
    }
}

fn main() {
    let brisbane = GpsPos { lat_deg: -27.469800, lon_deg: 153.025100, elevation_m: 0.0 };
    let ecef_orig = gps_to_ecef(&brisbane);
    
    // Face 1, UV from ecef_to_cube_face
    let u = -0.508974;
    let v = -0.579459;
    
    println!("Testing all 8 permutations for face 1:");
    println!("Original: ({:.1}, {:.1}, {:.1})", ecef_orig.x, ecef_orig.y, ecef_orig.z);
    
    for perm in 0..8 {
        let reconstructed = cube_to_sphere_test(1, u, v, WGS84_A, perm);
        let gps = ecef_to_gps(&reconstructed);
        let lat_err = (gps.lat_deg - brisbane.lat_deg).abs();
        let lon_err = (gps.lon_deg - brisbane.lon_deg).abs();
        let total_err = lat_err + lon_err;
        
        println!("{}: GPS({:.2}, {:.2}) error={:.3}°", 
            perm, gps.lat_deg, gps.lon_deg, total_err);
    }
}
