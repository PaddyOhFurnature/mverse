//! Convert Brisbane GeoTIFF to .hgt format

use metaverse_core::geotiff_converter::geotiff_to_hgt;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Converting Brisbane GeoTIFF to .hgt ===\n");
    
    let cache_dir = dirs::home_dir()
        .unwrap()
        .join(".metaverse/cache/srtm");
    
    let tif_path = cache_dir.join("S28E153.tif");
    let hgt_path = cache_dir.join("S28E153.hgt");
    
    if !tif_path.exists() {
        eprintln!("✗ GeoTIFF not found: {:?}", tif_path);
        eprintln!("Run download_brisbane_srtm first!");
        return Err("GeoTIFF not found".into());
    }
    
    println!("Reading GeoTIFF: {:?}", tif_path);
    let tif_bytes = std::fs::read(&tif_path)?;
    println!("  File size: {} bytes", tif_bytes.len());
    
    println!("\nConverting to .hgt format...");
    let hgt_bytes = geotiff_to_hgt(&tif_bytes)?;
    
    println!("\nWriting .hgt file: {:?}", hgt_path);
    std::fs::write(&hgt_path, &hgt_bytes)?;
    println!("  File size: {} bytes", hgt_bytes.len());
    
    println!("\n✓ SUCCESS!");
    println!("Brisbane SRTM tile converted to .hgt format.");
    println!("Cache contains both:");
    println!("  - S28E153.tif (GeoTIFF original)");
    println!("  - S28E153.hgt (converted for fast loading)");
    
    Ok(())
}
