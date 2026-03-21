/// Offline world generation — pre-bakes terrain, buildings, roads and other
/// static OSM features into a TileStore database that is served via P2P.
///
/// The output is written to `output_dir/tiles.db` (a TileStore RocksDB database).
/// Copying that single file to the server is all that's needed to deploy a region.
///
/// # Architecture
/// 1.  `RegionBounds` — a geographic bounding box (lat/lon degrees)
/// 2.  `enumerate_surface_chunks` — returns every surface-layer ChunkId covering
///     the bbox; buildings/tall objects may need the Y+1 layer too.
/// 3.  `generate_region` — parallel Rayon loop; calls
///     `TerrainGenerator::generate_chunk` (with `bake_buildings=true`) for
///     each ChunkId, serialises result, writes to TileStore.
/// 4.  `RegionManifest` — JSON summary written on completion; server uses this
///     to answer "what chunks do you have?" queries from peers.
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use crate::terrain_analysis::{RegionDem, TerrainAnalysis};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::chunk::ChunkId;
use crate::chunk_loader::TERRAIN_CACHE_VERSION;
use crate::coordinates::GPS;
use crate::terrain::TerrainGenerator;
use crate::tile_store::{PassId, TileStore};
use crate::voxel::VoxelCoord;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionBounds {
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
}

impl RegionBounds {
    pub fn new(lat_min: f64, lat_max: f64, lon_min: f64, lon_max: f64) -> Self {
        Self {
            lat_min,
            lat_max,
            lon_min,
            lon_max,
        }
    }

    /// Geographic centre of the region (lat, lon).
    /// Used as the worldgen origin so any region can be generated without
    /// hardcoded coordinates.
    pub fn center(&self) -> (f64, f64) {
        (
            (self.lat_min + self.lat_max) * 0.5,
            (self.lon_min + self.lon_max) * 0.5,
        )
    }

    /// A few pre-defined named regions.
    pub fn named(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "brisbane" => Some(Self::new(-27.70, -27.20, 152.70, 153.30)),
            "brisbane-cbd" => Some(Self::new(-27.50, -27.44, 153.00, 153.05)),
            "brisbane-small" | "test" => Some(Self::new(-27.48, -27.45, 153.01, 153.04)),
            // 2km radius around Gympie Hospital — visual regression test reference scene
            "gympie" => Some(Self::new(-26.208, -26.172, 152.646, 152.686)),
            _ => None,
        }
    }
}

/// Per-chunk entry in the manifest (used for incremental updates & P2P verification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestChunk {
    pub chunk_id: String, // "x,y,z"
    pub size: u64,        // bytes
    pub sha256: String,   // hex
}

/// Written to `manifest.json` in the output directory on completion.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegionManifest {
    pub format_version: u32,
    pub terrain_cache_version: u32,
    pub region: RegionBounds,
    pub origin_gps: [f64; 3], // [lat, lon, alt]
    pub generated_at: String, // RFC3339 UTC
    pub chunk_count: usize,
    pub chunks: Vec<ManifestChunk>,
}

/// Configuration for a worldgen run.
pub struct WorldgenConfig {
    pub region: RegionBounds,
    /// Directory for the manifest JSON and the `tiles.db` TileStore output.
    pub output_dir: PathBuf,
    pub workers: usize,
    /// Also generate Y+1 layer for tall buildings / multi-storey structures.
    pub extra_y_layers: i32,
    /// Print progress to stderr every `report_interval` chunks.
    pub report_interval: usize,
    /// Enable per-chunk timing output (prints generate/serialize times).
    pub verbose: bool,
    /// TileStore to write terrain chunks into.  Opens `output_dir/tiles.db` if None.
    pub tile_store: Option<Arc<TileStore>>,
    /// OSM cache for applying waterways and water polygons after terrain generation.
    /// When set, an `OsmProcessor` is applied to each chunk after `generate_chunk`.
    pub osm_cache: Option<Arc<crate::osm::OsmDiskCache>>,
    /// Pre-computed terrain analysis (slope, TWI, flow, TRI, aspect).
    /// When None, `generate_region` will sample and compute it automatically.
    pub analysis: Option<Arc<TerrainAnalysis>>,
    /// Path to a GSHHG binary coastline file (e.g. `gshhs_f.b`).
    /// When set, coastal distance is computed and fed into biome classification.
    pub gshhg_path: Option<std::path::PathBuf>,
    /// Skip all vegetation placement (trees, shrubs).
    pub skip_vegetation: bool,
    /// Skip OSM water/road processing even if osm_cache is set.
    pub skip_osm: bool,
    /// Skip road geometry only (carriageways, footpaths, bridges, tunnels).
    pub skip_roads: bool,
    /// Skip OSM water polygons and waterway channel carving only.
    pub skip_water: bool,
}

// ─── Chunk enumeration ────────────────────────────────────────────────────────

/// Returns the surface Y-layer chunk IDs for every XZ column in the region.
///
/// The surface Y is estimated from the region centre using the same mapping as
/// `VoxelCoord::from_ecef`.  All chunks in the same region share the same Y
/// layer (ECEF Y varies very little within a ≤100km bbox).
///
/// `extra_y_layers` additional layers are appended above the surface layer to
/// cover tall buildings.
pub fn enumerate_surface_chunks(
    bounds: &RegionBounds,
    origin_gps: &GPS,
    origin_voxel: &VoxelCoord,
    extra_y_layers: i32,
    dem_min_elev_m: f64,
    dem_max_elev_m: f64,
) -> Vec<ChunkId> {
    use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

    // terrain.rs places surface voxels at: origin_voxel.y + (dem_elev - origin_gps.alt)
    // so regions with large local relief need a Y range derived from the DEM,
    // not the old “one chunk below origin” approximation that only works for
    // relatively flat terrain.
    let min_surface_y = origin_voxel.y as f64 + (dem_min_elev_m - origin_gps.alt);
    let max_surface_y = origin_voxel.y as f64 + (dem_max_elev_m - origin_gps.alt);
    let min_chunk_y = (min_surface_y.floor() as i64).div_euclid(CHUNK_SIZE_Y) - 1;
    let max_chunk_y = (max_surface_y.ceil() as i64).div_euclid(CHUNK_SIZE_Y)
        + extra_y_layers as i64;

    // Corners → voxel space → chunk XZ range.
    // High-relief regions can shift X/Z noticeably with altitude in ECEF space,
    // so cover both the DEM minimum and maximum elevations rather than assuming
    // the whole bbox lives at the origin altitude.
    let sample_alts = if (dem_max_elev_m - dem_min_elev_m).abs() < f64::EPSILON {
        vec![origin_gps.alt]
    } else {
        vec![dem_min_elev_m, origin_gps.alt, dem_max_elev_m]
    };

    let mut min_cx = i64::MAX;
    let mut max_cx = i64::MIN;
    let mut min_cz = i64::MAX;
    let mut max_cz = i64::MIN;

    for alt_m in sample_alts {
        let corners = [
            GPS::new(bounds.lat_min, bounds.lon_min, alt_m),
            GPS::new(bounds.lat_min, bounds.lon_max, alt_m),
            GPS::new(bounds.lat_max, bounds.lon_min, alt_m),
            GPS::new(bounds.lat_max, bounds.lon_max, alt_m),
        ];

        for gps in &corners {
            let ecef = gps.to_ecef();
            let vox = VoxelCoord::from_ecef(&ecef);
            let cx = vox.x.div_euclid(CHUNK_SIZE_X);
            let cz = vox.z.div_euclid(CHUNK_SIZE_Z);
            min_cx = min_cx.min(cx);
            max_cx = max_cx.max(cx);
            min_cz = min_cz.min(cz);
            max_cz = max_cz.max(cz);
        }
    }

    let mut ids = Vec::new();
    for cy in min_chunk_y..=max_chunk_y {
        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                ids.push(ChunkId::new(cx, cy, cz));
            }
        }
    }
    ids
}

// ─── Chunk serialisation ──────────────────────────────────────────────────────

/// Serialise an octree plus optional smooth-surface cache to the TileStore wire format.
/// The payload format is handled by `chunk_loader::encode_stored_chunk` so baked
/// chunks can preserve exact fractional terrain heights across cache reloads.
pub fn serialise_chunk(
    octree: &crate::voxel::Octree,
    surface_cache: Option<&crate::terrain::SurfaceCache>,
) -> Result<Vec<u8>, String> {
    crate::chunk_loader::encode_stored_chunk(octree, surface_cache)
}

fn fnv1a_hex(data: &[u8]) -> String {
    // FNV-1a 64-bit hash for manifest integrity tagging (not crypto).
    let mut h: u64 = 14695981039346656037;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

fn preferred_output_pass(cfg: &WorldgenConfig) -> PassId {
    if !cfg.skip_osm && cfg.osm_cache.is_some() {
        if !cfg.skip_roads {
            return PassId::Roads;
        }
        if !cfg.skip_water {
            return PassId::Hydro;
        }
    }
    PassId::Terrain
}

// ─── Region generation ────────────────────────────────────────────────────────

/// Progress callback called after each chunk is written.
/// Args: (chunks_done, chunks_total, last_chunk_id)
pub type ProgressFn = Arc<dyn Fn(u64, u64, &ChunkId) + Send + Sync>;

/// Generate an entire region and write chunks into a TileStore.
///
/// The TileStore is written to `cfg.output_dir/tiles.db` unless `cfg.tile_store`
/// is pre-populated (useful when the caller wants to share a DB with the server).
/// A JSON manifest is written to `cfg.output_dir/manifest.json` on completion.
///
/// `terrain_gen` must already have `bake_buildings = true` set if you want
/// building voxels in the output.
pub fn generate_region(
    cfg: &WorldgenConfig,
    terrain_gen: Arc<TerrainGenerator>,
    origin_gps: &GPS,
    origin_voxel: &VoxelCoord,
    progress: Option<ProgressFn>,
) -> Result<RegionManifest, String> {
    std::fs::create_dir_all(&cfg.output_dir)
        .map_err(|e| format!("Cannot create output dir: {e}"))?;

    // Open or reuse the TileStore for this region.
    let ts: Arc<TileStore> = match cfg.tile_store.clone() {
        Some(existing) => existing,
        None => Arc::new(
            TileStore::open(&cfg.output_dir.join("tiles.db"))
                .map_err(|e| format!("TileStore open failed: {e}"))?,
        ),
    };

    // Compute (or reuse) terrain analysis before entering the parallel loop.
    let analysis: Option<Arc<TerrainAnalysis>> = if let Some(ref a) = cfg.analysis {
        Some(Arc::clone(a))
    } else {
        let pipeline_arc = terrain_gen.elevation_pipeline();
        let pipeline = pipeline_arc
            .read()
            .map_err(|e| format!("elevation pipeline lock: {e}"))?;
        let step = 0.0003_f64; // ~30 m
        let dem = RegionDem::sample_region(
            &*pipeline,
            cfg.region.lat_min,
            cfg.region.lat_max,
            cfg.region.lon_min,
            cfg.region.lon_max,
            step,
        );
        drop(pipeline);

        // Sea-level datum calibration: find the lowest-elevation cells in the DEM.
        // For any coastal region, those cells are near 0 m orthometric (sea level).
        // Compute a median-based correction and apply it to all subsequent queries.
        // For inland regions the lowest cells will be well above sea level, so the
        // plausibility gate (-8 m to 12 m) prevents spurious corrections.
        {
            let step = dem.cell_size_deg;
            let mut low_elevs: Vec<f64> = dem
                .elevations
                .iter()
                .enumerate()
                .filter_map(|(idx, &e)| {
                    if e < -50.0 {
                        return None;
                    } // exclude SRTM void artefacts
                    let row = idx / dem.cols;
                    let col = idx % dem.cols;
                    let lat = dem.lat_min + (row as f64 + 0.5) * step;
                    let lon = dem.lon_min + (col as f64 + 0.5) * step;
                    // Only consider cells plausibly near the coast: low elevation AND
                    // not obviously inland (use a generous ±0.5° buffer from region edge).
                    let near_edge = lat < dem.lat_min + 0.5
                        || lat > dem.lat_max - 0.5
                        || lon < dem.lon_min + 0.5
                        || lon > dem.lon_max - 0.5;
                    if near_edge || (e as f64) < 20.0 {
                        Some(e as f64)
                    } else {
                        None
                    }
                })
                .collect();
            low_elevs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            // Take median of the bottom 2%, capped at 30 samples.
            let n = (low_elevs.len() / 50).max(1).min(30);
            if let Some(&median) = low_elevs.get(n / 2) {
                if median > -8.0 && median < 12.0 {
                    // Plausibly coastal — calibrate.
                    let offset = pipeline_arc
                        .write()
                        .map_err(|e| format!("elevation pipeline lock: {e}"))?
                        .calibrate_from_elevations(&low_elevs[..n]);
                    if offset.abs() > 0.3 {
                        eprintln!(
                            "[worldgen] datum calibrated: {:.2} m offset (DEM min median {:.2} m, {} samples)",
                            offset, median, n
                        );
                    }
                }
            }
        }

        eprintln!("[worldgen] terrain analysis: computing derived rasters …");
        let mut analysis = TerrainAnalysis::compute(dem);
        if let Some(ref gshhg_path) = cfg.gshhg_path {
            analysis.compute_coastal_dist(gshhg_path);
        } else {
            eprintln!(
                "[worldgen] no GSHHG coastline file — coastal substrate disabled (use --coastline)"
            );
        }
        // Collect reservoir polygons from OSM and mark them in the mask.
        if let Some(ref osm_cache) = cfg.osm_cache {
            let mut water_polys: Vec<crate::osm::OsmWater> = Vec::new();
            let mut land_areas: Vec<crate::osm::OsmLandArea> = Vec::new();
            let mut aeroways: Vec<crate::osm::OsmAeroway> = Vec::new();
            const TILE: f64 = 0.01;
            let tile_lat_start = (cfg.region.lat_min / TILE).floor() as i64;
            let tile_lat_end = (cfg.region.lat_max / TILE).floor() as i64;
            let tile_lon_start = (cfg.region.lon_min / TILE).floor() as i64;
            let tile_lon_end = (cfg.region.lon_max / TILE).floor() as i64;

            for tile_lat in tile_lat_start..=tile_lat_end {
                let lat = tile_lat as f64 * TILE;
                for tile_lon in tile_lon_start..=tile_lon_end {
                    let lon = tile_lon as f64 * TILE;
                    if let Some(tile) = osm_cache.load(lat, lon, lat + TILE, lon + TILE) {
                        water_polys.extend(tile.water);
                        land_areas.extend(tile.land_areas);
                        aeroways.extend(tile.aeroways);
                    }
                }
            }
            if !water_polys.is_empty() {
                eprintln!(
                    "[worldgen] computing reservoir mask from {} water polygons …",
                    water_polys.len()
                );
                analysis.compute_reservoirs(&water_polys);
            }
            if !land_areas.is_empty() || !aeroways.is_empty() {
                eprintln!(
                    "[worldgen] computing OSM landuse and engineered-ground masks from {} land areas and {} aeroways …",
                    land_areas.len(),
                    aeroways.len()
                );
                if !land_areas.is_empty() {
                    analysis.compute_osm_landuse(&land_areas);
                }
                analysis.compute_engineered_ground_with_aeroways(&land_areas, &aeroways);
            }
        }
        Some(Arc::new(analysis))
    };

    let (dem_min_elev_m, dem_max_elev_m) = analysis
        .as_ref()
        .map(|analysis| {
            analysis
                .dem
                .elevations
                .iter()
                .copied()
                .filter(|e| e.is_finite() && *e > -1000.0 && *e < 10000.0)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min_e, max_e), e| {
                    (min_e.min(e as f64), max_e.max(e as f64))
                })
        })
        .filter(|(min_e, max_e)| min_e.is_finite() && max_e.is_finite())
        .unwrap_or((origin_gps.alt, origin_gps.alt));

    let chunk_ids = enumerate_surface_chunks(
        &cfg.region,
        origin_gps,
        origin_voxel,
        cfg.extra_y_layers,
        dem_min_elev_m,
        dem_max_elev_m,
    );
    let total = chunk_ids.len() as u64;

    eprintln!(
        "[worldgen] {} chunks to generate → {:?}/tiles.db (DEM {:.1}m..{:.1}m)",
        total, cfg.output_dir, dem_min_elev_m, dem_max_elev_m
    );

    // Terrain generation must use the same derived analysis that later OSM/hydro
    // stages see, otherwise terrain shaping masks are computed but never applied.
    let terrain_gen = Arc::new(terrain_gen.as_ref().clone_with_analysis(analysis.clone()));

    // Build river profiles once for the whole region (Tier 4a).
    // This requires both OSM cache and terrain analysis to be available.
    let river_profiles: Option<Arc<crate::worldgen_river::RiverProfileCache>> =
        if let (Some(osm_cache), Some(analysis_arc)) = (&cfg.osm_cache, &analysis) {
            eprintln!("[worldgen] building river profiles …");
            let elev_arc = terrain_gen.elevation_pipeline();
            let cache = crate::worldgen_river::RiverProfileCache::build(
                cfg.region.lat_min,
                cfg.region.lat_max,
                cfg.region.lon_min,
                cfg.region.lon_max,
                osm_cache,
                elev_arc.as_ref(),
                Some(analysis_arc.as_ref()),
            );
            eprintln!(
                "[worldgen] river profiles: {} segments",
                cache.segments.len()
            );
            Some(Arc::new(cache))
        } else {
            None
        };

    // Build a rayon thread pool with the requested worker count.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.workers)
        .build()
        .map_err(|e| format!("Rayon pool error: {e}"))?;

    let done = Arc::new(AtomicU64::new(0));
    let results = Arc::new(Mutex::new(Vec::<ManifestChunk>::new()));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let timing = Arc::new(Mutex::new((0u64, 0u64, 0u64)));
    let run_start = std::time::Instant::now();
    let target_pass = preferred_output_pass(cfg);

    pool.install(|| {
        chunk_ids.par_iter().for_each(|id| {
            let cx = id.x as i32;
            let cy = id.y as i32;
            let cz = id.z as i32;

            // Resume support: skip chunks already in the DB.
            if ts.has_chunk_pass(cx, cy, cz, target_pass) {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref p) = progress { p(n, total, id); }
                if let Some(data) = ts.get_chunk_pass(cx, cy, cz, target_pass) {
                    results.lock().unwrap().push(ManifestChunk {
                        chunk_id: format!("{},{},{}", id.x, id.y, id.z),
                        size: data.len() as u64,
                        sha256: fnv1a_hex(&data),
                    });
                }
                return;
            }

            let t_gen = std::time::Instant::now();
            match terrain_gen.generate_chunk(id) {
                Ok((terrain_octree, surface_cache)) => {
                    let gen_ms = t_gen.elapsed().as_millis() as u64;
                    let t_ser = std::time::Instant::now();
                    match serialise_chunk(&terrain_octree, Some(&surface_cache)) {
                        Ok(terrain_data) => {
                            ts.put_chunk_pass(cx, cy, cz, PassId::Terrain, &terrain_data);

                            let mut final_pass = PassId::Terrain;
                            let mut final_data = terrain_data;

                            if !cfg.skip_osm {
                                if let Some(ref osm_cache) = cfg.osm_cache {
                                    let mut hydro_octree = terrain_octree.clone();

                                    if !cfg.skip_water {
                                        let mut processor = crate::worldgen_osm::OsmProcessor::new(
                                            Arc::clone(osm_cache),
                                            *origin_gps,
                                            *origin_voxel,
                                            analysis.clone(),
                                        );
                                        if let Some(ref rp) = river_profiles {
                                            processor = processor.with_river_profiles(Arc::clone(rp));
                                        }
                                        processor.apply_to_chunk(id, &mut hydro_octree);
                                        if let Ok(hydro_data) =
                                            serialise_chunk(&hydro_octree, Some(&surface_cache))
                                        {
                                            ts.put_chunk_pass(cx, cy, cz, PassId::Hydro, &hydro_data);
                                            final_pass = PassId::Hydro;
                                            final_data = hydro_data;
                                        }
                                    }

                                    if !cfg.skip_roads {
                                        let mut roads_octree = hydro_octree.clone();
                                        let road_proc = crate::worldgen_roads::RoadProcessor::new(
                                            Arc::clone(osm_cache),
                                            *origin_voxel,
                                            *origin_gps,
                                        )
                                        .with_elevation(terrain_gen.elevation_pipeline());
                                        road_proc.apply_to_chunk(id, &mut roads_octree);
                                        if let Ok(roads_data) =
                                            serialise_chunk(&roads_octree, Some(&surface_cache))
                                        {
                                            ts.put_chunk_pass(cx, cy, cz, PassId::Roads, &roads_data);
                                            final_pass = PassId::Roads;
                                            final_data = roads_data;
                                        }
                                    }
                                } // end osm_cache
                            } // end skip_osm

                            let ser_ms = t_ser.elapsed().as_millis() as u64;
                            if final_pass != target_pass {
                                errors.lock().unwrap().push(format!(
                                    "{:?}: target pass {:?} not generated, final pass {:?}",
                                    id, target_pass, final_pass
                                ));
                            }
                            results.lock().unwrap().push(ManifestChunk {
                                chunk_id: format!("{},{},{}", id.x, id.y, id.z),
                                size: final_data.len() as u64,
                                sha256: fnv1a_hex(&final_data),
                            });
                            let mut t = timing.lock().unwrap();
                            t.0 += gen_ms; t.1 += ser_ms; t.2 += 1;
                            if cfg.verbose {
                                eprintln!("[chunk] {:?}  gen={gen_ms}ms  ser={ser_ms}ms", id);
                            }
                        }
                        Err(e) => errors.lock().unwrap().push(format!("{:?}: serialise: {e}", id)),
                    }
                }
                Err(e) => errors.lock().unwrap().push(format!("{:?}: generate: {e}", id)),
            }

            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 50 == 0 {
                let elapsed = run_start.elapsed().as_secs_f64();
                let rate = n as f64 / elapsed * 60.0;
                let t = timing.lock().unwrap();
                let (avg_gen, avg_ser) = if t.2 > 0 { (t.0/t.2, t.1/t.2) } else { (0,0) };
                eprintln!(
                    "[worldgen] {n}/{total} ({:.1}%)  {rate:.1} chunks/min  avg gen={avg_gen}ms ser={avg_ser}ms",
                    n as f64 / total as f64 * 100.0
                );
            }
            if let Some(ref p) = progress { p(n, total, id); }
        });
    });

    let errs = errors.lock().unwrap();
    if !errs.is_empty() {
        eprintln!("[worldgen] {} errors:", errs.len());
        for e in errs.iter().take(20) {
            eprintln!("  {e}");
        }
    }

    let chunks: Vec<ManifestChunk> = {
        let mut v = results.lock().unwrap().drain(..).collect::<Vec<_>>();
        v.sort_by(|a, b| a.chunk_id.cmp(&b.chunk_id));
        v
    };

    let generated_at = {
        let dur = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}", dur.as_secs())
    };

    let manifest = RegionManifest {
        format_version: 1,
        terrain_cache_version: TERRAIN_CACHE_VERSION,
        region: cfg.region.clone(),
        origin_gps: [origin_gps.lat, origin_gps.lon, origin_gps.alt],
        generated_at,
        chunk_count: chunks.len(),
        chunks,
    };

    let manifest_path = cfg.output_dir.join("manifest.json");
    let json =
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("manifest json: {e}"))?;
    std::fs::write(&manifest_path, json).map_err(|e| format!("manifest write: {e}"))?;

    eprintln!(
        "[worldgen] Done. {} chunks in tiles.db, {} errors. Manifest: {:?}",
        manifest.chunk_count,
        errs.len(),
        manifest_path
    );

    Ok(manifest)
}
