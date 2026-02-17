// Quick test to check SRTM elevation values at cliff location
use metaverse_core::srtm_loader::SrtmLoader;
use metaverse_core::coordinates::Coordinate;

fn main() {
    let loader = SrtmLoader::new();
    
    // Kangaroo Point Cliffs area - sample points across cliff
    let points = vec![
        (-27.4796, 153.0336, "Top of cliff (parking area)"),
        (-27.4798, 153.0334, "Middle of cliff face"),
        (-27.4800, 153.0332, "Bottom near river"),
    ];
    
    println!("SRTM Elevation Data at Kangaroo Point Cliffs:");
    println!("(Expected: ~30m drop from top to bottom)\n");
    
    for (lat, lon, desc) in points {
        let coord = Coordinate::from_lat_lon_alt(lat, lon, 0.0);
        match loader.get_elevation(coord) {
            Some(elev) => {
                println!("{:40} Lat: {:.6}, Lon: {:.6} -> {:6.1}m", desc, lat, lon, elev);
            }
            None => {
                println!("{:40} NO DATA", desc);
            }
        }
    }
}
