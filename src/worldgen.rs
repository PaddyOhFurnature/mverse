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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};

use crate::terrain_analysis::{RegionDem, TerrainAnalysis};

use serde::{Serialize, Deserialize};
use rayon::prelude::*;

use crate::chunk::ChunkId;
use crate::chunk_loader::TERRAIN_CACHE_VERSION;
use crate::coordinates::GPS;
use crate::voxel::VoxelCoord;
use crate::terrain::TerrainGenerator;
use crate::tile_store::{TileStore, PassId};

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
        Self { lat_min, lat_max, lon_min, lon_max }
    }

    /// A few pre-defined named regions.
    pub fn named(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "brisbane" => Some(Self::new(-27.70, -27.20, 152.70, 153.30)),
            "brisbane-cbd" => Some(Self::new(-27.50, -27.44, 153.00, 153.05)),
            "brisbane-small" | "test" => Some(Self::new(-27.48, -27.45, 153.01, 153.04)),
            _ => None,
        }
    }
}

/// Per-chunk entry in the manifest (used for incremental updates & P2P verification).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestChunk {
    pub chunk_id: String,     // "x,y,z"
    pub size:     u64,        // bytes
    pub sha256:   String,     // hex
}

/// Written to `manifest.json` in the output directory on completion.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegionManifest {
    pub format_version: u32,
    pub terrain_cache_version: u32,
    pub region: RegionBounds,
    pub origin_gps: [f64; 3],     // [lat, lon, alt]
    pub generated_at: String,     // RFC3339 UTC
    pub chunk_count: usize,
    pub chunks: Vec<ManifestChunk>,
}

/// Configuration for a worldgen run.
pub struct WorldgenConfig {
    pub region:      RegionBounds,
    /// Directory for the manifest JSON and the `tiles.db` TileStore output.
    pub output_dir:  PathBuf,
    pub workers:     usize,
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
) -> Vec<ChunkId> {
    use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Y, CHUNK_SIZE_Z};

    // terrain.rs places surface voxels at: origin_voxel.y + (srtm_ell - origin_gps.alt)
    // Highest terrain (at spawn altitude) → surface at origin_voxel.y
    // Lowest terrain (river/sea, ~57 voxels below origin) → origin_voxel.y - 57
    //
    // Start one chunk below origin_chunk_y so low-lying terrain (river level)
    // is captured. extra_y_layers adds chunks above for buildings.
    let origin_chunk_y = origin_voxel.y.div_euclid(CHUNK_SIZE_Y);
    let surface_chunk_y = origin_chunk_y - 1;

    // Corners → voxel space → chunk XZ range.
    let corners = [
        GPS::new(bounds.lat_min, bounds.lon_min, origin_gps.alt),
        GPS::new(bounds.lat_min, bounds.lon_max, origin_gps.alt),
        GPS::new(bounds.lat_max, bounds.lon_min, origin_gps.alt),
        GPS::new(bounds.lat_max, bounds.lon_max, origin_gps.alt),
    ];

    let mut min_cx = i64::MAX;
    let mut max_cx = i64::MIN;
    let mut min_cz = i64::MAX;
    let mut max_cz = i64::MIN;

    for gps in &corners {
        let ecef = gps.to_ecef();
        let vox  = VoxelCoord::from_ecef(&ecef);
        let dx = vox.x - origin_voxel.x;
        let dz = vox.z - origin_voxel.z;
        let cx = dx.div_euclid(CHUNK_SIZE_X) + origin_voxel.x.div_euclid(CHUNK_SIZE_X);
        let cz = dz.div_euclid(CHUNK_SIZE_Z) + origin_voxel.z.div_euclid(CHUNK_SIZE_Z);
        min_cx = min_cx.min(cx);
        max_cx = max_cx.max(cx);
        min_cz = min_cz.min(cz);
        max_cz = max_cz.max(cz);
    }

    let mut ids = Vec::new();
    for cy in surface_chunk_y..=(surface_chunk_y + extra_y_layers as i64) {
        for cx in min_cx..=max_cx {
            for cz in min_cz..=max_cz {
                ids.push(ChunkId::new(cx, cy, cz));
            }
        }
    }
    ids
}

// ─── Chunk serialisation ──────────────────────────────────────────────────────

/// Serialise an octree to the TileStore wire format:
///   `[u32 TERRAIN_CACHE_VERSION LE][bincode Octree bytes]`
/// (The TileStore then wraps this with a blake3 checksum internally.)
pub fn serialise_chunk(octree: &crate::voxel::Octree) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&TERRAIN_CACHE_VERSION.to_le_bytes());
    let encoded = bincode::serialize(octree)
        .map_err(|e| format!("bincode error: {e}"))?;
    buf.extend_from_slice(&encoded);
    Ok(buf)
}

fn fnv1a_hex(data: &[u8]) -> String {
    // FNV-1a 64-bit hash for manifest integrity tagging (not crypto).
    let mut h: u64 = 14695981039346656037;
    for &b in data { h ^= b as u64; h = h.wrapping_mul(1099511628211); }
    format!("{h:016x}")
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
        None => Arc::new(TileStore::open(&cfg.output_dir.join("tiles.db"))
            .map_err(|e| format!("TileStore open failed: {e}"))?),
    };

    let chunk_ids = enumerate_surface_chunks(
        &cfg.region, origin_gps, origin_voxel, cfg.extra_y_layers,
    );
    let total = chunk_ids.len() as u64;

    eprintln!("[worldgen] {} chunks to generate → {:?}/tiles.db", total, cfg.output_dir);

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
        eprintln!("[worldgen] terrain analysis: computing derived rasters …");
        let mut analysis = TerrainAnalysis::compute(dem);
        if let Some(ref gshhg_path) = cfg.gshhg_path {
            analysis.compute_coastal_dist(gshhg_path);
        } else {
            eprintln!("[worldgen] no GSHHG coastline file — coastal substrate disabled (use --coastline)");
        }
        Some(Arc::new(analysis))
    };

    // Build river profiles once for the whole region (Tier 4a).
    // This requires both OSM cache and terrain analysis to be available.
    let river_profiles: Option<Arc<crate::worldgen_river::RiverProfileCache>> =
        if let (Some(osm_cache), Some(analysis_arc)) = (&cfg.osm_cache, &analysis) {
            eprintln!("[worldgen] building river profiles …");
            let elev_arc = terrain_gen.elevation_pipeline();
            let cache = crate::worldgen_river::RiverProfileCache::build(
                cfg.region.lat_min, cfg.region.lat_max,
                cfg.region.lon_min, cfg.region.lon_max,
                osm_cache,
                elev_arc.as_ref(),
                Some(analysis_arc.as_ref()),
            );
            eprintln!("[worldgen] river profiles: {} segments", cache.segments.len());
            Some(Arc::new(cache))
        } else {
            None
        };

    // Build a rayon thread pool with the requested worker count.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.workers)
        .build()
        .map_err(|e| format!("Rayon pool error: {e}"))?;

    let done    = Arc::new(AtomicU64::new(0));
    let results = Arc::new(Mutex::new(Vec::<ManifestChunk>::new()));
    let errors  = Arc::new(Mutex::new(Vec::<String>::new()));
    let timing  = Arc::new(Mutex::new((0u64, 0u64, 0u64)));
    let run_start = std::time::Instant::now();

    pool.install(|| {
        chunk_ids.par_iter().for_each(|id| {
            let cx = id.x as i32;
            let cy = id.y as i32;
            let cz = id.z as i32;

            // Resume support: skip chunks already in the DB.
            if ts.has_chunk_pass(cx, cy, cz, PassId::Terrain) {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref p) = progress { p(n, total, id); }
                if let Some(data) = ts.get_chunk_pass(cx, cy, cz, PassId::Terrain) {
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
                Ok((mut octree, _surface_cache)) => {
                    // Apply OSM waterways/water-polygons when cache is available.
                    if let Some(ref osm_cache) = cfg.osm_cache {
                        let mut processor = crate::worldgen_osm::OsmProcessor::new(
                            Arc::clone(osm_cache),
                            *origin_gps,
                            *origin_voxel,
                            analysis.clone(),
                        );
                        if let Some(ref rp) = river_profiles {
                            processor = processor.with_river_profiles(Arc::clone(rp));
                        }
                        processor.apply_to_chunk(id, &mut octree);
                    }
                    let gen_ms = t_gen.elapsed().as_millis() as u64;
                    let t_ser = std::time::Instant::now();
                    match serialise_chunk(&octree) {
                        Ok(data) => {
                            let ser_ms = t_ser.elapsed().as_millis() as u64;
                            ts.put_chunk_pass(cx, cy, cz, PassId::Terrain, &data);
                            results.lock().unwrap().push(ManifestChunk {
                                chunk_id: format!("{},{},{}", id.x, id.y, id.z),
                                size: data.len() as u64,
                                sha256: fnv1a_hex(&data),
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
        for e in errs.iter().take(20) { eprintln!("  {e}"); }
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
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("manifest json: {e}"))?;
    std::fs::write(&manifest_path, json)
        .map_err(|e| format!("manifest write: {e}"))?;

    eprintln!(
        "[worldgen] Done. {} chunks in tiles.db, {} errors. Manifest: {:?}",
        manifest.chunk_count, errs.len(), manifest_path
    );

    Ok(manifest)
}
