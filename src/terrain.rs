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

use crate::chunk::ChunkId;
use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;
use crate::materials::MaterialId;
use crate::voxel::{Octree, VoxelCoord};
use std::sync::{Arc, RwLock};

/// Per-column fractional surface height used by the smooth marching cubes pass.
/// Maps (voxel_x, voxel_z) → sub-voxel surface Y (f32).
/// Positive density = solid, density ≥ 0 at surface voxel corner.
pub type SurfaceCache = std::collections::HashMap<(i64, i64), f64>;

fn apply_engineered_ground_control(
    analysis: Option<&crate::terrain_analysis::TerrainAnalysis>,
    lat: f64,
    lon: f64,
    surface_elevation: f64,
) -> f64 {
    let Some((target_level, strength)) =
        analysis.and_then(|analysis| analysis.engineered_ground_control_at(lat, lon))
    else {
        return surface_elevation;
    };
    let blend = strength.clamp(0.0, 1.0) as f64;
    if blend <= 0.0 {
        return surface_elevation;
    }
    surface_elevation + (target_level as f64 - surface_elevation) * blend
}

/// Generate terrain voxels from elevation data
pub struct TerrainGenerator {
    elevation: Arc<RwLock<ElevationPipeline>>,
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
    /// Optional pre-computed terrain analysis (slope, TWI, TRI, aspect).
    /// When set, biome classification uses real derived rasters; otherwise
    /// flat-moderate defaults are used.
    pub analysis: Option<Arc<crate::terrain_analysis::TerrainAnalysis>>,
    /// When true, skip all vegetation placement (trees, shrubs).
    /// Use for raw SRTM-only generation or debugging.
    pub skip_vegetation: bool,
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
            elevation: Arc::new(RwLock::new(elevation)),
            origin_gps,
            origin_voxel,
            analysis: None,
            skip_vegetation: false,
        }
    }

    /// Create with shared elevation pipeline (for parallel chunk generation)
    pub fn with_shared_elevation(
        elevation: Arc<RwLock<ElevationPipeline>>,
        origin_gps: GPS,
        origin_voxel: VoxelCoord,
    ) -> Self {
        Self {
            elevation,
            origin_gps,
            origin_voxel,
            analysis: None,
            skip_vegetation: false,
        }
    }

    /// Attach a pre-computed terrain analysis for biome-aware material selection.
    pub fn with_analysis(mut self, a: Arc<crate::terrain_analysis::TerrainAnalysis>) -> Self {
        self.analysis = Some(a);
        self
    }

    /// Disable deterministic tree/shrub placement for this generator.
    pub fn without_vegetation(mut self) -> Self {
        self.skip_vegetation = true;
        self
    }

    /// Clone this generator while swapping in a specific analysis product.
    pub fn clone_with_analysis(
        &self,
        analysis: Option<Arc<crate::terrain_analysis::TerrainAnalysis>>,
    ) -> Self {
        Self {
            elevation: Arc::clone(&self.elevation),
            origin_gps: self.origin_gps,
            origin_voxel: self.origin_voxel,
            analysis,
            skip_vegetation: self.skip_vegetation,
        }
    }

    /// Get shared elevation pipeline (for cloning generator)
    pub fn elevation_pipeline(&self) -> Arc<RwLock<ElevationPipeline>> {
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
        println!(
            "Generating {}m × {}m terrain region...",
            size_meters, size_meters
        );

        // Lock elevation pipeline
        let elevation = self
            .elevation
            .read()
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

        println!(
            "  Sampling SRTM: {} × {} = {} points",
            samples_per_axis,
            samples_per_axis,
            samples_per_axis * samples_per_axis
        );

        let mut elevation_grid = Vec::with_capacity(samples_per_axis * samples_per_axis);

        for i in 0..samples_per_axis {
            for j in 0..samples_per_axis {
                let t_lat = i as f64 / (samples_per_axis - 1) as f64;
                let t_lon = j as f64 / (samples_per_axis - 1) as f64;
                let lat = lat_min + t_lat * (lat_max - lat_min);
                let lon = lon_min + t_lon * (lon_max - lon_min);

                let gps = GPS::new(lat, lon, 0.0);
                let elev_meters = elevation
                    .query(&gps)
                    .map_err(|e| format!("SRTM query failed at ({}, {}): {}", lat, lon, e))?
                    .meters;

                elevation_grid.push(elev_meters);
            }
        }

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

                let max_diff = (e00 - e01)
                    .abs()
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

                    let depth_below_surface =
                        surface_elevation - (origin.alt + height_offset as f64);

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
    pub fn generate_chunk(&self, chunk_id: &ChunkId) -> Result<(Octree, SurfaceCache), String> {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};

        let min_voxel = chunk_id.min_voxel();
        let max_voxel = chunk_id.max_voxel();

        let mut octree = Octree::new();

        // Clone Arc for direct access (ElevationPipeline methods are now &self, thread-safe)
        let elevation = Arc::clone(&self.elevation);

        // Two-pass chunk generation:
        //   Pass 1 — sample every column, compute SRTM elevation.
        //   Pass 2 — fill voxels: STONE bedrock, DIRT sublayer, GRASS surface.
        //
        // GPS per column computed via ellipsoid equation (not fixed origin Y).
        // At Brisbane lon≈153°, fixing ECEF Y at origin causes 726m geographic error at 1km.
        // WGS-84: Y = sqrt(A²(1 - Z²/B²) - X²), sign matched to origin.
        struct ColSample {
            voxel_x: i64,
            voxel_z: i64,
            lat: f64,
            lon: f64,
            surface_elevation: f64,
            surface_voxel_y: i64,
            surface_y_f: f64,
            /// Terrain slope at this column in degrees (0 = flat, 90 = vertical cliff).
            /// Computed via central differences after all columns are sampled.
            slope_deg: f32,
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
                let y_sq = WGS84_A * WGS84_A * (1.0 - (ecef_z / WGS84_B).powi(2)) - ecef_x * ecef_x;
                let ecef_y = if y_sq > 0.0 {
                    y_sq.sqrt() * origin_ecef_y.signum()
                } else {
                    origin_ecef_y
                };
                let sample_gps = crate::coordinates::ECEF::new(ecef_x, ecef_y, ecef_z).to_gps();

                let surface_elevation = elevation
                    .read()
                    .unwrap()
                    .query_with_fill(&sample_gps)
                    .map(|e| e.meters)
                    .unwrap_or(self.origin_gps.alt);
                let surface_elevation = apply_engineered_ground_control(
                    self.analysis.as_deref(),
                    sample_gps.lat,
                    sample_gps.lon,
                    surface_elevation,
                );
                // SRTM/Copernicus = orthometric (EGM96). origin_gps.alt = WGS-84 ellipsoidal.
                // Add geoid undulation N so both are in the same datum before differencing.
                let n_col = crate::elevation::egm96_undulation(sample_gps.lat, sample_gps.lon);
                let surface_delta = surface_elevation + n_col - self.origin_gps.alt;
                let surface_voxel_y = self.origin_voxel.y + surface_delta as i64;
                // Fractional surface height — used by smooth marching cubes for sub-voxel placement.
                let surface_y_f = self.origin_voxel.y as f64 + surface_delta;

                columns.push(ColSample {
                    voxel_x,
                    voxel_z,
                    lat: sample_gps.lat,
                    lon: sample_gps.lon,
                    surface_elevation,
                    surface_voxel_y,
                    surface_y_f,
                    slope_deg: 0.0,
                });
            }
        }

        // --- Pass 1.5: slope (central differences on surface_voxel_y grid) ---
        // Each voxel is 1m, so dy/dx is already in m/m. Slope = atan(|∇|) in degrees.
        {
            let cx = CHUNK_SIZE_X as usize;
            let cz = CHUNK_SIZE_Z as usize;
            // Helper: get surface_voxel_y at grid index (i, k), clamped to edge.
            let y_at = |cols: &[ColSample], i: usize, k: usize| -> f32 {
                cols[i * cz + k].surface_voxel_y as f32
            };
            for i in 0..cx {
                for k in 0..cz {
                    let ie = if i + 1 < cx { i + 1 } else { i };
                    let iw = if i > 0 { i - 1 } else { i };
                    let kn = if k + 1 < cz { k + 1 } else { k };
                    let ks = if k > 0 { k - 1 } else { k };
                    // Central difference (or forward/backward at edges).
                    let dx = (ie - iw) as f32; // 2.0 for interior, 1.0 for edge
                    let dz = (kn - ks) as f32;
                    let dydx = (y_at(&columns, ie, k) - y_at(&columns, iw, k)) / dx;
                    let dydz = (y_at(&columns, i, kn) - y_at(&columns, i, ks)) / dz;
                    let grad = (dydx * dydx + dydz * dydz).sqrt();
                    columns[i * cz + k].slope_deg = grad.atan().to_degrees();
                }
            }
        }

        // --- Pass 2 ---
        const BEDROCK_DEPTH: i64 = 200;

        for col in &columns {
            let col_bot = col.surface_voxel_y - BEDROCK_DEPTH;
            let col_top = col.surface_voxel_y;
            let osm_landuse = self
                .analysis
                .as_ref()
                .and_then(|analysis| analysis.osm_landuse_at(col.lat, col.lon));

            let classification = crate::biome::classify_column(
                col.lat,
                col.lon,
                col.surface_elevation as f32,
                self.analysis.as_deref(),
                osm_landuse,
                self.analysis
                    .as_ref()
                    .map(|a| a.coastal_dist_at(col.lat, col.lon))
                    .unwrap_or(100_000.0),
            );

            // Slope-override: steep faces expose bedrock regardless of biome.
            // Thresholds (degrees):
            //   0–25  → biome default surface (GRASS / SAND / etc.)
            //   25–45 → DIRT (soil erodes off gentle slopes)
            //   45–65 → DIRT surface, halved soil depth (exposed subsoil)
            //   >65   → STONE surface, 1-voxel soil (cliff face)
            let s = col.slope_deg;
            let surface_mat = if s > 65.0 {
                MaterialId::STONE
            } else if s > 45.0 {
                // Steep: exposed DIRT unless biome already gives STONE/SAND.
                match surface_material_for(&classification) {
                    MaterialId::STONE | MaterialId::SAND => surface_material_for(&classification),
                    _ => MaterialId::DIRT,
                }
            } else if s > 25.0 {
                // Moderate slope: replace GRASS with DIRT; everything else unchanged.
                let base = surface_material_for(&classification);
                if base == MaterialId::GRASS {
                    MaterialId::DIRT
                } else {
                    base
                }
            } else {
                surface_material_for(&classification)
            };

            let subsurface_mat = subsurface_material_for(&classification);
            let bedrock_mat = bedrock_material_for(&classification);

            // Reduce soil depth on steep slopes — less accumulation.
            let soil_depth = if s > 65.0 {
                1_i64
            } else if s > 45.0 {
                (classification.soil_depth_voxels as i64 / 2).max(1)
            } else {
                classification.soil_depth_voxels as i64
            };

            for voxel_y in col_bot..=col_top {
                if voxel_y < min_voxel.y || voxel_y >= max_voxel.y {
                    continue;
                }
                let voxel_pos = VoxelCoord::new(col.voxel_x, voxel_y, col.voxel_z);
                let depth_below_surface = col.surface_voxel_y - voxel_y;

                let material = if depth_below_surface == 0 {
                    surface_mat
                } else if depth_below_surface <= soil_depth {
                    subsurface_mat
                } else {
                    bedrock_mat
                };

                octree.set_voxel(voxel_pos, material);
            }

            // Reservoir flood-fill: if this column is inside a reservoir, fill
            // voxels from the terrain surface up to the water surface with WATER.
            if let Some(water_surface_m) = self
                .analysis
                .as_ref()
                .and_then(|a| a.reservoir_level_at(col.lat, col.lon))
            {
                let water_voxel_top = col.surface_voxel_y
                    + (water_surface_m - col.surface_elevation as f32).ceil() as i64;
                let fill_bot = col.surface_voxel_y + 1;
                let fill_top = water_voxel_top.min(max_voxel.y - 1);
                for voxel_y in fill_bot..=fill_top {
                    let voxel_pos = VoxelCoord::new(col.voxel_x, voxel_y, col.voxel_z);
                    octree.set_voxel(voxel_pos, MaterialId::WATER);
                }
                // Replace the surface voxel with water too if submerged
                if water_surface_m > col.surface_elevation as f32 {
                    let surf_pos = VoxelCoord::new(col.voxel_x, col.surface_voxel_y, col.voxel_z);
                    octree.set_voxel(surf_pos, MaterialId::WATER);
                }
            }

            // Vegetation: deterministic tree/shrub placement based on biome.
            // Skips water columns, steep slopes, and biomes with no vegetation.
            if !self.skip_vegetation && surface_mat != MaterialId::WATER {
                crate::vegetation::maybe_place_vegetation(
                    &mut octree,
                    col.voxel_x,
                    col.voxel_z,
                    col.surface_voxel_y,
                    classification.biome,
                    col.slope_deg,
                    min_voxel.y,
                    max_voxel.y,
                );
            }
        }

        let mut surface_cache = SurfaceCache::with_capacity(columns.len());
        for col in &columns {
            surface_cache.insert((col.voxel_x, col.voxel_z), col.surface_y_f);
        }

        Ok((octree, surface_cache))
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
        let elevation = self
            .elevation
            .read()
            .map_err(|e| format!("Failed to lock elevation pipeline: {}", e))?;

        // Query elevation at this lat/lon
        let elev_meters = elevation
            .query(gps)
            .map_err(|e| format!("Elevation query failed: {:?}", e))?
            .meters;

        let surface_elevation = elev_meters;

        // Generate voxels from bedrock to sky
        const BEDROCK_DEPTH: f64 = 200.0; // 200m below surface
        const SKY_HEIGHT: f64 = 100.0; // 100m above surface

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

// ── Biome-aware material helpers ──────────────────────────────────────────────

fn surface_material_for(c: &crate::biome::ColumnClassification) -> MaterialId {
    use crate::biome::{Biome, SubstrateType};
    match c.biome {
        Biome::Ocean | Biome::Lake | Biome::River => MaterialId::WATER,
        Biome::Beach => MaterialId::SAND,
        Biome::Wetland | Biome::RiparianCorridor => MaterialId::GRASS,
        Biome::MangroveCoast => MaterialId::GRASS,
        Biome::Urban => MaterialId::ASPHALT,
        Biome::Agricultural => MaterialId::GRASS_DRY,
        // Australian dry eucalypt / subtropical grassland → olive-yellow surface
        Biome::DrySclerophyllForest
        | Biome::SubtropicalGrassland
        | Biome::Shrubland
        | Biome::Desert => MaterialId::GRASS_DRY,
        // Alpine / boreal: snow above, normal grass below treeline
        Biome::AlpineGrassland | Biome::Tundra | Biome::IceCap => MaterialId::SNOW,
        Biome::BorealForest => MaterialId::GRASS,
        _ => match c.substrate {
            SubstrateType::BedRock
            | SubstrateType::Sandstone
            | SubstrateType::Granite
            | SubstrateType::Basalt => MaterialId::STONE,
            SubstrateType::Sand => MaterialId::SAND,
            SubstrateType::UrbanFill => MaterialId::ASPHALT,
            // Red/tropical clay soils → laterite surface
            SubstrateType::RedClay | SubstrateType::TropicalRed => MaterialId::LATERITE,
            _ => MaterialId::GRASS,
        },
    }
}

fn subsurface_material_for(c: &crate::biome::ColumnClassification) -> MaterialId {
    use crate::biome::SubstrateType;
    match c.substrate {
        SubstrateType::Sand => MaterialId::SAND,
        SubstrateType::UrbanFill => MaterialId::CONCRETE,
        SubstrateType::BedRock
        | SubstrateType::Sandstone
        | SubstrateType::Granite
        | SubstrateType::Basalt => MaterialId::STONE,
        SubstrateType::RedClay | SubstrateType::TropicalRed => MaterialId::LATERITE,
        _ => MaterialId::DIRT,
    }
}

fn bedrock_material_for(_c: &crate::biome::ColumnClassification) -> MaterialId {
    MaterialId::STONE
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "terrain-gdal")]
    use crate::elevation::NasFileSource;
    use crate::elevation::OpenTopographySource;
    use crate::osm::OsmLandArea;
    use crate::terrain_analysis::{RegionDem, TerrainAnalysis};
    use std::path::PathBuf;

    #[test]
    fn engineered_ground_control_flattens_surface_toward_analysis_target() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 110.0, 120.0, 130.0, 100.0, //
                100.0, 110.0, 120.0, 130.0, 100.0, //
                100.0, 110.0, 120.0, 130.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        analysis.compute_engineered_ground(&[OsmLandArea {
            osm_id: 1,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Flat Reserve".into()),
            area_type: "recreation_ground".into(),
            category: "landuse".into(),
        }]);

        let adjusted = apply_engineered_ground_control(Some(&analysis), 0.025, 0.025, 130.0);
        assert!((adjusted - 100.0).abs() < 0.01);
    }

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

                generator
                    .generate_column(&mut octree, &gps)
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
        println!(
            "Time per column: {:.3}ms",
            elapsed.as_secs_f64() * 1000.0 / column_count as f64
        );
        println!("\nVoxel counts:");
        for (material, count) in voxel_counts.iter() {
            println!("  {:?}: {}", material, count);
        }
        println!("Total solid voxels: {}", total_voxels);
        println!("Expected voxels: ~30,000 (100 columns × 300 voxels each)");

        // Validate results
        assert!(column_count == 100, "Should generate 100 columns");
        assert!(total_voxels > 10000, "Should have significant solid voxels");
        assert!(
            elapsed.as_secs_f64() < 30.0,
            "Should complete in <30s (target <5s for 100m×100m)"
        );

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
        println!(
            "  • Time: {:.3}s ({:.1}× faster than target)",
            elapsed.as_secs_f64(),
            30.0 / elapsed.as_secs_f64()
        );
    }
}
