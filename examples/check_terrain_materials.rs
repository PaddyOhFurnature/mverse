use metaverse_core::continuous_world::ContinuousWorld;

fn main() {
    let cache_dir = std::path::PathBuf::from("/home/main/.metaverse/cache");
    let world = ContinuousWorld::new(cache_dir).expect("Failed to create world");
    
    // Brisbane test location - check some blocks
    let test_positions = vec![
        (-27.4786, 153.0338, 5.0),   // Ground level
        (-27.4786, 153.0338, 15.0),  // 15m up
        (-27.4786, 153.0338, 30.0),  // 30m up
        (-27.4786, 153.0338, 50.0),  // 50m up
    ];
    
    for (lat, lon, alt) in test_positions {
        match world.query_voxel(lat, lon, alt) {
            Ok(material) => {
                let name = match material {
                    0 => "AIR",
                    1 => "STONE",
                    2 => "DIRT",
                    8 => "GRASS",
                    10 => "WATER",
                    11 => "CONCRETE",
                    _ => "UNKNOWN",
                };
                println!("At ({:.4}, {:.4}, {:.1}m): Material {} ({})", lat, lon, alt, material, name);
            }
            Err(e) => println!("At ({:.4}, {:.4}, {:.1}m): Error: {}", lat, lon, alt, e),
        }
    }
}
