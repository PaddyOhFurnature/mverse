/// Pre-fetch and cache OSM tiles for a geographic area.
///
/// Run with:  METAVERSE_OVERPASS=1 cargo run --example prefetch_osm
///
/// This populates world_data/osm/*.bin so the game works offline.
/// Set PREFETCH_RADIUS_DEG env var to override the default 0.05° radius.
fn main() {
    // Brisbane area around Story Bridge
    let centre_lat: f64 = -27.463675;
    let centre_lon: f64 = 153.035645;
    let radius: f64 = std::env::var("PREFETCH_RADIUS_DEG")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.05); // ~5.5 km

    let tile = 0.01_f64;
    let osm_dir = std::path::Path::new("world_data/osm");
    std::fs::create_dir_all(osm_dir).ok();

    let lat0 = (((centre_lat - radius) / tile).floor() * tile * 10000.0).round() / 10000.0;
    let lon0 = (((centre_lon - radius) / tile).floor() * tile * 10000.0).round() / 10000.0;
    let lat1 = (((centre_lat + radius) / tile).ceil() * tile * 10000.0).round() / 10000.0;
    let lon1 = (((centre_lon + radius) / tile).ceil() * tile * 10000.0).round() / 10000.0;

    let lat_n = ((lat1 - lat0) / tile).round() as usize;
    let lon_n = ((lon1 - lon0) / tile).round() as usize;
    let total = lat_n * lon_n;

    println!(
        "Prefetching {} OSM tiles for Brisbane ({:.3}..{:.3}, {:.3}..{:.3})…",
        total, lat0, lat1, lon0, lon1
    );

    let mut n_fetched = 0usize;
    let mut n_cached = 0usize;
    let mut n_failed = 0usize;
    let endpoints: Vec<String> = Vec::new();

    for i in 0..lat_n {
        for j in 0..lon_n {
            let s = ((lat0 + i as f64 * tile) * 10000.0).round() / 10000.0;
            let w = ((lon0 + j as f64 * tile) * 10000.0).round() / 10000.0;
            let n = ((s + tile) * 10000.0).round() / 10000.0;
            let e = ((w + tile) * 10000.0).round() / 10000.0;

            let idx = i * lon_n + j + 1;
            print!("  [{idx}/{total}] ({s:.4},{w:.4})→({n:.4},{e:.4})  ");

            match metaverse_core::osm::fetch_osm_for_bounds(s, w, n, e, osm_dir, &endpoints) {
                Ok(data) => {
                    if data.is_empty() {
                        println!("(empty tile)");
                        n_cached += 1;
                    } else {
                        println!(
                            "✓  b:{} r:{} w:{}",
                            data.buildings.len(),
                            data.roads.len(),
                            data.water.len()
                        );
                        n_fetched += 1;
                    }
                }
                Err(e) => {
                    println!("✗  {}", e);
                    n_failed += 1;
                }
            }
        }
    }

    println!();
    println!(
        "Done: {} fetched/cached, {} already fresh, {} failed",
        n_fetched, n_cached, n_failed
    );
    if n_failed > 0 {
        println!("⚠  Re-run to retry failed tiles (Overpass may have rate-limited).");
    }
}
