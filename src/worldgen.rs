/// Offline world generation — pre-bakes terrain, buildings, roads and other
/// static OSM features into versioned chunk files that are served via P2P.
///
/// The output format is identical to the live chunk cache produced by
/// `ChunkLoader`, so the client transparently reads worldgen chunks from disk
/// (or downloaded via P2P) without any special-case logic.
///
/// # Architecture
/// 1.  `RegionBounds` — a geographic bounding box (lat/lon degrees)
/// 2.  `enumerate_surface_chunks` — returns every surface-layer ChunkId covering
///     the bbox; buildings/tall objects may need the Y+1 layer too.
/// 3.  `generate_region` — parallel Rayon loop; calls
///     `TerrainGenerator::generate_chunk` (with `bake_buildings=true`) for
///     each ChunkId, serialises result, writes to output dir.
/// 4.  `RegionManifest` — JSON summary written on completion; server uses this
///     to answer "what chunks do you have?" queries from peers.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}};

use serde::{Serialize, Deserialize};
use rayon::prelude::*;

use crate::chunk::ChunkId;
use crate::coordinates::GPS;
use crate::voxel::VoxelCoord;
use crate::terrain::TerrainGenerator;

/// Matches `WORLDGEN_CACHE_VERSION` in chunk_loader.rs — bump both together.
const WORLDGEN_CACHE_VERSION: u32 = 13;

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
    pub output_dir:  PathBuf,
    pub workers:     usize,
    /// Also generate Y+1 layer for tall buildings / multi-storey structures.
    pub extra_y_layers: i32,
    /// Print progress to stderr every `report_interval` chunks.
    pub report_interval: usize,
    /// Enable per-chunk timing output (prints generate/serialize times).
    pub verbose: bool,
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

    // Estimate surface chunk_y from origin.
    // We sample a GPS point at the centre of the region and compute which chunk
    // Y layer contains its ECEF Y coordinate.
    let centre_gps = GPS::new(
        (bounds.lat_min + bounds.lat_max) * 0.5,
        (bounds.lon_min + bounds.lon_max) * 0.5,
        origin_gps.alt,
    );
    let centre_ecef = centre_gps.to_ecef();
    let centre_vox  = VoxelCoord::from_ecef(&centre_ecef);

    // Chunk Y for the surface at the centre of the region.
    let surface_chunk_y = (centre_vox.y - origin_voxel.y).div_euclid(CHUNK_SIZE_Y)
        + (origin_voxel.y).div_euclid(CHUNK_SIZE_Y);

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

/// Serialise an octree to the same format as `ChunkLoader`'s disk cache:
///   `[u32 version][bincode Octree]`
pub fn serialise_chunk(octree: &crate::voxel::Octree) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    // 4-byte little-endian version header
    buf.extend_from_slice(&(WORLDGEN_CACHE_VERSION as u32).to_le_bytes());
    let encoded = bincode::serialize(octree)
        .map_err(|e| format!("bincode error: {e}"))?;
    buf.extend_from_slice(&encoded);
    Ok(buf)
}

fn chunk_cache_path(output_dir: &Path, id: &ChunkId) -> PathBuf {
    // Mirror ChunkLoader's path scheme: output_dir/<x>_<y>_<z>.bin
    output_dir.join(format!("{}_{}_{}.bin", id.x, id.y, id.z))
}

fn sha256_hex(data: &[u8]) -> String {
    // FNV-1a 64-bit hash for file integrity tagging (not crypto).
    let mut h: u64 = 14695981039346656037;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

// ─── Region generation ────────────────────────────────────────────────────────

/// Progress callback called after each chunk is written.
/// Args: (chunks_done, chunks_total, last_chunk_id)
pub type ProgressFn = Arc<dyn Fn(u64, u64, &ChunkId) + Send + Sync>;

/// Generate an entire region.  Returns the manifest.
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

    let chunk_ids = enumerate_surface_chunks(
        &cfg.region, origin_gps, origin_voxel, cfg.extra_y_layers,
    );
    let total = chunk_ids.len() as u64;

    eprintln!("[worldgen] {} chunks to generate across {:?}", total, cfg.output_dir);

    // Build a rayon thread pool with the requested worker count.
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(cfg.workers)
        .build()
        .map_err(|e| format!("Rayon pool error: {e}"))?;

    let done    = Arc::new(AtomicU64::new(0));
    let results = Arc::new(Mutex::new(Vec::<ManifestChunk>::new()));
    let errors  = Arc::new(Mutex::new(Vec::<String>::new()));
    // For rate tracking: (total_gen_ms, total_ser_ms, count)
    let timing  = Arc::new(Mutex::new((0u64, 0u64, 0u64)));
    let run_start = std::time::Instant::now();

    pool.install(|| {
        chunk_ids.par_iter().for_each(|id| {
            let out_path = chunk_cache_path(&cfg.output_dir, id);

            // Skip already-generated chunks (resume support).
            if out_path.exists() {
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref p) = progress {
                    p(n, total, id);
                }
                // Still collect manifest entry from existing file.
                if let Ok(data) = std::fs::read(&out_path) {
                    let entry = ManifestChunk {
                        chunk_id: format!("{},{},{}", id.x, id.y, id.z),
                        size: data.len() as u64,
                        sha256: sha256_hex(&data),
                    };
                    results.lock().unwrap().push(entry);
                }
                return;
            }

            let t_gen = std::time::Instant::now();
            match terrain_gen.generate_chunk(id) {
                Ok((octree, _surface_cache)) => {
                    let gen_ms = t_gen.elapsed().as_millis() as u64;
                    let t_ser = std::time::Instant::now();
                    match serialise_chunk(&octree) {
                        Ok(data) => {
                            let ser_ms = t_ser.elapsed().as_millis() as u64;
                            let entry = ManifestChunk {
                                chunk_id: format!("{},{},{}", id.x, id.y, id.z),
                                size: data.len() as u64,
                                sha256: sha256_hex(&data),
                            };
                            if let Err(e) = std::fs::write(&out_path, &data) {
                                errors.lock().unwrap().push(
                                    format!("{:?}: write error: {e}", id)
                                );
                            } else {
                                results.lock().unwrap().push(entry);
                                let mut t = timing.lock().unwrap();
                                t.0 += gen_ms; t.1 += ser_ms; t.2 += 1;
                                if cfg.verbose {
                                    eprintln!("[chunk] {:?}  gen={gen_ms}ms  ser={ser_ms}ms", id);
                                }
                            }
                        }
                        Err(e) => {
                            errors.lock().unwrap().push(format!("{:?}: serialise: {e}", id));
                        }
                    }
                }
                Err(e) => {
                    errors.lock().unwrap().push(format!("{:?}: generate: {e}", id));
                }
            }

            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            // Print rolling rate stats every 50 generated chunks
            if n % 50 == 0 {
                let elapsed = run_start.elapsed().as_secs_f64();
                let rate = n as f64 / elapsed * 60.0;
                let t = timing.lock().unwrap();
                let (avg_gen, avg_ser) = if t.2 > 0 {
                    (t.0 / t.2, t.1 / t.2)
                } else { (0, 0) };
                eprintln!(
                    "[worldgen] {n}/{total} ({:.1}%)  {rate:.1} chunks/min  avg gen={avg_gen}ms ser={avg_ser}ms",
                    n as f64 / total as f64 * 100.0
                );
            }
            if let Some(ref p) = progress {
                p(n, total, id);
            }
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
        terrain_cache_version: WORLDGEN_CACHE_VERSION as u32,
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
        "[worldgen] Done. {} chunks written, {} errors. Manifest: {:?}",
        manifest.chunk_count, errs.len(), manifest_path
    );

    Ok(manifest)
}
