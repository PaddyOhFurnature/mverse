//! Download OSM and SRTM data for Brisbane
//!
//! This script downloads and caches data for testing the viewer.
//! Run with: cargo run --example download_brisbane_data

use metaverse_core::cache::DiskCache;
use metaverse_core::osm::{OverpassClient, load_chunk_osm_data};
use metaverse_core::chunks::ChunkId;
use metaverse_core::coordinates::{gps_to_ecef, GpsPos};
use std::time::Duration;
use std::io::Write;

fn main() {
    println!("=== Brisbane Data Downloader ===\n");
    
    // Initialize cache
    let cache = DiskCache::new().expect("Failed to create cache");
    println!("Cache location: .metaverse/cache/");
    
    // Download SRTM tiles for Brisbane area
    println!("\n--- Downloading SRTM Elevation Data ---");
    download_srtm_tiles(&cache);
    
    // Download OSM data for Brisbane
    println!("\n--- Downloading OSM Data ---");
    download_osm_data(&cache);
    
    println!("\n=== Download Complete ===");
    println!("You can now run the viewer: cargo run --example viewer --release");
}

fn download_srtm_tiles(cache: &DiskCache) {
    // Brisbane is at approximately -27.5°, 153°
    // We need tiles: S28E153 (covers Brisbane CBD)
    // Also download neighbors for terrain continuity
    let tiles = vec![
        ("S28E153", -28, 153),  // Brisbane CBD
        ("S27E153", -27, 153),  // North of Brisbane
        ("S28E152", -28, 152),  // West of Brisbane
    ];
    
    for (name, lat, lon) in tiles {
        print!("Checking {} ... ", name);
        std::io::stdout().flush().unwrap();
        
        // Check if already cached
        let filename = format!("{}.hgt", name);
        if cache.read_srtm(&filename).is_ok() {
            println!("already cached");
            continue;
        }
        
        println!("downloading...");
        
        // Try multiple providers with delays
        let providers = vec![
            format!("https://viewfinderpanoramas.org/dem3/{}.hgt", name),
            format!("https://viewfinderpanoramas.org/dem3/{}.zip", name),
            format!("https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/SRTM_Data_GeoTiff/{}.zip", name),
        ];
        
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("MetaverseCore/0.1")
            .build()
            .expect("Failed to create HTTP client");
        
        let mut success = false;
        for url in &providers {
            println!("  Trying: {}", url);
            std::thread::sleep(Duration::from_secs(2)); // Rate limiting
            
            match client.get(url).send() {
                Ok(resp) if resp.status().is_success() => {
                    match resp.bytes() {
                        Ok(bytes) => {
                            let data = bytes.to_vec();
                            
                            // Check if ZIP file
                            if data.len() >= 2 && &data[0..2] == b"PK" {
                                println!("  Downloaded ZIP, extracting...");
                                match extract_hgt_from_zip(&data) {
                                    Some(hgt_data) => {
                                        if cache.write_srtm(&filename, &hgt_data).is_ok() {
                                            println!("  ✓ Cached {}", filename);
                                            success = true;
                                            break;
                                        }
                                    }
                                    None => println!("  Failed to extract HGT from ZIP"),
                                }
                            } else {
                                // Raw HGT
                                if cache.write_srtm(&filename, &data).is_ok() {
                                    println!("  ✓ Cached {}", filename);
                                    success = true;
                                    break;
                                }
                            }
                        }
                        Err(e) => println!("  Failed to read response: {}", e),
                    }
                }
                Ok(resp) => println!("  HTTP {}", resp.status()),
                Err(e) => println!("  Connection failed: {}", e),
            }
        }
        
        if !success {
            println!("  ✗ Failed to download {}", name);
        }
    }
}

fn extract_hgt_from_zip(zip_data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Cursor;
    
    let cursor = Cursor::new(zip_data);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).ok()?;
        let name = file.name().to_string();
        
        if name.to_lowercase().ends_with(".hgt") {
            let mut buffer = Vec::new();
            std::io::copy(&mut file, &mut buffer).ok()?;
            return Some(buffer);
        }
    }
    
    None
}

fn download_osm_data(cache: &DiskCache) {
    let client = OverpassClient::new(2); // 2-second cooldown
    
    // Brisbane CBD chunk (depth 14)
    // This is an approximation - adjust based on actual chunk system
    let brisbane_gps = GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    };
    
    println!("Brisbane CBD: lat={}, lon={}", brisbane_gps.lat_deg, brisbane_gps.lon_deg);
    
    // For now, download a simple bounding box around Brisbane CBD
    let min_lat = -27.50;
    let max_lat = -27.44;
    let min_lon = 153.00;
    let max_lon = 153.06;
    
    print!("Downloading OSM data for Brisbane CBD ... ");
    std::io::stdout().flush().unwrap();
    
    match client.query_bbox(min_lat, min_lon, max_lat, max_lon) {
        Ok(json) => {
            println!("✓ Downloaded OSM data");
            
            // Parse and cache
            match metaverse_core::osm::parse_overpass_response(&json) {
                Ok(osm_data) => {
                    println!("  Buildings: {}", osm_data.buildings.len());
                    println!("  Roads: {}", osm_data.roads.len());
                    
                    // Cache it with a simple key
                    let cache_key = "brisbane_cbd";
                    match serde_json::to_vec(&osm_data) {
                        Ok(serialized) => {
                            if cache.write_osm(cache_key, &serialized).is_ok() {
                                println!("  ✓ Cached as '{}'", cache_key);
                            } else {
                                println!("  ✗ Failed to cache");
                            }
                        }
                        Err(e) => println!("  ✗ Serialization failed: {}", e),
                    }
                }
                Err(e) => println!("  ✗ Parse failed: {}", e),
            }
        }
        Err(e) => {
            println!("✗ Failed: {}", e);
            println!("  This is often due to rate limiting or network issues.");
            println!("  Try again in a few minutes.");
        }
    }
}
