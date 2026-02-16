/// Procedural generation for continuous query system.
///
/// Generates voxel blocks on-demand from SRTM elevation + OSM feature data.
/// Designed for arbitrary query bounds (not constrained to chunk grid).

use crate::coordinates::{GpsPos, EcefPos, ecef_to_gps, gps_to_ecef};
use crate::spatial_index::{VoxelBlock, AABB};
use crate::svo::{MaterialId, AIR, GRASS, DIRT, STONE, GRASS as BEDROCK};
use crate::elevation::{SrtmTile, get_elevation};
use crate::osm::{OsmBuilding, OsmRoad, OsmWater};
use crate::srtm_cache::SrtmCache;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Block size in meters (8m³ = 512 voxels at 1m resolution)
pub const BLOCK_SIZE_M: f64 = 8.0;

/// Voxel resolution in meters
pub const VOXEL_SIZE_M: f64 = 1.0;

/// Voxels per block dimension (8m / 1m = 8 voxels)
pub const VOXELS_PER_BLOCK: usize = 8;

/// Generator configuration
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Cache directory for SRTM tiles
    pub srtm_cache_path: PathBuf,
    /// Cache directory for OSM features
    pub osm_cache_path: PathBuf,
    /// Test area center (ECEF)
    pub area_center: EcefPos,
    /// Test area radius (meters)
    pub area_radius: f64,
}

/// Procedural generator for voxel blocks
pub struct ProceduralGenerator {
    config: GeneratorConfig,
    /// SRTM tile cache
    srtm_cache: SrtmCache,
    /// Cached SRTM tiles (keyed by lat/lon)
    srtm_tiles: Arc<Mutex<HashMap<(i16, i16), Arc<SrtmTile>>>>,
    /// Cached OSM buildings
    osm_buildings: Arc<Mutex<Vec<OsmBuilding>>>,
    /// Cached OSM roads
    osm_roads: Arc<Mutex<Vec<OsmRoad>>>,
    /// Cached OSM water features
    osm_water: Arc<Mutex<Vec<OsmWater>>>,
}

impl ProceduralGenerator {
    /// Create a new procedural generator
    pub fn new(config: GeneratorConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let srtm_cache = SrtmCache::new(config.srtm_cache_path.clone())?;
        
        Ok(Self {
            config,
            srtm_cache,
            srtm_tiles: Arc::new(Mutex::new(HashMap::new())),
            osm_buildings: Arc::new(Mutex::new(Vec::new())),
            osm_roads: Arc::new(Mutex::new(Vec::new())),
            osm_water: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Generate a voxel block for the given ECEF bounds
    pub fn generate_block(&self, ecef_min: [f64; 3]) -> VoxelBlock {
        // Initialize with AIR
        let mut voxels = Box::new([AIR; 512]);

        // Generate terrain from SRTM
        self.fill_terrain(&mut voxels, ecef_min);

        // Add OSM features
        self.fill_buildings(&mut voxels, ecef_min);
        self.fill_roads(&mut voxels, ecef_min);
        self.fill_water(&mut voxels, ecef_min);

        VoxelBlock {
            ecef_min,
            size: BLOCK_SIZE_M,
            voxels,
        }
    }

    /// Fill terrain voxels from SRTM elevation data
    fn fill_terrain(&self, voxels: &mut [MaterialId; 512], ecef_min: [f64; 3]) {
        // For each voxel in the block, check if it's below ground
        for x in 0..VOXELS_PER_BLOCK {
            for y in 0..VOXELS_PER_BLOCK {
                for z in 0..VOXELS_PER_BLOCK {
                    let voxel_idx = Self::voxel_index(x, y, z);
                    
                    // Calculate voxel center in ECEF
                    let voxel_ecef = EcefPos {
                        x: ecef_min[0] + (x as f64 + 0.5) * VOXEL_SIZE_M,
                        y: ecef_min[1] + (y as f64 + 0.5) * VOXEL_SIZE_M,
                        z: ecef_min[2] + (z as f64 + 0.5) * VOXEL_SIZE_M,
                    };

                    // Convert to GPS to query elevation
                    let gps = ecef_to_gps(&voxel_ecef);
                    
                    // Get ground elevation at this position
                    if let Some(ground_elevation) = self.get_ground_elevation(gps) {
                        // Get altitude of this voxel (height above ellipsoid)
                        let voxel_altitude = gps.elevation_m;
                        
                        // If voxel is below ground, fill with appropriate material
                        if voxel_altitude < ground_elevation {
                            // Depth below surface
                            let depth = ground_elevation - voxel_altitude;
                            
                            // Material selection based on depth
                            voxels[voxel_idx] = if depth < 0.5 {
                                GRASS
                            } else if depth < 2.0 {
                                DIRT
                            } else if depth < 10.0 {
                                STONE
                            } else {
                                BEDROCK
                            };
                        }
                        // else: voxel is above ground, leave as AIR
                    }
                    // If no elevation data, leave as AIR
                }
            }
        }
    }

    /// Fill building voxels from OSM building data
    fn fill_buildings(&self, voxels: &mut [MaterialId; 512], ecef_min: [f64; 3]) {
        let buildings = self.osm_buildings.lock().unwrap();
        
        for building in buildings.iter() {
            // Check if building intersects this block
            if self.building_intersects_block(building, ecef_min) {
                self.voxelize_building(voxels, building, ecef_min);
            }
        }
    }

    /// Fill road voxels from OSM road data
    fn fill_roads(&self, voxels: &mut [MaterialId; 512], ecef_min: [f64; 3]) {
        let roads = self.osm_roads.lock().unwrap();
        
        for road in roads.iter() {
            // Check if road intersects this block
            if self.road_intersects_block(road, ecef_min) {
                self.voxelize_road(voxels, road, ecef_min);
            }
        }
    }

    /// Fill water voxels from OSM water data
    fn fill_water(&self, voxels: &mut [MaterialId; 512], ecef_min: [f64; 3]) {
        let water_features = self.osm_water.lock().unwrap();
        
        for water in water_features.iter() {
            // Check if water intersects this block
            if self.water_intersects_block(water, ecef_min) {
                self.voxelize_water(voxels, water, ecef_min);
            }
        }
    }

    /// Get ground elevation at GPS position from SRTM data
    fn get_ground_elevation(&self, gps: GpsPos) -> Option<f64> {
        let lat = gps.lat_deg;
        let lon = gps.lon_deg;
        
        // Determine which SRTM tile we need (tiles are 1° × 1°)
        let tile_lat = lat.floor() as i16;
        let tile_lon = lon.floor() as i16;
        
        // Try to get cached tile
        let tiles = self.srtm_tiles.lock().unwrap();
        if let Some(tile) = tiles.get(&(tile_lat, tile_lon)) {
            // Query elevation from tile with bilinear interpolation
            return get_elevation(tile, lat, lon);
        }
        
        // TODO: Load tile from disk if not cached
        // For now, return None (will be implemented in p2-srtm-cache)
        None
    }

    /// Check if building intersects block bounds
    fn building_intersects_block(&self, building: &OsmBuilding, ecef_min: [f64; 3]) -> bool {
        // TODO: Implement proper polygon-AABB intersection test
        // For now, simple AABB test
        let block_max = [
            ecef_min[0] + BLOCK_SIZE_M,
            ecef_min[1] + BLOCK_SIZE_M,
            ecef_min[2] + BLOCK_SIZE_M,
        ];
        
        // Convert building polygon to ECEF and check bounds
        for point in &building.polygon {
            let ecef = gps_to_ecef(point);
            if ecef.x >= ecef_min[0] && ecef.x <= block_max[0] &&
               ecef.y >= ecef_min[1] && ecef.y <= block_max[1] &&
               ecef.z >= ecef_min[2] && ecef.z <= block_max[2] {
                return true;
            }
        }
        
        false
    }

    /// Check if road intersects block bounds
    fn road_intersects_block(&self, road: &OsmRoad, ecef_min: [f64; 3]) -> bool {
        // TODO: Implement proper line-AABB intersection test
        // For now, simple point-in-AABB test
        let block_max = [
            ecef_min[0] + BLOCK_SIZE_M,
            ecef_min[1] + BLOCK_SIZE_M,
            ecef_min[2] + BLOCK_SIZE_M,
        ];
        
        for point in &road.nodes {
            let ecef = gps_to_ecef(point);
            if ecef.x >= ecef_min[0] && ecef.x <= block_max[0] &&
               ecef.y >= ecef_min[1] && ecef.y <= block_max[1] &&
               ecef.z >= ecef_min[2] && ecef.z <= block_max[2] {
                return true;
            }
        }
        
        false
    }

    /// Check if water feature intersects block bounds
    fn water_intersects_block(&self, water: &OsmWater, ecef_min: [f64; 3]) -> bool {
        // TODO: Implement proper polygon-AABB intersection test
        // For now, simple point-in-AABB test
        let block_max = [
            ecef_min[0] + BLOCK_SIZE_M,
            ecef_min[1] + BLOCK_SIZE_M,
            ecef_min[2] + BLOCK_SIZE_M,
        ];
        
        for point in &water.polygon {
            let ecef = gps_to_ecef(point);
            if ecef.x >= ecef_min[0] && ecef.x <= block_max[0] &&
               ecef.y >= ecef_min[1] && ecef.y <= block_max[1] &&
               ecef.z >= ecef_min[2] && ecef.z <= block_max[2] {
                return true;
            }
        }
        
        false
    }

    /// Voxelize building into block
    fn voxelize_building(&self, _voxels: &mut [MaterialId; 512], _building: &OsmBuilding, _ecef_min: [f64; 3]) {
        // TODO: Implement proper building voxelization
        // For Phase 2, this is a placeholder
        // Will be fully implemented in Phase 4 (Interior Spaces)
    }

    /// Voxelize road into block
    fn voxelize_road(&self, _voxels: &mut [MaterialId; 512], _road: &OsmRoad, _ecef_min: [f64; 3]) {
        // TODO: Implement proper road voxelization
        // For Phase 2, this is a placeholder
        // Will add road surface material in p2-voxelization
    }

    /// Voxelize water feature into block
    fn voxelize_water(&self, _voxels: &mut [MaterialId; 512], _water: &OsmWater, _ecef_min: [f64; 3]) {
        // TODO: Implement proper water voxelization
        // For Phase 2, this is a placeholder
        // Will add water material in p2-voxelization
    }

    /// Convert 3D voxel coordinates to linear array index
    fn voxel_index(x: usize, y: usize, z: usize) -> usize {
        x + y * VOXELS_PER_BLOCK + z * VOXELS_PER_BLOCK * VOXELS_PER_BLOCK
    }

    /// Load SRTM tiles covering the test area
    pub fn load_srtm_tiles(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Convert test area center to GPS
        let center_gps = ecef_to_gps(&self.config.area_center);
        let lat = center_gps.lat_deg;
        let lon = center_gps.lon_deg;
        
        // Calculate approximate degrees covered by area radius
        // At this latitude, roughly 111km per degree
        let degree_radius = (self.config.area_radius / 111_000.0).ceil();
        
        let min_lat = (lat - degree_radius).floor() as i16;
        let max_lat = (lat + degree_radius).ceil() as i16;
        let min_lon = (lon - degree_radius).floor() as i16;
        let max_lon = (lon + degree_radius).ceil() as i16;
        
        // Load all tiles in range
        let mut tiles = self.srtm_tiles.lock().unwrap();
        
        for tile_lat in min_lat..=max_lat {
            for tile_lon in min_lon..=max_lon {
                // Check if tile already loaded
                if tiles.contains_key(&(tile_lat, tile_lon)) {
                    continue;
                }
                
                // Try to load from cache
                if let Some(tile) = self.load_srtm_tile_from_disk(tile_lat, tile_lon)? {
                    tiles.insert((tile_lat, tile_lon), Arc::new(tile));
                    println!("Loaded SRTM tile: ({}, {})", tile_lat, tile_lon);
                } else {
                    println!("SRTM tile not found: ({}, {}) - will use empty terrain", tile_lat, tile_lon);
                }
            }
        }
        
        println!("Loaded {} SRTM tiles", tiles.len());
        Ok(())
    }

    /// Load SRTM tile from disk cache
    fn load_srtm_tile_from_disk(&self, lat: i16, lon: i16) -> Result<Option<SrtmTile>, Box<dyn std::error::Error>> {
        match self.srtm_cache.get_tile(lat, lon) {
            Ok(tile) => Ok(Some(tile)),
            Err(e) => {
                println!("Failed to load SRTM tile ({}, {}): {}", lat, lon, e);
                Ok(None)
            }
        }
    }

    /// Load OSM features for test area
    pub fn load_osm_features(&self) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Implement in p2-osm-cache
        // For now, keep empty vectors
        println!("OSM features loading not yet implemented");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_config() -> GeneratorConfig {
        let cache_dir = env::temp_dir().join("metaverse_test_cache");
        GeneratorConfig {
            srtm_cache_path: cache_dir.join("srtm"),
            osm_cache_path: cache_dir.join("osm"),
            // Kangaroo Point, Brisbane test area
            area_center: EcefPos {
                x: -5_047_081.96,
                y: 2_567_891.19,
                z: -2_925_600.68,
            },
            area_radius: 100.0,
        }
    }

    #[test]
    fn test_generator_creation() {
        let config = test_config();
        let generator = ProceduralGenerator::new(config).unwrap();
        
        // Generator should initialize with empty caches
        assert_eq!(generator.srtm_tiles.lock().unwrap().len(), 0);
        assert_eq!(generator.osm_buildings.lock().unwrap().len(), 0);
        assert_eq!(generator.osm_roads.lock().unwrap().len(), 0);
        assert_eq!(generator.osm_water.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_generate_block_no_data() {
        let config = test_config();
        let generator = ProceduralGenerator::new(config).unwrap();
        
        // Generate block at test area center
        let ecef_min = [-5_047_100.0, 2_567_900.0, -2_925_600.0];
        let block = generator.generate_block(ecef_min);
        
        // Should produce a block with all AIR (no data loaded)
        assert_eq!(block.ecef_min, ecef_min);
        assert_eq!(block.size, BLOCK_SIZE_M);
        
        // Count non-AIR voxels (should be 0 with no data)
        let non_air_count = block.voxels.iter()
            .filter(|&&m| m != AIR)
            .count();
        
        assert_eq!(non_air_count, 0, "Expected all AIR voxels with no data loaded");
    }

    #[test]
    fn test_voxel_index() {
        // Test corner voxels
        assert_eq!(ProceduralGenerator::voxel_index(0, 0, 0), 0);
        assert_eq!(ProceduralGenerator::voxel_index(7, 0, 0), 7);
        assert_eq!(ProceduralGenerator::voxel_index(0, 7, 0), 7 * 8);
        assert_eq!(ProceduralGenerator::voxel_index(0, 0, 7), 7 * 8 * 8);
        assert_eq!(ProceduralGenerator::voxel_index(7, 7, 7), 511);
    }

    #[test]
    fn test_block_size_constants() {
        // Verify constants are consistent
        assert_eq!(VOXELS_PER_BLOCK, 8);
        assert_eq!(BLOCK_SIZE_M / VOXEL_SIZE_M, VOXELS_PER_BLOCK as f64);
        
        // Total voxels per block
        let total_voxels = VOXELS_PER_BLOCK * VOXELS_PER_BLOCK * VOXELS_PER_BLOCK;
        assert_eq!(total_voxels, 512);
    }

    #[test]
    fn test_load_srtm_tiles_empty() {
        let config = test_config();
        let generator = ProceduralGenerator::new(config).unwrap();
        
        // Should succeed even with no tiles on disk
        let result = generator.load_srtm_tiles();
        assert!(result.is_ok());
    }

    #[test]
    fn test_load_osm_features_empty() {
        let config = test_config();
        let generator = ProceduralGenerator::new(config).unwrap();
        
        // Should succeed even with no features on disk
        let result = generator.load_osm_features();
        assert!(result.is_ok());
    }
}
