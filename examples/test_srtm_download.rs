/// Test script to download SRTM tile for Brisbane Story Bridge area
/// 
/// This script tests the new async multi-source downloader.
/// Location: Story Bridge (-27.463697, 153.035725)
/// Required tile: S28E153.hgt (covers -28° to -27° lat, 153° to 154° lon)

use metaverse_core::{cache::DiskCache, srtm_downloader::SrtmDownloader};

#[tokio::main]
async fn main() {
    env_logger::init();
    
    println!("=== SRTM Tile Download Test ===\n");
    
    // Story Bridge location
    let lat: f64 = -27.463697;
    let lon: f64 = 153.035725;
    println!("Target location: Story Bridge");
    println!("GPS: {:.6}, {:.6}\n", lat, lon);
    
    // Determine required tile (tile named by SW corner)
    let tile_lat = lat.floor() as i16;  // -28
    let tile_lon = lon.floor() as i16;  // 153
    println!("Required SRTM tile: S{:02}E{:03}.hgt\n", tile_lat.abs(), tile_lon);
    
    // Create cache and downloader
    let cache = DiskCache::new().expect("Failed to create cache");
    println!("Disk cache: {}\n", cache.root().display());
    
    let downloader = SrtmDownloader::new(cache.clone());
    println!("Downloader initialized with sources:");
    
    // Check for API keys
    if std::env::var("OPENTOPOGRAPHY_API_KEY").is_ok() {
        println!("  ✓ OpenTopography API key found");
    } else {
        println!("  ✗ OpenTopography API key not set (export OPENTOPOGRAPHY_API_KEY=your_key)");
        println!("    Get free key at: https://portal.opentopography.org/");
    }
    println!("  ✓ AWS Terrain Tiles (no auth required)");
    println!("  ✓ CGIAR Direct (may 404)");
    println!();
    
    // Try to download the tile
    println!("Starting download...\n");
    println!("Note: First request to each provider has 2-second cooldown (project rule)");
    println!("      Failed providers retry with exponential backoff (2s, 4s, 8s)\n");
    
    match downloader.download_tile(tile_lat, tile_lon).await {
        Some(tile) => {
            println!("\n=== SUCCESS ===");
            println!("Tile downloaded and parsed successfully!");
            println!("Resolution: {:?}", tile.resolution);
            println!("Samples: {}×{}", 
                    tile.resolution.samples(), 
                    tile.resolution.samples());
            println!("SW corner: ({}, {})", tile.sw_lat, tile.sw_lon);
            println!("Data points: {}", tile.elevations.len());
            
            // Check for void values
            let voids = tile.elevations.iter().filter(|&&e| e == -32768).count();
            let non_voids = tile.elevations.len() - voids;
            println!("Valid elevations: {} ({:.1}%)", 
                    non_voids, 
                    100.0 * non_voids as f64 / tile.elevations.len() as f64);
            
            if voids > 0 {
                println!("Void values: {} ({:.1}%)", 
                        voids, 
                        100.0 * voids as f64 / tile.elevations.len() as f64);
            }
            
            // Query elevation at Story Bridge location
            println!("\n=== Story Bridge Elevation ===");
            if let Some(elev) = metaverse_core::elevation::get_elevation(&tile, lat, lon) {
                println!("Elevation at bridge: {:.2}m above sea level", elev);
                println!("Expected: ~5-30m (river to bridge deck)");
                
                // Verify it's in reasonable range
                if elev >= -10.0 && elev <= 100.0 {
                    println!("✓ Elevation is in reasonable range for Brisbane");
                } else {
                    println!("⚠ Elevation seems unusual for Brisbane (expected 0-50m)");
                }
            } else {
                println!("✗ Failed to query elevation (coordinate might be in void area)");
            }
            
            // Sample a few other points for validation
            println!("\n=== Additional Sample Points ===");
            let samples = vec![
                (-27.460, 153.030, "River"),
                (-27.468, 153.037, "Kangaroo Point cliffs"),
                (-27.475, 153.025, "Parklands"),
            ];
            
            for (lat, lon, name) in samples {
                if let Some(elev) = metaverse_core::elevation::get_elevation(&tile, lat, lon) {
                    println!("{}: {:.2}m", name, elev);
                } else {
                    println!("{}: No data", name);
                }
            }
            
            println!("\n✓ Tile successfully cached at: {}/srtm/",
                    cache.root().display());
            println!("  Future runs will load from cache (no network request)");
        }
        None => {
            println!("\n=== FAILED ===");
            println!("All sources failed to provide tile data.");
            println!("\nTroubleshooting:");
            println!("1. Check internet connection");
            println!("2. Set OpenTopography API key (most reliable source):");
            println!("   export OPENTOPOGRAPHY_API_KEY=your_key");
            println!("   Get key at: https://portal.opentopography.org/");
            println!("3. Check SRTM_SOURCES_2026.md for alternative sources");
            println!("4. View logs above for specific error messages");
            std::process::exit(1);
        }
    }
}
