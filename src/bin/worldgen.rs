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
///   --srtm-dir <dir>         DEM cache directory [default: ./world_data/srtm]
///   --cop30-dir <dir>        Local bulk COP30/Copernicus GeoTIFF directory
///   --osm-cache <dir>        OSM tile cache for waterway carving [default: ./world_data/osm]
///   --coastline <file>       GSHHG binary coastline file for coastal substrate (e.g. gshhs_f.b)
///   --no-vegetation          Explicitly disable tree/shrub placement (default)
///   --with-vegetation        Re-enable tree/shrub placement
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use metaverse_core::{
    coordinates::GPS,
    elevation::{egm96_undulation, make_offline_elevation_pipeline},
    osm::OsmDiskCache,
    terrain::TerrainGenerator,
    tile_store::TileStore,
    voxel::VoxelCoord,
    worldgen::{RegionBounds, WorldgenConfig, generate_region},
};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // ── Parse CLI args ──────────────────────────────────────────────────────
    let mut region_name = "brisbane-cbd".to_string();
    let mut custom_bbox: Option<RegionBounds> = None;
    let mut output_dir = PathBuf::from("world_data/worldgen");
    let mut workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let mut extra_layers = 2i32;
    let mut srtm_dir = PathBuf::from("world_data/srtm");
    let mut cop30_dir: Option<PathBuf> = None;
    let mut osm_cache_dir = PathBuf::from("world_data/osm");
    let mut osm_db_path: Option<PathBuf> = None; // --osm-db overrides derived tiles.db path
    let mut gshhg_path: Option<PathBuf> = None;
    let mut verbose = false;
    let mut no_osm = false;
    let mut no_vegetation = true;
    let mut no_roads = false;
    let mut no_water = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--region" => {
                i += 1;
                region_name = args[i].clone();
            }
            "--bbox" => {
                i += 1;
                let parts: Vec<f64> = args[i]
                    .split(',')
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
            "--cop30-dir" => {
                i += 1;
                cop30_dir = Some(PathBuf::from(&args[i]));
            }
            "--osm-cache" => {
                i += 1;
                osm_cache_dir = PathBuf::from(&args[i]);
            }
            "--osm-db" => {
                i += 1;
                osm_db_path = Some(PathBuf::from(&args[i]));
            }
            "--coastline" => {
                i += 1;
                gshhg_path = Some(PathBuf::from(&args[i]));
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            "--no-osm" => {
                no_osm = true;
            }
            "--no-vegetation" => {
                no_vegetation = true;
            }
            "--with-vegetation" => {
                no_vegetation = false;
            }
            "--no-roads" => {
                no_roads = true;
            }
            "--no-water" => {
                no_water = true;
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

    if let Some(dir) = &cop30_dir {
        if !dir.exists() {
            eprintln!(
                "[worldgen] --cop30-dir {:?} does not exist; provide the bulk Copernicus directory",
                dir
            );
            std::process::exit(1);
        }
    }

    // ── Compute origin GPS ──────────────────────────────────────────────────
    // Origin is the geographic centre of the requested region.
    // Both worldgen and clients must use the same formula so chunk IDs align.
    // The computed origin_gps is stored in the region manifest so clients can
    // discover it without hardcoded coordinates.
    let (base_lat, base_lon) = region.center();
    let init_elev = make_offline_elevation_pipeline(srtm_dir.clone(), cop30_dir.clone());
    let srtm_origin = init_elev
        .query_with_fill(&GPS::new(base_lat, base_lon, 0.0))
        .map(|e| e.meters)
        .unwrap_or(0.0); // sea-level fallback if SRTM unavailable
    let n_origin = egm96_undulation(base_lat, base_lon);
    let origin_gps = GPS::new(base_lat, base_lon, srtm_origin + n_origin);
    let origin_ecef = origin_gps.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    eprintln!(
        "[worldgen] Origin GPS: ({:.6}, {:.6}, {:.1}m ell / {:.1}m ortho)",
        origin_gps.lat, origin_gps.lon, origin_gps.alt, srtm_origin
    );

    // ── Set up elevation + terrain ──────────────────────────────────────────
    let elevation = make_offline_elevation_pipeline(srtm_dir.clone(), cop30_dir.clone());
    // Sea-level datum calibration runs automatically in generate_region() after
    // the DEM is sampled — no region-specific coordinates needed here.

    let mut terrain_gen_inner = TerrainGenerator::new(elevation, origin_gps.clone(), origin_voxel);
    terrain_gen_inner.skip_vegetation = no_vegetation;
    let terrain_gen = Arc::new(terrain_gen_inner);

    // ── Open OSM cache if available ─────────────────────────────────────────
    let osm_cache = if let Some(ref db) = osm_db_path {
        // --osm-db specified: open that TileStore directly — no LOCK conflict
        match TileStore::open(db) {
            Ok(ts) => Some(Arc::new(OsmDiskCache::from_arc(Arc::new(ts)))),
            Err(e) => {
                eprintln!(
                    "[worldgen] --osm-db {db:?} failed to open: {e}; waterway carving disabled"
                );
                None
            }
        }
    } else if osm_cache_dir.exists() {
        Some(Arc::new(OsmDiskCache::new(&osm_cache_dir)))
    } else {
        eprintln!(
            "[worldgen] OSM cache dir not found ({:?}); waterway carving disabled",
            osm_cache_dir
        );
        None
    };

    // ── Worldgen config ─────────────────────────────────────────────────────
    // Open the output TileStore up front so we can report the path clearly.
    std::fs::create_dir_all(&output_dir).expect("Cannot create output dir");
    let tile_store = Arc::new(
        TileStore::open(&output_dir.join("tiles.db")).expect("Failed to open output TileStore"),
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
        gshhg_path,
        skip_vegetation: no_vegetation,
        skip_osm: no_osm,
        skip_roads: no_roads,
        skip_water: no_water,
    };

    eprintln!("[worldgen] Region: {:?}", region);
    eprintln!("[worldgen] Output: {:?}/tiles.db", output_dir);
    eprintln!("[worldgen] Workers: {workers}");
    eprintln!("[worldgen] Extra Y layers: {extra_layers}");

    let start = Instant::now();

    // ── Progress callback ───────────────────────────────────────────────────
    let progress = Arc::new(
        |done: u64, total: u64, _id: &metaverse_core::chunk::ChunkId| {
            if done % 100 == 0 || done == total {
                let pct = done as f64 / total as f64 * 100.0;
                eprint!("\r[worldgen] {done}/{total} ({pct:.1}%)  ");
            }
        },
    );

    match generate_region(
        &cfg,
        terrain_gen,
        &origin_gps,
        &origin_voxel,
        Some(progress),
    ) {
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

    // When --osm-db was used, OSM tiles live in a separate TileStore. Merge
    // only the tiles around this region into the output so each proof world
    // stays self-contained without copying a whole global cache into every site.
    if let Some(ref db_path) = osm_db_path {
        eprintln!(
            "[worldgen] Merging OSM tiles from {:?} into output tiles.db …",
            db_path
        );
        match TileStore::open(db_path) {
            Ok(src) => {
                let out = tile_store.as_ref();
                let coords = src.iter_osm_coords();
                let merge_margin_deg = 0.02_f64;
                let lat_min = region.lat_min - merge_margin_deg;
                let lat_max = region.lat_max + merge_margin_deg;
                let lon_min = region.lon_min - merge_margin_deg;
                let lon_max = region.lon_max + merge_margin_deg;
                let mut merged = 0usize;
                let mut skipped = 0usize;
                for (s, w, n, e) in coords {
                    let intersects_region =
                        n >= lat_min && s <= lat_max && e >= lon_min && w <= lon_max;
                    if !intersects_region {
                        skipped += 1;
                        continue;
                    }
                    if let Some(data) = src.get_osm(s, w, n, e) {
                        out.put_osm(s, w, n, e, &data);
                        merged += 1;
                    }
                }
                eprintln!(
                    "[worldgen] Merged {merged} nearby OSM tiles into output (skipped {skipped} outside region margin)."
                );
            }
            Err(e) => eprintln!("[worldgen] Warning: could not re-open osm-db for merge: {e}"),
        }
    }
}

fn print_help() {
    eprintln!(
        "\
worldgen — offline metaverse world pre-baking tool

USAGE:
    worldgen [OPTIONS]

OPTIONS:
    --region <name>       Named region (brisbane, brisbane-cbd, test) [default: brisbane-cbd]
    --bbox <coords>       Custom bounding box: lat_min,lon_min,lat_max,lon_max
    --output <dir>        Output directory [default: ./world_data/worldgen]
    --workers <n>         Parallel workers [default: CPU count]
    --extra-layers <n>    Extra Y layers above surface for tall buildings [default: 1]
    --srtm-dir <dir>      DEM cache directory [default: ./world_data/srtm]
    --cop30-dir <dir>     Local bulk COP30/Copernicus GeoTIFF directory
    --osm-cache <dir>     OSM tile cache for waterway carving [default: ./world_data/osm]
    --coastline <file>    GSHHG binary coastline file for coastal substrate (e.g. gshhs_f.b)
    --no-osm              Skip OSM water/road processing entirely
    --no-roads            Skip road geometry only (keep water)
    --no-water            Skip water carving only (keep roads)
    --no-vegetation       Explicitly disable tree/shrub placement (default)
    --with-vegetation     Re-enable tree/shrub placement
    --help                Show this help

EXAMPLES:
    worldgen --region test
    worldgen --region brisbane --output /mnt/data/worldgen --workers 8
    worldgen --region gympie --output world_data/test/gympie --no-osm
    worldgen --region brisbane --output /mnt/data/worldgen --with-vegetation
    worldgen --bbox -27.48,-27.45,153.01,153.04 --output ./test_region
"
    );
}
