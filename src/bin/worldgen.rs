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
///   --osm-dir <dir>          OSM tile cache directory [default: ./world_data/osm_cache]
///   --resume                 Skip already-generated chunks (default: enabled)
///   --no-buildings           Disable building voxelisation

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, SkadiElevationSource},
    terrain::TerrainGenerator,
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
    let mut extra_layers  = 1i32;
    let mut srtm_dir      = PathBuf::from("world_data/srtm");
    let mut osm_dir       = PathBuf::from("world_data/osm_cache");
    let mut bake_buildings = true;

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
            "--osm-dir" => {
                i += 1;
                osm_dir = PathBuf::from(&args[i]);
            }
            "--no-buildings" => {
                bake_buildings = false;
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

    // ── Set up elevation + terrain ──────────────────────────────────────────
    let origin_gps    = GPS::new(-27.3996, 153.1871, 2.0);
    let origin_ecef   = origin_gps.to_ecef();
    let origin_voxel  = VoxelCoord::from_ecef(&origin_ecef);

    let mut elevation = ElevationPipeline::new();
    elevation.add_source(Box::new(SkadiElevationSource::new(srtm_dir.clone())));

    let mut terrain_gen = TerrainGenerator::new(elevation, origin_gps.clone(), origin_voxel);
    terrain_gen.bake_buildings = bake_buildings;
    terrain_gen = terrain_gen.with_osm_cache(osm_dir);

    let terrain_gen = Arc::new(terrain_gen);

    // ── Worldgen config ─────────────────────────────────────────────────────
    let cfg = WorldgenConfig {
        region: region.clone(),
        output_dir: output_dir.clone(),
        workers,
        extra_y_layers: extra_layers,
        report_interval: 100,
    };

    eprintln!("[worldgen] Region: {:?}", region);
    eprintln!("[worldgen] Output: {:?}", output_dir);
    eprintln!("[worldgen] Workers: {workers}");
    eprintln!("[worldgen] Extra Y layers: {extra_layers}");
    eprintln!("[worldgen] Bake buildings: {bake_buildings}");

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
    --osm-dir <dir>       OSM tile cache directory [default: ./world_data/osm_cache]
    --no-buildings        Disable building voxelisation
    --help                Show this help

EXAMPLES:
    worldgen --region test
    worldgen --region brisbane --output /mnt/data/worldgen --workers 8
    worldgen --bbox -27.48,-27.45,153.01,153.04 --output ./test_region
");
}
