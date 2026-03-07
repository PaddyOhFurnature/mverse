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
    osm_cache_dir: Option<std::path::PathBuf>,
    /// Optional handle to request OSM tiles from P2P peers when not cached locally.
    tile_fetcher: Option<Arc<crate::multiplayer::TileFetcher>>,
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
            osm_cache_dir: None,
            tile_fetcher: None,
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
            osm_cache_dir: None,
            tile_fetcher: None,
        }
    }

    /// Set OSM cache directory for water-aware terrain generation.
    pub fn with_osm_cache(mut self, dir: std::path::PathBuf) -> Self {
        self.osm_cache_dir = Some(dir);
        self
    }

    /// Set an optional P2P tile fetcher for fetching OSM tiles from peers on cache miss.
    pub fn with_tile_fetcher(mut self, fetcher: Arc<crate::multiplayer::TileFetcher>) -> Self {
        self.tile_fetcher = Some(fetcher);
        self
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
        let elevation = self.elevation.lock()
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
                
                // Bilinear or nearest-neighbour depending on slope steepness.
                // Bilinear interpolation smooths gradual hills correctly, but turns
                // steep cliff faces into ramps.  When any adjacent SRTM sample pair
                // differs by more than CLIFF_THRESHOLD metres, snap to nearest-neighbour
                // so the cliff renders as a vertical wall rather than a ramp.
                const CLIFF_THRESHOLD: f64 = 4.0;
                let e00 = elevation_grid[i0 * samples_per_axis + j0];
                let e01 = elevation_grid[i0 * samples_per_axis + j1];
                let e10 = elevation_grid[i1 * samples_per_axis + j0];
                let e11 = elevation_grid[i1 * samples_per_axis + j1];

                let max_diff = (e00 - e01).abs()
                    .max((e00 - e10).abs())
                    .max((e01 - e11).abs())
                    .max((e10 - e11).abs());

                let surface_elevation = if max_diff > CLIFF_THRESHOLD {
                    // Nearest-neighbour: snap to closest SRTM sample point.
                    // Produces a vertical wall at the sample boundary.
                    let ni = if frac_i < 0.5 { i0 } else { i1 };
                    let nj = if frac_j < 0.5 { j0 } else { j1 };
                    elevation_grid[ni * samples_per_axis + nj]
                } else {
                    // Bilinear interpolation for smooth terrain.
                    let e0 = e00 * (1.0 - frac_j) + e01 * frac_j;
                    let e1 = e10 * (1.0 - frac_j) + e11 * frac_j;
                    e0 * (1.0 - frac_i) + e1 * frac_i
                };
                
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

        // Load OSM data for this chunk (water + roads for terrain carving)
        let (chunk_water_polys, chunk_waterway_lines, chunk_roads) = if let Some(ref dir) = self.osm_cache_dir {
            let (lat_min, lat_max, lon_min, lon_max) = chunk_id.gps_bounds();
            let mut osm = crate::osm::fetch_osm_for_chunk(lat_min, lat_max, lon_min, lon_max, dir);
            // On disk miss, try fetching from a P2P peer
            if osm.is_empty() {
                if let Some(ref fetcher) = self.tile_fetcher {
                    let tile_size = 0.01_f64;
                    let s = ((lat_min + lat_max) * 0.5 / tile_size).floor() * tile_size;
                    let w = ((lon_min + lon_max) * 0.5 / tile_size).floor() * tile_size;
                    if let Some(bytes) = fetcher.fetch_osm(s, w, s + tile_size, w + tile_size) {
                        if let Ok(data) = bincode::deserialize::<crate::osm::OsmData>(&bytes) {
                            let cache = crate::osm::OsmDiskCache::new(dir);
                            cache.save(s, w, s + tile_size, w + tile_size, &data);
                            // Announce to DHT — we now have this tile cached
                            fetcher.announce_osm(s, w, s + tile_size, w + tile_size);
                            osm = crate::osm::fetch_osm_for_chunk(lat_min, lat_max, lon_min, lon_max, dir);
                        }
                    }
                }
            }
            (osm.water, osm.waterway_lines, osm.roads)
        } else {
            (vec![], vec![], vec![])
        };

        // Two-pass chunk generation:
        //   Pass 1 — sample every column, compute SRTM elevation, check OSM water polygon.
        //   Post-pass — mark boundary (bank) columns adjacent to water as is_bank.
        //   Pass 2 — generate voxels per-column.
        //
        //   Water columns use their OWN SRTM elevation as the water surface — not a
        //   single chunk-minimum. SRTM measures the actual water surface for rivers
        //   so this correctly follows the terrain slope (15m upstream → 5m at bay).
        //
        // GPS per column computed via ellipsoid equation (not fixed origin Y).
        // At Brisbane lon≈153°, fixing ECEF Y at origin causes 726m geographic error at 1km.
        // WGS-84: Y = sqrt(A²(1 - Z²/B²) - X²), sign matched to origin.
        struct ColSample {
            voxel_x: i64,
            voxel_z: i64,
            lat:              f64,
            lon:              f64,
            surface_elevation: f64,
            surface_voxel_y:   i64,
            in_water:          bool,
            is_road:           bool,
        }

        const WGS84_A: f64 = 6_378_137.0;
        const WGS84_B: f64 = 6_356_752.3142;
        let origin_ecef_y = (self.origin_voxel.y as f64 + 0.5) + crate::voxel::WORLD_MIN_METERS;

        let mut columns: Vec<ColSample> =
            Vec::with_capacity((CHUNK_SIZE_X * CHUNK_SIZE_Z) as usize);

        // --- Pass 1 ---
        for i in 0..CHUNK_SIZE_X {
            for k in 0..CHUNK_SIZE_Z {
                let voxel_x = min_voxel.x + i;
                let voxel_z = min_voxel.z + k;

                // Ellipsoid-corrected ECEF Y for this column's GPS
                let ecef_x = (voxel_x as f64 + 0.5) + crate::voxel::WORLD_MIN_METERS;
                let ecef_z = (voxel_z as f64 + 0.5) + crate::voxel::WORLD_MIN_METERS;
                let y_sq = WGS84_A * WGS84_A * (1.0 - (ecef_z / WGS84_B).powi(2))
                    - ecef_x * ecef_x;
                let ecef_y = if y_sq > 0.0 {
                    y_sq.sqrt() * origin_ecef_y.signum()
                } else {
                    origin_ecef_y
                };
                let sample_gps =
                    crate::coordinates::ECEF::new(ecef_x, ecef_y, ecef_z).to_gps();

                let surface_elevation = elevation.lock().unwrap().query(&sample_gps)
                    .map(|e| e.meters)
                    .unwrap_or(self.origin_gps.alt);
                let surface_voxel_y = self.origin_voxel.y
                    + (surface_elevation - self.origin_gps.alt) as i64;

                // Water from OSM polygon only (centreline handled in post-pass below).
                let in_water = !chunk_water_polys.is_empty()
                    && chunk_water_polys.iter().any(|w| {
                        crate::osm::point_in_polygon(
                            sample_gps.lat, sample_gps.lon, &w.polygon,
                        ) && !w.holes.iter().any(|hole| {
                            crate::osm::point_in_polygon(
                                sample_gps.lat, sample_gps.lon, hole,
                            )
                        })
                    });

                columns.push(ColSample {
                    voxel_x, voxel_z,
                    lat: sample_gps.lat,
                    lon: sample_gps.lon,
                    surface_elevation, surface_voxel_y, in_water,
                    is_road: false,
                });
            }
        }

        // --- Water level normalisation pass ---
        // SRTM elevation over open water has 1–3 m of pixel noise, which
        // produces visible "steps" or layered surfaces across the river.
        // Within any chunk cross-section the water surface must be flat,
        // so we level all in_water columns to the minimum surface_voxel_y
        // found in this chunk.  The natural downstream slope is preserved
        // because each chunk independently finds its own minimum.
        if columns.iter().any(|c| c.in_water) {
            let water_level_y = columns.iter()
                .filter(|c| c.in_water)
                .map(|c| c.surface_voxel_y)
                .min()
                .unwrap_or(0);
            for col in columns.iter_mut() {
                if col.in_water {
                    col.surface_voxel_y = water_level_y;
                }
            }
        }

        // --- Road carving pass ---
        // For each non-bridge road segment, flatten terrain to road elevation.
        // Uses the same Y formula as the OSM render pipeline:
        //   render_y = (geographic_elevation - origin_gps.alt)
        // X/Z from sea-level ECEF, matching terrain column coords.
        if !chunk_roads.is_empty() {
            use std::collections::HashMap;
            let mut col_lookup: HashMap<(i64, i64), usize> = HashMap::with_capacity(columns.len());
            for (i, c) in columns.iter().enumerate() {
                col_lookup.insert((c.voxel_x, c.voxel_z), i);
            }

            let elev_arc = Arc::clone(&elevation);
            for road in &chunk_roads {
                if road.is_bridge || road.is_tunnel { continue; }
                let half_w = (road.road_type.width_m() / 2.0).ceil() as i64 + 1;

                for pair in road.nodes.windows(2) {
                    let ga = &pair[0];
                    let gb = &pair[1];

                    // GPS → voxel XZ (sea-level ECEF matches terrain column formula)
                    let ea = crate::coordinates::GPS::new(ga.lat, ga.lon, 0.0).to_ecef();
                    let eb = crate::coordinates::GPS::new(gb.lat, gb.lon, 0.0).to_ecef();
                    let vax = (ea.x - crate::voxel::WORLD_MIN_METERS).round() as i64;
                    let vaz = (ea.z - crate::voxel::WORLD_MIN_METERS).round() as i64;
                    let vbx = (eb.x - crate::voxel::WORLD_MIN_METERS).round() as i64;
                    let vbz = (eb.z - crate::voxel::WORLD_MIN_METERS).round() as i64;

                    // Endpoint surface Y (from column data if in chunk, else elevation query)
                    let ya = col_lookup.get(&(vax, vaz))
                        .map(|&i| columns[i].surface_voxel_y)
                        .unwrap_or_else(|| {
                            elev_arc.lock().unwrap()
                                .query(&crate::coordinates::GPS::new(ga.lat, ga.lon, 0.0))
                                .map(|e| self.origin_voxel.y + (e.meters - self.origin_gps.alt) as i64)
                                .unwrap_or(self.origin_voxel.y)
                        });
                    let yb = col_lookup.get(&(vbx, vbz))
                        .map(|&i| columns[i].surface_voxel_y)
                        .unwrap_or_else(|| {
                            elev_arc.lock().unwrap()
                                .query(&crate::coordinates::GPS::new(gb.lat, gb.lon, 0.0))
                                .map(|e| self.origin_voxel.y + (e.meters - self.origin_gps.alt) as i64)
                                .unwrap_or(self.origin_voxel.y)
                        });

                    // Step along segment, one voxel at a time
                    let dx = vbx - vax;
                    let dz = vbz - vaz;
                    let steps = dx.abs().max(dz.abs());
                    if steps == 0 { continue; }

                    for step in 0..=steps {
                        let t = step as f32 / steps as f32;
                        let cx = vax + (dx as f32 * t).round() as i64;
                        let cz = vaz + (dz as f32 * t).round() as i64;
                        let road_y = ya + ((yb - ya) as f32 * t).round() as i64;

                        // Paint road width as a circle around centerline
                        for ox in -half_w..=half_w {
                            for oz in -half_w..=half_w {
                                if ox * ox + oz * oz > half_w * half_w { continue; }
                                if let Some(&idx) = col_lookup.get(&(cx + ox, cz + oz)) {
                                    // Only flatten if not already water
                                    if !columns[idx].in_water {
                                        columns[idx].surface_voxel_y = road_y;
                                        columns[idx].is_road = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // --- Pass 2 ---
        const BEDROCK_DEPTH: i64 = 200;
        const SKY_HEIGHT:    i64 = 100;
        const WATER_DEPTH:   i64 = 5;

        for col in &columns {
            // Per-column water: each column uses its own SRTM surface elevation.
            // SRTM measures the water surface for rivers — no flat chunk-minimum needed.
            // This lets the river follow the actual terrain slope voxel-by-voxel.
            let col_top = col.surface_voxel_y + SKY_HEIGHT;
            let col_bot = col.surface_voxel_y - BEDROCK_DEPTH;

            for voxel_y in col_bot..=col_top {
                if voxel_y < min_voxel.y || voxel_y >= max_voxel.y {
                    continue;
                }
                let voxel_pos = VoxelCoord::new(col.voxel_x, voxel_y, col.voxel_z);
                let depth_below_surface = col.surface_voxel_y - voxel_y;

                let material = if col.in_water {
                    // Water column: surface_voxel_y IS the water surface (SRTM = water level).
                    // WATER fills from surface down WATER_DEPTH voxels, then GRAVEL riverbed.
                    if depth_below_surface < 0 {
                        MaterialId::AIR
                    } else if depth_below_surface < WATER_DEPTH {
                        MaterialId::WATER
                    } else if depth_below_surface < WATER_DEPTH + 5 {
                        MaterialId::GRAVEL
                    } else {
                        MaterialId::STONE
                    }
                } else {
                    if depth_below_surface < 0 {
                        MaterialId::AIR
                    } else if depth_below_surface == 0 {
                        if col.is_road { MaterialId::ASPHALT } else { MaterialId::GRASS }
                    } else if depth_below_surface <= 5 {
                        MaterialId::DIRT
                    } else {
                        MaterialId::STONE
                    }
                };

                octree.set_voxel(voxel_pos, material);
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
        let elevation = self.elevation.lock()
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
    use crate::elevation::OpenTopographySource;
    #[cfg(feature = "terrain-gdal")]
    use crate::elevation::NasFileSource;
    use std::path::PathBuf;
    
    #[test]
    #[ignore] // Requires SRTM data
    fn test_generate_column_kangaroo_point() {
        // Kangaroo Point Cliffs
        let gps = GPS::new(-27.4775, 153.0355, 0.0);
        
        // Setup elevation pipeline (try NAS, fall back to API)
        #[cfg(feature = "terrain-gdal")]
        let nas = NasFileSource::new();
        let api_key = "3e607de6969c687053f9e107a4796962".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let api = OpenTopographySource::new(api_key, cache_dir);
        
        let mut pipeline = ElevationPipeline::new();
        #[cfg(feature = "terrain-gdal")]
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
        #[cfg(feature = "terrain-gdal")]
        let nas = NasFileSource::new();
        let api_key = "3e607de6969c687053f9e107a4796962".to_string();
        let cache_dir = PathBuf::from("./elevation_cache");
        let api = OpenTopographySource::new(api_key, cache_dir);
        
        let mut pipeline = ElevationPipeline::new();
        #[cfg(feature = "terrain-gdal")]
        if let Some(nas_source) = nas {
            println!("✓ Using NAS file source");
            pipeline.add_source(Box::new(nas_source));
        }
        #[cfg(not(feature = "terrain-gdal"))]
        println!("⚠ NAS not available (terrain-gdal feature disabled), using API");
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
