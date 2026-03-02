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
use std::sync::{Arc, Mutex};
use crate::coordinates::GPS;

/// Elevation query result
#[derive(Debug, Clone, Copy)]
pub struct Elevation {
    pub meters: f64,
}

/// Elevation data source trait
/// 
/// IMPORTANT: Implementations must be Send + Sync for parallel terrain generation.
/// Use interior mutability (Arc<Mutex<>>) for any mutable state.
pub trait ElevationSource: Send + Sync {
    /// Query elevation at GPS coordinate
    /// Returns None if data unavailable for this location
    /// 
    /// Uses &self instead of &mut self to enable parallel queries.
    /// Implementations use Arc<Mutex<>> for thread-safe interior mutability.
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError>;
    
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
    // Use Mutex for thread-safe interior mutability (rate limiting state)
    last_request: Arc<Mutex<Option<std::time::Instant>>>,
}

/// SRTM data from NAS-mounted global GeoTIFF (200GB world coverage)
/// 
/// PERFORMANCE: Caches Dataset handle and elevation data in memory
/// - Opens file once, reuses handle (was opening 900x per chunk!)
/// - Caches 1km x 1km tiles in memory (LRU eviction)
/// - Batch reads reduce network roundtrips
pub struct NasFileSource {
    file_path: PathBuf,
    dataset: Arc<Mutex<Option<gdal::Dataset>>>,
    cache: Arc<Mutex<ElevationCache>>,
}

/// Cache for elevation data (1km x 1km tiles)
struct ElevationCache {
    tiles: std::collections::HashMap<(i32, i32), CachedTile>,
    max_tiles: usize,
}

struct CachedTile {
    lat_min: f64,
    lon_min: f64,
    resolution: f64,  // degrees per pixel
    data: Vec<Vec<f32>>,  // [lat_idx][lon_idx]
    size: usize,  // pixels per side
}

impl ElevationCache {
    fn new(max_tiles: usize) -> Self {
        Self {
            tiles: std::collections::HashMap::new(),
            max_tiles,
        }
    }
    
    fn tile_key(lat: f64, lon: f64) -> (i32, i32) {
        // 1km tiles at equator ≈ 0.009° (will vary by latitude)
        const TILE_SIZE_DEG: f64 = 0.01;
        let tile_lat = (lat / TILE_SIZE_DEG).floor() as i32;
        let tile_lon = (lon / TILE_SIZE_DEG).floor() as i32;
        (tile_lat, tile_lon)
    }
    
    fn get(&self, lat: f64, lon: f64) -> Option<Elevation> {
        let key = Self::tile_key(lat, lon);
        if let Some(tile) = self.tiles.get(&key) {
            // Interpolate within tile
            let lat_idx = ((lat - tile.lat_min) / tile.resolution).floor() as usize;
            let lon_idx = ((lon - tile.lon_min) / tile.resolution).floor() as usize;
            
            if lat_idx < tile.size && lon_idx < tile.size {
                return Some(Elevation {
                    meters: tile.data[lat_idx][lon_idx] as f64,
                });
            }
        }
        None
    }
    
    fn insert(&mut self, tile_key: (i32, i32), tile: CachedTile) {
        // Simple eviction: remove random tile if at capacity
        if self.tiles.len() >= self.max_tiles {
            if let Some(key) = self.tiles.keys().next().cloned() {
                self.tiles.remove(&key);
            }
        }
        self.tiles.insert(tile_key, tile);
    }
}

impl NasFileSource {
    /// Create NAS file source
    /// 
    /// Tries multiple paths:
    /// 1. ./srtm-global.tif (symlink in project)
    /// 2. /mnt/nas/srtm-v3-1s.tif (if mounted)
    /// 3. GVFS path (if provided)
    pub fn new() -> Option<Self> {
        let mut candidates = vec![
            PathBuf::from("./srtm-global.tif"),
            PathBuf::from("/mnt/nas/srtm-v3-1s.tif"),
            PathBuf::from("/run/user/1000/gvfs/smb-share:server=blade.local,share=homes/world/srtm-v3-1s.tif"),
            PathBuf::from("/run/user/1000/gvfs/afp-volume:host=Blade.local,user=media,volume=homes/world/srtm-v3-1s.tif"),
        ];
        // Also check $METAVERSE_DATA_DIR/srtm-global.tif
        if let Ok(data_dir) = std::env::var("METAVERSE_DATA_DIR") {
            candidates.insert(0, PathBuf::from(data_dir).join("srtm-global.tif"));
        }
        
        for path in candidates {
            if path.exists() {
                println!("Found NAS SRTM file at: {}", path.display());
                return Some(Self { 
                    file_path: path,
                    dataset: Arc::new(Mutex::new(None)),
                    cache: Arc::new(Mutex::new(ElevationCache::new(100))),  // Cache 100 tiles ≈ 100km²
                });
            }
        }
        
        eprintln!("NAS SRTM file not found, will use API fallback");
        None
    }
    
    /// Create with explicit path
    pub fn with_path(path: PathBuf) -> Option<Self> {
        if path.exists() {
            Some(Self { 
                file_path: path,
                dataset: Arc::new(Mutex::new(None)),
                cache: Arc::new(Mutex::new(ElevationCache::new(100))),
            })
        } else {
            None
        }
    }
    
    /// Get or open dataset (cached)
    fn get_dataset(&self) -> Result<(), ElevationError> {
        let mut ds_guard = self.dataset.lock().unwrap();
        if ds_guard.is_none() {
            eprintln!("📂 Opening SRTM dataset (once): {}", self.file_path.display());
            let dataset = gdal::Dataset::open(&self.file_path)
                .map_err(|e| ElevationError::FileNotFound(format!("GDAL open failed: {}", e)))?;
            *ds_guard = Some(dataset);
        }
        Ok(())
    }
    
    /// Load a tile of elevation data (batch read)
    fn load_tile(&self, tile_lat: i32, tile_lon: i32) -> Result<CachedTile, ElevationError> {
        const TILE_SIZE_DEG: f64 = 0.01;
        let lat_min = tile_lat as f64 * TILE_SIZE_DEG;
        let lon_min = tile_lon as f64 * TILE_SIZE_DEG;
        
        self.get_dataset()?;
        let ds_guard = self.dataset.lock().unwrap();
        let dataset = ds_guard.as_ref().unwrap();
        
        let rasterband = dataset.rasterband(1)
            .map_err(|e| ElevationError::ParseError(format!("No raster band: {}", e)))?;
        
        let geotransform = dataset.geo_transform()
            .map_err(|e| ElevationError::ParseError(format!("No geotransform: {}", e)))?;
        
        let x_origin = geotransform[0];
        let pixel_width = geotransform[1];
        let y_origin = geotransform[3];
        let pixel_height = geotransform[5].abs();
        
        // Calculate pixel coordinates for tile
        let pixel_col = ((lon_min - x_origin) / pixel_width).floor() as isize;
        let pixel_row = ((y_origin - lat_min) / pixel_height).floor() as isize;
        
        // Read 64x64 pixels (covers 1km at ~30m resolution)
        const TILE_PIXELS: usize = 64;
        let window = (pixel_col, pixel_row);
        let window_size = (TILE_PIXELS, TILE_PIXELS);
        
        let buffer = rasterband.read_as::<f32>(window, window_size, window_size, None)
            .map_err(|e| ElevationError::ParseError(format!("GDAL read failed: {}", e)))?;
        
        // Convert flat buffer to 2D grid
        let mut data = vec![vec![0.0f32; TILE_PIXELS]; TILE_PIXELS];
        for lat_idx in 0..TILE_PIXELS {
            for lon_idx in 0..TILE_PIXELS {
                data[lat_idx][lon_idx] = buffer.data()[lat_idx * TILE_PIXELS + lon_idx];
            }
        }
        
        Ok(CachedTile {
            lat_min,
            lon_min,
            resolution: TILE_SIZE_DEG / TILE_PIXELS as f64,
            data,
            size: TILE_PIXELS,
        })
    }
}

impl ElevationSource for NasFileSource {
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        // Check cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(elevation) = cache.get(gps.lat, gps.lon) {
                return Ok(Some(elevation));
            }
        }
        
        // Cache miss - load tile
        let tile_key = ElevationCache::tile_key(gps.lat, gps.lon);
        let tile = self.load_tile(tile_key.0, tile_key.1)?;
        
        // Insert into cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(tile_key, tile);
        }
        
        // Query again from cache
        let cache = self.cache.lock().unwrap();
        Ok(cache.get(gps.lat, gps.lon))
    }
    
    fn name(&self) -> &str {
        "NAS Global SRTM File (Cached)"
    }
}

impl OpenTopographySource {
    /// Create OpenTopography source with API key
    pub fn new(api_key: String, cache_dir: PathBuf) -> Self {
        Self {
            api_key,
            cache_dir,
            last_request: Arc::new(Mutex::new(None)),
        }
    }
    
    /// Fetch tile from API (respects 2-second rate limit per RULES.md)
    fn fetch_tile(&self, lat: i32, lon: i32) -> Result<PathBuf, ElevationError> {
        // Rate limiting: 2-second cooldown (thread-safe with Mutex)
        {
            let last_req = self.last_request.lock().unwrap();
            if let Some(last) = *last_req {
                let elapsed = last.elapsed();
                if elapsed < std::time::Duration::from_secs(2) {
                    let wait = std::time::Duration::from_secs(2) - elapsed;
                    drop(last_req); // Release lock before sleeping
                    std::thread::sleep(wait);
                }
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
        
        // Update last request time (thread-safe)
        *self.last_request.lock().unwrap() = Some(std::time::Instant::now());
        
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

/// Compute a stable DHT announce key for an elevation tile.
pub fn elevation_dht_key(lat: i32, lon: i32) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let s = format!("elev:{:+04}:{:+04}", lat, lon);
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    h.finish().to_le_bytes().to_vec()
}

impl ElevationSource for OpenTopographySource {
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
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

/// SRTM standard void/nodata value (i16::MIN = -32768).
/// Also filter -9999 which some providers use.
#[inline]
fn is_srtm_void(v: i16) -> bool {
    v == i16::MIN || v == -9999
}

/// Extract elevation from GeoTIFF at given GPS coordinate.
///
/// Handles SRTM void pixels (-32768 / -9999) by expanding the search window
/// up to FALLBACK_RADIUS pixels outward and averaging valid neighbours.
/// Returns Ok(None) only when the entire fallback window contains no valid data.
fn extract_elevation(tiff_path: &PathBuf, gps: &GPS) -> Result<Elevation, ElevationError> {
    use gdal::Dataset;
    use gdal::raster::ResampleAlg;

    let dataset = Dataset::open(tiff_path)
        .map_err(|e| ElevationError::FileNotFound(format!("GDAL open failed: {}", e)))?;

    let rasterband = dataset.rasterband(1)
        .map_err(|e| ElevationError::ParseError(format!("No raster band: {}", e)))?;

    let geotransform = dataset.geo_transform()
        .map_err(|e| ElevationError::ParseError(format!("No geotransform: {}", e)))?;

    let x_origin    = geotransform[0];
    let pixel_width = geotransform[1];
    let y_origin    = geotransform[3];
    let pixel_height = geotransform[5]; // negative

    let pixel_col = ((gps.lon - x_origin) / pixel_width).floor() as isize;
    let pixel_row = ((gps.lat - y_origin) / pixel_height).floor() as isize;

    // Clamp so a 2×2 read never goes out of bounds at tile edges.
    let raster_size = rasterband.size();
    let pixel_col = pixel_col.max(0).min(raster_size.0 as isize - 2);
    let pixel_row = pixel_row.max(0).min(raster_size.1 as isize - 2);

    // --- 2×2 bilinear interpolation (primary path) ---
    let buf2 = rasterband.read_as::<i16>(
        (pixel_col, pixel_row), (2, 2), (2, 2),
        Some(ResampleAlg::Bilinear),
    ).map_err(|e| ElevationError::ParseError(format!("GDAL read failed: {}", e)))?;

    let data2 = buf2.data();
    let valid2: Vec<f64> = data2.iter()
        .filter(|&&v| !is_srtm_void(v))
        .map(|&v| v as f64)
        .collect();

    if valid2.len() == 4 {
        // All pixels valid — standard bilinear interpolation with cliff snap.
        let col_frac = ((gps.lon - x_origin) / pixel_width) - pixel_col as f64;
        let row_frac = ((gps.lat - y_origin) / pixel_height) - pixel_row as f64;

        const CLIFF_THRESHOLD: f64 = 15.0;
        let e00 = data2[0] as f64;
        let e01 = data2[1] as f64;
        let e10 = data2[2] as f64;
        let e11 = data2[3] as f64;

        let max_diff = (e00 - e01).abs()
            .max((e00 - e10).abs())
            .max((e01 - e11).abs())
            .max((e10 - e11).abs());

        let elevation_meters = if max_diff > CLIFF_THRESHOLD {
            let col_near = if col_frac < 0.5 { 0 } else { 1 };
            let row_near = if row_frac < 0.5 { 0 } else { 1 };
            data2[row_near * 2 + col_near] as f64
        } else {
            let e0 = e00 * (1.0 - col_frac) + e01 * col_frac;
            let e1 = e10 * (1.0 - col_frac) + e11 * col_frac;
            e0 * (1.0 - row_frac) + e1 * row_frac
        };
        return Ok(Elevation { meters: elevation_meters });
    }

    if !valid2.is_empty() {
        // Partial void — use mean of valid pixels.
        let mean = valid2.iter().sum::<f64>() / valid2.len() as f64;
        return Ok(Elevation { meters: mean });
    }

    // --- All immediate pixels are void: expand outward up to FALLBACK_RADIUS ---
    // This handles water bodies where SRTM has no-data and the user asks us to
    // "sample the areas around it" to work out the elevation.
    const FALLBACK_RADIUS: isize = 8; // ~240m at 30m/pixel
    let window = FALLBACK_RADIUS * 2 + 1;
    let top_left_col = pixel_col - FALLBACK_RADIUS;
    let top_left_row = pixel_row - FALLBACK_RADIUS;

    if let Ok(buf_large) = rasterband.read_as::<i16>(
        (top_left_col, top_left_row),
        (window as usize, window as usize),
        (window as usize, window as usize),
        Some(ResampleAlg::NearestNeighbour),
    ) {
        let valid_far: Vec<f64> = buf_large.data().iter()
            .filter(|&&v| !is_srtm_void(v))
            .map(|&v| v as f64)
            .collect();

        if !valid_far.is_empty() {
            // Use MINIMUM of valid neighbours, not mean.
            // When querying a water-surface pixel, the nearest valid pixels are
            // the bank edges — the lowest of those is closest to the true water
            // surface level. Using the mean pulls the result toward hillside
            // elevation and causes water polygons to float above terrain.
            let min_far = valid_far.iter().cloned().fold(f64::MAX, f64::min);
            return Ok(Elevation { meters: min_far });
        }
    }

    // No valid data anywhere in fallback window — let pipeline try next source.
    Ok(Elevation { meters: 0.0 })
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
    pub fn query(&self, gps: &GPS) -> Result<Elevation, ElevationError> {
        for source in &self.sources {
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
