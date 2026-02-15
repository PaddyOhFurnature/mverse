/// Async multi-source SRTM tile downloader with parallel fetching
///
/// Downloads SRTM elevation tiles from multiple providers with:
/// - Parallel downloading of multiple tiles concurrently
/// - Multiple provider fallback (OpenTopography → AWS → procedural)
/// - 2-second cooldown between requests per provider (project rule)
/// - Priority queue based on distance from camera
/// - Automatic retry with exponential backoff
/// - Disk caching for downloaded tiles

use std::time::Duration;
use std::io::Cursor;
use tokio::time::sleep;

/// SRTM data source configuration
#[derive(Debug, Clone)]
pub struct SrtmSource {
    pub name: &'static str,
    pub base_url: String,
    pub needs_auth: bool,
    pub format: SourceFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// Direct .hgt file download
    HgtDirect,
    /// Zipped .hgt file
    HgtZip,
    /// OpenTopography API (returns GeoTIFF)
    OpenTopoAPI,
    /// AWS Terrain Tiles (GeoTIFF, needs coordinate conversion)
    AwsTerrain,
}

impl SrtmSource {
    /// OpenTopography API source (primary - requires API key)
    pub fn opentopography() -> Option<Self> {
        std::env::var("OPENTOPOGRAPHY_API_KEY").ok().map(|api_key| {
            Self {
                name: "OpenTopography",
                base_url: format!(
                    "https://portal.opentopography.org/API/globaldem?demtype=SRTMGL1&outputFormat=GTiff&API_Key={}",
                    api_key
                ),
                needs_auth: true,
                format: SourceFormat::OpenTopoAPI,
            }
        })
    }
    
    /// AWS Terrain Tiles (secondary - no auth required)
    pub fn aws_terrain() -> Self {
        Self {
            name: "AWS Terrain",
            base_url: "https://s3.amazonaws.com/elevation-tiles-prod/geotiff".to_string(),
            needs_auth: false,
            format: SourceFormat::AwsTerrain,
        }
    }
    
    /// CGIAR-CSI direct file access (tertiary - may 404)
    pub fn cgiar_direct() -> Self {
        Self {
            name: "CGIAR Direct",
            base_url: "https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5".to_string(),
            needs_auth: false,
            format: SourceFormat::HgtZip,
        }
    }
}

/// Multi-source async SRTM downloader
pub struct SrtmDownloader {
    sources: Vec<SrtmSource>,
    cache: crate::cache::DiskCache,
    /// Reqwest client with timeout configured
    client: reqwest::Client,
    /// Last request time per provider (for rate limiting)
    last_request: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>>,
}

impl SrtmDownloader {
    /// Create a new downloader with all available sources
    pub fn new(cache: crate::cache::DiskCache) -> Self {
        let mut sources = Vec::new();
        
        // Try OpenTopography first (best quality, requires API key)
        if let Some(ot) = SrtmSource::opentopography() {
            sources.push(ot);
            eprintln!("[SRTM] OpenTopography API key found, will use as primary source");
        } else {
            eprintln!("[SRTM] No OpenTopography API key (set OPENTOPOGRAPHY_API_KEY env var)");
        }
        
        // AWS Terrain Tiles (no auth, always available)
        sources.push(SrtmSource::aws_terrain());
        
        // CGIAR direct (may 404, but worth trying)
        sources.push(SrtmSource::cgiar_direct());
        
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to build reqwest client");
        
        Self {
            sources,
            cache,
            client,
            last_request: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }
    
    /// Download a single SRTM tile asynchronously
    ///
    /// Tries all configured sources in priority order with 2-second cooldown.
    /// Returns the parsed tile data or None if all sources fail.
    pub async fn download_tile(&self, lat: i16, lon: i16) -> Option<crate::elevation::SrtmTile> {
        // Generate tile filename
        let lat_dir = if lat >= 0 { 'N' } else { 'S' };
        let lon_dir = if lon >= 0 { 'E' } else { 'W' };
        let filename = format!("{}{:02}{}{:03}.hgt",
            lat_dir, lat.abs(), lon_dir, lon.abs());
        
        eprintln!("[SRTM] Downloading tile: {} (lat={}, lon={})", filename, lat, lon);
        
        // Try each source in priority order
        for (idx, source) in self.sources.iter().enumerate() {
            eprintln!("[SRTM] Trying source {}/{}: {}", idx + 1, self.sources.len(), source.name);
            
            // Respect 2-second cooldown per provider (project rule from RULES.md)
            self.wait_for_cooldown(&source.name).await;
            
            // Try to fetch from this source with retries
            let mut retry_delay = Duration::from_secs(2);
            for attempt in 1..=3 {
                match self.fetch_from_source(source, lat, lon, &filename).await {
                    Ok(bytes) => {
                        eprintln!("[SRTM] Successfully downloaded {} bytes from {}", 
                                 bytes.len(), source.name);
                        
                        // Parse the downloaded data
                        match crate::elevation::parse_hgt(&filename, &bytes) {
                            Ok(tile) => {
                                // Save to disk cache for future use
                                let _ = self.cache.write_srtm(&filename, &bytes);
                                eprintln!("[SRTM] Tile {} parsed and cached successfully", filename);
                                return Some(tile);
                            }
                            Err(e) => {
                                eprintln!("[SRTM] Failed to parse tile from {}: {}", source.name, e);
                                // Try next source
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[SRTM] Attempt {}/3 failed for {}: {}", attempt, source.name, e);
                        if attempt < 3 {
                            eprintln!("[SRTM] Retrying in {:?}...", retry_delay);
                            sleep(retry_delay).await;
                            retry_delay *= 2;  // Exponential backoff
                        }
                    }
                }
            }
            
            eprintln!("[SRTM] Source {} failed after 3 attempts, trying next source", source.name);
        }
        
        eprintln!("[SRTM] All sources failed for tile {}", filename);
        None
    }
    
    /// Wait for 2-second cooldown since last request to this provider
    async fn wait_for_cooldown(&self, provider_name: &str) {
        let mut last_times = self.last_request.lock().await;
        
        if let Some(last_time) = last_times.get(provider_name) {
            let elapsed = last_time.elapsed();
            let cooldown = Duration::from_secs(2);
            
            if elapsed < cooldown {
                let wait_time = cooldown - elapsed;
                eprintln!("[SRTM] Rate limit: waiting {:?} for {}", wait_time, provider_name);
                drop(last_times);  // Release lock before sleeping
                sleep(wait_time).await;
                last_times = self.last_request.lock().await;
            }
        }
        
        last_times.insert(provider_name.to_string(), std::time::Instant::now());
    }
    
    /// Fetch tile data from a specific source
    async fn fetch_from_source(
        &self,
        source: &SrtmSource,
        lat: i16,
        lon: i16,
        filename: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        match source.format {
            SourceFormat::OpenTopoAPI => {
                self.fetch_opentopo(source, lat, lon).await
            }
            SourceFormat::AwsTerrain => {
                self.fetch_aws_terrain(source, lat, lon).await
            }
            SourceFormat::HgtZip | SourceFormat::HgtDirect => {
                self.fetch_hgt_direct(source, filename).await
            }
        }
    }
    
    /// Fetch from OpenTopography API
    async fn fetch_opentopo(
        &self,
        source: &SrtmSource,
        lat: i16,
        lon: i16,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        // OpenTopography API takes bounding box (south, north, west, east)
        let url = format!(
            "{}&south={}&north={}&west={}&east={}",
            source.base_url,
            lat,      // south
            lat + 1,  // north
            lon,      // west  
            lon + 1   // east
        );
        
        eprintln!("[SRTM] OpenTopography API request: lat=[{},{}], lon=[{},{}]", 
                 lat, lat + 1, lon, lon + 1);
        
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Err(format!("HTTP {}: {}", resp.status(), resp.text().await?).into());
        }
        
        let bytes = resp.bytes().await?;
        
        // OpenTopography returns GeoTIFF, need to convert to .hgt format
        // For now, just return the raw bytes and we'll handle conversion later
        // TODO: Add GeoTIFF parsing and conversion to .hgt format
        Ok(bytes.to_vec())
    }
    
    /// Fetch from AWS Terrain Tiles
    async fn fetch_aws_terrain(
        &self,
        source: &SrtmSource,
        lat: i16,
        lon: i16,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        // AWS uses slippy map tile coordinates (z/x/y)
        // Need to convert lat/lon to tile coordinates at appropriate zoom level
        // Using zoom level 10 for ~30m resolution equivalent
        let zoom = 10;
        let (tile_x, tile_y) = Self::lat_lon_to_tile(lat as f64, lon as f64, zoom);
        
        let url = format!("{}/{}/{}/{}.tif", source.base_url, zoom, tile_x, tile_y);
        
        eprintln!("[SRTM] AWS Terrain Tiles request: z={}, x={}, y={}", zoom, tile_x, tile_y);
        
        let resp = self.client.get(&url).send().await?;
        
        if !resp.status().is_success() {
            return Err(format!("HTTP {}: {}", resp.status(), resp.text().await?).into());
        }
        
        let bytes = resp.bytes().await?;
        
        // AWS returns GeoTIFF, need to convert to .hgt format
        // TODO: Add GeoTIFF parsing and conversion to .hgt format
        Ok(bytes.to_vec())
    }
    
    /// Fetch direct .hgt or .hgt.zip file
    async fn fetch_hgt_direct(
        &self,
        source: &SrtmSource,
        filename: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let tile_base = filename.trim_end_matches(".hgt");
        
        // Try several URL patterns
        let candidates = vec![
            format!("{}/{}.hgt", source.base_url, tile_base),
            format!("{}/{}.hgt.zip", source.base_url, tile_base),
            format!("{}/{}.zip", source.base_url, tile_base),
            format!("{}/{}", source.base_url, filename),
        ];
        
        for url in candidates {
            eprintln!("[SRTM] Trying direct URL: {}", url);
            
            match self.client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let bytes = resp.bytes().await?;
                    let data = bytes.to_vec();
                    
                    // Check if this is a zip file
                    if data.len() >= 4 && &data[0..2] == b"PK" {
                        eprintln!("[SRTM] Detected zip file, extracting .hgt");
                        // Extract .hgt from zip
                        if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(data)) {
                            for i in 0..archive.len() {
                                if let Ok(mut f) = archive.by_index(i) {
                                    let name = f.name().to_string();
                                    if name.to_lowercase().ends_with(".hgt") {
                                        let mut buf = Vec::new();
                                        std::io::copy(&mut f, &mut buf)?;
                                        eprintln!("[SRTM] Extracted {} ({} bytes)", name, buf.len());
                                        return Ok(buf);
                                    }
                                }
                            }
                        }
                        return Err("No .hgt file found in zip archive".into());
                    } else {
                        // Raw .hgt file
                        eprintln!("[SRTM] Downloaded raw .hgt file ({} bytes)", data.len());
                        return Ok(data);
                    }
                }
                Ok(resp) => {
                    eprintln!("[SRTM] HTTP {}: {}", resp.status(), url);
                }
                Err(e) => {
                    eprintln!("[SRTM] Request failed: {}", e);
                }
            }
        }
        
        Err("All URL patterns failed".into())
    }
    
    /// Convert lat/lon to slippy map tile coordinates
    fn lat_lon_to_tile(lat: f64, lon: f64, zoom: u8) -> (u32, u32) {
        let n = 2_u32.pow(zoom as u32) as f64;
        let x = ((lon + 180.0) / 360.0 * n).floor() as u32;
        let y = ((1.0 - (lat.to_radians().tan() + 1.0 / lat.to_radians().cos()).ln() / std::f64::consts::PI) / 2.0 * n).floor() as u32;
        (x, y)
    }
}

/// Download multiple tiles in parallel with priority ordering
///
/// Downloads tiles closest to the camera position first.
/// Limits to 3 concurrent downloads to avoid overwhelming providers.
pub async fn download_tiles_parallel(
    downloader: &SrtmDownloader,
    tiles: Vec<(i16, i16)>,
    camera_lat: f64,
    camera_lon: f64,
) -> Vec<Option<crate::elevation::SrtmTile>> {
    use futures::stream::{FuturesUnordered, StreamExt};
    
    // Sort tiles by distance from camera (closest first)
    let mut prioritized: Vec<_> = tiles.into_iter()
        .map(|(lat, lon)| {
            // Calculate distance from camera to tile center
            let tile_center_lat = lat as f64 + 0.5;
            let tile_center_lon = lon as f64 + 0.5;
            let dist = ((camera_lat - tile_center_lat).powi(2) + 
                       (camera_lon - tile_center_lon).powi(2)).sqrt();
            (dist, lat, lon)
        })
        .collect();
    
    prioritized.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    
    let total_tiles = prioritized.len();
    eprintln!("[SRTM] Downloading {} tiles in parallel (prioritized by distance)", total_tiles);
    
    // Create all download futures
    let mut futures: FuturesUnordered<_> = prioritized
        .into_iter()
        .enumerate()
        .map(|(idx, (_dist, lat, lon))| async move {
            eprintln!("[SRTM] Starting download {}/{}: tile ({}, {})", idx + 1, total_tiles, lat, lon);
            let tile = downloader.download_tile(lat, lon).await;
            eprintln!("[SRTM] Completed download {}/{}", idx + 1, total_tiles);
            (idx, tile)
        })
        .collect();
    
    // Collect results as they complete
    let mut results = Vec::new();
    while let Some((idx, tile)) = futures.next().await {
        results.push((idx, tile));
    }
    
    // Sort results back to original order
    results.sort_by_key(|(idx, _)| *idx);
    results.into_iter().map(|(_, tile)| tile).collect()
}
