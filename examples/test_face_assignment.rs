use metaverse_core::chunks::ecef_to_cube_face;
use metaverse_core::coordinates::{GpsPos, gps_to_ecef};

fn main() {
    let brisbane = GpsPos {
        lat_deg: -27.469800,
        lon_deg: 153.025100,
        elevation_m: 100.0,
    };
    
    println!("Brisbane GPS: ({:.6}, {:.6})", brisbane.lat_deg, brisbane.lon_deg);
    
    let ecef = gps_to_ecef(&brisbane);
    println!("Brisbane ECEF: ({:.1}, {:.1}, {:.1})", ecef.x, ecef.y, ecef.z);
    println!("  |X|={:.1}, |Y|={:.1}, |Z|={:.1}", ecef.x.abs(), ecef.y.abs(), ecef.z.abs());
    
    let (face, u, v) = ecef_to_cube_face(&ecef);
    println!("\nCube face assignment:");
    println!("  Face: {}", face);
    println!("  UV: ({:.6}, {:.6})", u, v);
    
    // Brisbane is 153°E longitude → X is very negative, Y is positive
    // Brisbane is -27° latitude → Z is negative (southern hemisphere)
    println!("\nExpected face:");
    println!("  X < 0, Y > 0, Z < 0");
    println!("  |X| = {:.1} vs |Y| = {:.1} vs |Z| = {:.1}", 
        ecef.x.abs(), ecef.y.abs(), ecef.z.abs());
    
    if ecef.x.abs() >= ecef.y.abs() && ecef.x.abs() >= ecef.z.abs() {
        println!("  → X is dominant, so face 0 (+X) or 1 (-X)");
        if ecef.x < 0.0 {
            println!("  → X < 0, so face 1 (-X antimeridian)");
        }
    }
}
