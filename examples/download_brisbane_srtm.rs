//! Download and cache Brisbane SRTM tile

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Downloading Brisbane SRTM Tile ===\n");
    
    let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY")
        .expect("OPENTOPOGRAPHY_API_KEY not set");
    
    // Brisbane tile: S28E153 (covers -28 to -27, 153 to 154)
    let url = format!(
        "https://portal.opentopography.org/API/globaldem?demtype=SRTMGL1&south={}&north={}&west={}&east={}&outputFormat=GTiff&API_Key={}",
        -28, -27, 153, 154, api_key
    );
    
    println!("Downloading from OpenTopography...");
    println!("URL: {}", &url[..100]);
    
    let response = reqwest::blocking::get(&url)?;
    
    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), response.status().canonical_reason().unwrap_or("Unknown")).into());
    }
    
    let bytes = response.bytes()?;
    println!("✓ Downloaded {} bytes", bytes.len());
    
    // Save to cache as GeoTIFF (we'll convert later)
    let cache_dir = dirs::home_dir()
        .unwrap()
        .join(".metaverse/cache/srtm");
    std::fs::create_dir_all(&cache_dir)?;
    
    let cache_path = cache_dir.join("S28E153.tif");
    std::fs::write(&cache_path, &bytes)?;
    println!("✓ Cached to: {:?}", cache_path);
    
    println!("\n✓ SUCCESS!");
    println!("Brisbane SRTM tile is now cached.");
    println!("Next: Convert GeoTIFF to .hgt format for fast loading.");
    
    Ok(())
}
