//! osm-import — converts a .osm.pbf file into the spatial tile cache used by worldgen.
//!
//! This is a one-time (or as-needed) step.  The resulting cache at `world_data/osm/`
//! is then read by `worldgen` when carving waterways, roads, and buildings into terrain.
//!
//! USAGE:
//!     osm-import [--pbf <path>] [--cache <dir>]
//!
//! DEFAULTS:
//!     --pbf    ./world_data/map.osm.pbf
//!     --cache  ./world_data/osm

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn main() {
    let mut pbf_path = PathBuf::from("./world_data/map.osm.pbf");
    let mut cache_dir = PathBuf::from("./world_data/osm");
    let mut db_path: Option<PathBuf> = None;
    let mut replace_existing = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pbf" => {
                if let Some(v) = args.next() {
                    pbf_path = PathBuf::from(v);
                }
            }
            "--cache" => {
                if let Some(v) = args.next() {
                    cache_dir = PathBuf::from(v);
                }
            }
            "--db" => {
                if let Some(v) = args.next() {
                    db_path = Some(PathBuf::from(v));
                }
            }
            "--help" => {
                print_help();
                return;
            }
            "--replace-existing" => {
                replace_existing = true;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                print_help();
                std::process::exit(1);
            }
        }
    }

    if !pbf_path.exists() {
        eprintln!("❌  PBF file not found: {}", pbf_path.display());
        eprintln!("    Download Brisbane OSM data or specify --pbf <path>");
        std::process::exit(1);
    }

    // Wipe any stale flat .bin files left by old code before opening RocksDB there.
    wipe_flat_bin_files(&cache_dir);

    std::fs::create_dir_all(&cache_dir).expect("failed to create cache dir");

    let pbf_mb = std::fs::metadata(&pbf_path)
        .map(|m| m.len() / 1_048_576)
        .unwrap_or(0);
    println!("🗺️  OSM import");
    println!("    PBF:   {} ({} MB)", pbf_path.display(), pbf_mb);
    println!("    Cache: {}", cache_dir.display());
    if let Some(ref db) = db_path {
        println!("    DB:    {}", db.display());
    }
    println!("    Replace existing: {}", replace_existing);
    println!();

    let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_clone = Arc::clone(&log);

    // Spawn a thread to drain and print the progress log while import runs.
    let printer = std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let msgs: Vec<String> = {
                let mut buf = log_clone.lock().unwrap();
                std::mem::take(&mut *buf)
            };
            for m in msgs {
                println!("{m}");
            }
        }
    });

    let start = Instant::now();
    let result = if let Some(db) = db_path {
        // Use a separate TileStore path — avoids LOCK conflict when another
        // import is running against the default world_data/tiles.db.
        std::fs::create_dir_all(&db).expect("failed to create db dir");
        let ts = std::sync::Arc::new(
            metaverse_core::tile_store::TileStore::open(&db)
                .unwrap_or_else(|e| panic!("Failed to open TileStore at {db:?}: {e}")),
        );
        metaverse_core::osm::import_pbf_with_store_options(
            &pbf_path,
            ts,
            Some(Arc::clone(&log)),
            replace_existing,
        )
    } else {
        metaverse_core::osm::import_pbf_with_log_options(
            &pbf_path,
            &cache_dir,
            Arc::clone(&log),
            replace_existing,
        )
    };
    match result {
        Ok(tiles_written) => {
            // Drain any remaining messages.
            std::thread::sleep(std::time::Duration::from_millis(600));
            let remaining: Vec<String> = std::mem::take(&mut *log.lock().unwrap());
            for m in remaining {
                println!("{m}");
            }

            let elapsed = start.elapsed();
            println!();
            println!(
                "✅  Done — {} tiles written in {:.1}s",
                tiles_written,
                elapsed.as_secs_f32()
            );
            println!("    Cache ready at: {}", cache_dir.display());
            println!("    Run worldgen next:  worldgen --region brisbane");
        }
        Err(e) => {
            eprintln!("❌  Import failed: {e}");
            std::process::exit(1);
        }
    }

    drop(printer); // printer thread will be killed when main exits
}

/// Remove old flat `.bin` OSM tile files that the previous (broken) code wrote.
/// The new OsmDiskCache is RocksDB — it needs an empty or RocksDB-format directory.
fn wipe_flat_bin_files(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("bin") {
            if std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        println!(
            "🧹  Removed {removed} stale flat .bin files from {}",
            dir.display()
        );
    }
}

fn print_help() {
    println!("osm-import — index a .osm.pbf file into the worldgen OSM tile cache");
    println!();
    println!("USAGE:");
    println!("    osm-import [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --pbf <path>    Path to .osm.pbf file  [default: ./world_data/map.osm.pbf]");
    println!("    --cache <dir>   Output cache directory  [default: ./world_data/osm]");
    println!(
        "    --db <path>     TileStore (RocksDB) output path  [default: derived from --cache]"
    );
    println!("                    Use to avoid LOCK conflict when another import is running.");
    println!("    --replace-existing  Overwrite existing OSM tiles instead of skipping them.");
    println!("    --help          Show this help");
    println!();
    println!("EXAMPLES:");
    println!("    osm-import");
    println!("    osm-import --pbf ~/downloads/brisbane.osm.pbf --cache ./world_data/osm");
    println!("    osm-import --pbf gympie.osm.pbf --cache ./osm_gympie --db ./gympie.db");
    println!("    osm-import --pbf extract.osm.pbf --db ./atlas-osm.db --replace-existing");
}
