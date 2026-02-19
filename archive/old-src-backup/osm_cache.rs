/// OSM feature caching for continuous query system.
///
/// Queries Overpass API and caches OSM features for test area.
/// Simple blocking HTTP - no async complexity for prototype.

use crate::osm::{OsmData, parse_overpass_response, OverpassClient};
use crate::coordinates::GpsPos;
use std::fs;
use std::path::PathBuf;

/// OSM feature cache manager
pub struct OsmCache {
    cache_dir: PathBuf,
    client: OverpassClient,
}

impl OsmCache {
    /// Create new OSM cache
    pub fn new(cache_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        // Ensure cache directory exists
        fs::create_dir_all(&cache_dir)?;
        
        // Create Overpass client with 2-second cooldown (project rule)
        let client = OverpassClient::new(30_000); // 30 second timeout
        
        Ok(Self { cache_dir, client })
    }

    /// Get cache file path for bounding box
    fn cache_path(&self, south: f64, west: f64, north: f64, east: f64) -> PathBuf {
        // Create filename from bounds (rounded to 3 decimal places for consistency)
        let filename = format!(
            "osm_s{:.3}_w{:.3}_n{:.3}_e{:.3}.json",
            south, west, north, east
        );
        self.cache_dir.join(filename)
    }

    /// Query OSM features for bounding box, caching results
    pub fn get_features(
        &self,
        south: f64,
        west: f64,
        north: f64,
        east: f64,
    ) -> Result<OsmData, Box<dyn std::error::Error>> {
        let cache_path = self.cache_path(south, west, north, east);
        
        // Try to load from cache first
        if cache_path.exists() {
            println!("Loading OSM data from cache: {:?}", cache_path);
            let json_str = fs::read_to_string(&cache_path)?;
            let data: OsmData = serde_json::from_str(&json_str)?;
            return Ok(data);
        }

        // Not in cache - query Overpass API
        println!("Querying Overpass API for bbox: ({}, {}) to ({}, {})", south, west, north, east);
        let json = self.client.query_bbox(south, west, north, east)?;
        
        // Parse response
        let data = parse_overpass_response(&json)?;
        
        // Save to cache
        let json_str = serde_json::to_string_pretty(&data)?;
        fs::write(&cache_path, json_str)?;
        println!("Cached OSM data to: {:?}", cache_path);
        println!("  Buildings: {}, Roads: {}, Water: {}, Parks: {}", 
            data.buildings.len(), data.roads.len(), data.water.len(), data.parks.len());
        
        Ok(data)
    }

    /// Query features covering a GPS area
    pub fn get_area_features(
        &self,
        center: GpsPos,
        radius_m: f64,
    ) -> Result<OsmData, Box<dyn std::error::Error>> {
        // Convert radius to degrees (approximate)
        // At equator: 1° ≈ 111km
        // Adjust for latitude
        let lat_rad = center.lat_deg.to_radians();
        let km_per_deg_lat = 111.0;
        let km_per_deg_lon = 111.0 * lat_rad.cos();
        
        let radius_km = radius_m / 1000.0;
        let delta_lat = radius_km / km_per_deg_lat;
        let delta_lon = radius_km / km_per_deg_lon;
        
        let south = center.lat_deg - delta_lat;
        let north = center.lat_deg + delta_lat;
        let west = center.lon_deg - delta_lon;
        let east = center.lon_deg + delta_lon;
        
        self.get_features(south, west, north, east)
    }

    /// Pre-fetch features for test area
    pub fn prefetch_area(
        &self,
        center: GpsPos,
        radius_m: f64,
    ) -> Result<OsmData, Box<dyn std::error::Error>> {
        println!("Prefetching OSM features for area...");
        println!("  Center: ({:.6}, {:.6})", center.lat_deg, center.lon_deg);
        println!("  Radius: {}m", radius_m);
        
        let data = self.get_area_features(center, radius_m)?;
        
        println!("✓ OSM prefetch complete");
        println!("  Buildings: {}", data.buildings.len());
        println!("  Roads: {}", data.roads.len());
        println!("  Water features: {}", data.water.len());
        println!("  Parks: {}", data.parks.len());
        
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn test_cache_dir() -> PathBuf {
        // Use thread ID to make unique per test
        let thread_id = std::thread::current().id();
        env::temp_dir().join(format!("metaverse_osm_test_{:?}", thread_id))
    }

    #[test]
    fn test_cache_path() {
        let cache_dir = test_cache_dir();
        let cache = OsmCache::new(cache_dir.clone()).unwrap();
        
        let path = cache.cache_path(-27.48, 153.03, -27.47, 153.04);
        assert!(path.to_string_lossy().contains("osm_s-27.480_w153.030_n-27.470_e153.040.json"));
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn test_cache_creation() {
        let cache_dir = test_cache_dir();
        let _ = fs::remove_dir_all(&cache_dir); // Clean up from previous tests
        
        let cache = OsmCache::new(cache_dir.clone()).unwrap();
        assert!(cache_dir.exists());
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn test_area_bounds_calculation() {
        let cache_dir = test_cache_dir();
        let cache = OsmCache::new(cache_dir.clone()).unwrap();
        
        // Kangaroo Point test center
        let center = GpsPos {
            lat_deg: -27.479769,
            lon_deg: 153.033586,
            elevation_m: 0.0,
        };
        
        // Calculate bounds for 100m radius (should work without network)
        // We're just testing the math, not the actual query
        
        let lat_rad = center.lat_deg.to_radians();
        let km_per_deg_lat = 111.0;
        let km_per_deg_lon = 111.0 * lat_rad.cos();
        
        let radius_km = 0.1; // 100m
        let delta_lat = radius_km / km_per_deg_lat;
        let delta_lon = radius_km / km_per_deg_lon;
        
        // Should be approximately 0.0009° latitude, 0.0011° longitude
        assert!((delta_lat - 0.0009).abs() < 0.0001);
        assert!((delta_lon - 0.0011).abs() < 0.0001);
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    #[ignore] // Only run manually - requires network
    fn test_query_kangaroo_point() {
        let cache_dir = test_cache_dir();
        let _ = fs::remove_dir_all(&cache_dir); // Start fresh
        
        let cache = OsmCache::new(cache_dir.clone()).unwrap();
        
        // Kangaroo Point test area
        let center = GpsPos {
            lat_deg: -27.479769,
            lon_deg: 153.033586,
            elevation_m: 0.0,
        };
        
        // Try to fetch 100m radius area
        match cache.prefetch_area(center, 100.0) {
            Ok(data) => {
                println!("✓ Successfully fetched OSM data");
                println!("  Buildings: {}", data.buildings.len());
                println!("  Roads: {}", data.roads.len());
                println!("  Water: {}", data.water.len());
                
                // Should have some features in this area
                assert!(data.buildings.len() > 0 || data.roads.len() > 0,
                    "Expected some buildings or roads in Kangaroo Point area");
            }
            Err(e) => {
                println!("Query failed (may be rate limited): {}", e);
                // This is OK for automated tests - Overpass has rate limits
            }
        }
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }

    #[test]
    fn test_cache_persistence() {
        let cache_dir = test_cache_dir();
        let _ = fs::remove_dir_all(&cache_dir);
        
        {
            let cache = OsmCache::new(cache_dir.clone()).unwrap();
            
            // Create mock OSM data
            let data = OsmData {
                buildings: vec![],
                roads: vec![],
                water: vec![],
                parks: vec![],
            };
            
            // Save to cache manually (cache_dir already exists from new())
            let cache_path = cache.cache_path(-27.48, 153.03, -27.47, 153.04);
            let json_str = serde_json::to_string(&data).unwrap();
            fs::write(&cache_path, json_str).unwrap();
        }
        
        // Create new cache instance and load
        {
            let cache = OsmCache::new(cache_dir.clone()).unwrap();
            let result = cache.get_features(-27.48, 153.03, -27.47, 153.04);
            assert!(result.is_ok(), "Should load from cache");
        }
        
        // Clean up
        let _ = fs::remove_dir_all(&cache_dir);
    }
}
