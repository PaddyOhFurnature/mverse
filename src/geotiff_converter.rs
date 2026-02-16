//! GeoTIFF to .hgt converter for SRTM tiles
//!
//! Converts GeoTIFF elevation data to standard SRTM .hgt format:
//! - 16-bit big-endian signed integers
//! - Row-major order, north-to-south, west-to-east
//! - No header (raw grid data)

use std::io::Cursor;

/// Convert GeoTIFF elevation data to .hgt format
///
/// # Arguments
/// * `geotiff_bytes` - Raw GeoTIFF file data
///
/// # Returns
/// * `Ok(Vec<u8>)` - .hgt format bytes (16-bit big-endian signed integers)
/// * `Err` - If conversion fails
pub fn geotiff_to_hgt(geotiff_bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use tiff::decoder::{Decoder, DecodingResult};
    
    println!("[GeoTIFF] Converting {} bytes to .hgt format...", geotiff_bytes.len());
    
    let cursor = Cursor::new(geotiff_bytes);
    let mut decoder = Decoder::new(cursor)?;
    
    // Read image dimensions
    let (width, height) = decoder.dimensions()?;
    println!("[GeoTIFF] Dimensions: {}x{}", width, height);
    
    // SRTM1 tiles should be 3601×3601
    if width != 3601 || height != 3601 {
        println!("[GeoTIFF] Warning: Expected 3601×3601, got {}×{}", width, height);
    }
    
    // Decode the image data
    let image_data = decoder.read_image()?;
    println!("[GeoTIFF] Decoded image data");
    
    // Convert to elevation values based on data type
    let elevations: Vec<i16> = match image_data {
        DecodingResult::U8(data) => {
            println!("[GeoTIFF] Format: U8 (unusual for elevation)");
            data.iter().map(|&v| v as i16).collect()
        }
        DecodingResult::U16(data) => {
            println!("[GeoTIFF] Format: U16");
            data.iter().map(|&v| {
                if v == 65535 {
                    -32768 // Void value
                } else {
                    v as i16
                }
            }).collect()
        }
        DecodingResult::U32(data) => {
            println!("[GeoTIFF] Format: U32");
            data.iter().map(|&v| {
                if v == u32::MAX {
                    -32768
                } else {
                    (v as i32).clamp(-32768, 32767) as i16
                }
            }).collect()
        }
        DecodingResult::U64(data) => {
            println!("[GeoTIFF] Format: U64");
            data.iter().map(|&v| {
                if v == u64::MAX {
                    -32768
                } else {
                    (v as i64).clamp(-32768, 32767) as i16
                }
            }).collect()
        }
        DecodingResult::I8(data) => {
            println!("[GeoTIFF] Format: I8");
            data.iter().map(|&v| v as i16).collect()
        }
        DecodingResult::I16(data) => {
            println!("[GeoTIFF] Format: I16 (native SRTM format)");
            data.clone()
        }
        DecodingResult::I32(data) => {
            println!("[GeoTIFF] Format: I32");
            data.iter().map(|&v| v.clamp(-32768, 32767) as i16).collect()
        }
        DecodingResult::I64(data) => {
            println!("[GeoTIFF] Format: I64");
            data.iter().map(|&v| v.clamp(-32768, 32767) as i16).collect()
        }
        DecodingResult::F16(data) => {
            println!("[GeoTIFF] Format: F16 (half-precision float)");
            data.iter().map(|v| {
                let f = v.to_f32();
                if f.is_nan() || f.is_infinite() {
                    -32768
                } else {
                    f.round().clamp(-32768.0, 32767.0) as i16
                }
            }).collect()
        }
        DecodingResult::F32(data) => {
            println!("[GeoTIFF] Format: F32 (floating-point elevation)");
            data.iter().map(|&v| {
                if v.is_nan() || v.is_infinite() {
                    -32768 // Void
                } else {
                    v.round().clamp(-32768.0, 32767.0) as i16
                }
            }).collect()
        }
        DecodingResult::F64(data) => {
            println!("[GeoTIFF] Format: F64 (floating-point elevation)");
            data.iter().map(|&v| {
                if v.is_nan() || v.is_infinite() {
                    -32768
                } else {
                    v.round().clamp(-32768.0, 32767.0) as i16
                }
            }).collect()
        }
    };
    
    println!("[GeoTIFF] Converted {} elevation values", elevations.len());
    
    // Validate data
    let non_void = elevations.iter().filter(|&&e| e != -32768).count();
    if non_void == 0 {
        return Err("All elevation values are void".into());
    }
    
    let min_elev = elevations.iter().filter(|&&e| e != -32768).min().copied().unwrap_or(0);
    let max_elev = elevations.iter().filter(|&&e| e != -32768).max().copied().unwrap_or(0);
    println!("[GeoTIFF] Valid elevations: {}/{}", non_void, elevations.len());
    println!("[GeoTIFF] Range: {}m to {}m", min_elev, max_elev);
    
    // Convert to big-endian bytes
    let mut hgt_bytes = Vec::with_capacity(elevations.len() * 2);
    for elev in elevations {
        hgt_bytes.extend_from_slice(&elev.to_be_bytes());
    }
    
    println!("[GeoTIFF] ✓ Conversion complete: {} bytes", hgt_bytes.len());
    
    Ok(hgt_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_conversion() {
        // This would need a real GeoTIFF file to test
        // For now, just ensure the function compiles
    }
}
