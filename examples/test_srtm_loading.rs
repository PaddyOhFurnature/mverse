//! Test if SRTM data is loading

use metaverse_core::srtm_cache::SrtmCache;
use std::path::PathBuf;

const TEST_LAT: i16 = -28;  // Brisbane is -27.x, so tile is -28
const TEST_LON: i16 = 153;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== SRTM Data Loading Test ===\n");
    
    let cache_dir = dirs::home_dir()
        .unwrap()
        .join(".metaverse")
        .join("cache")
        .join("srtm");
    
    println!("Cache directory: {:?}", cache_dir);
    println!("Exists: {}", cache_dir.exists());
    
    if cache_dir.exists() {
        let files: Vec<_> = std::fs::read_dir(&cache_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        println!("Files in cache: {}", files.len());
        if !files.is_empty() {
            println!("First 5 files:");
            for f in files.iter().take(5) {
                println!("  {}", f);
            }
        }
    }
    
    println!("\nAttempting to load SRTM cache...");
    let cache = SrtmCache::new(cache_dir.clone())?;
    
    println!("\nTrying to get tile for Brisbane ({}, {})...", TEST_LAT, TEST_LON);
    match cache.get_tile(TEST_LAT, TEST_LON) {
        Ok(tile) => {
            println!("✓ SUCCESS: Got tile!");
            println!("  SW corner: ({}, {})", tile.sw_lat, tile.sw_lon);
            println!("  Resolution: {:?}", tile.resolution);
            println!("  Sample elevations:");
            for i in 0..5 {
                if i < tile.elevations.len() {
                    println!("    [{}] = {}", i, tile.elevations[i]);
                }
            }
        }
        Err(e) => {
            println!("✗ ERROR: {}", e);
        }
    }
    
    println!("\nChecking API key...");
    match std::env::var("OPENTOPOGRAPHY_API_KEY") {
        Ok(key) => println!("✓ API key set: {}...{}", &key[..8], &key[key.len()-4..]),
        Err(_) => println!("✗ API key NOT set"),
    }
    
    Ok(())
}
