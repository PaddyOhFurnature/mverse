use metaverse_core::cache::DiskCache;
use metaverse_core::osm::OsmData;

fn main() {
    let cache = DiskCache::new().unwrap();
    let cache_keys = ["brisbane_cbd_full_osmdata", "brisbane_cbd_osmdata", "brisbane_cbd"];
    
    for key in &cache_keys {
        if let Ok(cached_bytes) = cache.read_osm(key) {
            if let Ok(osm_data) = serde_json::from_slice::<OsmData>(&cached_bytes) {
                println!("\nCache key: {}", key);
                println!("Buildings: {}, Roads: {}", osm_data.buildings.len(), osm_data.roads.len());
                
                if let Some(first_building) = osm_data.buildings.first() {
                    if let Some(first_point) = first_building.polygon.first() {
                        println!("First building at: ({:.6}, {:.6})", first_point.lat_deg, first_point.lon_deg);
                    }
                }
                
                // Find bounds
                let mut min_lat = 999.0_f64;
                let mut max_lat = -999.0_f64;
                let mut min_lon = 999.0_f64;
                let mut max_lon = -999.0_f64;
                
                for building in &osm_data.buildings {
                    for point in &building.polygon {
                        min_lat = min_lat.min(point.lat_deg);
                        max_lat = max_lat.max(point.lat_deg);
                        min_lon = min_lon.min(point.lon_deg);
                        max_lon = max_lon.max(point.lon_deg);
                    }
                }
                
                println!("Bounds: lat [{:.6}, {:.6}], lon [{:.6}, {:.6}]", min_lat, max_lat, min_lon, max_lon);
                println!("Center: ({:.6}, {:.6})", (min_lat + max_lat) / 2.0, (min_lon + max_lon) / 2.0);
                println!("Target (Queen St Mall): (-27.469800, 153.025100)");
                
                let center_lat = (min_lat + max_lat) / 2.0;
                let center_lon = (min_lon + max_lon) / 2.0;
                let dist_lat = (center_lat - (-27.469800)).abs() * 111000.0; // ~111km per degree
                let dist_lon = (center_lon - 153.025100).abs() * 111000.0 * (-27.469800_f64).to_radians().cos();
                
                println!("Distance from target: ~{:.0}m lat, ~{:.0}m lon", dist_lat, dist_lon);
                break;
            }
        }
    }
}
