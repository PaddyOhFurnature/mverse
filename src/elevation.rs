/// SRTM elevation data parsing.
///
/// SRTM (Shuttle Radar Topography Mission) provides global elevation data.
/// Data is stored in HGT files as 16-bit big-endian signed integers.

/// SRTM resolution variants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrtmResolution {
    /// SRTM1: 1 arc-second resolution (3601 × 3601 samples per tile)
    Srtm1,
    /// SRTM3: 3 arc-second resolution (1201 × 1201 samples per tile)
    Srtm3,
}

impl SrtmResolution {
    /// Returns the number of samples per side for this resolution
    pub fn samples(&self) -> usize {
        match self {
            SrtmResolution::Srtm1 => 3601,
            SrtmResolution::Srtm3 => 1201,
        }
    }

    /// Returns the expected file size in bytes
    pub fn file_size(&self) -> usize {
        let samples = self.samples();
        samples * samples * 2 // 2 bytes per sample
    }
}

/// Parsed SRTM tile data
#[derive(Debug, Clone)]
pub struct SrtmTile {
    /// Southwest corner latitude (degrees)
    pub sw_lat: i16,
    /// Southwest corner longitude (degrees)
    pub sw_lon: i16,
    /// Resolution of this tile
    pub resolution: SrtmResolution,
    /// Elevation data in row-major order (north to south, west to east)
    /// Value -32768 indicates void/no data
    pub elevations: Vec<i16>,
}

/// Parses an SRTM HGT file
pub fn parse_hgt(filename: &str, bytes: &[u8]) -> Result<SrtmTile, Box<dyn std::error::Error>> {
    // Parse filename for tile origin
    let (sw_lat, sw_lon) = parse_hgt_filename(filename)?;
    
    // Detect resolution from file size
    let resolution = if bytes.len() == SrtmResolution::Srtm1.file_size() {
        SrtmResolution::Srtm1
    } else if bytes.len() == SrtmResolution::Srtm3.file_size() {
        SrtmResolution::Srtm3
    } else {
        return Err(format!(
            "Invalid HGT file size: {} bytes (expected {} for SRTM1 or {} for SRTM3)",
            bytes.len(),
            SrtmResolution::Srtm1.file_size(),
            SrtmResolution::Srtm3.file_size()
        ).into());
    };
    
    // Parse elevation data (16-bit big-endian signed integers)
    let sample_count = resolution.samples() * resolution.samples();
    let mut elevations = Vec::with_capacity(sample_count);
    
    for i in 0..sample_count {
        let offset = i * 2;
        if offset + 1 >= bytes.len() {
            return Err("Truncated HGT file".into());
        }
        
        // Big-endian: high byte first, then low byte
        let high = bytes[offset] as i16;
        let low = bytes[offset + 1] as i16;
        let value = (high << 8) | (low & 0xFF);
        
        elevations.push(value);
    }
    
    Ok(SrtmTile {
        sw_lat,
        sw_lon,
        resolution,
        elevations,
    })
}

/// Queries elevation at a specific GPS coordinate using bilinear interpolation
///
/// Returns None if:
/// - The coordinate is outside the tile bounds
/// - Any of the 4 nearest samples is void (-32768)
pub fn get_elevation(tile: &SrtmTile, lat: f64, lon: f64) -> Option<f64> {
    // Check if coordinate is within tile bounds
    // Tile covers [sw_lat, sw_lat+1) × [sw_lon, sw_lon+1)
    if lat < tile.sw_lat as f64 || lat >= (tile.sw_lat + 1) as f64 {
        return None;
    }
    if lon < tile.sw_lon as f64 || lon >= (tile.sw_lon + 1) as f64 {
        return None;
    }
    
    let samples = tile.resolution.samples();
    
    // Convert lat/lon to grid coordinates
    // Grid origin is NW corner (sw_lat + 1, sw_lon)
    // Row 0 = north edge, Col 0 = west edge
    let grid_lat = (tile.sw_lat + 1) as f64 - lat; // Distance from north edge
    let grid_lon = lon - tile.sw_lon as f64; // Distance from west edge
    
    // Convert to sample indices (fractional)
    let row_f = grid_lat * (samples - 1) as f64;
    let col_f = grid_lon * (samples - 1) as f64;
    
    // Get integer indices of the 4 nearest samples
    let row0 = row_f.floor() as usize;
    let col0 = col_f.floor() as usize;
    let row1 = row0 + 1;
    let col1 = col0 + 1;
    
    // Check bounds
    if row1 >= samples || col1 >= samples {
        return None;
    }
    
    // Get the 4 corner elevations
    let idx_00 = row0 * samples + col0; // NW
    let idx_01 = row0 * samples + col1; // NE
    let idx_10 = row1 * samples + col0; // SW
    let idx_11 = row1 * samples + col1; // SE
    
    let elev_00 = tile.elevations[idx_00];
    let elev_01 = tile.elevations[idx_01];
    let elev_10 = tile.elevations[idx_10];
    let elev_11 = tile.elevations[idx_11];
    
    // Check for void values
    const VOID: i16 = -32768;
    if elev_00 == VOID || elev_01 == VOID || elev_10 == VOID || elev_11 == VOID {
        return None;
    }
    
    // Bilinear interpolation
    let dx = col_f - col0 as f64; // Fractional part [0, 1)
    let dy = row_f - row0 as f64; // Fractional part [0, 1)
    
    // Interpolate along top edge (row0)
    let top = elev_00 as f64 * (1.0 - dx) + elev_01 as f64 * dx;
    
    // Interpolate along bottom edge (row1)
    let bottom = elev_10 as f64 * (1.0 - dx) + elev_11 as f64 * dx;
    
    // Interpolate between top and bottom
    let elevation = top * (1.0 - dy) + bottom * dy;
    
    Some(elevation)
}


/// Parses SRTM HGT filename to extract tile origin
///
/// Examples:
/// - N37W122.hgt → lat=37, lon=-122
/// - S27E153.hgt → lat=-27, lon=153
pub fn parse_hgt_filename(filename: &str) -> Result<(i16, i16), Box<dyn std::error::Error>> {
    // Remove .hgt extension if present
    let name = filename.trim_end_matches(".hgt").trim_end_matches(".HGT");
    
    if name.len() < 7 {
        return Err(format!("Invalid HGT filename format: {}", filename).into());
    }
    
    // Parse latitude
    let lat_char = name.chars().nth(0).ok_or("Missing latitude direction")?;
    let lat_str = &name[1..3];
    let mut lat: i16 = lat_str.parse()
        .map_err(|_| format!("Invalid latitude value: {}", lat_str))?;
    
    if lat_char == 'S' || lat_char == 's' {
        lat = -lat;
    } else if lat_char != 'N' && lat_char != 'n' {
        return Err(format!("Invalid latitude direction: {} (expected N or S)", lat_char).into());
    }
    
    // Parse longitude
    let lon_char = name.chars().nth(3).ok_or("Missing longitude direction")?;
    let lon_str = &name[4..7];
    let mut lon: i16 = lon_str.parse()
        .map_err(|_| format!("Invalid longitude value: {}", lon_str))?;
    
    if lon_char == 'W' || lon_char == 'w' {
        lon = -lon;
    } else if lon_char != 'E' && lon_char != 'e' {
        return Err(format!("Invalid longitude direction: {} (expected E or W)", lon_char).into());
    }
    
    Ok((lat, lon))
}

/// Simple SRTM elevation manager with in-memory tile cache
///
/// Loads SRTM tiles on demand and caches them for repeated queries.
pub struct SrtmManager {
    tiles: std::collections::HashMap<(i16, i16), SrtmTile>,
    cache: crate::cache::DiskCache,
}

impl SrtmManager {
    /// Create a new SRTM manager with the given cache
    pub fn new(cache: crate::cache::DiskCache) -> Self {
        Self {
            tiles: std::collections::HashMap::new(),
            cache,
        }
    }
    
    /// Get elevation at a GPS coordinate
    ///
    /// Returns None if:
    /// - The tile isn't cached and can't be loaded
    /// - The coordinate has void/no-data in SRTM
    pub fn get_elevation(&mut self, lat: f64, lon: f64) -> Option<f64> {
        // Determine which tile this coordinate is in
        // SRTM tiles are named by their SW corner
        let tile_lat = lat.floor() as i16;
        let tile_lon = lon.floor() as i16;
        
        // Try to get tile from cache, or load it
        if !self.tiles.contains_key(&(tile_lat, tile_lon)) {
            if let Some(tile) = self.load_tile(tile_lat, tile_lon) {
                self.tiles.insert((tile_lat, tile_lon), tile);
            } else {
                // Tile not available
                return None;
            }
        }
        
        // Query elevation from the tile
        let tile = self.tiles.get(&(tile_lat, tile_lon))?;
        get_elevation(tile, lat, lon)
    }
    
    /// Load an SRTM tile from cache or remote providers
    ///
    /// Tries (in order):
    /// 1) Project-local disk cache
    /// 2) Several remote providers (attempts multiple URL patterns)
    ///
    /// Returns None if tile isn't available or can't be parsed.
    fn load_tile(&self, lat: i16, lon: i16) -> Option<SrtmTile> {
        use std::time::Duration;
        use std::io::Cursor;

        // Generate tile filename and base (without .hgt)
        let lat_dir = if lat >= 0 { 'N' } else { 'S' };
        let lon_dir = if lon >= 0 { 'E' } else { 'W' };
        let filename = format!("{}{:02}{}{:03}.hgt",
            lat_dir, lat.abs(), lon_dir, lon.abs());
        let tile_base = filename.trim_end_matches(".hgt");

        // 1) Try to load from cache
        if let Ok(bytes) = self.cache.read_srtm(&filename) {
            return parse_hgt(&filename, &bytes).ok();
        }

        // 2) Remote providers (multiple fallback URLs)
        // Respect 2-second cooldown between external calls
        let providers = vec![
            "https://viewfinderpanoramas.org/dem3/".to_string(),
            "https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/".to_string(),
            "https://srtm.kurviger.de/SRTM3/".to_string(),
        ];

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .ok()?;

        for base in providers.iter() {
            // Try several candidate URL patterns
            let candidates = vec![
                format!("{}{}.hgt", base, tile_base),
                format!("{}{}.hgt.zip", base, tile_base),
                format!("{}{}.zip", base, tile_base),
                format!("{}{}", base, filename),
            ];

            for url in candidates {
                // 2-second cooldown as per project rules
                std::thread::sleep(Duration::from_secs(2));

                let resp = client.get(&url).send();
                if let Ok(resp) = resp {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes() {
                            let data = bytes.to_vec();

                            // If zip archive (PK..), try to extract .hgt inside
                            if data.len() >= 4 && &data[0..2] == b"PK" {
                                if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(data)) {
                                    for i in 0..archive.len() {
                                        if let Ok(mut f) = archive.by_index(i) {
                                            let name = f.name().to_string();
                                            if name.to_lowercase().ends_with(".hgt") {
                                                let mut buf = Vec::new();
                                                if std::io::copy(&mut f, &mut buf).is_ok() {
                                                    let _ = self.cache.write_srtm(&filename, &buf);
                                                    return parse_hgt(&filename, &buf).ok();
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                // Raw HGT bytes
                                let _ = self.cache.write_srtm(&filename, &data);
                                return parse_hgt(&filename, &data).ok();
                            }
                        }
                    }
                }
            }
        }

        // No tile available
        None
    }
    
    /// Get elevation with a fallback value for missing data
    ///
    /// Returns the elevation if available, otherwise returns the fallback value.
    pub fn get_elevation_or(&mut self, lat: f64, lon: f64, fallback: f64) -> f64 {
        self.get_elevation(lat, lon).unwrap_or(fallback)
    }
}
