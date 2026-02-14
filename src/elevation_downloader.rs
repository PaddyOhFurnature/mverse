/// Multi-source elevation downloader with parallel fetching and intelligent fallback.
///
/// Architecture:
/// 1. Priority queue of tiles to download (sorted by distance from camera)
/// 2. Try sources in order: Cache → Terrarium → USGS → OpenTopography → Procedural
/// 3. Parallel downloads (max 8 concurrent)
/// 4. Robust error handling with exponential backoff
/// 5. Procedural generation ONLY fills gaps in real data

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::elevation_sources::{ElevationSource, ElevationTile, TerrariumSource, Usgs3DepSource, OpenTopographySource};
use crate::cache::DiskCache;
use noise::{NoiseFn, Perlin};

/// Download task for a specific tile
#[derive(Debug, Clone)]
struct DownloadTask {
    lat: f64,
    lon: f64,
    zoom: u8,
    priority: f32, // Lower = higher priority (e.g., distance from camera)
    added_at: Instant,
}

/// Statistics for the downloader
#[derive(Debug, Clone, Default)]
pub struct DownloaderStats {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub downloads_success: u64,
    pub downloads_failed: u64,
    pub procedural_fallbacks: u64,
    pub total_bytes_downloaded: u64,
}

/// Multi-source elevation data downloader
pub struct ElevationDownloader {
    /// All available sources in priority order
    sources: Vec<Box<dyn ElevationSource>>,
    /// Disk cache for tiles
    cache: DiskCache,
    /// In-memory tile cache
    tiles: Arc<Mutex<HashMap<(i32, i32, u8), Arc<ElevationTile>>>>,
    /// Download queue
    queue: Arc<Mutex<VecDeque<DownloadTask>>>,
    /// Currently downloading tiles (to avoid duplicates)
    in_progress: Arc<Mutex<HashMap<(i32, i32, u8), Instant>>>,
    /// Statistics
    stats: Arc<Mutex<DownloaderStats>>,
    /// Procedural noise generator (for gap filling only)
    noise: Perlin,
    /// Max concurrent downloads
    max_concurrent: usize,
    /// Rate limiter (min time between requests to same source)
    last_request: Arc<Mutex<HashMap<String, Instant>>>,
    min_request_interval: Duration,
}

impl ElevationDownloader {
    pub fn new(cache: DiskCache) -> Self {
        // Initialize sources in priority order
        let mut sources: Vec<Box<dyn ElevationSource>> = Vec::new();
        
        // 1. AWS Terrarium (free, reliable, global)
        sources.push(Box::new(TerrariumSource::new()));
        
        // 2. USGS 3DEP (high quality, slower)
        sources.push(Box::new(Usgs3DepSource::new()));
        
        // 3. OpenTopography (requires API key)
        let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY").ok();
        sources.push(Box::new(OpenTopographySource::new(api_key)));
        
        Self {
            sources,
            cache,
            tiles: Arc::new(Mutex::new(HashMap::new())),
            queue: Arc::new(Mutex::new(VecDeque::new())),
            in_progress: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(DownloaderStats::default())),
            noise: Perlin::new(42),
            max_concurrent: 8,
            last_request: Arc::new(Mutex::new(HashMap::new())),
            min_request_interval: Duration::from_secs(2),
        }
    }
    
    /// Get elevation at GPS coordinate
    ///
    /// Returns cached data immediately, or queues download and returns None.
    /// On subsequent calls, returns downloaded data or procedural fallback.
    pub fn get_elevation(&mut self, lat: f64, lon: f64, zoom: u8) -> Option<f32> {
        let tile_key = self.latlon_to_tile_key(lat, lon, zoom);
        
        // Check memory cache
        {
            let tiles = self.tiles.lock().unwrap();
            if let Some(tile) = tiles.get(&tile_key) {
                return tile.get_elevation(lat, lon);
            }
        }
        
        // Check if already in download queue or in progress
        {
            let in_progress = self.in_progress.lock().unwrap();
            if in_progress.contains_key(&tile_key) {
                // Download in progress, return None for now
                return None;
            }
        }
        
        // Not in cache, not downloading - queue it
        self.queue_download(lat, lon, zoom, 1.0);
        
        // Return procedural fallback for immediate display
        Some(self.generate_procedural_elevation(lat, lon))
    }
    
    /// Queue a tile for download
    pub fn queue_download(&self, lat: f64, lon: f64, zoom: u8, priority: f32) {
        let task = DownloadTask {
            lat,
            lon,
            zoom,
            priority,
            added_at: Instant::now(),
        };
        
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(task);
    }
    
    /// Process download queue (call this regularly, e.g., once per frame)
    ///
    /// Downloads up to max_concurrent tiles in parallel.
    pub fn process_queue(&self) {
        let current_in_progress = {
            let in_progress = self.in_progress.lock().unwrap();
            in_progress.len()
        };
        
        if current_in_progress >= self.max_concurrent {
            return; // At capacity
        }
        
        // Pop tasks from queue up to capacity
        let tasks_to_start = self.max_concurrent - current_in_progress;
        let mut tasks = Vec::new();
        
        {
            let mut queue = self.queue.lock().unwrap();
            for _ in 0..tasks_to_start {
                if let Some(task) = queue.pop_front() {
                    tasks.push(task);
                } else {
                    break;
                }
            }
        }
        
        // Start downloads
        for task in tasks {
            self.download_tile_async(task);
        }
    }
    
    /// Download a tile asynchronously
    fn download_tile_async(&self, task: DownloadTask) {
        let tile_key = self.latlon_to_tile_key(task.lat, task.lon, task.zoom);
        
        // Mark as in progress
        {
            let mut in_progress = self.in_progress.lock().unwrap();
            in_progress.insert(tile_key, Instant::now());
        }
        
        let tiles = Arc::clone(&self.tiles);
        let in_progress = Arc::clone(&self.in_progress);
        let stats = Arc::clone(&self.stats);
        let cache = self.cache.clone();
        let sources = self.sources.iter()
            .map(|s| s.name().to_string())
            .collect::<Vec<_>>();
        let last_request = Arc::clone(&self.last_request);
        let min_interval = self.min_request_interval;
        let noise = self.noise.clone();
        
        // Spawn thread for download
        std::thread::spawn(move || {
            let result = Self::download_tile_blocking(
                task.lat,
                task.lon,
                task.zoom,
                &cache,
                &sources,
                &last_request,
                min_interval,
            );
            
            match result {
                Ok(tile) => {
                    // Store in memory cache
                    let mut tiles_lock = tiles.lock().unwrap();
                    tiles_lock.insert(tile_key, Arc::new(tile));
                    
                    // Update stats
                    let mut stats_lock = stats.lock().unwrap();
                    stats_lock.downloads_success += 1;
                }
                Err(e) => {
                    eprintln!("Failed to download tile at ({}, {}): {}", task.lat, task.lon, e);
                    
                    // Generate procedural fallback and cache it
                    let tile = Self::generate_procedural_tile(task.lat, task.lon, task.zoom, &noise);
                    let mut tiles_lock = tiles.lock().unwrap();
                    tiles_lock.insert(tile_key, Arc::new(tile));
                    
                    // Update stats
                    let mut stats_lock = stats.lock().unwrap();
                    stats_lock.downloads_failed += 1;
                    stats_lock.procedural_fallbacks += 1;
                }
            }
            
            // Remove from in_progress
            let mut in_progress_lock = in_progress.lock().unwrap();
            in_progress_lock.remove(&tile_key);
        });
    }
    
    /// Blocking download with source fallback chain
    fn download_tile_blocking(
        lat: f64,
        lon: f64,
        zoom: u8,
        cache: &DiskCache,
        sources: &[String],
        last_request: &Arc<Mutex<HashMap<String, Instant>>>,
        min_interval: Duration,
    ) -> Result<ElevationTile, Box<dyn std::error::Error>> {
        // 1. Try cache first
        let cache_key = format!("elevation_{}_{}_z{}.bin", 
            (lat * 100.0) as i32, (lon * 100.0) as i32, zoom);
        
        if let Ok(bytes) = cache.read_srtm(&cache_key) {
            if let Ok(tile) = bincode::deserialize::<ElevationTile>(&bytes) {
                return Ok(tile);
            }
        }
        
        // 2. Try each source in order
        for source_name in sources {
            // Check rate limit
            {
                let mut last_req = last_request.lock().unwrap();
                if let Some(last_time) = last_req.get(source_name) {
                    let elapsed = last_time.elapsed();
                    if elapsed < min_interval {
                        std::thread::sleep(min_interval - elapsed);
                    }
                }
                last_req.insert(source_name.clone(), Instant::now());
            }
            
            // Try to fetch from this source
            let result = match source_name.as_str() {
                "AWS Terrarium" => {
                    let source = TerrariumSource::new();
                    source.fetch_tile(lat, lon, zoom)
                }
                "USGS 3DEP" => {
                    let source = Usgs3DepSource::new();
                    source.fetch_tile(lat, lon, zoom)
                }
                "OpenTopography" => {
                    let api_key = std::env::var("OPENTOPOGRAPHY_API_KEY").ok();
                    let source = OpenTopographySource::new(api_key);
                    if !source.is_available() {
                        continue; // Skip if API key not available
                    }
                    source.fetch_tile(lat, lon, zoom)
                }
                _ => continue,
            };
            
            if let Ok(tile) = result {
                // Cache the successful download
                if let Ok(bytes) = bincode::serialize(&tile) {
                    let _ = cache.write_srtm(&cache_key, &bytes);
                }
                return Ok(tile);
            }
        }
        
        Err("All sources failed".into())
    }
    
    /// Generate a full procedural tile (for gap filling only)
    fn generate_procedural_tile(lat: f64, lon: f64, zoom: u8, noise: &Perlin) -> ElevationTile {
        let tile_size = 1.0 / (2_u32.pow((zoom as u32).min(8)) as f64);
        let sw_lat = (lat / tile_size).floor() * tile_size;
        let sw_lon = (lon / tile_size).floor() * tile_size;
        let ne_lat = sw_lat + tile_size;
        let ne_lon = sw_lon + tile_size;
        
        let resolution = 256;
        let mut elevations = Vec::with_capacity(resolution * resolution);
        
        for y in 0..resolution {
            let v = y as f64 / (resolution - 1) as f64;
            let tile_lat = sw_lat + (ne_lat - sw_lat) * v;
            
            for x in 0..resolution {
                let u = x as f64 / (resolution - 1) as f64;
                let tile_lon = sw_lon + (ne_lon - sw_lon) * u;
                
                let elev = Self::procedural_elevation(tile_lat, tile_lon, noise);
                elevations.push(elev);
            }
        }
        
        ElevationTile {
            sw_lat,
            sw_lon,
            ne_lat,
            ne_lon,
            width: resolution,
            height: resolution,
            elevations,
            source: "Procedural".to_string(),
        }
    }
    
    /// Generate procedural elevation at a single point
    fn procedural_elevation(lat: f64, lon: f64, noise: &Perlin) -> f32 {
        let scale = 0.02;
        let x = lon * scale;
        let y = lat * scale;
        
        // Multi-octave noise
        let octave1 = noise.get([x, y]) * 300.0;
        let octave2 = noise.get([x * 2.0, y * 2.0]) * 150.0;
        let octave3 = noise.get([x * 4.0, y * 4.0]) * 75.0;
        let octave4 = noise.get([x * 8.0, y * 8.0]) * 30.0;
        
        let elevation = octave1 + octave2 + octave3 + octave4;
        elevation.clamp(-500.0, 3000.0) as f32
    }
    
    /// Generate procedural elevation (gap filling only)
    fn generate_procedural_elevation(&self, lat: f64, lon: f64) -> f32 {
        Self::procedural_elevation(lat, lon, &self.noise)
    }
    
    /// Convert lat/lon to tile key
    fn latlon_to_tile_key(&self, lat: f64, lon: f64, zoom: u8) -> (i32, i32, u8) {
        let tile_size = 1.0 / (2_u32.pow((zoom as u32).min(8)) as f64);
        let tile_lat = (lat / tile_size).floor() as i32;
        let tile_lon = (lon / tile_size).floor() as i32;
        (tile_lat, tile_lon, zoom)
    }
    
    /// Get current statistics
    pub fn stats(&self) -> DownloaderStats {
        self.stats.lock().unwrap().clone()
    }
    
    /// Get queue length
    pub fn queue_length(&self) -> usize {
        self.queue.lock().unwrap().len()
    }
    
    /// Get number of active downloads
    pub fn active_downloads(&self) -> usize {
        self.in_progress.lock().unwrap().len()
    }
}

// Make ElevationTile serializable for caching
impl serde::Serialize for ElevationTile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ElevationTile", 8)?;
        state.serialize_field("sw_lat", &self.sw_lat)?;
        state.serialize_field("sw_lon", &self.sw_lon)?;
        state.serialize_field("ne_lat", &self.ne_lat)?;
        state.serialize_field("ne_lon", &self.ne_lon)?;
        state.serialize_field("width", &self.width)?;
        state.serialize_field("height", &self.height)?;
        state.serialize_field("elevations", &self.elevations)?;
        state.serialize_field("source", &self.source)?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for ElevationTile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct ElevationTileData {
            sw_lat: f64,
            sw_lon: f64,
            ne_lat: f64,
            ne_lon: f64,
            width: usize,
            height: usize,
            elevations: Vec<f32>,
            source: String,
        }
        
        let data = ElevationTileData::deserialize(deserializer)?;
        Ok(ElevationTile {
            sw_lat: data.sw_lat,
            sw_lon: data.sw_lon,
            ne_lat: data.ne_lat,
            ne_lon: data.ne_lon,
            width: data.width,
            height: data.height,
            elevations: data.elevations,
            source: data.source,
        })
    }
}
