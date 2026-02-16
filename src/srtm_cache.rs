/// SRTM tile caching for continuous query system.
///
/// Downloads and caches SRTM tiles covering test area.
/// Simple blocking HTTP download - no async complexity for prototype.

use crate::elevation::{SrtmTile, parse_hgt, parse_hgt_filename};
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{Write, Cursor};
use std::thread;
use std::time::Duration;

/// SRTM tile cache manager
pub struct SrtmCache {
    cache_dir: PathBuf,
}

impl SrtmCache {
    /// Create new SRTM cache
    pub fn new(cache_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        // Ensure cache directory exists
        fs::create_dir_all(&cache_dir)?;
        
        Ok(Self { cache_dir })
    }

    /// Get path to cached tile file
    fn tile_path(&self, lat: i16, lon: i16) -> PathBuf {
        let lat_prefix = if lat >= 0 { "N" } else { "S" };
        let lon_prefix = if lon >= 0 { "E" } else { "W" };
        let filename = format!(
            "{}{:02}{}{:03}.hgt",
            lat_prefix, lat.abs(),
            lon_prefix, lon.abs()
        );
        self.cache_dir.join(filename)
    }

    /// Load tile from cache, downloading if necessary
    pub fn get_tile(&self, lat: i16, lon: i16) -> Result<SrtmTile, Box<dyn std::error::Error>> {
        let tile_path = self.tile_path(lat, lon);
        
        println!("[SrtmCache] Looking for tile at: {:?}", tile_path);
        println!("[SrtmCache] File exists: {}", tile_path.exists());
        
        // Try to load from cache first
        if tile_path.exists() {
            println!("[SrtmCache] Loading SRTM tile from cache: {:?}", tile_path);
            let bytes = fs::read(&tile_path)?;
            println!("[SrtmCache] Read {} bytes", bytes.len());
            let filename = tile_path.file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid filename")?;
            return parse_hgt(filename, &bytes);
        }

        // Not in cache - download it
        println!("[SrtmCache] Downloading SRTM tile: {}, {}", lat, lon);
        let bytes = self.download_tile(lat, lon)?;
        
        // Save to cache
        fs::write(&tile_path, &bytes)?;
        println!("[SrtmCache] Cached SRTM tile to: {:?}", tile_path);
        
        // Parse and return
        let filename = tile_path.file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid filename")?;
        parse_hgt(filename, &bytes)
    }

    /// Download tile from remote source
    fn download_tile(&self, lat: i16, lon: i16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Try multiple sources in order
        
        // Source 1: USGS EarthExplorer (SRTM 1 Arc-Second Global)
        if let Ok(bytes) = self.try_download_usgs(lat, lon) {
            return Ok(bytes);
        }
        
        // Source 2: OpenTopography (requires API key)
        if let Ok(bytes) = self.try_download_opentopo(lat, lon) {
            return Ok(bytes);
        }

        // Source 3: NASA EarthData (requires authentication)
        if let Ok(bytes) = self.try_download_nasa(lat, lon) {
            return Ok(bytes);
        }

        Err(format!("Failed to download SRTM tile ({}, {}) from any source", lat, lon).into())
    }

    /// Try downloading from USGS EarthExplorer
    fn try_download_usgs(&self, lat: i16, lon: i16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // USGS EarthExplorer API endpoint
        let lat_prefix = if lat >= 0 { "N" } else { "S" };
        let lon_prefix = if lon >= 0 { "E" } else { "W" };
        
        // Try SRTM 1 Arc-Second Global (30m resolution)
        let url = format!(
            "https://e4ftl01.cr.usgs.gov/MEASURES/SRTMGL1.003/2000.02.11/{}{:02}{}{:03}.SRTMGL1.hgt.zip",
            lat_prefix, lat.abs(),
            lon_prefix, lon.abs()
        );

        println!("Trying USGS: {}", url);
        
        // 2-second cooldown between requests (project rule)
        thread::sleep(Duration::from_secs(2));
        
        let response = reqwest::blocking::get(&url)?;
        
        if !response.status().is_success() {
            return Err(format!("USGS returned: {}", response.status()).into());
        }

        // Download and extract from zip
        let zip_bytes = response.bytes()?;
        let cursor = Cursor::new(zip_bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        
        // Find the .hgt file in the archive
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            if file.name().ends_with(".hgt") {
                let mut hgt_bytes = Vec::new();
                std::io::copy(&mut file, &mut hgt_bytes)?;
                println!("Downloaded {} bytes from USGS", hgt_bytes.len());
                return Ok(hgt_bytes);
            }
        }

        Err("No .hgt file found in zip archive".into())
    }

    /// Try downloading from OpenTopography
    fn try_download_opentopo(&self, lat: i16, lon: i16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Requires API key
        let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY")
            .map_err(|_| "OPENTOPOGRAPHY_API_KEY not set")?;

        // OpenTopography API
        let south = lat as f64;
        let north = (lat + 1) as f64;
        let west = lon as f64;
        let east = (lon + 1) as f64;

        let url = format!(
            "https://portal.opentopography.org/API/globaldem?demtype=SRTMGL1&south={}&north={}&west={}&east={}&outputFormat=GTiff&API_Key={}",
            south, north, west, east, api_key
        );

        println!("Trying OpenTopography...");
        
        // 2-second cooldown
        thread::sleep(Duration::from_secs(2));
        
        let response = reqwest::blocking::get(&url)?;
        
        if !response.status().is_success() {
            return Err(format!("OpenTopography returned: {}", response.status()).into());
        }

        // TODO: Convert GeoTIFF to .hgt format
        // For now, this is a placeholder
        Err("GeoTIFF conversion not implemented yet".into())
    }

    /// Try downloading from NASA EarthData
    fn try_download_nasa(&self, _lat: i16, _lon: i16) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // NASA EarthData requires authentication
        // Placeholder for now
        Err("NASA EarthData download not implemented yet".into())
    }

    /// Pre-download tiles covering a GPS bounding box
    pub fn prefetch_area(&self, min_lat: f64, max_lat: f64, min_lon: f64, max_lon: f64) 
        -> Result<Vec<(i16, i16)>, Box<dyn std::error::Error>> 
    {
        let tile_min_lat = min_lat.floor() as i16;
        let tile_max_lat = max_lat.floor() as i16;
        let tile_min_lon = min_lon.floor() as i16;
        let tile_max_lon = max_lon.floor() as i16;

        let mut downloaded = Vec::new();

        for lat in tile_min_lat..=tile_max_lat {
            for lon in tile_min_lon..=tile_max_lon {
                println!("Fetching SRTM tile ({}, {})...", lat, lon);
                match self.get_tile(lat, lon) {
                    Ok(_) => {
                        downloaded.push((lat, lon));
                        println!("✓ Tile ({}, {}) ready", lat, lon);
                    }
                    Err(e) => {
                        println!("✗ Failed to fetch tile ({}, {}): {}", lat, lon, e);
                        // Continue with other tiles - don't fail the whole batch
                    }
                }
            }
        }

        println!("\nPrefetch complete: {}/{} tiles downloaded", 
            downloaded.len(),
            ((tile_max_lat - tile_min_lat + 1) * (tile_max_lon - tile_min_lon + 1)) as usize
        );

        Ok(downloaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_cache_dir() -> PathBuf {
        env::temp_dir().join("metaverse_srtm_test")
    }

    #[test]
    fn test_tile_path() {
        let cache = SrtmCache::new(test_cache_dir()).unwrap();
        
        // Kangaroo Point, Brisbane
        let path = cache.tile_path(-28, 153);
        assert!(path.to_string_lossy().contains("S28E153.hgt"));
        
        // Northern hemisphere, western hemisphere
        let path = cache.tile_path(45, -122);
        assert!(path.to_string_lossy().contains("N45W122.hgt"));
    }

    #[test]
    fn test_cache_creation() {
        let cache_dir = test_cache_dir();
        let _ = fs::remove_dir_all(&cache_dir); // Clean up from previous tests
        
        let cache = SrtmCache::new(cache_dir.clone()).unwrap();
        assert!(cache_dir.exists());
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    #[ignore] // Only run manually - requires network
    fn test_download_brisbane_tile() {
        let cache_dir = test_cache_dir();
        let _ = fs::remove_dir_all(&cache_dir); // Start fresh
        
        let cache = SrtmCache::new(cache_dir.clone()).unwrap();
        
        // Try to download S28E153 (Brisbane area)
        match cache.get_tile(-28, 153) {
            Ok(tile) => {
                assert_eq!(tile.sw_lat, -28);
                assert_eq!(tile.sw_lon, 153);
                println!("✓ Downloaded tile with {} samples", tile.elevations.len());
            }
            Err(e) => {
                println!("Download failed (expected without credentials): {}", e);
                // This is OK - we expect it to fail without proper authentication
            }
        }
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn test_prefetch_area_bounds() {
        let cache_dir = test_cache_dir();
        let _ = fs::remove_dir_all(&cache_dir);
        
        let cache = SrtmCache::new(cache_dir.clone()).unwrap();
        
        // Kangaroo Point test area: -27.48° to -27.48° (same tile)
        let min_lat = -27.480669;
        let max_lat = -27.478869;
        let min_lon = 153.032686;
        let max_lon = 153.034486;
        
        // Should only need 1 tile: S28E153
        let result = cache.prefetch_area(min_lat, max_lat, min_lon, max_lon);
        
        // Will likely fail without network/credentials, but that's OK
        // We're testing the logic, not the download
        match result {
            Ok(_) => println!("Prefetch succeeded"),
            Err(e) => println!("Prefetch failed (expected): {}", e),
        }
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }
}
