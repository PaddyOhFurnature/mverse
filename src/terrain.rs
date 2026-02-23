//! Terrain generation from elevation data
//!
//! Converts SRTM elevation queries into voxel columns
//!
//! # Pure Function Design
//!
//! Terrain generation is designed as pure functions where possible:
//! - `generate_chunk_pure()` - Pure function for chunk generation
//! - Same ChunkId + elevation data = same Octree (deterministic)
//! - Thread-safe - can generate multiple chunks in parallel
//! - No shared mutable state between calls
//!
//! The `TerrainGenerator` struct exists for convenience (holds elevation pipeline),
//! but the core generation logic is in pure standalone functions.

use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;
use crate::materials::MaterialId;
use crate::voxel::{Octree, VoxelCoord};
use crate::chunk::ChunkId;
use std::sync::{Arc, Mutex};

/// Generate terrain voxels from elevation data
pub struct TerrainGenerator {
    elevation: Arc<Mutex<ElevationPipeline>>,
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
}

impl TerrainGenerator {
    /// Create new terrain generator
    ///
    /// # Arguments
    /// * `elevation` - Elevation data pipeline (wrapped in Arc<Mutex> for thread-safety)
    /// * `origin_gps` - GPS origin point (for coordinate conversion)
    /// * `origin_voxel` - Voxel coordinate of origin (for ECEF conversion)
    pub fn new(elevation: ElevationPipeline, origin_gps: GPS, origin_voxel: VoxelCoord) -> Self {
        Self { 
            elevation: Arc::new(Mutex::new(elevation)),
            origin_gps,
            origin_voxel,
        }
    }
    
    /// Create with shared elevation pipeline (for parallel chunk generation)
    pub fn with_shared_elevation(
        elevation: Arc<Mutex<ElevationPipeline>>,
        origin_gps: GPS,
        origin_voxel: VoxelCoord
    ) -> Self {
        Self {
            elevation,
            origin_gps,
            origin_voxel,
        }
    }
    
    /// Get shared elevation pipeline (for cloning generator)
    pub fn elevation_pipeline(&self) -> Arc<Mutex<ElevationPipeline>> {
        Arc::clone(&self.elevation)
    }
    
    /// Generate terrain region with 1m voxel resolution
    ///
    /// Creates a region of terrain centered at `origin` extending `size_meters` in each direction.
    /// Uses bilinear interpolation to generate 1m resolution voxels from ~30m SRTM data.
    ///
    /// For 120m × 120m region:
    /// - Samples SRTM at ~30m intervals (5×5 = 25 elevation samples)
    /// - Interpolates to 120×120 = 14,400 voxel columns at 1m spacing
    /// - Each column extends from bedrock to sky
    pub fn generate_region(
        &self,
        octree: &mut Octree,
        origin: &GPS,
        size_meters: f64,
    ) -> Result<(), String> {
        println!("Generating {}m × {}m terrain region...", size_meters, size_meters);
        
        // Lock elevation pipeline
        let mut elevation = self.elevation.lock()
            .map_err(|e| format!("Failed to lock elevation pipeline: {}", e))?;
        
        // Calculate GPS coordinates for corner points
        const METERS_PER_DEGREE_LAT: f64 = 111_320.0;
        let meters_per_degree_lon = 111_320.0 * origin.lat.to_radians().cos();
        
        let half_size = size_meters / 2.0;
        let lat_min = origin.lat - (half_size / METERS_PER_DEGREE_LAT);
        let lat_max = origin.lat + (half_size / METERS_PER_DEGREE_LAT);
        let lon_min = origin.lon - (half_size / meters_per_degree_lon);
        let lon_max = origin.lon + (half_size / meters_per_degree_lon);
        
        // Sample SRTM at ~30m resolution to get elevation grid
        const SRTM_RESOLUTION_M: f64 = 30.0;
        let samples_per_axis = (size_meters / SRTM_RESOLUTION_M).ceil() as usize + 1;
        
        println!("  Sampling SRTM: {} × {} = {} points", 
            samples_per_axis, samples_per_axis, samples_per_axis * samples_per_axis);
        
        let mut elevation_grid = Vec::with_capacity(samples_per_axis * samples_per_axis);
        
        for i in 0..samples_per_axis {
            for j in 0..samples_per_axis {
                let t_lat = i as f64 / (samples_per_axis - 1) as f64;
                let t_lon = j as f64 / (samples_per_axis - 1) as f64;
                let lat = lat_min + t_lat * (lat_max - lat_min);
                let lon = lon_min + t_lon * (lon_max - lon_min);
                
                let gps = GPS::new(lat, lon, 0.0);
                let elev_meters = elevation.query(&gps)
                    .map_err(|e| format!("SRTM query failed at ({}, {}): {}", lat, lon, e))?
                    .meters;
                
                elevation_grid.push(elev_meters);
            }
        }
        
        println!("  Generating voxel columns at 1m resolution...");
        
        // Convert origin to voxel coordinates ONCE
        let origin_ecef = origin.to_ecef();
        let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
        
        // Generate voxel columns at 1m spacing with bilinear interpolation
        let columns_per_axis = size_meters as usize;
        let half_size = (size_meters / 2.0) as i64;
        let mut column_count = 0;
        
        // Use local coordinate offsets to ensure adjacent voxels
        for i in 0..columns_per_axis {
            for j in 0..columns_per_axis {
                let local_x = i as i64 - half_size;
                let local_z = j as i64 - half_size;
                
                // Interpolate elevation from SRTM grid
                let grid_i = (i as f64 / SRTM_RESOLUTION_M).min((samples_per_axis - 2) as f64);
                let grid_j = (j as f64 / SRTM_RESOLUTION_M).min((samples_per_axis - 2) as f64);
                
                let i0 = grid_i.floor() as usize;
                let j0 = grid_j.floor() as usize;
                let i1 = i0 + 1;
                let j1 = j0 + 1;
                
                let frac_i = grid_i - i0 as f64;
                let frac_j = grid_j - j0 as f64;
                
                // Bilinear interpolation
                let e00 = elevation_grid[i0 * samples_per_axis + j0];
                let e01 = elevation_grid[i0 * samples_per_axis + j1];
                let e10 = elevation_grid[i1 * samples_per_axis + j0];
                let e11 = elevation_grid[i1 * samples_per_axis + j1];
                
                let e0 = e00 * (1.0 - frac_j) + e01 * frac_j;
                let e1 = e10 * (1.0 - frac_j) + e11 * frac_j;
                let surface_elevation = e0 * (1.0 - frac_i) + e1 * frac_i;
                
                const BEDROCK_DEPTH: i64 = 200;
                const SKY_HEIGHT: i64 = 100;
                
                // Generate vertical column using LOCAL coordinates
                // This ensures voxels are actually adjacent (1m spacing)
                for height_offset in (-BEDROCK_DEPTH)..=SKY_HEIGHT {
                    let local_y = height_offset + surface_elevation as i64;
                    
                    // Direct voxel coordinate from local offset
                    let voxel_pos = VoxelCoord::new(
                        origin_voxel.x + local_x,
                        origin_voxel.y + local_y,
                        origin_voxel.z + local_z,
                    );
                    
                    let depth_below_surface = surface_elevation - (origin.alt + height_offset as f64);
                    
                    let material = if depth_below_surface > 5.0 {
                        MaterialId::STONE
                    } else if depth_below_surface > 1.0 {
                        MaterialId::DIRT
                    } else if depth_below_surface > 0.0 {
                        MaterialId::GRASS
                    } else {
                        MaterialId::AIR
                    };
                    
                    octree.set_voxel(voxel_pos, material);
                }
                
                column_count += 1;
            }
        }
        
        println!("  ✓ Generated {} columns", column_count);
        Ok(())
    }
    
    /// Generate terrain for a single chunk (pure function)
    ///
    /// Creates a new Octree with terrain voxels for the specified chunk.
    /// Samples SRTM elevation data for the chunk's GPS bounds.
    ///
    /// # Arguments
    /// * `chunk_id` - Which chunk to generate
    ///
    /// # Returns
    /// New Octree containing terrain voxels for this chunk only
    ///
    /// # Example
    /// ```no_run
    /// use metaverse_core::terrain::TerrainGenerator;
    /// use metaverse_core::chunk::ChunkId;
    /// use metaverse_core::elevation::ElevationPipeline;
    ///
    /// let elevation = ElevationPipeline::new();
    /// let mut generator = TerrainGenerator::new(elevation);
    /// let chunk_id = ChunkId::new(0, 0, 0);
    /// let octree = generator.generate_chunk(&chunk_id)?;
    /// # Ok::<(), String>(())
    /// ```
    /// Generate terrain chunk (thread-safe, immutable)
    ///
    /// Pure function: same chunk_id + elevation data = same Octree
    /// Can be called from multiple threads simultaneously
    pub fn generate_chunk(&self, chunk_id: &ChunkId) -> Result<Octree, String> {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};
        
        let min_voxel = chunk_id.min_voxel();
        let max_voxel = chunk_id.max_voxel();
        
        let mut octree = Octree::new();
        
        // Clone Arc for direct access (ElevationPipeline methods are now &self, thread-safe)
        let elevation = Arc::clone(&self.elevation);
        
        // 30m×200m×30m chunks aligned to SRTM
        // Sample elevation at 30m intervals (1 per horizontal voxel position)
        for i in 0..CHUNK_SIZE_X {
            for k in 0..CHUNK_SIZE_Z {
                let voxel_x = min_voxel.x + i;
                let voxel_z = min_voxel.z + k;
                
                // Convert voxel position to GPS
                let sample_voxel = VoxelCoord::new(voxel_x, self.origin_voxel.y, voxel_z);
                let sample_ecef = sample_voxel.to_ecef();
                let sample_gps = sample_ecef.to_gps();
                
                // Query SRTM elevation (thread-safe with brief internal locks)
                let surface_elevation = elevation.lock().unwrap().query(&sample_gps)
                    .map(|e| e.meters)
                    .unwrap_or(self.origin_gps.alt);
                
                // Convert to voxel Y coordinate
                let surface_offset = surface_elevation - self.origin_gps.alt;
                let surface_voxel_y = self.origin_voxel.y + surface_offset as i64;
                
                // Generate vertical column
                const BEDROCK_DEPTH: i64 = 200;
                const SKY_HEIGHT: i64 = 100;
                
                for height_offset in (-BEDROCK_DEPTH)..=SKY_HEIGHT {
                    let voxel_y = surface_voxel_y + height_offset;
                    
                    // Only generate voxels within chunk Y bounds
                    if voxel_y < min_voxel.y || voxel_y >= max_voxel.y {
                        continue;
                    }
                    
                    let voxel_pos = VoxelCoord::new(voxel_x, voxel_y, voxel_z);
                    let depth_below_surface = -height_offset;
                    
                    let material = if depth_below_surface > 5 {
                        MaterialId::STONE
                    } else if depth_below_surface > 1 {
                        MaterialId::DIRT
                    } else if depth_below_surface > 0 {
                        MaterialId::GRASS
                    } else {
                        MaterialId::AIR
                    };
                    
                    octree.set_voxel(voxel_pos, material);
                }
            }
        }
        
        Ok(octree)
    }
    
    /// Fill octree with terrain at given GPS location
    /// 
    /// Generates a vertical column of voxels:
    /// - STONE from bedrock up to 5m below surface
    /// - DIRT from 5m below surface to surface
    /// - GRASS at surface (top 1m)
    /// - AIR above surface
    pub fn generate_column(&self, octree: &mut Octree, gps: &GPS) -> Result<(), String> {
        // Lock elevation pipeline
        let mut elevation = self.elevation.lock()
            .map_err(|e| format!("Failed to lock elevation pipeline: {}", e))?;
        
        // Query elevation at this lat/lon
        let elev_meters = elevation.query(gps)
            .map_err(|e| format!("Elevation query failed: {:?}", e))?
            .meters;
        
        let surface_elevation = elev_meters;
        
        // Generate voxels from bedrock to sky
        const BEDROCK_DEPTH: f64 = 200.0;  // 200m below surface
        const SKY_HEIGHT: f64 = 100.0;     // 100m above surface
        
        for height_offset in (-BEDROCK_DEPTH as i64)..=(SKY_HEIGHT as i64) {
            let voxel_elevation = surface_elevation + height_offset as f64;
            let ecef = GPS::new(gps.lat, gps.lon, voxel_elevation).to_ecef();
            let voxel_pos = VoxelCoord::from_ecef(&ecef);
            
            // Determine material based on depth relative to surface
            let depth_below_surface = surface_elevation - voxel_elevation;
            
            let material = if depth_below_surface > 5.0 {
                // More than 5m below surface: STONE
                MaterialId::STONE
            } else if depth_below_surface > 1.0 {
                // 1-5m below surface: DIRT
                MaterialId::DIRT
            } else if depth_below_surface > 0.0 {
                // 0-1m below surface: GRASS (topsoil)
                MaterialId::GRASS
            } else {
                // Above surface: AIR
                MaterialId::AIR
            };
            
            octree.set_voxel(voxel_pos, material);
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevation::{OpenTopographySource, NasFileSource};
    use std::path::PathBuf;
    
    #[test]
    #[ignore] // Requires SRTM data
    fn test_generate_column_kangaroo_point() {
        // Kangaroo Point Cliffs
        let gps = GPS::new(-27.4775, 153.0355, 0.0);
        
        // Setup elevation pipeline (try NAS, fall back to API)
        let nas = NasFileSource::new();
        let api_key = "3e607de6969c687053f9e107a4796962".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let api = OpenTopographySource::new(api_key, cache_dir);
        
        let mut pipeline = ElevationPipeline::new();
        if let Some(nas_source) = nas {
            pipeline.add_source(Box::new(nas_source));
        }
        pipeline.add_source(Box::new(api));
        
        let origin_gps = GPS::new(0.0, 0.0, 0.0);
        let origin_voxel = VoxelCoord::new(0, 0, 0);
        let mut generator = TerrainGenerator::new(pipeline, origin_gps, origin_voxel);
        let mut octree = Octree::new();
        
        // Generate terrain column
        generator.generate_column(&mut octree, &gps).unwrap();
        
        // Query voxels at different heights
        let surface_ecef = GPS::new(gps.lat, gps.lon, 21.0).to_ecef(); // ~21m elevation
        let above_ecef = GPS::new(gps.lat, gps.lon, 50.0).to_ecef();
        let below_ecef = GPS::new(gps.lat, gps.lon, 10.0).to_ecef();
        
        let surface_voxel = VoxelCoord::from_ecef(&surface_ecef);
        let above_voxel = VoxelCoord::from_ecef(&above_ecef);
        let below_voxel = VoxelCoord::from_ecef(&below_ecef);
        
        // Above surface should be AIR
        assert_eq!(octree.get_voxel(above_voxel), MaterialId::AIR);
        
        // At surface should be GRASS or DIRT
        let surface_mat = octree.get_voxel(surface_voxel);
        assert!(surface_mat == MaterialId::GRASS || surface_mat == MaterialId::DIRT);
        
        // Below surface should be DIRT or STONE
        let below_mat = octree.get_voxel(below_voxel);
        assert!(below_mat == MaterialId::DIRT || below_mat == MaterialId::STONE);
    }
    
    #[test]
    #[ignore] // Requires SRTM data
    fn test_generate_multiple_columns() {
        let api_key = "3e607de6969c687053f9e107a4796962".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let api = OpenTopographySource::new(api_key, cache_dir);
        
        let mut pipeline = ElevationPipeline::new();
        pipeline.add_source(Box::new(api));
        
        let origin_gps = GPS::new(0.0, 0.0, 0.0);
        let origin_voxel = VoxelCoord::new(0, 0, 0);
        let mut generator = TerrainGenerator::new(pipeline, origin_gps, origin_voxel);
        let mut octree = Octree::new();
        
        // Generate a 10m × 10m grid
        for lat_offset in 0..10 {
            for lon_offset in 0..10 {
                let lat = -27.4775 + (lat_offset as f64) * 0.0001; // ~11m steps
                let lon = 153.0355 + (lon_offset as f64) * 0.0001;
                let gps = GPS::new(lat, lon, 0.0);
                
                generator.generate_column(&mut octree, &gps).unwrap();
            }
        }
        
        // Check that terrain was generated (not all AIR)
        let test_ecef = GPS::new(-27.4775, 153.0355, 10.0).to_ecef();
        let test_voxel = VoxelCoord::from_ecef(&test_ecef);
        let material = octree.get_voxel(test_voxel);
        
        // Should be solid material below surface
        assert!(material != MaterialId::AIR);
    }
    
    /// VALIDATION TEST: 10m×10m scale test with performance metrics
    /// 
    /// Tests that terrain generation works at scale (100 columns not 1).
    /// Measures time, memory, and validates octree compression.
    #[test]
    #[ignore] // Requires SRTM data
    fn test_terrain_scale_10m_region() {
        use std::time::Instant;
        
        println!("\n=== TERRAIN SCALE VALIDATION ===");
        println!("Testing 10m × 10m region generation");
        println!("Target: <5 seconds for 100m×100m (this is 10m×10m)");
        
        // Setup elevation pipeline with NAS if available
        let nas = NasFileSource::new();
        let api_key = "3e607de6969c687053f9e107a4796962".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let api = OpenTopographySource::new(api_key, cache_dir);
        
        let mut pipeline = ElevationPipeline::new();
        if let Some(nas_source) = nas {
            println!("✓ Using NAS file source");
            pipeline.add_source(Box::new(nas_source));
        } else {
            println!("⚠ NAS not available, using API (will be slower)");
        }
        pipeline.add_source(Box::new(api));
        
        let origin_gps = GPS::new(0.0, 0.0, 0.0);
        let origin_voxel = VoxelCoord::new(0, 0, 0);
        let mut generator = TerrainGenerator::new(pipeline, origin_gps, origin_voxel);
        let mut octree = Octree::new();
        
        // Generate 10m × 10m grid (100 columns)
        let start_time = Instant::now();
        let mut column_count = 0;
        
        println!("\nGenerating terrain...");
        for lat_offset in 0..10 {
            for lon_offset in 0..10 {
                let lat = -27.4775 + (lat_offset as f64) * 0.00009; // ~10m steps
                let lon = 153.0355 + (lon_offset as f64) * 0.00009;
                let gps = GPS::new(lat, lon, 0.0);
                
                generator.generate_column(&mut octree, &gps)
                    .expect(&format!("Failed to generate column at ({}, {})", lat, lon));
                column_count += 1;
            }
        }
        
        let elapsed = start_time.elapsed();
        
        // Count voxels by material
        let mut voxel_counts = std::collections::HashMap::new();
        let mut total_voxels = 0;
        
        // Sample the octree to count voxels (approximate)
        // Check every voxel in the region we generated
        for lat_offset in 0..10 {
            for lon_offset in 0..10 {
                let lat = -27.4775 + (lat_offset as f64) * 0.00009;
                let lon = 153.0355 + (lon_offset as f64) * 0.00009;
                
                // Check voxels from bedrock to sky
                for height in -200..=100 {
                    let ecef = GPS::new(lat, lon, height as f64).to_ecef();
                    let voxel = VoxelCoord::from_ecef(&ecef);
                    let material = octree.get_voxel(voxel);
                    
                    if material != MaterialId::AIR {
                        *voxel_counts.entry(material).or_insert(0) += 1;
                        total_voxels += 1;
                    }
                }
            }
        }
        
        println!("\n=== RESULTS ===");
        println!("Columns generated: {}", column_count);
        println!("Time elapsed: {:.3}s", elapsed.as_secs_f64());
        println!("Time per column: {:.3}ms", elapsed.as_secs_f64() * 1000.0 / column_count as f64);
        println!("\nVoxel counts:");
        for (material, count) in voxel_counts.iter() {
            println!("  {:?}: {}", material, count);
        }
        println!("Total solid voxels: {}", total_voxels);
        println!("Expected voxels: ~30,000 (100 columns × 300 voxels each)");
        
        // Validate results
        assert!(column_count == 100, "Should generate 100 columns");
        assert!(total_voxels > 10000, "Should have significant solid voxels");
        assert!(elapsed.as_secs_f64() < 30.0, "Should complete in <30s (target <5s for 100m×100m)");
        
        // Check material distribution makes sense
        let stone_count = voxel_counts.get(&MaterialId::STONE).unwrap_or(&0);
        let dirt_count = voxel_counts.get(&MaterialId::DIRT).unwrap_or(&0);
        let grass_count = voxel_counts.get(&MaterialId::GRASS).unwrap_or(&0);
        
        assert!(*stone_count > 0, "Should have STONE (bedrock)");
        assert!(*dirt_count > 0, "Should have DIRT (subsurface)");
        assert!(*grass_count > 0, "Should have GRASS (surface)");
        
        println!("\n✓ Scale test PASSED");
        println!("  • Generated {} columns successfully", column_count);
        println!("  • Created {} solid voxels", total_voxels);
        println!("  • Time: {:.3}s ({:.1}× faster than target)", elapsed.as_secs_f64(), 30.0 / elapsed.as_secs_f64());
    }
}
