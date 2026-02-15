use metaverse_core::cache::DiskCache;
use metaverse_core::osm::OsmData;

fn main() {
    let cache = DiskCache::new().unwrap();
    let cache_keys = ["brisbane_cbd"];
    
    for key in &cache_keys {
        if let Ok(cached_bytes) = cache.read_osm(key) {
            if let Ok(osm_data) = serde_json::from_slice::<OsmData>(&cached_bytes) {
                let mut heights: Vec<f64> = osm_data.buildings.iter()
                    .map(|b| b.height_m)
                    .collect();
                heights.sort_by(|a, b| a.partial_cmp(b).unwrap());
                
                println!("Building heights in dataset:");
                println!("  Total buildings: {}", heights.len());
                println!("  Min height: {:.1}m", heights.first().unwrap_or(&0.0));
                println!("  Max height: {:.1}m", heights.last().unwrap_or(&0.0));
                println!("  Median: {:.1}m", heights[heights.len() / 2]);
                println!("  75th percentile: {:.1}m", heights[heights.len() * 3 / 4]);
                println!("  90th percentile: {:.1}m", heights[heights.len() * 9 / 10]);
                
                let tall_buildings = heights.iter().filter(|&&h| h > 50.0).count();
                println!("\nBuildings taller than 50m: {} ({:.1}%)", 
                    tall_buildings, 
                    (tall_buildings as f64 / heights.len() as f64) * 100.0);
                    
                let very_tall = heights.iter().filter(|&&h| h > 100.0).count();
                println!("Buildings taller than 100m: {}", very_tall);
            }
        }
    }
}
