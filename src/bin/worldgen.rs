/// worldgen — offline world pre-baking tool
///
/// Generates terrain + building voxel chunks for a geographic region and
/// writes them in the same binary format as the live chunk cache, so the
/// client/server can serve them directly via P2P without re-generating.
///
/// Usage:
///   worldgen [OPTIONS]
///
/// Options:
///   --region <name>          Named region (brisbane, brisbane-cbd, test) [default: brisbane-cbd]
///   --bbox <lat_min,lon_min,lat_max,lon_max>  Custom bounding box (overrides --region)
///   --output <dir>           Output directory [default: ./world_data/worldgen]
///   --workers <n>            Parallel workers [default: logical CPU count]
///   --extra-layers <n>       Extra Y chunk layers above surface for tall buildings [default: 1]
///   --srtm-dir <dir>         SRTM tile cache directory [default: ./world_data/srtm]
///   --osm-cache <dir>        OSM tile cache for waterway carving [default: ./world_data/osm]
///   --resume                 Skip already-generated chunks (default: enabled)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, SkadiElevationSource, egm96_undulation},
    osm::OsmDiskCache,
    terrain::TerrainGenerator,
    tile_store::TileStore,
    voxel::VoxelCoord,
    worldgen::{RegionBounds, WorldgenConfig, generate_region},
};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // ── Parse CLI args ──────────────────────────────────────────────────────
    let mut region_name   = "brisbane-cbd".to_string();
    let mut custom_bbox: Option<RegionBounds> = None;
    let mut output_dir    = PathBuf::from("world_data/worldgen");
    let mut workers       = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let mut extra_layers  = 2i32; // origin-1 (river), origin (surface), origin+1 (buildings)
    let mut srtm_dir      = PathBuf::from("world_data/srtm");
    let mut osm_cache_dir = PathBuf::from("world_data/osm");
    let mut verbose = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--region" => {
                i += 1;
                region_name = args[i].clone();
            }
            "--bbox" => {
                i += 1;
                let parts: Vec<f64> = args[i].split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect();
                if parts.len() != 4 {
                    eprintln!("--bbox expects lat_min,lon_min,lat_max,lon_max");
                    std::process::exit(1);
                }
                custom_bbox = Some(RegionBounds::new(parts[0], parts[2], parts[1], parts[3]));
            }
            "--output" => {
                i += 1;
                output_dir = PathBuf::from(&args[i]);
            }
            "--workers" => {
                i += 1;
                workers = args[i].parse().unwrap_or(workers);
            }
            "--extra-layers" => {
                i += 1;
                extra_layers = args[i].parse().unwrap_or(extra_layers);
            }
            "--srtm-dir" => {
                i += 1;
                srtm_dir = PathBuf::from(&args[i]);
            }
            "--osm-cache" => {
                i += 1;
                osm_cache_dir = PathBuf::from(&args[i]);
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            "--help" | "-h" => {
                print_help();
                return;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_help();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let region = if let Some(bbox) = custom_bbox {
        bbox
    } else {
        match RegionBounds::named(&region_name) {
            Some(r) => r,
            None => {
                eprintln!(
                    "Unknown region '{}'. Known: brisbane, brisbane-cbd, test\n\
                     Use --bbox lat_min,lon_min,lat_max,lon_max for a custom area.",
                    region_name
                );
                std::process::exit(1);
            }
        }
    };

    // ── Compute origin GPS (same formula as metaworld_alpha.rs) ────────────
    // Origin is Kangaroo Point, Brisbane — same as the client spawn point.
    // We query SRTM at the origin to get the true ellipsoidal altitude so that
    // surface_delta = 0 at spawn → terrain is placed at the correct absolute voxel Y.
    let base_lat = -27.4672_f64;
    let base_lon = 153.0300_f64;
    let mut init_elev = ElevationPipeline::new();
    init_elev.add_source(Box::new(SkadiElevationSource::new(srtm_dir.clone())));
    let srtm_origin = init_elev
        .query(&GPS::new(base_lat, base_lon, 0.0))
        .map(|e| e.meters)
        .unwrap_or(26.0); // Kangaroo Point ~26 m orthometric fallback
    let n_origin  = egm96_undulation(base_lat, base_lon);
    let origin_gps   = GPS::new(base_lat, base_lon, srtm_origin + n_origin);
    let origin_ecef  = origin_gps.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    eprintln!("[worldgen] Origin GPS: ({:.6}, {:.6}, {:.1}m ell / {:.1}m ortho)",
        origin_gps.lat, origin_gps.lon, origin_gps.alt, srtm_origin);

    // ── Set up elevation + terrain ──────────────────────────────────────────
    let mut elevation = ElevationPipeline::new();
    elevation.add_source(Box::new(SkadiElevationSource::new(srtm_dir.clone())));

    let terrain_gen = Arc::new(TerrainGenerator::new(elevation, origin_gps.clone(), origin_voxel));

    // ── Open OSM cache if available ─────────────────────────────────────────
    let osm_cache = if osm_cache_dir.exists() {
        Some(Arc::new(OsmDiskCache::new(&osm_cache_dir)))
    } else {
        eprintln!("[worldgen] OSM cache dir not found ({:?}); waterway carving disabled", osm_cache_dir);
        None
    };

    // ── Worldgen config ─────────────────────────────────────────────────────
    // Open the output TileStore up front so we can report the path clearly.
    std::fs::create_dir_all(&output_dir).expect("Cannot create output dir");
    let tile_store = Arc::new(
        TileStore::open(&output_dir.join("tiles.db"))
            .expect("Failed to open output TileStore")
    );

    let cfg = WorldgenConfig {
        region: region.clone(),
        output_dir: output_dir.clone(),
        workers,
        extra_y_layers: extra_layers,
        report_interval: 100,
        verbose,
        tile_store: Some(Arc::clone(&tile_store)),
        osm_cache,
        analysis: None,
    };

    eprintln!("[worldgen] Region: {:?}", region);
    eprintln!("[worldgen] Output: {:?}/tiles.db", output_dir);
    eprintln!("[worldgen] Workers: {workers}");
    eprintln!("[worldgen] Extra Y layers: {extra_layers}");

    let start = Instant::now();

    // ── Progress callback ───────────────────────────────────────────────────
    let progress = Arc::new(|done: u64, total: u64, _id: &metaverse_core::chunk::ChunkId| {
        if done % 100 == 0 || done == total {
            let pct = done as f64 / total as f64 * 100.0;
            eprint!("\r[worldgen] {done}/{total} ({pct:.1}%)  ");
        }
    });

    match generate_region(&cfg, terrain_gen, &origin_gps, &origin_voxel, Some(progress)) {
        Ok(manifest) => {
            eprintln!(
                "\n[worldgen] Complete in {:.1}s — {} chunks",
                start.elapsed().as_secs_f64(),
                manifest.chunk_count
            );
        }
        Err(e) => {
            eprintln!("\n[worldgen] Failed: {e}");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    eprintln!("\
worldgen — offline metaverse world pre-baking tool

USAGE:
    worldgen [OPTIONS]

OPTIONS:
    --region <name>       Named region (brisbane, brisbane-cbd, test) [default: brisbane-cbd]
    --bbox <coords>       Custom bounding box: lat_min,lon_min,lat_max,lon_max
    --output <dir>        Output directory [default: ./world_data/worldgen]
    --workers <n>         Parallel workers [default: CPU count]
    --extra-layers <n>    Extra Y layers above surface for tall buildings [default: 1]
    --srtm-dir <dir>      SRTM tile cache directory [default: ./world_data/srtm]
    --osm-cache <dir>     OSM tile cache for waterway carving [default: ./world_data/osm]
    --help                Show this help

EXAMPLES:
    worldgen --region test
    worldgen --region brisbane --output /mnt/data/worldgen --workers 8
    worldgen --bbox -27.48,-27.45,153.01,153.04 --output ./test_region
");
}
