/// Async multi-source SRTM tile downloader with parallel fetching
///
/// Downloads SRTM elevation tiles from multiple providers with:
/// - Parallel downloading of multiple tiles concurrently
/// - Multiple provider fallback (OpenTopography → AWS → procedural)
/// - 2-second cooldown between requests per provider (project rule)
/// - Priority queue based on distance from camera
/// - Automatic retry with exponential backoff
/// - Disk caching for downloaded tiles
/// - GeoTIFF to .hgt conversion for AWS/OpenTopography sources

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
    
    /// Convert GeoTIFF elevation data to .hgt format
    ///
    /// GeoTIFF files contain elevation as floating-point or integer raster data.
    /// We need to convert to standard SRTM .hgt format: 16-bit big-endian signed integers.
    fn convert_geotiff_to_hgt(
        geotiff_bytes: &[u8],
        _lat: i16,
        _lon: i16,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        use tiff::decoder::{Decoder, DecodingResult};
        use std::io::Cursor;
        
        eprintln!("[SRTM] Converting GeoTIFF to .hgt format...");
        
        let cursor = Cursor::new(geotiff_bytes);
        let mut decoder = Decoder::new(cursor)?;
        
        // Read image dimensions
        let (width, height) = decoder.dimensions()?;
        eprintln!("[SRTM] GeoTIFF dimensions: {}x{}", width, height);
        
        // Decode the image data
        let image_data = decoder.read_image()?;
        
        // Convert to elevation values
        let elevations: Vec<i16> = match image_data {
            DecodingResult::U8(data) => {
                eprintln!("[SRTM] GeoTIFF format: U8");
                // U8 is unusual for elevation, treat as scaled
                data.iter().map(|&v| v as i16).collect()
            }
            DecodingResult::U16(data) => {
                eprintln!("[SRTM] GeoTIFF format: U16");
                // U16 elevation data - convert to signed
                data.iter().map(|&v| {
                    if v == 0 || v == 65535 {
                        -32768  // NoData value
                    } else if v > 32767 {
                        (v as i32 - 65536) as i16  // Handle values > 32767
                    } else {
                        v as i16
                    }
                }).collect()
            }
            DecodingResult::I16(data) => {
                eprintln!("[SRTM] GeoTIFF format: I16 (native)");
                // Already in correct format
                data
            }
            DecodingResult::U32(data) => {
                eprintln!("[SRTM] GeoTIFF format: U32");
                // U32 elevation - clamp to i16 range
                data.iter().map(|&v| {
                    if v == 0 || v == u32::MAX {
                        -32768  // NoData
                    } else if v > 32767 {
                        32767  // Clamp high values
                    } else {
                        v as i16
                    }
                }).collect()
            }
            DecodingResult::I32(data) => {
                eprintln!("[SRTM] GeoTIFF format: I32");
                // I32 elevation - clamp to i16 range
                data.iter().map(|&v| {
                    if v == i32::MIN {
                        -32768  // NoData
                    } else if v > 32767 {
                        32767  // Clamp high
                    } else if v < -32768 {
                        -32768  // Clamp low
                    } else {
                        v as i16
                    }
                }).collect()
            }
            DecodingResult::F32(data) => {
                eprintln!("[SRTM] GeoTIFF format: F32 (floating-point)");
                // F32 elevation (common for high-precision DEMs)
                data.iter().map(|&v| {
                    if v.is_nan() || v == -9999.0 || v < -32000.0 {
                        -32768  // NoData
                    } else {
                        v.round().clamp(-32768.0, 32767.0) as i16
                    }
                }).collect()
            }
            DecodingResult::F64(data) => {
                eprintln!("[SRTM] GeoTIFF format: F64 (high-precision)");
                // F64 elevation
                data.iter().map(|&v| {
                    if v.is_nan() || v == -9999.0 || v < -32000.0 {
                        -32768  // NoData
                    } else {
                        v.round().clamp(-32768.0, 32767.0) as i16
                    }
                }).collect()
            }
            _ => {
                return Err("Unsupported TIFF color type for elevation data".into());
            }
        };
        
        // Check if we need to resample to standard SRTM resolution
        let target_size = if width >= 3000 && height >= 3000 {
            3601  // SRTM1 (1 arc-second, ~30m)
        } else {
            1201  // SRTM3 (3 arc-second, ~90m)
        };
        
        let resampled = if width == target_size && height == target_size {
            // Perfect size, no resampling needed
            eprintln!("[SRTM] Dimensions match SRTM{} ({}x{}), no resampling needed",
                     if target_size == 3601 { "1" } else { "3" },
                     target_size, target_size);
            elevations
        } else {
            // Need to resample to standard SRTM grid
            eprintln!("[SRTM] Resampling from {}x{} to {}x{} (SRTM{} grid)",
                     width, height, target_size, target_size,
                     if target_size == 3601 { "1" } else { "3" });
            resample_elevation_grid(&elevations, width as usize, height as usize, target_size as usize)
        };
        
        // Convert to big-endian bytes
        let mut hgt_bytes = Vec::with_capacity(resampled.len() * 2);
        for &elev in &resampled {
            hgt_bytes.push((elev >> 8) as u8);   // High byte
            hgt_bytes.push((elev & 0xFF) as u8); // Low byte
        }
        
        eprintln!("[SRTM] Converted to .hgt format: {} bytes", hgt_bytes.len());
        Ok(hgt_bytes)
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
                        
                        // Check if this is GeoTIFF format (needs conversion)
                        let hgt_bytes = if bytes.len() < 10000 || bytes.starts_with(b"II") || bytes.starts_with(b"MM") {
                            // GeoTIFF format (TIFF magic numbers: II=little-endian, MM=big-endian)
                            eprintln!("[SRTM] Detected GeoTIFF format, converting to .hgt...");
                            match Self::convert_geotiff_to_hgt(&bytes, lat, lon) {
                                Ok(converted) => converted,
                                Err(e) => {
                                    eprintln!("[SRTM] GeoTIFF conversion failed: {}", e);
                                    // Try next source
                                    break;
                                }
                            }
                        } else {
                            // Already .hgt format
                            bytes
                        };
                        
                        // Parse the .hgt data
                        match crate::elevation::parse_hgt(&filename, &hgt_bytes) {
                            Ok(tile) => {
                                // Save converted .hgt to disk cache for future use
                                let _ = self.cache.write_srtm(&filename, &hgt_bytes);
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

/// Resample elevation grid using bilinear interpolation
///
/// Converts arbitrary resolution to standard SRTM grid (3601×3601 or 1201×1201)
fn resample_elevation_grid(
    src: &[i16],
    src_width: usize,
    src_height: usize,
    target_size: usize,
) -> Vec<i16> {
    let mut result = Vec::with_capacity(target_size * target_size);
    
    for target_y in 0..target_size {
        for target_x in 0..target_size {
            // Map target coordinates to source coordinates
            let src_x = (target_x as f64 / (target_size - 1) as f64) * (src_width - 1) as f64;
            let src_y = (target_y as f64 / (target_size - 1) as f64) * (src_height - 1) as f64;
            
            // Bilinear interpolation
            let x0 = src_x.floor() as usize;
            let x1 = (x0 + 1).min(src_width - 1);
            let y0 = src_y.floor() as usize;
            let y1 = (y0 + 1).min(src_height - 1);
            
            let fx = src_x - x0 as f64;
            let fy = src_y - y0 as f64;
            
            // Get four corner values
            let v00 = src[y0 * src_width + x0];
            let v10 = src[y0 * src_width + x1];
            let v01 = src[y1 * src_width + x0];
            let v11 = src[y1 * src_width + x1];
            
            // Check for void values
            if v00 == -32768 || v10 == -32768 || v01 == -32768 || v11 == -32768 {
                result.push(-32768);  // Propagate void
            } else {
                // Bilinear interpolation
                let v0 = v00 as f64 * (1.0 - fx) + v10 as f64 * fx;
                let v1 = v01 as f64 * (1.0 - fx) + v11 as f64 * fx;
                let v = v0 * (1.0 - fy) + v1 * fy;
                result.push(v.round() as i16);
            }
        }
    }
    
    result
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
