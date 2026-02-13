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
