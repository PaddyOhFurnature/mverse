/// Procedural generation for continuous query system.
///
/// Generates voxel blocks on-demand from SRTM elevation + OSM feature data.
/// Designed for arbitrary query bounds (not constrained to chunk grid).

use crate::coordinates::{GpsPos, EcefPos, ecef_to_gps, gps_to_ecef};
use crate::spatial_index::{VoxelBlock, AABB};
use crate::svo::{MaterialId, AIR, GRASS, DIRT, STONE, GRASS as BEDROCK, WATER, ASPHALT, CONCRETE};
use crate::elevation::{SrtmTile, get_elevation};
use crate::osm::{OsmBuilding, OsmRoad, OsmWater, OsmData};
use crate::srtm_cache::SrtmCache;
use crate::osm_cache::OsmCache;
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
    /// OSM feature cache
    osm_cache: OsmCache,
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
        let osm_cache = OsmCache::new(config.osm_cache_path.clone())?;
        
        Ok(Self {
            config,
            srtm_cache,
            osm_cache,
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
        let block_max = [
            ecef_min[0] + BLOCK_SIZE_M,
            ecef_min[1] + BLOCK_SIZE_M,
            ecef_min[2] + BLOCK_SIZE_M,
        ];
        
        // Convert all polygon points to ECEF once
        let ecef_points: Vec<_> = building.polygon.iter().map(|p| gps_to_ecef(p)).collect();
        
        // Check if any point is inside block
        for ecef in &ecef_points {
            if ecef.x >= ecef_min[0] && ecef.x <= block_max[0] &&
               ecef.y >= ecef_min[1] && ecef.y <= block_max[1] &&
               ecef.z >= ecef_min[2] && ecef.z <= block_max[2] {
                return true;
            }
        }
        
        // Check if any polygon edge intersects block
        for i in 0..ecef_points.len() {
            let p1 = &ecef_points[i];
            let p2 = &ecef_points[(i + 1) % ecef_points.len()]; // Wrap around for closed polygon
            
            // Bounding box of edge
            let seg_min = [
                p1.x.min(p2.x),
                p1.y.min(p2.y),
                p1.z.min(p2.z),
            ];
            let seg_max = [
                p1.x.max(p2.x),
                p1.y.max(p2.y),
                p1.z.max(p2.z),
            ];
            
            // AABB overlap test
            if seg_max[0] >= ecef_min[0] && seg_min[0] <= block_max[0] &&
               seg_max[1] >= ecef_min[1] && seg_min[1] <= block_max[1] &&
               seg_max[2] >= ecef_min[2] && seg_min[2] <= block_max[2] {
                return true;
            }
        }
        
        // Check if block center is inside building polygon
        let block_center = [
            ecef_min[0] + BLOCK_SIZE_M / 2.0,
            ecef_min[1] + BLOCK_SIZE_M / 2.0,
            ecef_min[2] + BLOCK_SIZE_M / 2.0,
        ];
        
        if Self::point_in_polygon_3d(&block_center, &ecef_points) {
            return true;
        }
        
        false
    }

    /// Check if road intersects block bounds
    /// Check if road intersects block bounds
    fn road_intersects_block(&self, road: &OsmRoad, ecef_min: [f64; 3]) -> bool {
        let block_max = [
            ecef_min[0] + BLOCK_SIZE_M,
            ecef_min[1] + BLOCK_SIZE_M,
            ecef_min[2] + BLOCK_SIZE_M,
        ];
        
        // Convert all nodes to ECEF once
        let ecef_nodes: Vec<_> = road.nodes.iter().map(|p| gps_to_ecef(p)).collect();
        
        // Check if any node is inside block
        for ecef in &ecef_nodes {
            if ecef.x >= ecef_min[0] && ecef.x <= block_max[0] &&
               ecef.y >= ecef_min[1] && ecef.y <= block_max[1] &&
               ecef.z >= ecef_min[2] && ecef.z <= block_max[2] {
                return true;
            }
        }
        
        // Check if any line segment intersects block (segment-AABB test)
        // Use simple conservative test: expand block slightly and check if segment AABB overlaps
        for i in 0..ecef_nodes.len().saturating_sub(1) {
            let p1 = &ecef_nodes[i];
            let p2 = &ecef_nodes[i + 1];
            
            // Bounding box of line segment
            let seg_min = [
                p1.x.min(p2.x),
                p1.y.min(p2.y),
                p1.z.min(p2.z),
            ];
            let seg_max = [
                p1.x.max(p2.x),
                p1.y.max(p2.y),
                p1.z.max(p2.z),
            ];
            
            // AABB overlap test
            if seg_max[0] >= ecef_min[0] && seg_min[0] <= block_max[0] &&
               seg_max[1] >= ecef_min[1] && seg_min[1] <= block_max[1] &&
               seg_max[2] >= ecef_min[2] && seg_min[2] <= block_max[2] {
                return true;
            }
        }
        
        false
    }

    /// Check if water feature intersects block bounds
    fn water_intersects_block(&self, water: &OsmWater, ecef_min: [f64; 3]) -> bool {
        let block_max = [
            ecef_min[0] + BLOCK_SIZE_M,
            ecef_min[1] + BLOCK_SIZE_M,
            ecef_min[2] + BLOCK_SIZE_M,
        ];
        
        // Convert all polygon points to ECEF once
        let ecef_points: Vec<_> = water.polygon.iter().map(|p| gps_to_ecef(p)).collect();
        
        // Check if any point is inside block
        for ecef in &ecef_points {
            if ecef.x >= ecef_min[0] && ecef.x <= block_max[0] &&
               ecef.y >= ecef_min[1] && ecef.y <= block_max[1] &&
               ecef.z >= ecef_min[2] && ecef.z <= block_max[2] {
                return true;
            }
        }
        
        // Check if any polygon edge intersects block
        for i in 0..ecef_points.len() {
            let p1 = &ecef_points[i];
            let p2 = &ecef_points[(i + 1) % ecef_points.len()]; // Wrap around for closed polygon
            
            // Bounding box of edge
            let seg_min = [
                p1.x.min(p2.x),
                p1.y.min(p2.y),
                p1.z.min(p2.z),
            ];
            let seg_max = [
                p1.x.max(p2.x),
                p1.y.max(p2.y),
                p1.z.max(p2.z),
            ];
            
            // AABB overlap test
            if seg_max[0] >= ecef_min[0] && seg_min[0] <= block_max[0] &&
               seg_max[1] >= ecef_min[1] && seg_min[1] <= block_max[1] &&
               seg_max[2] >= ecef_min[2] && seg_min[2] <= block_max[2] {
                return true;
            }
        }
        
        // Check if block center is inside polygon (polygon contains block)
        let block_center = [
            ecef_min[0] + BLOCK_SIZE_M / 2.0,
            ecef_min[1] + BLOCK_SIZE_M / 2.0,
            ecef_min[2] + BLOCK_SIZE_M / 2.0,
        ];
        
        if Self::point_in_polygon_3d(&block_center, &ecef_points) {
            return true;
        }
        
        false
    }
    
    /// Check if point is inside 3D polygon (projects to 2D for ray casting)
    fn point_in_polygon_3d(point: &[f64; 3], polygon: &[EcefPos]) -> bool {
        // Simple ray casting in XY plane (good enough for Earth surface features)
        let mut inside = false;
        let px = point[0];
        let py = point[1];
        
        for i in 0..polygon.len() {
            let j = (i + 1) % polygon.len();
            let xi = polygon[i].x;
            let yi = polygon[i].y;
            let xj = polygon[j].x;
            let yj = polygon[j].y;
            
            let intersect = ((yi > py) != (yj > py)) &&
                (px < (xj - xi) * (py - yi) / (yj - yi) + xi);
            
            if intersect {
                inside = !inside;
            }
        }
        
        inside
    }

    /// Voxelize building into block
    fn voxelize_building(&self, _voxels: &mut [MaterialId; 512], _building: &OsmBuilding, _ecef_min: [f64; 3]) {
        // TODO: Implement proper building voxelization
        // For Phase 2, this is a placeholder
        // Will be fully implemented in Phase 4 (Interior Spaces)
    }

    /// Voxelize road into block
    fn voxelize_road(&self, voxels: &mut [MaterialId; 512], road: &OsmRoad, ecef_min: [f64; 3]) {
        // For each road segment, fill voxels along the path
        if road.nodes.len() < 2 {
            return; // Need at least 2 points for a road
        }

        // Get road material based on type
        let road_material = if road.is_tunnel {
            return; // Skip tunnels for now (Phase 4)
        } else if road.is_bridge {
            CONCRETE // Bridges use concrete
        } else {
            ASPHALT // Regular roads use asphalt
        };

        // For simplicity, voxelize road as a series of thick lines
        // This is a basic implementation - Phase 4 will add proper road geometry
        for i in 0..(road.nodes.len() - 1) {
            let start_gps = road.nodes[i];
            let end_gps = road.nodes[i + 1];
            
            let start_ecef = gps_to_ecef(&start_gps);
            let end_ecef = gps_to_ecef(&end_gps);
            
            // Check if segment intersects this block
            let block_max = [
                ecef_min[0] + BLOCK_SIZE_M,
                ecef_min[1] + BLOCK_SIZE_M,
                ecef_min[2] + BLOCK_SIZE_M,
            ];
            
            // Simple AABB intersection test
            let seg_min_x = start_ecef.x.min(end_ecef.x);
            let seg_max_x = start_ecef.x.max(end_ecef.x);
            let seg_min_y = start_ecef.y.min(end_ecef.y);
            let seg_max_y = start_ecef.y.max(end_ecef.y);
            let seg_min_z = start_ecef.z.min(end_ecef.z);
            let seg_max_z = start_ecef.z.max(end_ecef.z);
            
            if seg_max_x < ecef_min[0] || seg_min_x > block_max[0] ||
               seg_max_y < ecef_min[1] || seg_min_y > block_max[1] ||
               seg_max_z < ecef_min[2] || seg_min_z > block_max[2] {
                continue; // Segment doesn't intersect block
            }
            
            // Sample points along the road segment
            let length = ((end_ecef.x - start_ecef.x).powi(2) +
                         (end_ecef.y - start_ecef.y).powi(2) +
                         (end_ecef.z - start_ecef.z).powi(2)).sqrt();
            
            // Sample every 0.5m along the road
            let num_samples = (length / 0.5).ceil() as usize + 1;
            
            for j in 0..num_samples {
                let t = j as f64 / (num_samples - 1).max(1) as f64;
                
                // Interpolate position
                let sample_ecef = EcefPos {
                    x: start_ecef.x + t * (end_ecef.x - start_ecef.x),
                    y: start_ecef.y + t * (end_ecef.y - start_ecef.y),
                    z: start_ecef.z + t * (end_ecef.z - start_ecef.z),
                };
                
                // Get ground elevation at this point
                let sample_gps = ecef_to_gps(&sample_ecef);
                let ground_elevation = self.get_ground_elevation(sample_gps).unwrap_or(sample_gps.elevation_m);
                
                // Fill voxels at road surface level
                // Road width based on road type
                let half_width = road.width_m / 2.0;
                
                // Fill voxels within road width (simplified - just fills nearby voxels)
                for x in 0..VOXELS_PER_BLOCK {
                    for y in 0..VOXELS_PER_BLOCK {
                        for z in 0..VOXELS_PER_BLOCK {
                            let voxel_ecef = EcefPos {
                                x: ecef_min[0] + (x as f64 + 0.5) * VOXEL_SIZE_M,
                                y: ecef_min[1] + (y as f64 + 0.5) * VOXEL_SIZE_M,
                                z: ecef_min[2] + (z as f64 + 0.5) * VOXEL_SIZE_M,
                            };
                            
                            // Distance from voxel to road sample point
                            let dx = voxel_ecef.x - sample_ecef.x;
                            let dy = voxel_ecef.y - sample_ecef.y;
                            let dz = voxel_ecef.z - sample_ecef.z;
                            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                            
                            // If within road width, set to road material
                            if dist <= half_width {
                                let voxel_gps = ecef_to_gps(&voxel_ecef);
                                
                                // Only place road at/near ground level
                                if (voxel_gps.elevation_m - ground_elevation).abs() < 0.5 {
                                    let voxel_idx = Self::voxel_index(x, y, z);
                                    voxels[voxel_idx] = road_material;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Voxelize water feature into block
    fn voxelize_water(&self, voxels: &mut [MaterialId; 512], water: &OsmWater, ecef_min: [f64; 3]) {
        // Water features are typically polygons representing rivers, lakes, etc.
        if water.polygon.len() < 3 {
            return; // Need at least 3 points for a water polygon
        }

        // Calculate centroid and approximate water level
        let mut centroid_lat = 0.0;
        let mut centroid_lon = 0.0;
        for point in &water.polygon {
            centroid_lat += point.lat_deg;
            centroid_lon += point.lon_deg;
        }
        centroid_lat /= water.polygon.len() as f64;
        centroid_lon /= water.polygon.len() as f64;
        
        let centroid = GpsPos {
            lat_deg: centroid_lat,
            lon_deg: centroid_lon,
            elevation_m: 0.0,
        };
        
        // Get approximate water surface elevation
        let water_elevation = self.get_ground_elevation(centroid).unwrap_or(0.0);
        
        // For each voxel in the block, check if it's inside the water polygon
        for x in 0..VOXELS_PER_BLOCK {
            for y in 0..VOXELS_PER_BLOCK {
                for z in 0..VOXELS_PER_BLOCK {
                    let voxel_ecef = EcefPos {
                        x: ecef_min[0] + (x as f64 + 0.5) * VOXEL_SIZE_M,
                        y: ecef_min[1] + (y as f64 + 0.5) * VOXEL_SIZE_M,
                        z: ecef_min[2] + (z as f64 + 0.5) * VOXEL_SIZE_M,
                    };
                    
                    let voxel_gps = ecef_to_gps(&voxel_ecef);
                    
                    // Simple point-in-polygon test (ray casting)
                    if self.point_in_polygon(&voxel_gps, &water.polygon) {
                        // Fill with water if at/below water surface level
                        // Water depth: 2m default
                        if voxel_gps.elevation_m >= water_elevation - 2.0 &&
                           voxel_gps.elevation_m <= water_elevation {
                            let voxel_idx = Self::voxel_index(x, y, z);
                            voxels[voxel_idx] = WATER;
                        }
                    }
                }
            }
        }
    }

    /// Simple point-in-polygon test using ray casting
    fn point_in_polygon(&self, point: &GpsPos, polygon: &[GpsPos]) -> bool {
        if polygon.len() < 3 {
            return false;
        }

        let mut inside = false;
        let mut j = polygon.len() - 1;
        
        for i in 0..polygon.len() {
            let vi = &polygon[i];
            let vj = &polygon[j];
            
            if ((vi.lat_deg > point.lat_deg) != (vj.lat_deg > point.lat_deg)) &&
               (point.lon_deg < (vj.lon_deg - vi.lon_deg) * (point.lat_deg - vi.lat_deg) / 
                                (vj.lat_deg - vi.lat_deg) + vi.lon_deg) {
                inside = !inside;
            }
            
            j = i;
        }
        
        inside
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
        // Convert test area center to GPS
        let center_gps = ecef_to_gps(&self.config.area_center);
        
        // Fetch OSM features for area
        let data = self.osm_cache.get_area_features(center_gps, self.config.area_radius)?;
        
        // Store in caches
        *self.osm_buildings.lock().unwrap() = data.buildings;
        *self.osm_roads.lock().unwrap() = data.roads;
        *self.osm_water.lock().unwrap() = data.water;
        
        println!("Loaded OSM features:");
        println!("  Buildings: {}", self.osm_buildings.lock().unwrap().len());
        println!("  Roads: {}", self.osm_roads.lock().unwrap().len());
        println!("  Water: {}", self.osm_water.lock().unwrap().len());
        
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

    #[test]
    fn test_point_in_polygon() {
        let config = test_config();
        let generator = ProceduralGenerator::new(config).unwrap();
        
        // Create a simple square polygon
        let polygon = vec![
            GpsPos { lat_deg: 0.0, lon_deg: 0.0, elevation_m: 0.0 },
            GpsPos { lat_deg: 0.0, lon_deg: 1.0, elevation_m: 0.0 },
            GpsPos { lat_deg: 1.0, lon_deg: 1.0, elevation_m: 0.0 },
            GpsPos { lat_deg: 1.0, lon_deg: 0.0, elevation_m: 0.0 },
        ];
        
        // Test points inside
        assert!(generator.point_in_polygon(
            &GpsPos { lat_deg: 0.5, lon_deg: 0.5, elevation_m: 0.0 },
            &polygon
        ));
        
        // Test points outside
        assert!(!generator.point_in_polygon(
            &GpsPos { lat_deg: 2.0, lon_deg: 2.0, elevation_m: 0.0 },
            &polygon
        ));
        
        assert!(!generator.point_in_polygon(
            &GpsPos { lat_deg: -1.0, lon_deg: 0.5, elevation_m: 0.0 },
            &polygon
        ));
    }
}
