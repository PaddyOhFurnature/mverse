/// Multi-source elevation data providers with fallback chain.
///
/// Implements redundant data sources for global elevation data:
/// 1. AWS Terrarium Tiles (PNG format, free, global coverage)
/// 2. USGS 3DEP ImageServer (high quality, US-focused but global DEM available)
/// 3. OpenTopography API (requires free API key, global SRTM)
/// 4. Procedural generation (gap filling only, not primary source)

use std::error::Error;
use std::io::Cursor;

/// Standard elevation tile format (internal representation)
///
/// All sources convert their native format to this.
#[derive(Debug, Clone)]
pub struct ElevationTile {
    /// Southwest corner latitude
    pub sw_lat: f64,
    /// Southwest corner longitude  
    pub sw_lon: f64,
    /// Northeast corner latitude
    pub ne_lat: f64,
    /// Northeast corner longitude
    pub ne_lon: f64,
    /// Width in samples
    pub width: usize,
    /// Height in samples
    pub height: usize,
    /// Elevation data in row-major order (north to south, west to east)
    /// NaN indicates void/no-data
    pub elevations: Vec<f32>,
    /// Source that provided this data
    pub source: String,
}

impl ElevationTile {
    /// Query elevation at GPS coordinate with bilinear interpolation
    pub fn get_elevation(&self, lat: f64, lon: f64) -> Option<f32> {
        // Check bounds
        if lat < self.sw_lat || lat > self.ne_lat || lon < self.sw_lon || lon > self.ne_lon {
            return None;
        }
        
        // Normalize to [0, 1]
        let u = (lon - self.sw_lon) / (self.ne_lon - self.sw_lon);
        let v = (lat - self.sw_lat) / (self.ne_lat - self.sw_lat);
        
        // Convert to pixel coordinates (v=0 is north, so invert)
        let x = u * (self.width - 1) as f64;
        let y = (1.0 - v) * (self.height - 1) as f64;
        
        // Get integer and fractional parts
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        
        let dx = x - x0 as f64;
        let dy = y - y0 as f64;
        
        // Get 4 corner elevations
        let e00 = self.elevations[y0 * self.width + x0];
        let e10 = self.elevations[y0 * self.width + x1];
        let e01 = self.elevations[y1 * self.width + x0];
        let e11 = self.elevations[y1 * self.width + x1];
        
        // Check for NaN (void data)
        if e00.is_nan() || e10.is_nan() || e01.is_nan() || e11.is_nan() {
            return None;
        }
        
        // Bilinear interpolation
        let top = e00 as f64 * (1.0 - dx) + e10 as f64 * dx;
        let bottom = e01 as f64 * (1.0 - dx) + e11 as f64 * dx;
        let elevation = top * (1.0 - dy) + bottom * dy;
        
        Some(elevation as f32)
    }
}

/// Trait for elevation data providers
pub trait ElevationSource: Send + Sync {
    /// Name of this source
    fn name(&self) -> &str;
    
    /// Fetch elevation tile covering the given GPS coordinate
    ///
    /// Returns None if tile unavailable or fetch fails.
    fn fetch_tile(&self, lat: f64, lon: f64, zoom: u8) -> Result<ElevationTile, Box<dyn Error>>;
    
    /// Check if this source is available (e.g., API key configured)
    fn is_available(&self) -> bool {
        true
    }
}

/// AWS Terrarium elevation tiles
///
/// Format: PNG with elevation encoded as RGB
/// Formula: elevation_meters = (R * 256 + G + B / 256) - 32768
/// URL: https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png
pub struct TerrariumSource {
    client: reqwest::blocking::Client,
}

impl TerrariumSource {
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }
    
    /// Convert GPS coordinate to tile coordinates at given zoom
    fn latlon_to_tile(lat: f64, lon: f64, zoom: u8) -> (u32, u32) {
        // Normalize longitude to [-180, 180)
        let lon_normalized = ((lon + 180.0) % 360.0 + 360.0) % 360.0 - 180.0;
        
        // Clamp latitude to Web Mercator limits (~85.05°)
        let lat_clamped = lat.clamp(-85.0511, 85.0511);
        
        let n = 2_u32.pow(zoom as u32);
        let x = ((lon_normalized + 180.0) / 360.0 * n as f64).floor() as u32;
        let y = ((1.0 - (lat_clamped.to_radians().tan() + 1.0 / lat_clamped.to_radians().cos()).ln() / std::f64::consts::PI) / 2.0 * n as f64).floor() as u32;
        
        // Clamp to valid tile range
        let x_clamped = x.min(n - 1);
        let y_clamped = y.min(n - 1);
        
        (x_clamped, y_clamped)
    }
    
    /// Convert tile coordinates to GPS bounds
    fn tile_to_bounds(x: u32, y: u32, zoom: u8) -> (f64, f64, f64, f64) {
        let n = 2_f64.powi(zoom as i32);
        let lon_min = x as f64 / n * 360.0 - 180.0;
        let lon_max = (x + 1) as f64 / n * 360.0 - 180.0;
        
        let lat_max = ((std::f64::consts::PI * (1.0 - 2.0 * y as f64 / n)).sinh().atan()).to_degrees();
        let lat_min = ((std::f64::consts::PI * (1.0 - 2.0 * (y + 1) as f64 / n)).sinh().atan()).to_degrees();
        
        (lat_min, lon_min, lat_max, lon_max)
    }
    
    /// Decode Terrarium PNG to elevation values
    fn decode_terrarium_png(&self, png_bytes: &[u8]) -> Result<(usize, usize, Vec<f32>), Box<dyn Error>> {
        let decoder = png::Decoder::new(Cursor::new(png_bytes));
        let mut reader = decoder.read_info()?;
        
        let info = reader.info();
        let width = info.width as usize;
        let height = info.height as usize;
        let color_type = info.color_type;
        
        let mut buf = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut buf)?;
        
        // Determine bytes per pixel from color type
        let bytes_per_pixel = match color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            _ => return Err(format!("Unsupported PNG color type: {:?}", color_type).into()),
        };
        
        // Decode RGB to elevation
        // Formula: elevation = (R * 256 + G + B / 256) - 32768
        let mut elevations = Vec::with_capacity(width * height);
        
        for i in 0..width * height {
            let offset = i * bytes_per_pixel;
            if offset + 2 < buf.len() {
                let r = buf[offset] as f32;
                let g = buf[offset + 1] as f32;
                let b = buf[offset + 2] as f32;
                
                let elevation = (r * 256.0 + g + b / 256.0) - 32768.0;
                
                // Terrarium uses elevation = 0 for ocean/void
                // We use NaN for void data
                if elevation.abs() < 0.1 {
                    elevations.push(f32::NAN);
                } else {
                    elevations.push(elevation);
                }
            } else {
                elevations.push(f32::NAN);
            }
        }
        
        Ok((width, height, elevations))
    }
}

impl ElevationSource for TerrariumSource {
    fn name(&self) -> &str {
        "AWS Terrarium"
    }
    
    fn fetch_tile(&self, lat: f64, lon: f64, zoom: u8) -> Result<ElevationTile, Box<dyn Error>> {
        // Web Mercator doesn't cover poles - clamp latitude
        if lat.abs() > 85.0511 {
            return Err(format!("Latitude {} outside Web Mercator range (±85.05°)", lat).into());
        }
        
        let (x, y) = Self::latlon_to_tile(lat, lon, zoom);
        let (sw_lat, sw_lon, ne_lat, ne_lon) = Self::tile_to_bounds(x, y, zoom);
        
        let url = format!(
            "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{}/{}/{}.png",
            zoom, x, y
        );
        
        let response = self.client.get(&url).send()?;
        
        if !response.status().is_success() {
            return Err(format!("HTTP {}: {}", response.status(), url).into());
        }
        
        let png_bytes = response.bytes()?;
        let (width, height, elevations) = self.decode_terrarium_png(&png_bytes)?;
        
        Ok(ElevationTile {
            sw_lat,
            sw_lon,
            ne_lat,
            ne_lon,
            width,
            height,
            elevations,
            source: self.name().to_string(),
        })
    }
}

/// USGS 3DEP ImageServer source
///
/// High-quality elevation data, especially for US but has global coverage.
/// REST API: https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer
pub struct Usgs3DepSource {
    client: reqwest::blocking::Client,
    base_url: String,
}

impl Usgs3DepSource {
    pub fn new() -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            base_url: "https://elevation.nationalmap.gov/arcgis/rest/services/3DEPElevation/ImageServer".to_string(),
        }
    }
}

impl ElevationSource for Usgs3DepSource {
    fn name(&self) -> &str {
        "USGS 3DEP"
    }
    
    fn fetch_tile(&self, lat: f64, lon: f64, zoom: u8) -> Result<ElevationTile, Box<dyn Error>> {
        // Calculate tile bounds (roughly 1 degree tiles for now)
        let tile_size = 1.0 / (2_u32.pow((zoom as u32).min(8)) as f64);
        let sw_lat = (lat / tile_size).floor() * tile_size;
        let sw_lon = (lon / tile_size).floor() * tile_size;
        let ne_lat = sw_lat + tile_size;
        let ne_lon = sw_lon + tile_size;
        
        // Query USGS ImageServer
        // TODO: Implement actual ImageServer query
        // For now, return error to fallback to next source
        Err("USGS 3DEP not yet implemented".into())
    }
}

/// OpenTopography API source
///
/// Requires free API key from https://portal.opentopography.org/
pub struct OpenTopographySource {
    client: reqwest::blocking::Client,
    api_key: Option<String>,
}

impl OpenTopographySource {
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap(),
            api_key,
        }
    }
}

impl ElevationSource for OpenTopographySource {
    fn name(&self) -> &str {
        "OpenTopography"
    }
    
    fn is_available(&self) -> bool {
        self.api_key.is_some()
    }
    
    fn fetch_tile(&self, _lat: f64, _lon: f64, _zoom: u8) -> Result<ElevationTile, Box<dyn Error>> {
        if !self.is_available() {
            return Err("OpenTopography API key not configured".into());
        }
        
        // TODO: Implement OpenTopography API query
        Err("OpenTopography not yet implemented".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_terrarium_tile_coordinates() {
        // Brisbane at zoom 10
        let (x, y) = TerrariumSource::latlon_to_tile(-27.4698, 153.0251, 10);
        assert_eq!(x, 947);
        assert_eq!(y, 593);
    }
    
    #[test]
    fn test_terrarium_tile_antimeridian() {
        // Test longitude 180 normalization
        let (x1, y1) = TerrariumSource::latlon_to_tile(0.0, 180.0, 10);
        let (x2, y2) = TerrariumSource::latlon_to_tile(0.0, -180.0, 10);
        // Should map to same or adjacent tiles
        assert!((x1 as i32 - x2 as i32).abs() <= 1);
        assert_eq!(y1, y2);
    }
    
    #[test]
    fn test_terrarium_tile_poles() {
        // Test pole clamping
        let (x, y) = TerrariumSource::latlon_to_tile(89.0, 0.0, 10);
        let n = 2_u32.pow(10);
        assert!(x < n);
        assert!(y < n);
        
        // Very high latitude should clamp
        let (x2, y2) = TerrariumSource::latlon_to_tile(88.0, 0.0, 10);
        assert!(x2 < n);
        assert!(y2 < n);
    }
    
    #[test]
    fn test_terrarium_bounds() {
        let (sw_lat, sw_lon, ne_lat, ne_lon) = TerrariumSource::tile_to_bounds(947, 593, 10);
        // Brisbane should be within these bounds
        assert!(sw_lat < -27.4698 && -27.4698 < ne_lat);
        assert!(sw_lon < 153.0251 && 153.0251 < ne_lon);
    }
    
    #[test]
    #[ignore] // Network test
    fn test_terrarium_download_brisbane() {
        let source = TerrariumSource::new();
        let tile = source.fetch_tile(-27.4698, 153.0251, 10).unwrap();
        
        assert_eq!(tile.width, 256);
        assert_eq!(tile.height, 256);
        assert_eq!(tile.source, "AWS Terrarium");
        
        // Query elevation at Brisbane CBD (should be ~0-50m)
        let elev = tile.get_elevation(-27.4698, 153.0251);
        assert!(elev.is_some());
        let elev = elev.unwrap();
        assert!(elev >= -10.0 && elev <= 100.0, "Elevation {} seems wrong", elev);
    }
}
