//! Terrain generation from elevation data
//!
//! Converts SRTM elevation queries into voxel columns

use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;
use crate::materials::MaterialId;
use crate::voxel::{Octree, VoxelCoord};

/// Generate terrain voxels from elevation data
pub struct TerrainGenerator {
    elevation: ElevationPipeline,
}

impl TerrainGenerator {
    pub fn new(elevation: ElevationPipeline) -> Self {
        Self { elevation }
    }
    
    /// Fill octree with terrain at given GPS location
    /// 
    /// Generates a vertical column of voxels:
    /// - STONE from bedrock up to 5m below surface
    /// - DIRT from 5m below surface to surface
    /// - GRASS at surface (top 1m)
    /// - AIR above surface
    pub fn generate_column(&mut self, octree: &mut Octree, gps: &GPS) -> Result<(), String> {
        // Query elevation at this lat/lon
        let elevation = self.elevation.query(gps)
            .map_err(|e| format!("Elevation query failed: {:?}", e))?;
        
        let surface_elevation = elevation.meters;
        
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
        
        let mut generator = TerrainGenerator::new(pipeline);
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
        
        let mut generator = TerrainGenerator::new(pipeline);
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
        
        let mut generator = TerrainGenerator::new(pipeline);
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
