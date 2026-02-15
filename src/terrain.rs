//! Terrain generation from elevation data
//!
//! Converts 2D elevation data (SRTM heightmaps) into 3D volumetric SVO representation.
//! The terrain is solid below the surface (rock/soil) and air above.

use crate::svo::{SparseVoxelOctree, MaterialId, STONE, DIRT, AIR, WATER};
use crate::coordinates::GpsPos;

/// Sea level in meters (WGS84 reference)
pub const SEA_LEVEL: f64 = 0.0;

/// Depth of soil layer at surface (meters)
/// Surface voxels use DIRT, deeper voxels use STONE
pub const DIRT_DEPTH: f64 = 2.0;

/// Generate terrain SVO from elevation query function
///
/// This fills an SVO chunk with terrain based on elevation data.
/// For each (x, z) horizontal position:
/// - Query elevation h at that position
/// - Fill y < (h - DIRT_DEPTH) with STONE
/// - Fill (h - DIRT_DEPTH) <= y < h with DIRT  
/// - Fill y >= h with AIR (or WATER if below sea level)
///
/// # Arguments
/// * `svo` - Sparse voxel octree to fill (will be modified)
/// * `elevation_fn` - Function that returns elevation in meters for (lat, lon)
/// * `coords_fn` - Function that converts voxel (x, y, z) to (lat, lon, elevation)
/// * `voxel_size` - Size of each voxel in meters (for determining soil/rock boundary)
///
/// # Example
/// ```ignore
/// let mut svo = SparseVoxelOctree::new(8); // 256^3 voxels
/// generate_terrain_from_elevation(
///     &mut svo,
///     |lat, lon| elevation_downloader.get_elevation(lat, lon, 10).unwrap_or(0.0),
///     |x, y, z| chunk_voxel_to_gps(chunk_id, x, y, z),
///     1.0, // 1m voxels
/// );
/// ```
pub fn generate_terrain_from_elevation<F, G>(
    svo: &mut SparseVoxelOctree,
    mut elevation_fn: F,
    coords_fn: G,
    voxel_size: f64,
) where
    F: FnMut(f64, f64) -> Option<f32>,
    G: Fn(u32, u32, u32) -> GpsPos,
{
    let size = 1u32 << svo.max_depth();
    
    // Iterate over horizontal (x, z) positions
    for x in 0..size {
        for z in 0..size {
            // Get GPS coordinates for this horizontal position (at y=0 reference)
            let gps_ref = coords_fn(x, 0, z);
            let lat = gps_ref.lat_deg;
            let lon = gps_ref.lon_deg;
            
            // Query elevation at this position
            let elevation = elevation_fn(lat, lon)
                .map(|e| e as f64)
                .unwrap_or(SEA_LEVEL);
            
            // DEBUG: Log first few elevations
            static mut COUNT: usize = 0;
            unsafe {
                if COUNT < 5 {
                    println!("[terrain] Position ({}, {}): elevation = {:.1}m", lat, lon, elevation);
                    COUNT += 1;
                }
            }
            
            // Determine material boundaries in voxel coordinates
            // We need to figure out which y-voxels are rock, soil, air, or water
            
            // Sample the vertical column
            for y in 0..size {
                let gps = coords_fn(x, y, z);
                let voxel_elevation = gps.elevation_m;
                
                // DEBUG: Log first column
                static mut LOGGED: bool = false;
                unsafe {
                    if !LOGGED && x == size/2 && z == size/2 && y % 64 == 0 {
                        println!("[terrain] Voxel Y={}: voxel_elev={:.1}m, terrain_elev={:.1}m", 
                                 y, voxel_elevation, elevation);
                        if y == size - 64 { LOGGED = true; }
                    }
                }
                
                // Determine material at this voxel based on elevation
                let material = if voxel_elevation < elevation - DIRT_DEPTH {
                    // Deep underground → rock
                    STONE
                } else if voxel_elevation < elevation {
                    // Near surface → soil
                    static mut DIRT_COUNT: usize = 0;
                    unsafe {
                        DIRT_COUNT += 1;
                        if DIRT_COUNT <= 5 {
                            println!("[terrain] DIRT voxel at Y={} (voxel_elev={:.1}m < terrain={:.1}m)", 
                                     y, voxel_elevation, elevation);
                        }
                    }
                    DIRT
                } else if voxel_elevation < SEA_LEVEL && elevation < SEA_LEVEL {
                    // Below sea level and terrain is underwater → water
                    WATER
                } else {
                    // Above terrain surface → air
                    AIR
                };
                
                // Only set non-AIR voxels (AIR is default Empty state)
                if material != AIR {
                    svo.set_voxel(x, y, z, material);
                }
            }
        }
    }
}

/// Generate flat terrain at specified elevation
///
/// Simpler version for testing or procedural generation.
/// Creates flat terrain with soil on top, rock below.
///
/// # Arguments
/// * `svo` - Sparse voxel octree to fill
/// * `elevation` - Height of terrain surface in meters
/// * `coords_fn` - Function to convert voxel coords to GPS position
pub fn generate_flat_terrain<G>(
    svo: &mut SparseVoxelOctree,
    elevation: f64,
    coords_fn: G,
) where
    G: Fn(u32, u32, u32) -> GpsPos,
{
    let size = 1u32 << svo.max_depth();
    
    for x in 0..size {
        for y in 0..size {
            for z in 0..size {
                let gps = coords_fn(x, y, z);
                let voxel_elevation = gps.elevation_m;
                
                let material = if voxel_elevation < elevation - DIRT_DEPTH {
                    STONE
                } else if voxel_elevation < elevation {
                    DIRT
                } else {
                    AIR
                };
                
                if material != AIR {
                    svo.set_voxel(x, y, z, material);
                }
            }
        }
    }
}

/// Estimate number of voxels needed to represent terrain
///
/// Useful for memory estimation and performance planning.
///
/// # Arguments
/// * `max_depth` - SVO maximum depth (size = 2^max_depth)
/// * `avg_elevation` - Average terrain elevation in chunk
/// * `voxel_size` - Size of each voxel in meters
///
/// # Returns
/// Estimated number of non-empty voxels
pub fn estimate_terrain_voxel_count(
    max_depth: u8,
    avg_elevation: f64,
    voxel_size: f64,
) -> usize {
    let size = 1usize << max_depth;
    let horizontal_voxels = size * size;
    
    // Estimate vertical voxels per column
    let vertical_filled = ((avg_elevation + DIRT_DEPTH) / voxel_size).ceil() as usize;
    let vertical_filled = vertical_filled.min(size);
    
    horizontal_voxels * vertical_filled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinates::{gps_to_ecef, ecef_to_gps, GpsPos};
    
    // Helper: create a simple coordinate mapping for testing
    // Maps voxel (x,y,z) to GPS with elevation based on y
    fn test_coords_fn(x: u32, y: u32, z: u32) -> GpsPos {
        // Simple mapping: each voxel is 1m
        // x,z map to small offsets from Brisbane (-27.47°, 153.03°)
        // y maps directly to elevation
        let lat = -27.47 + (z as f64) * 0.0001; // ~11m per 0.0001 degree
        let lon = 153.03 + (x as f64) * 0.0001;
        let elevation = y as f64; // 1m per voxel vertically
        
        GpsPos {
            lat_deg: lat,
            lon_deg: lon,
            elevation_m: elevation,
        }
    }
    
    #[test]
    fn test_flat_terrain_generation() {
        let mut svo = SparseVoxelOctree::new(5); // 32^3 voxels
        
        // Generate flat terrain at 10m elevation
        generate_flat_terrain(&mut svo, 10.0, test_coords_fn);
        
        // Check voxels below surface are solid
        assert_eq!(svo.get_voxel(0, 5, 0), STONE);  // Below soil depth
        assert_eq!(svo.get_voxel(0, 9, 0), DIRT);  // In soil layer
        
        // Check voxels above surface are air
        assert_eq!(svo.get_voxel(0, 10, 0), AIR);
        assert_eq!(svo.get_voxel(0, 15, 0), AIR);
    }
    
    #[test]
    fn test_terrain_from_elevation_flat() {
        let mut svo = SparseVoxelOctree::new(5); // 32^3 voxels
        
        // Elevation function returns constant 20m
        let elevation_fn = |_lat: f64, _lon: f64| Some(20.0f32);
        
        generate_terrain_from_elevation(&mut svo, elevation_fn, test_coords_fn, 1.0);
        
        // Check materials at different heights
        assert_eq!(svo.get_voxel(0, 10, 0), STONE);  // Well below surface
        assert_eq!(svo.get_voxel(0, 19, 0), DIRT);  // Just below surface
        assert_eq!(svo.get_voxel(0, 20, 0), AIR);   // Above surface
        assert_eq!(svo.get_voxel(0, 25, 0), AIR);
    }
    
    #[test]
    fn test_terrain_from_elevation_varying() {
        let mut svo = SparseVoxelOctree::new(5); // 32^3 voxels
        
        // Elevation varies with longitude (x coordinate)
        let elevation_fn = |_lat: f64, lon: f64| {
            let offset = (lon - 153.03) * 10000.0; // ~1m per voxel
            Some((10.0 + offset) as f32)
        };
        
        generate_terrain_from_elevation(&mut svo, elevation_fn, test_coords_fn, 1.0);
        
        // Check that elevation varies horizontally
        // At x=0: elevation ~10m
        assert_eq!(svo.get_voxel(0, 9, 0), DIRT);
        assert_eq!(svo.get_voxel(0, 10, 0), AIR);
        
        // At x=10: elevation ~20m  
        assert_eq!(svo.get_voxel(10, 19, 0), DIRT);
        assert_eq!(svo.get_voxel(10, 20, 0), AIR);
    }
    
    #[test]
    fn test_estimate_voxel_count() {
        // Depth 5 = 32^3 = 32,768 total voxels
        // With 20m average elevation and 1m voxels:
        // ~22 voxels per column (20 + 2 for soil depth)
        // 32*32 = 1024 columns
        // Expected: ~22,528 voxels
        
        let count = estimate_terrain_voxel_count(5, 20.0, 1.0);
        assert!(count > 20_000);
        assert!(count < 25_000);
    }
    
    #[test]
    fn test_soil_layer_present() {
        let mut svo = SparseVoxelOctree::new(5);
        
        generate_flat_terrain(&mut svo, 10.0, test_coords_fn);
        
        // Check soil layer exists at surface
        assert_eq!(svo.get_voxel(5, 9, 5), DIRT);
        assert_eq!(svo.get_voxel(5, 8, 5), DIRT); // Still in 2m soil layer
        assert_eq!(svo.get_voxel(5, 7, 5), STONE); // Below soil layer
    }
}

/// Smooth terrain transitions at chunk boundaries
///
/// When generating terrain across multiple chunks, we need to ensure
/// adjacent chunks have matching voxels at their boundaries. This prevents
/// cracks or mismatches in the terrain surface.
///
/// For now, this is a placeholder. Smoothing is achieved by ensuring
/// consistent elevation queries across chunk boundaries. The elevation
/// data itself should be continuous with our tile-based system.
///
/// # Arguments
/// * `_svo` - The SVO chunk to smooth (currently unused)
///
/// # Note
/// More advanced smoothing (gradient-based, dual contouring) will be
/// added when we integrate marching cubes mesh extraction.
pub fn smooth_chunk_boundaries(_svo: &mut SparseVoxelOctree) {
    // Current implementation: boundaries match automatically if elevation
    // queries are consistent (which they should be with our tile-based system)
    
    // TODO: Implement gradient-based smoothing
    // TODO: Use dual contouring hints for surface normals
    // TODO: Handle edge cases at poles and antimeridian
}

/// Calculate surface gradient at a voxel position
///
/// Used for smooth terrain transitions and lighting calculations.
/// Returns normalized gradient vector (surface normal direction).
///
/// # Arguments
/// * `svo` - Sparse voxel octree to query
/// * `x, y, z` - Voxel position
///
/// # Returns
/// Gradient vector (dx, dy, dz) or None if position is not on surface
pub fn calculate_surface_gradient(
    svo: &SparseVoxelOctree,
    x: u32,
    y: u32, 
    z: u32,
) -> Option<(f32, f32, f32)> {
    let size = 1u32 << svo.max_depth();
    
    // Check if this voxel is on the surface (solid below, air above)
    let current = svo.get_voxel(x, y, z);
    if current == AIR {
        return None;
    }
    
    if y + 1 < size {
        let above = svo.get_voxel(x, y + 1, z);
        if above != AIR {
            return None; // Not on surface
        }
    }
    
    // Calculate height differences to neighbors
    // For surface normal, we want the gradient of HEIGHT, not density
    let height_here = y as f32;
    
    let height_left = if x > 0 {
        find_surface_height(svo, x - 1, z).unwrap_or(height_here)
    } else {
        height_here
    };
    
    let height_right = if x + 1 < size {
        find_surface_height(svo, x + 1, z).unwrap_or(height_here)
    } else {
        height_here
    };
    
    let height_back = if z > 0 {
        find_surface_height(svo, x, z - 1).unwrap_or(height_here)
    } else {
        height_here
    };
    
    let height_front = if z + 1 < size {
        find_surface_height(svo, x, z + 1).unwrap_or(height_here)
    } else {
        height_here
    };
    
    // Gradient of height field
    let dx = (height_right - height_left) / 2.0;
    let dz = (height_front - height_back) / 2.0;
    
    // Surface normal is perpendicular to gradient
    // Normal = cross product of tangent vectors
    // For heightfield: normal = normalize(-dx, 1, -dz)
    let len = (dx * dx + 1.0 + dz * dz).sqrt();
    Some((-dx / len, 1.0 / len, -dz / len))
}

/// Find the height of the surface at a given (x, z) position
pub fn find_surface_height(svo: &SparseVoxelOctree, x: u32, z: u32) -> Option<f32> {
    let size = 1u32 << svo.max_depth();
    
    // Search from top down for first solid voxel
    for y in (0..size).rev() {
        let mat = svo.get_voxel(x, y, z);
        if mat != AIR {
            return Some(y as f32);
        }
    }
    
    None
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    
    #[test]
    fn test_surface_gradient_flat() {
        let mut svo = SparseVoxelOctree::new(5); // 32^3
        
        // Create flat surface at y=10
        for x in 0..32 {
            for z in 0..32 {
                for y in 0..10 {
                    svo.set_voxel(x, y, z, STONE);
                }
            }
        }
        
        // Gradient at flat surface should point up
        if let Some((dx, dy, dz)) = calculate_surface_gradient(&svo, 15, 9, 15) {
            assert!(dy > 0.5); // Mostly pointing up
            assert!(dx.abs() < 0.1); // Minimal horizontal component
            assert!(dz.abs() < 0.1);
        } else {
            panic!("Expected gradient on surface");
        }
    }
    
    #[test]
    fn test_surface_gradient_slope() {
        let mut svo = SparseVoxelOctree::new(5); // 32^3
        
        // Create sloped surface (rising in +x direction)
        for x in 0..32 {
            for z in 0..32 {
                let height = 5 + x / 4; // Gradual slope
                for y in 0..height {
                    svo.set_voxel(x, y, z, STONE);
                }
            }
        }
        
        // At x=15, height should be 5 + 15/4 = 8
        // So the surface voxel is at y=7 (0-indexed, height-1)
        let test_y = 5 + 15 / 4 - 1; // Surface is at height-1
        
        // For a surface rising in +x direction, the normal tilts BACKWARD (-x)
        // because the normal is perpendicular to the surface
        if let Some((dx, dy, _dz)) = calculate_surface_gradient(&svo, 15, test_y, 15) {
            assert!(dx < 0.0); // Normal tilts backward for upward slope
            assert!(dy > 0.0); // Still pointing somewhat up
        } else {
            panic!("Expected gradient on sloped surface at y={}", test_y);
        }
    }
    
    #[test]
    fn test_no_gradient_underground() {
        let mut svo = SparseVoxelOctree::new(5);
        
        // Fill completely solid
        for x in 0..32 {
            for y in 0..32 {
                for z in 0..32 {
                    svo.set_voxel(x, y, z, STONE);
                }
            }
        }
        
        // No surface gradient in middle of solid block
        assert!(calculate_surface_gradient(&svo, 15, 15, 15).is_none());
    }
}
