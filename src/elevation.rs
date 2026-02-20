//! Elevation data pipeline - Phase 2
//!
//! Multi-source redundant pipeline for SRTM elevation data:
//! 1. Local cache (./elevation_cache/)
//! 2. NAS file (/mnt/nas/srtm-v3-1s.tif) - when available
//! 3. OpenTopography API (on-demand)
//! 4. NASA Earthdata (fallback)
//!
//! See: docs/SRTM_REDUNDANT_PIPELINE.md

use std::path::PathBuf;
use crate::coordinates::GPS;

/// Elevation query result
#[derive(Debug, Clone, Copy)]
pub struct Elevation {
    pub meters: f64,
}

/// Elevation data source trait
pub trait ElevationSource {
    /// Query elevation at GPS coordinate
    /// Returns None if data unavailable for this location
    fn query(&mut self, gps: &GPS) -> Result<Option<Elevation>, ElevationError>;
    
    /// Source name for logging
    fn name(&self) -> &str;
}

/// Elevation query errors
#[derive(Debug)]
pub enum ElevationError {
    NetworkError(String),
    FileNotFound(String),
    ParseError(String),
    RateLimited,
    OutOfBounds,
}

impl std::fmt::Display for ElevationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElevationError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            ElevationError::FileNotFound(path) => write!(f, "File not found: {}", path),
            ElevationError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ElevationError::RateLimited => write!(f, "Rate limited"),
            ElevationError::OutOfBounds => write!(f, "Location out of bounds"),
        }
    }
}

impl std::error::Error for ElevationError {}

/// OpenTopography API client
pub struct OpenTopographySource {
    api_key: String,
    cache_dir: PathBuf,
    last_request: Option<std::time::Instant>,
}

/// NAS global SRTM file source
pub struct NasFileSource {
    file_path: PathBuf,
}

impl NasFileSource {
    /// Create NAS file source
    /// 
    /// Tries multiple paths:
    /// 1. ./srtm-global.tif (symlink in project)
    /// 2. /mnt/nas/srtm-v3-1s.tif (if mounted)
    /// 3. GVFS path (if provided)
    pub fn new() -> Option<Self> {
        let candidates = vec![
            PathBuf::from("./srtm-global.tif"),
            PathBuf::from("/mnt/nas/srtm-v3-1s.tif"),
            PathBuf::from("/run/user/1000/gvfs/smb-share:server=blade.local,share=homes/world/srtm-v3-1s.tif"),
            PathBuf::from("/run/user/1000/gvfs/afp-volume:host=Blade.local,user=media,volume=homes/world/srtm-v3-1s.tif"),
        ];
        
        for path in candidates {
            if path.exists() {
                println!("Found NAS SRTM file at: {}", path.display());
                return Some(Self { file_path: path });
            }
        }
        
        eprintln!("NAS SRTM file not found, will use API fallback");
        None
    }
    
    /// Create with explicit path
    pub fn with_path(path: PathBuf) -> Option<Self> {
        if path.exists() {
            Some(Self { file_path: path })
        } else {
            None
        }
    }
}

impl ElevationSource for NasFileSource {
    fn query(&mut self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        // Query directly from global GeoTIFF
        // This is a single 200GB file covering the entire world
        extract_elevation(&self.file_path, gps).map(Some)
    }
    
    fn name(&self) -> &str {
        "NAS Global SRTM File"
    }
}

impl OpenTopographySource {
    /// Create OpenTopography source with API key
    pub fn new(api_key: String, cache_dir: PathBuf) -> Self {
        Self {
            api_key,
            cache_dir,
            last_request: None,
        }
    }
    
    /// Fetch tile from API (respects 2-second rate limit per RULES.md)
    fn fetch_tile(&mut self, lat: i32, lon: i32) -> Result<PathBuf, ElevationError> {
        // Rate limiting: 2-second cooldown
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < std::time::Duration::from_secs(2) {
                let wait = std::time::Duration::from_secs(2) - elapsed;
                std::thread::sleep(wait);
            }
        }
        
        // Construct API request
        // Request 1° tile centered on (lat, lon)
        let south = lat as f64;
        let north = (lat + 1) as f64;
        let west = lon as f64;
        let east = (lon + 1) as f64;
        
        let url = format!(
            "https://portal.opentopography.org/API/globaldem?\
             demtype=SRTMGL1&\
             south={}&north={}&west={}&east={}&\
             outputFormat=GTiff&\
             API_Key={}",
            south, north, west, east, self.api_key
        );
        
        // Download tile
        let response = reqwest::blocking::get(&url)
            .map_err(|e| ElevationError::NetworkError(e.to_string()))?;
        
        self.last_request = Some(std::time::Instant::now());
        
        if !response.status().is_success() {
            return Err(ElevationError::NetworkError(
                format!("HTTP {}", response.status())
            ));
        }
        
        // Save to cache
        let tile_path = self.cache_dir
            .join(format!("N{:02}", lat.abs()))
            .join(format!("E{:03}", lon.abs()))
            .join(format!("srtm_n{:02}_e{:03}.tif", lat.abs(), lon.abs()));
        
        std::fs::create_dir_all(tile_path.parent().unwrap())
            .map_err(|e| ElevationError::FileNotFound(e.to_string()))?;
        
        let bytes = response.bytes()
            .map_err(|e| ElevationError::NetworkError(e.to_string()))?;
        
        std::fs::write(&tile_path, &bytes)
            .map_err(|e| ElevationError::FileNotFound(e.to_string()))?;
        
        Ok(tile_path)
    }
}

impl ElevationSource for OpenTopographySource {
    fn query(&mut self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        // Determine which 1° tile contains this point
        let lat_tile = gps.lat.floor() as i32;
        let lon_tile = gps.lon.floor() as i32;
        
        // Check cache first
        let tile_path = self.cache_dir
            .join(format!("N{:02}", lat_tile.abs()))
            .join(format!("E{:03}", lon_tile.abs()))
            .join(format!("srtm_n{:02}_e{:03}.tif", lat_tile.abs(), lon_tile.abs()));
        
        let tile_file = if tile_path.exists() {
            tile_path
        } else {
            // Fetch from API
            self.fetch_tile(lat_tile, lon_tile)?
        };
        
        // Parse GeoTIFF and extract elevation
        let elevation = extract_elevation(&tile_file, gps)?;
        Ok(Some(elevation))
    }
    
    fn name(&self) -> &str {
        "OpenTopography API"
    }
}

/// Extract elevation from GeoTIFF at given GPS coordinate
fn extract_elevation(tiff_path: &PathBuf, gps: &GPS) -> Result<Elevation, ElevationError> {
    use gdal::Dataset;
    use gdal::raster::ResampleAlg;
    
    // Open dataset (doesn't load into memory)
    let dataset = Dataset::open(tiff_path)
        .map_err(|e| ElevationError::FileNotFound(format!("GDAL open failed: {}", e)))?;
    
    // Get raster band (elevation is usually band 1)
    let rasterband = dataset.rasterband(1)
        .map_err(|e| ElevationError::ParseError(format!("No raster band: {}", e)))?;
    
    // Get geotransform (maps pixel coords → lat/lon)
    let geotransform = dataset.geo_transform()
        .map_err(|e| ElevationError::ParseError(format!("No geotransform: {}", e)))?;
    
    // Geotransform format: [x_origin, pixel_width, 0, y_origin, 0, -pixel_height]
    // x = geotransform[0] + pixel_col * geotransform[1]
    // y = geotransform[3] + pixel_row * geotransform[5]
    
    let x_origin = geotransform[0];
    let pixel_width = geotransform[1];
    let y_origin = geotransform[3];
    let pixel_height = geotransform[5];  // Usually negative
    
    // Convert lat/lon to pixel coordinates
    let pixel_col = ((gps.lon - x_origin) / pixel_width).floor() as isize;
    let pixel_row = ((gps.lat - y_origin) / pixel_height).floor() as isize;
    
    // Read a 2×2 window for bilinear interpolation
    // GDAL read_as uses (x_off, y_off, x_size, y_size) in pixels
    let buffer = rasterband.read_as::<i16>(
        (pixel_col, pixel_row),  // offset
        (2, 2),                   // window size
        (2, 2),                   // buffer size (no resampling)
        Some(ResampleAlg::Bilinear)
    ).map_err(|e| ElevationError::ParseError(format!("GDAL read failed: {}", e)))?;
    
    // Calculate fractional position within pixel
    let col_frac = ((gps.lon - x_origin) / pixel_width) - pixel_col as f64;
    let row_frac = ((gps.lat - y_origin) / pixel_height) - pixel_row as f64;
    
    // Bilinear interpolation
    let data = buffer.data();
    let e00 = data[0] as f64;  // Top-left
    let e01 = data[1] as f64;  // Top-right
    let e10 = data[2] as f64;  // Bottom-left
    let e11 = data[3] as f64;  // Bottom-right
    
    let e0 = e00 * (1.0 - col_frac) + e01 * col_frac;  // Top edge
    let e1 = e10 * (1.0 - col_frac) + e11 * col_frac;  // Bottom edge
    let elevation_meters = e0 * (1.0 - row_frac) + e1 * row_frac;
    
    Ok(Elevation { meters: elevation_meters })
}

/// Multi-source elevation pipeline
pub struct ElevationPipeline {
    sources: Vec<Box<dyn ElevationSource>>,
}

impl ElevationPipeline {
    /// Create pipeline with default sources
    /// 
    /// Priority order:
    /// 1. NAS file (if available)
    /// 2. OpenTopography API
    pub fn with_defaults(api_key: String, cache_dir: PathBuf) -> Self {
        let mut pipeline = Self::new();
        
        // Try NAS file first (highest priority, no rate limits)
        if let Some(nas) = NasFileSource::new() {
            pipeline.add_source(Box::new(nas));
        }
        
        // OpenTopography API as fallback
        let api = OpenTopographySource::new(api_key, cache_dir);
        pipeline.add_source(Box::new(api));
        
        pipeline
    }
    
    /// Create pipeline with no sources
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
        }
    }
    
    /// Add a source to the pipeline (in priority order)
    pub fn add_source(&mut self, source: Box<dyn ElevationSource>) {
        self.sources.push(source);
    }
    
    /// Query elevation, trying sources in order until one succeeds
    pub fn query(&mut self, gps: &GPS) -> Result<Elevation, ElevationError> {
        for source in &mut self.sources {
            match source.query(gps) {
                Ok(Some(elevation)) => return Ok(elevation),
                Ok(None) => continue, // Try next source
                Err(e) => {
                    eprintln!("Source {} failed: {}", source.name(), e);
                    continue;
                }
            }
        }
        
        Err(ElevationError::FileNotFound(
            "No sources could provide elevation data".to_string()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    #[ignore] // Requires API key and network
    fn test_opentopography_api() {
        let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY")
            .unwrap_or_else(|_| "3e607de6969c687053f9e107a4796962".to_string());
        
        let cache_dir = PathBuf::from("./elevation_cache");
        let mut source = OpenTopographySource::new(api_key, cache_dir);
        
        // Test Kangaroo Point Cliffs, Brisbane
        let gps = GPS::new(-27.4775, 153.0355, 0.0);
        let result = source.query(&gps);
        
        assert!(result.is_ok(), "Query should succeed");
        let elevation = result.unwrap();
        assert!(elevation.is_some(), "Should have elevation data");
        
        let elev_m = elevation.unwrap().meters;
        println!("Kangaroo Point elevation: {} meters", elev_m);
        
        // Kangaroo Point Cliffs are ~20m above sea level
        assert!(elev_m > 15.0 && elev_m < 25.0, 
                "Kangaroo Point should be ~20m elevation, got {}", elev_m);
    }
    
    #[test]
    #[ignore] // Requires cached data
    fn test_known_elevations() {
        let api_key = "test_key".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let mut source = OpenTopographySource::new(api_key, cache_dir);
        
        // Test various locations with known elevations
        let test_cases = vec![
            // (lat, lon, expected_min, expected_max, name)
            (-27.4775, 153.0355, 15.0, 25.0, "Kangaroo Point"),
            // Add more test points as tiles are cached
        ];
        
        for (lat, lon, min_elev, max_elev, name) in test_cases {
            let gps = GPS::new(lat, lon, 0.0);
            if let Ok(Some(elevation)) = source.query(&gps) {
                let elev_m = elevation.meters;
                println!("{}: {} meters", name, elev_m);
                assert!(elev_m >= min_elev && elev_m <= max_elev,
                        "{} elevation should be between {} and {} meters, got {}",
                        name, min_elev, max_elev, elev_m);
            }
        }
    }
    
    #[test]
    fn test_bilinear_interpolation_accuracy() {
        // Test that bilinear interpolation provides smooth transitions
        let api_key = "test_key".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let mut source = OpenTopographySource::new(api_key, cache_dir);
        
        // Query points in a line, verify smoothness
        let lat_start = -27.4775;
        let lon_start = 153.0355;
        
        let mut prev_elev: Option<f64> = None;
        for i in 0..10 {
            let offset = i as f64 * 0.0001; // ~10m steps
            let gps = GPS::new(lat_start + offset, lon_start, 0.0);
            
            if let Ok(Some(elevation)) = source.query(&gps) {
                if let Some(prev) = prev_elev {
                    let diff = (elevation.meters - prev).abs();
                    // Elevation shouldn't change by more than 10m over 10m distance
                    assert!(diff < 10.0, 
                            "Elevation changed too rapidly: {} meters over 10m", diff);
                }
                prev_elev = Some(elevation.meters);
            }
        }
    }
    
    #[test]
    #[ignore] // Requires NAS file
    fn test_nas_file_source() {
        let nas = NasFileSource::new();
        assert!(nas.is_some(), "NAS file should be available");
        
        let mut source = nas.unwrap();
        
        // Test Kangaroo Point
        let gps = GPS::new(-27.4775, 153.0355, 0.0);
        let result = source.query(&gps);
        
        match &result {
            Ok(_) => {},
            Err(e) => println!("Query error: {}", e),
        }
        
        assert!(result.is_ok(), "Query should succeed");
        let elevation = result.unwrap();
        assert!(elevation.is_some(), "Should have elevation data");
        
        let elev_m = elevation.unwrap().meters;
        println!("Kangaroo Point elevation (NAS): {} meters", elev_m);
        
        // Should match API result (~20m)
        assert!(elev_m > 15.0 && elev_m < 25.0, 
                "Kangaroo Point should be ~20m elevation, got {}", elev_m);
    }
    
    #[test]
    #[ignore] // Requires NAS file
    fn test_pipeline_with_nas() {
        let api_key = "3e607de6969c687053f9e107a4796962".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        
        let mut pipeline = ElevationPipeline::with_defaults(api_key, cache_dir);
        
        // Test multiple locations
        let test_points = vec![
            (-27.4775, 153.0355, "Kangaroo Point"),
            (0.0, 0.0, "Null Island"),
            (27.9881, 86.9250, "Mount Everest"),
        ];
        
        for (lat, lon, name) in test_points {
            let gps = GPS::new(lat, lon, 0.0);
            let result = pipeline.query(&gps);
            
            if let Ok(elevation) = result {
                println!("{}: {} meters", name, elevation.meters);
            }
        }
    }
}
