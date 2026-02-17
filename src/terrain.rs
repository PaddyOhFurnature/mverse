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
}
