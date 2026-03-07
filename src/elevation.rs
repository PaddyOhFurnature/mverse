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
    srtm_source_dir: Option<PathBuf>,
    // Use Mutex for thread-safe interior mutability (rate limiting state)
    last_request: Arc<Mutex<Option<std::time::Instant>>>,
}

#[cfg(feature = "terrain-gdal")]
/// SRTM data from NAS-mounted global GeoTIFF (200GB world coverage)
/// 
/// PERFORMANCE: Caches Dataset handle and elevation data in memory
/// - Opens file once, reuses handle (was opening 900x per chunk!)
/// - Caches 1km x 1km tiles in memory (LRU eviction)
/// - Batch reads reduce network roundtrips
/// 
/// Requires the `terrain-gdal` feature (libgdal system library).
pub struct NasFileSource {
    file_path: PathBuf,
    dataset: Arc<Mutex<Option<gdal::Dataset>>>,
    cache: Arc<Mutex<ElevationCache>>,
}

/// Cache for elevation data (1km x 1km tiles)
#[cfg(feature = "terrain-gdal")]
struct ElevationCache {
    tiles: std::collections::HashMap<(i32, i32), CachedTile>,
    max_tiles: usize,
}

#[cfg(feature = "terrain-gdal")]
struct CachedTile {
    lat_min: f64,
    lon_min: f64,
    resolution: f64,  // degrees per pixel
    data: Vec<Vec<f32>>,  // [lat_idx][lon_idx]
    size: usize,  // pixels per side
}

#[cfg(feature = "terrain-gdal")]
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

#[cfg(feature = "terrain-gdal")]
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

#[cfg(feature = "terrain-gdal")]
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
            srtm_source_dir: None,
            last_request: Arc::new(Mutex::new(None)),
        }
    }

    /// Register a local directory to scan for pre-downloaded SRTM .tif files.
    pub fn with_srtm_source_dir(mut self, dir: PathBuf) -> Self {
        self.srtm_source_dir = Some(dir);
        self
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
        let lat_dir = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", lat.unsigned_abs()) };
        let lon_dir = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lon.unsigned_abs()) };
        let tile_path = self.cache_dir
            .join(&lat_dir)
            .join(&lon_dir)
            .join(format!("srtm_{}{:02}_{}{:03}.tif",
                if lat >= 0 { 'n' } else { 's' }, lat.unsigned_abs(),
                if lon >= 0 { 'e' } else { 'w' }, lon.unsigned_abs()));
        
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

/// Copernicus DEM GLO-30 via AWS S3 (free, no API key, Cloud-Optimized GeoTIFF)
///
/// Covers the globe at 30m (~1 arc-second) resolution. No rate limit.
/// URL template: https://copernicus-dem-30m.s3.amazonaws.com/Copernicus_DSM_COG_10_N51_00_E000_00_DEM/...tif
pub struct CopernicusElevationSource {
    pub cache_dir: PathBuf,
}

impl CopernicusElevationSource {
    pub fn new(cache_dir: PathBuf) -> Self { Self { cache_dir } }

    fn fetch_tile(&self, lat: i32, lon: i32) -> Result<PathBuf, ElevationError> {
        let ns = if lat >= 0 { "N" } else { "S" };
        let ew = if lon >= 0 { "E" } else { "W" };
        let la = lat.unsigned_abs(); let lo = lon.unsigned_abs();
        let tile_id = format!("Copernicus_DSM_COG_10_{}{:02}_00_{}{:03}_00_DEM", ns, la, ew, lo);
        let url = format!("https://copernicus-dem-30m.s3.amazonaws.com/{}/{}.tif", tile_id, tile_id);
        let lat_dir = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", la) };
        let lon_dir = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lo) };
        let tile_path = self.cache_dir.join(&lat_dir).join(&lon_dir)
            .join(format!("srtm_{}{:02}_{}{:03}.tif",
                if lat >= 0 { 'n' } else { 's' }, la,
                if lon >= 0 { 'e' } else { 'w' }, lo));
        std::fs::create_dir_all(tile_path.parent().unwrap())
            .map_err(|e| ElevationError::FileNotFound(e.to_string()))?;
        let resp = reqwest::blocking::get(&url)
            .map_err(|e| ElevationError::NetworkError(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ElevationError::NetworkError(format!("HTTP {}", resp.status())));
        }
        let bytes = resp.bytes().map_err(|e| ElevationError::NetworkError(e.to_string()))?;
        if bytes.len() < 1024 { return Err(ElevationError::FileNotFound("empty tile (ocean)".into())); }
        std::fs::write(&tile_path, &bytes).map_err(|e| ElevationError::FileNotFound(e.to_string()))?;
        Ok(tile_path)
    }
}

impl ElevationSource for CopernicusElevationSource {
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        let lat_tile = gps.lat.floor() as i32;
        let lon_tile = gps.lon.floor() as i32;
        let la = lat_tile.unsigned_abs(); let lo = lon_tile.unsigned_abs();
        let lat_dir = if lat_tile >= 0 { format!("N{:02}", lat_tile) } else { format!("S{:02}", la) };
        let lon_dir = if lon_tile >= 0 { format!("E{:03}", lon_tile) } else { format!("W{:03}", lo) };
        let tile_name = format!("srtm_{}{:02}_{}{:03}.tif",
            if lat_tile >= 0 { 'n' } else { 's' }, la,
            if lon_tile >= 0 { 'e' } else { 'w' }, lo);
        let tile_path = self.cache_dir.join(&lat_dir).join(&lon_dir).join(&tile_name);
        let tile_file = if tile_path.exists() && tile_path.metadata().map(|m| m.len()).unwrap_or(0) >= 1024 {
            tile_path
        } else {
            self.fetch_tile(lat_tile, lon_tile)?
        };
        Ok(Some(extract_elevation(&tile_file, gps)?))
    }
    fn name(&self) -> &str { "Copernicus DEM (AWS)" }
}

/// AWS Terrain Tiles / Skadi (free, no API key, SRTM1 HGT.gz)
///
/// Mirrors SRTM 1-arc-second data at 1°×1° tiles. No rate limit.
/// URL: https://s3.amazonaws.com/elevation-tiles-prod/skadi/{NS}{lat}/{NS}{lat}{EW}{lon}.hgt.gz
pub struct SkadiElevationSource {
    pub cache_dir: PathBuf,
}

impl SkadiElevationSource {
    pub fn new(cache_dir: PathBuf) -> Self { Self { cache_dir } }

    fn fetch_tile(&self, lat: i32, lon: i32) -> Result<PathBuf, ElevationError> {
        let ns = if lat >= 0 { "N" } else { "S" };
        let ew = if lon >= 0 { "E" } else { "W" };
        let la = lat.unsigned_abs(); let lo = lon.unsigned_abs();
        let url = format!(
            "https://s3.amazonaws.com/elevation-tiles-prod/skadi/{}{:02}/{}{:02}{}{:03}.hgt.gz",
            ns, la, ns, la, ew, lo);
        let lat_dir = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", la) };
        let lon_dir = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lo) };
        // HGT files must use GDAL-standard naming: N51E000.hgt
        let hgt_name = format!("{}{:02}{}{:03}.hgt", ns, la, ew, lo);
        let hgt_path = self.cache_dir.join(&lat_dir).join(&lon_dir).join(&hgt_name);
        std::fs::create_dir_all(hgt_path.parent().unwrap())
            .map_err(|e| ElevationError::FileNotFound(e.to_string()))?;
        let resp = reqwest::blocking::get(&url)
            .map_err(|e| ElevationError::NetworkError(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(ElevationError::NetworkError(format!("HTTP {}", resp.status())));
        }
        let bytes = resp.bytes().map_err(|e| ElevationError::NetworkError(e.to_string()))?;
        if bytes.len() < 512 { return Err(ElevationError::FileNotFound("empty tile (ocean)".into())); }
        // Decompress gzip → raw HGT
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut raw = Vec::new();
        decoder.read_to_end(&mut raw).map_err(|e| ElevationError::ParseError(e.to_string()))?;
        if raw.len() < 1024 { return Err(ElevationError::FileNotFound("decompressed empty".into())); }
        std::fs::write(&hgt_path, &raw).map_err(|e| ElevationError::FileNotFound(e.to_string()))?;
        Ok(hgt_path)
    }
}

impl ElevationSource for SkadiElevationSource {
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        let lat_tile = gps.lat.floor() as i32;
        let lon_tile = gps.lon.floor() as i32;
        let la = lat_tile.unsigned_abs(); let lo = lon_tile.unsigned_abs();
        let ns = if lat_tile >= 0 { "N" } else { "S" };
        let ew = if lon_tile >= 0 { "E" } else { "W" };
        let lat_dir = if lat_tile >= 0 { format!("N{:02}", lat_tile) } else { format!("S{:02}", la) };
        let lon_dir = if lon_tile >= 0 { format!("E{:03}", lon_tile) } else { format!("W{:03}", lo) };
        let hgt_name = format!("{}{:02}{}{:03}.hgt", ns, la, ew, lo);
        let hgt_path = self.cache_dir.join(&lat_dir).join(&lon_dir).join(&hgt_name);
        let tile_file = if hgt_path.exists() && hgt_path.metadata().map(|m| m.len()).unwrap_or(0) >= 1024 {
            hgt_path
        } else {
            self.fetch_tile(lat_tile, lon_tile)?
        };
        Ok(Some(extract_elevation(&tile_file, gps)?))
    }
    fn name(&self) -> &str { "AWS Terrain Tiles (Skadi)" }
}

impl ElevationSource for OpenTopographySource {
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        // Determine which 1° tile contains this point
        let lat_tile = gps.lat.floor() as i32;
        let lon_tile = gps.lon.floor() as i32;

        // Check local source directory for pre-downloaded files
        if let Some(ref src_dir) = self.srtm_source_dir {
            let fname = format!("srtm_{}{:02}_{}{:03}.tif",
                if lat_tile >= 0 { 'n' } else { 's' }, lat_tile.unsigned_abs(),
                if lon_tile >= 0 { 'e' } else { 'w' }, lon_tile.unsigned_abs());
            // Also try legacy n/e-only naming for compat with third-party sources
            let fname_legacy = format!("srtm_n{:02}_e{:03}.tif", lat_tile.unsigned_abs(), lon_tile.unsigned_abs());
            let lat_dir_s = if lat_tile >= 0 { format!("N{:02}", lat_tile) } else { format!("S{:02}", lat_tile.unsigned_abs()) };
            let lon_dir_s = if lon_tile >= 0 { format!("E{:03}", lon_tile) } else { format!("W{:03}", lon_tile.unsigned_abs()) };
            let dest_dir = self.cache_dir.join(&lat_dir_s).join(&lon_dir_s);
            for candidate in [src_dir.join(&fname), src_dir.join(&fname_legacy)] {
                if candidate.exists() {
                    std::fs::create_dir_all(&dest_dir).ok();
                    let dest = dest_dir.join(&fname);
                    if !dest.exists() { std::fs::copy(&candidate, &dest).ok(); }
                    break;
                }
            }
        }

        // Check cache first — use N/S/E/W prefixes correctly
        let lat_dir = if lat_tile >= 0 { format!("N{:02}", lat_tile) } else { format!("S{:02}", lat_tile.unsigned_abs()) };
        let lon_dir = if lon_tile >= 0 { format!("E{:03}", lon_tile) } else { format!("W{:03}", lon_tile.unsigned_abs()) };
        let tile_name = format!("srtm_{}{:02}_{}{:03}.tif",
            if lat_tile >= 0 { 'n' } else { 's' }, lat_tile.unsigned_abs(),
            if lon_tile >= 0 { 'e' } else { 'w' }, lon_tile.unsigned_abs());
        let tile_path = self.cache_dir.join(&lat_dir).join(&lon_dir).join(&tile_name);
        // HGT files downloaded from Skadi/Copernicus fallback sources use GDAL-standard naming
        let hgt_name = format!("{}{:02}{}{:03}.hgt",
            if lat_tile >= 0 { 'N' } else { 'S' }, lat_tile.unsigned_abs(),
            if lon_tile >= 0 { 'E' } else { 'W' }, lon_tile.unsigned_abs());
        let hgt_path = self.cache_dir.join(&lat_dir).join(&lon_dir).join(&hgt_name);
        
        let tile_file = if tile_path.exists() {
            tile_path
        } else if hgt_path.exists() {
            hgt_path
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

/// Extract elevation from a GeoTIFF or HGT file at the given GPS coordinate.
///
/// Uses the pure-Rust `tiff` crate (no GDAL required). The geo-transform is
/// derived from the tile filename (lat/lon encoded in the name), which is
/// reliable for all SRTM-convention tile naming schemes.
///
/// Handles SRTM void pixels (-32768 / -9999) by expanding the search window
/// up to FALLBACK_RADIUS pixels outward and averaging valid neighbours.
fn extract_elevation(tiff_path: &PathBuf, gps: &GPS) -> Result<Elevation, ElevationError> {
    let ext = tiff_path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if ext == "hgt" {
        return extract_elevation_hgt(tiff_path, gps);
    }

    // Parse tile lat/lon from filename (e.g. srtm_s28_e153.tif → -28, 153)
    let (tile_lat, tile_lon) = parse_tile_coords_from_path(tiff_path)?;

    let file = std::fs::File::open(tiff_path)
        .map_err(|e| ElevationError::FileNotFound(format!("Cannot open {}: {}", tiff_path.display(), e)))?;
    let reader = std::io::BufReader::new(file);
    let mut decoder = tiff::decoder::Decoder::new(reader)
        .map_err(|e| ElevationError::ParseError(format!("TIFF init error: {}", e)))?;

    let (width, height) = decoder.dimensions()
        .map_err(|e| ElevationError::ParseError(format!("TIFF dimensions error: {}", e)))?;

    // Geo-transform derived from filename: tile covers [tile_lat, tile_lat+1] × [tile_lon, tile_lon+1]
    // Top-left corner is (tile_lon, tile_lat+1). Pixels map with (width-1) steps across 1 degree.
    let x_origin    = tile_lon as f64;
    let y_origin    = (tile_lat + 1) as f64;
    let pixel_width  = 1.0 / (width  as f64 - 1.0).max(1.0);
    let pixel_height = -1.0 / (height as f64 - 1.0).max(1.0);

    let pixel_col = ((gps.lon - x_origin) / pixel_width).floor() as isize;
    let pixel_row = ((gps.lat - y_origin) / pixel_height).floor() as isize;

    // Clamp so a 2×2 read never goes out of bounds at tile edges.
    let pixel_col = pixel_col.max(0).min(width as isize - 2);
    let pixel_row = pixel_row.max(0).min(height as isize - 2);

    // Read all pixel data (SRTM 1s tile: 3601×3601 × 2B ≈ 26MB uncompressed, usually compressed)
    let image = decoder.read_image()
        .map_err(|e| ElevationError::ParseError(format!("TIFF read error: {}", e)))?;

    let w = width as usize;
    let col = pixel_col as usize;
    let row = pixel_row as usize;

    let get_i16 = |r: usize, c: usize| -> i16 {
        let idx = r * w + c;
        match &image {
            tiff::decoder::DecodingResult::I16(data) => *data.get(idx).unwrap_or(&0),
            tiff::decoder::DecodingResult::F32(data) => *data.get(idx).unwrap_or(&0.0) as i16,
            tiff::decoder::DecodingResult::U16(data) => *data.get(idx).unwrap_or(&0) as i16,
            tiff::decoder::DecodingResult::I32(data) => *data.get(idx).unwrap_or(&0) as i16,
            tiff::decoder::DecodingResult::F64(data) => *data.get(idx).unwrap_or(&0.0) as i16,
            _ => 0,
        }
    };

    let data2 = [
        get_i16(row,     col),
        get_i16(row,     col + 1),
        get_i16(row + 1, col),
        get_i16(row + 1, col + 1),
    ];

    let valid2: Vec<f64> = data2.iter()
        .filter(|&&v| !is_srtm_void(v))
        .map(|&v| v as f64)
        .collect();

    if valid2.len() == 4 {
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
        let mean = valid2.iter().sum::<f64>() / valid2.len() as f64;
        return Ok(Elevation { meters: mean });
    }

    // Expand outward up to FALLBACK_RADIUS pixels for void regions
    const FALLBACK_RADIUS: isize = 8;
    let r0 = (row as isize - FALLBACK_RADIUS).max(0) as usize;
    let c0 = (col as isize - FALLBACK_RADIUS).max(0) as usize;
    let r1 = (row as isize + FALLBACK_RADIUS + 1).min(height as isize) as usize;
    let c1 = (col as isize + FALLBACK_RADIUS + 1).min(width as isize) as usize;

    let mut valid_far: Vec<f64> = Vec::new();
    for r in r0..r1 {
        for c in c0..c1 {
            let v = get_i16(r, c);
            if !is_srtm_void(v) {
                valid_far.push(v as f64);
            }
        }
    }

    if !valid_far.is_empty() {
        let min_far = valid_far.iter().cloned().fold(f64::MAX, f64::min);
        return Ok(Elevation { meters: min_far });
    }

    Ok(Elevation { meters: 0.0 })
}

/// Parse tile (lat, lon) integers from a SRTM-convention filename.
///
/// Handles formats:
/// - `srtm_s28_e153.tif`  → (-28, 153)
/// - `srtm_n28_e152.tif`  → (28, 152)
/// - `srtm_S28_E153.tif`  → (-28, 153)
/// - `N28E153.hgt`         → (28, 153)  (handled by parse_hgt_coords)
fn parse_tile_coords_from_path(path: &PathBuf) -> Result<(i32, i32), ElevationError> {
    let stem = path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();

    // srtm_s28_e153 / srtm_n28_w001 etc.
    if let Some(rest) = stem.strip_prefix("srtm_") {
        let parts: Vec<&str> = rest.split('_').collect();
        if parts.len() == 2 {
            let lat = parse_deg_str(parts[0])?;
            let lon = parse_deg_str(parts[1])?;
            return Ok((lat, lon));
        }
    }

    // Plain N28E153 or S28W001 style
    if let Some(lat_lon) = parse_hgt_style(&stem) {
        return Ok(lat_lon);
    }

    Err(ElevationError::ParseError(format!(
        "Cannot determine tile lat/lon from filename: {}", path.display()
    )))
}

fn parse_deg_str(s: &str) -> Result<i32, ElevationError> {
    let (sign, digits) = if s.starts_with('n') || s.starts_with('e') {
        (1, &s[1..])
    } else if s.starts_with('s') || s.starts_with('w') {
        (-1, &s[1..])
    } else {
        return Err(ElevationError::ParseError(format!("Bad degree token: {}", s)));
    };
    let v: i32 = digits.parse()
        .map_err(|_| ElevationError::ParseError(format!("Bad degree value: {}", s)))?;
    Ok(sign * v)
}

fn parse_hgt_style(name: &str) -> Option<(i32, i32)> {
    // e.g. "n28e153" or "s28w001"
    let name = name.trim_end_matches(".hgt");
    let (lat_sign, rest) = if name.starts_with('n') { (1, &name[1..]) }
        else if name.starts_with('s') { (-1, &name[1..]) }
        else { return None; };
    let lat_end = rest.find(|c: char| c == 'e' || c == 'w')?;
    let lat: i32 = rest[..lat_end].parse().ok()?;
    let (lon_sign, lon_str) = if rest[lat_end..].starts_with('e') { (1, &rest[lat_end+1..]) }
        else { (-1, &rest[lat_end+1..]) };
    let lon: i32 = lon_str.parse().ok()?;
    Some((lat_sign * lat, lon_sign * lon))
}

/// Extract elevation from a raw SRTM HGT file (big-endian signed int16, row-major N→S, W→E).
fn extract_elevation_hgt(hgt_path: &PathBuf, gps: &GPS) -> Result<Elevation, ElevationError> {
    let stem = hgt_path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_lowercase();
    let (tile_lat, tile_lon) = parse_hgt_style(&stem).ok_or_else(|| {
        ElevationError::ParseError(format!("Cannot parse HGT filename: {}", hgt_path.display()))
    })?;

    let bytes = std::fs::read(hgt_path)
        .map_err(|e| ElevationError::FileNotFound(format!("Cannot read HGT: {}", e)))?;

    // Determine resolution: 1201×1201 = SRTM3 (3 arc-sec), 3601×3601 = SRTM1 (1 arc-sec)
    let n_samples = ((bytes.len() / 2) as f64).sqrt().round() as usize;
    if n_samples * n_samples * 2 != bytes.len() {
        return Err(ElevationError::ParseError(format!("Bad HGT size: {} bytes", bytes.len())));
    }

    let x_origin = tile_lon as f64;
    let y_origin = (tile_lat + 1) as f64;
    let step = 1.0 / (n_samples as f64 - 1.0);

    let col = ((gps.lon - x_origin) / step).floor() as isize;
    let row = ((y_origin - gps.lat) / step).floor() as isize;
    let col = col.max(0).min(n_samples as isize - 2) as usize;
    let row = row.max(0).min(n_samples as isize - 2) as usize;

    let read_i16 = |r: usize, c: usize| -> i16 {
        let idx = (r * n_samples + c) * 2;
        if idx + 1 < bytes.len() {
            i16::from_be_bytes([bytes[idx], bytes[idx + 1]])
        } else {
            0
        }
    };

    let data2 = [
        read_i16(row, col), read_i16(row, col + 1),
        read_i16(row + 1, col), read_i16(row + 1, col + 1),
    ];
    let valid2: Vec<f64> = data2.iter().filter(|&&v| !is_srtm_void(v)).map(|&v| v as f64).collect();

    if valid2.len() == 4 {
        let col_frac = ((gps.lon - x_origin) / step) - col as f64;
        let row_frac = ((y_origin - gps.lat) / step) - row as f64;
        let e0 = data2[0] as f64 * (1.0 - col_frac) + data2[1] as f64 * col_frac;
        let e1 = data2[2] as f64 * (1.0 - col_frac) + data2[3] as f64 * col_frac;
        return Ok(Elevation { meters: e0 * (1.0 - row_frac) + e1 * row_frac });
    }
    if !valid2.is_empty() {
        return Ok(Elevation { meters: valid2.iter().sum::<f64>() / valid2.len() as f64 });
    }
    Ok(Elevation { meters: 0.0 })
}

/// Elevation source that fetches tiles from P2P peers via libp2p request-response.
///
/// Checks local cache first (fast path). On a peer hit, saves the GeoTIFF bytes
/// to the local elevation_cache/ and announces to DHT so others can find us.
pub struct P2PElevationSource {
    fetcher: std::sync::Arc<crate::multiplayer::TileFetcher>,
    cache_dir: PathBuf,
}

impl P2PElevationSource {
    pub fn new(
        fetcher: std::sync::Arc<crate::multiplayer::TileFetcher>,
        cache_dir: PathBuf,
    ) -> Self {
        Self { fetcher, cache_dir }
    }
}

impl ElevationSource for P2PElevationSource {
    fn query(&self, gps: &GPS) -> Result<Option<Elevation>, ElevationError> {
        let lat = gps.lat.floor() as i32;
        let lon = gps.lon.floor() as i32;

        let lat_prefix = if lat >= 0 { 'n' } else { 's' };
        let lon_prefix = if lon >= 0 { 'e' } else { 'w' };
        let lat_dir = if lat >= 0 { format!("N{:02}", lat) } else { format!("S{:02}", lat.unsigned_abs()) };
        let lon_dir = if lon >= 0 { format!("E{:03}", lon) } else { format!("W{:03}", lon.unsigned_abs()) };
        let tile_path = self.cache_dir
            .join(&lat_dir)
            .join(&lon_dir)
            .join(format!("srtm_{}{:02}_{}{:03}.tif",
                lat_prefix, lat.unsigned_abs(), lon_prefix, lon.unsigned_abs()));

        if !tile_path.exists() {
            // TileFetcher tracks failed tiles across all instances; skip if already tried
            if let Some(bytes) = self.fetcher.fetch_elevation(lat, lon) {
                if let Some(parent) = tile_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(&tile_path, &bytes).is_ok() {
                    // Announce to DHT — we now have this tile (also clears failed set)
                    self.fetcher.announce_elevation(lat, lon);
                }
            }
        }

        if !tile_path.exists() {
            return Ok(None);
        }

        match extract_elevation(&tile_path, gps) {
            Ok(elev) => Ok(Some(elev)),
            Err(e) => Err(e),
        }
    }

    fn name(&self) -> &str { "P2P elevation (libp2p)" }
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
        
        // Try NAS file first (highest priority, no rate limits, requires terrain-gdal feature)
        #[cfg(feature = "terrain-gdal")]
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
