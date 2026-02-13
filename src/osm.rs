/// OpenStreetMap data fetching via Overpass API.

use crate::coordinates::GpsPos;
use crate::chunks::{ChunkId, gps_to_chunk_id, chunk_bounds_gps};
use crate::cache::DiskCache;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::thread;

/// Road type classification
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RoadType {
    Motorway,
    Trunk,
    Primary,
    Secondary,
    Tertiary,
    Residential,
    Service,
    Path,
    Cycleway,
    Other(String),
}

impl RoadType {
    /// Returns default width in metres for this road type
    pub fn default_width_m(&self) -> f64 {
        match self {
            RoadType::Motorway => 12.0,
            RoadType::Trunk => 10.0,
            RoadType::Primary => 8.0,
            RoadType::Secondary => 7.0,
            RoadType::Tertiary => 6.0,
            RoadType::Residential => 6.0,
            RoadType::Service => 4.0,
            RoadType::Path => 2.0,
            RoadType::Cycleway => 2.0,
            RoadType::Other(_) => 5.0,
        }
    }

    /// Parses from OSM highway tag value
    pub fn from_highway_tag(tag: &str) -> Self {
        match tag {
            "motorway" | "motorway_link" => RoadType::Motorway,
            "trunk" | "trunk_link" => RoadType::Trunk,
            "primary" | "primary_link" => RoadType::Primary,
            "secondary" | "secondary_link" => RoadType::Secondary,
            "tertiary" | "tertiary_link" => RoadType::Tertiary,
            "residential" | "living_street" | "unclassified" => RoadType::Residential,
            "service" => RoadType::Service,
            "footway" | "path" | "pedestrian" => RoadType::Path,
            "cycleway" => RoadType::Cycleway,
            other => RoadType::Other(other.to_string()),
        }
    }
}

/// Parsed building data from OSM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmBuilding {
    pub id: u64,
    pub polygon: Vec<GpsPos>,
    pub height_m: f64,
    pub building_type: String,
    pub levels: u8,
}

/// Parsed road data from OSM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmRoad {
    pub id: u64,
    pub nodes: Vec<GpsPos>,
    pub road_type: RoadType,
    pub width_m: f64,
    pub name: Option<String>,
}

/// Parsed water feature from OSM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmWater {
    pub id: u64,
    pub polygon: Vec<GpsPos>,
    pub name: Option<String>,
}

/// Parsed park/leisure area from OSM
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OsmPark {
    pub id: u64,
    pub polygon: Vec<GpsPos>,
    pub name: Option<String>,
}

/// Collection of parsed OSM data
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OsmData {
    pub buildings: Vec<OsmBuilding>,
    pub roads: Vec<OsmRoad>,
    pub water: Vec<OsmWater>,
    pub parks: Vec<OsmPark>,
}

/// Parses an Overpass API JSON response into structured OSM data
pub fn parse_overpass_response(json: &serde_json::Value) -> Result<OsmData, Box<dyn std::error::Error>> {
    let mut data = OsmData::default();
    
    // Build node ID -> GPS position lookup
    let mut node_lookup: HashMap<u64, GpsPos> = HashMap::new();
    
    let elements = json.get("elements")
        .and_then(|e| e.as_array())
        .ok_or("Missing or invalid 'elements' array")?;
    
    // First pass: collect all nodes
    for element in elements {
        if element.get("type").and_then(|t| t.as_str()) == Some("node") {
            let id = element.get("id").and_then(|i| i.as_u64()).ok_or("Node missing id")?;
            let lat = element.get("lat").and_then(|l| l.as_f64()).ok_or("Node missing lat")?;
            let lon = element.get("lon").and_then(|l| l.as_f64()).ok_or("Node missing lon")?;
            node_lookup.insert(id, GpsPos { 
                lat_deg: lat, 
                lon_deg: lon, 
                elevation_m: 0.0 
            });
        }
    }
    
    // Second pass: process ways and relations
    for element in elements {
        let elem_type = element.get("type").and_then(|t| t.as_str()).unwrap_or("");
        
        if elem_type != "way" && elem_type != "relation" {
            continue;
        }
        
        let id = element.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
        let tags = element.get("tags").and_then(|t| t.as_object());
        
        if tags.is_none() {
            continue;
        }
        
        let tags = tags.unwrap();
        
        // Extract node references for ways
        let mut coords = Vec::new();
        if elem_type == "way" {
            if let Some(nodes) = element.get("nodes").and_then(|n| n.as_array()) {
                for node_id in nodes {
                    if let Some(id) = node_id.as_u64() {
                        if let Some(pos) = node_lookup.get(&id) {
                            coords.push(*pos);
                        }
                    }
                }
            }
        }
        
        // Parse based on tags
        if tags.contains_key("building") {
            let building_type = tags.get("building")
                .and_then(|b| b.as_str())
                .unwrap_or("yes")
                .to_string();
            
            let levels = tags.get("building:levels")
                .and_then(|l| l.as_str())
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(3);
            
            let height_m = tags.get("height")
                .and_then(|h| h.as_str())
                .and_then(|s| s.trim_end_matches(" m").parse::<f64>().ok())
                .unwrap_or(levels as f64 * 3.0);
            
            data.buildings.push(OsmBuilding {
                id,
                polygon: coords,
                height_m,
                building_type,
                levels,
            });
        } else if tags.contains_key("highway") {
            let highway_tag = tags.get("highway")
                .and_then(|h| h.as_str())
                .unwrap_or("unclassified");
            
            let road_type = RoadType::from_highway_tag(highway_tag);
            
            let width_m = tags.get("width")
                .and_then(|w| w.as_str())
                .and_then(|s| s.trim_end_matches(" m").parse::<f64>().ok())
                .unwrap_or_else(|| road_type.default_width_m());
            
            let name = tags.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            
            data.roads.push(OsmRoad {
                id,
                nodes: coords,
                road_type,
                width_m,
                name,
            });
        } else if tags.get("natural").and_then(|n| n.as_str()) == Some("water") {
            let name = tags.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            
            data.water.push(OsmWater {
                id,
                polygon: coords,
                name,
            });
        } else if tags.get("leisure").and_then(|l| l.as_str()) == Some("park") {
            let name = tags.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            
            data.parks.push(OsmPark {
                id,
                polygon: coords,
                name,
            });
        }
    }
    
    Ok(data)
}

/// Assigns OSM entities to chunks based on their GPS coordinates
pub fn assign_osm_to_chunks(data: &OsmData, depth: u8) -> HashMap<ChunkId, OsmData> {
    let mut chunk_map: HashMap<ChunkId, OsmData> = HashMap::new();
    
    // Assign buildings by centroid
    for building in &data.buildings {
        if building.polygon.is_empty() {
            continue;
        }
        
        // Calculate centroid
        let mut lat_sum = 0.0;
        let mut lon_sum = 0.0;
        let count = building.polygon.len() as f64;
        
        for pos in &building.polygon {
            lat_sum += pos.lat_deg;
            lon_sum += pos.lon_deg;
        }
        
        let centroid = GpsPos {
            lat_deg: lat_sum / count,
            lon_deg: lon_sum / count,
            elevation_m: 0.0,
        };
        
        let chunk_id = gps_to_chunk_id(&centroid, depth);
        let chunk_data = chunk_map.entry(chunk_id).or_insert_with(OsmData::default);
        chunk_data.buildings.push(building.clone());
    }
    
    // Assign roads to all chunks they pass through
    for road in &data.roads {
        let mut visited_chunks = std::collections::HashSet::new();
        
        for pos in &road.nodes {
            let chunk_id = gps_to_chunk_id(pos, depth);
            visited_chunks.insert(chunk_id);
        }
        
        for chunk_id in visited_chunks {
            let chunk_data = chunk_map.entry(chunk_id).or_insert_with(OsmData::default);
            chunk_data.roads.push(road.clone());
        }
    }
    
    // Assign water by centroid
    for water in &data.water {
        if water.polygon.is_empty() {
            continue;
        }
        
        let mut lat_sum = 0.0;
        let mut lon_sum = 0.0;
        let count = water.polygon.len() as f64;
        
        for pos in &water.polygon {
            lat_sum += pos.lat_deg;
            lon_sum += pos.lon_deg;
        }
        
        let centroid = GpsPos {
            lat_deg: lat_sum / count,
            lon_deg: lon_sum / count,
            elevation_m: 0.0,
        };
        
        let chunk_id = gps_to_chunk_id(&centroid, depth);
        let chunk_data = chunk_map.entry(chunk_id).or_insert_with(OsmData::default);
        chunk_data.water.push(water.clone());
    }
    
    // Assign parks by centroid
    for park in &data.parks {
        if park.polygon.is_empty() {
            continue;
        }
        
        let mut lat_sum = 0.0;
        let mut lon_sum = 0.0;
        let count = park.polygon.len() as f64;
        
        for pos in &park.polygon {
            lat_sum += pos.lat_deg;
            lon_sum += pos.lon_deg;
        }
        
        let centroid = GpsPos {
            lat_deg: lat_sum / count,
            lon_deg: lon_sum / count,
            elevation_m: 0.0,
        };
        
        let chunk_id = gps_to_chunk_id(&centroid, depth);
        let chunk_data = chunk_map.entry(chunk_id).or_insert_with(OsmData::default);
        chunk_data.parks.push(park.clone());
    }
    
    chunk_map
}



/// Client for querying OpenStreetMap data via Overpass API
pub struct OverpassClient {
    last_request: Mutex<Option<Instant>>,
    min_cooldown: Duration,
    endpoint: String,
}

impl OverpassClient {
    /// Creates a new client with specified cooldown in seconds (default endpoint)
    pub fn new(cooldown_seconds: u64) -> Self {
        Self {
            last_request: Mutex::new(None),
            min_cooldown: Duration::from_secs(cooldown_seconds),
            endpoint: "https://overpass-api.de/api/interpreter".to_string(),
        }
    }

    /// Creates a client with custom endpoint (for testing)
    pub fn with_endpoint(cooldown_seconds: u64, endpoint: String) -> Self {
        Self {
            last_request: Mutex::new(None),
            min_cooldown: Duration::from_secs(cooldown_seconds),
            endpoint,
        }
    }

    /// Queries a bounding box for buildings, roads, water, and parks
    pub fn query_bbox(
        &self,
        south: f64,
        west: f64,
        north: f64,
        east: f64,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let query = Self::build_query(south, west, north, east);
        self.execute_with_retry(&query, 5)
    }

    /// Builds Overpass QL query for a bounding box
    pub fn build_query(south: f64, west: f64, north: f64, east: f64) -> String {
        format!(
            "[out:json][timeout:25];\n\
             (\n  \
               way[\"building\"]({},{},{},{});\n  \
               way[\"highway\"]({},{},{},{});\n  \
               way[\"natural\"=\"water\"]({},{},{},{});\n  \
               way[\"leisure\"=\"park\"]({},{},{},{});\n  \
               relation[\"natural\"=\"water\"]({},{},{},{});\n\
             );\n\
             out body;\n\
             >;\n\
             out skel qt;",
            south, west, north, east, // building
            south, west, north, east, // highway
            south, west, north, east, // water way
            south, west, north, east, // park
            south, west, north, east, // water relation
        )
    }

    /// Executes query with exponential backoff retry
    fn execute_with_retry(
        &self,
        query: &str,
        max_retries: u32,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        let mut attempt = 0;
        let mut backoff = Duration::from_secs(3);

        loop {
            // Enforce rate limiting
            self.wait_for_cooldown();

            // Execute request
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("metaverse-core/0.1 (Earth metaverse project)")
                .build()?;

            match client.post(&self.endpoint).body(query.to_string()).send() {
                Ok(response) => {
                    if response.status().is_success() {
                        let json = response.json()?;
                        return Ok(json);
                    } else if response.status() == 429 {
                        // Too Many Requests - exponential backoff
                        if attempt >= max_retries {
                            return Err(format!("Max retries exceeded (429 errors)").into());
                        }
                        thread::sleep(backoff);
                        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                        attempt += 1;
                    } else {
                        return Err(format!("HTTP error: {}", response.status()).into());
                    }
                }
                Err(e) => {
                    // Network error - retry with backoff
                    if attempt >= max_retries {
                        return Err(format!("Max retries exceeded: {}", e).into());
                    }
                    thread::sleep(backoff);
                    backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
                    attempt += 1;
                }
            }
        }
    }

    /// Waits for cooldown period if needed (public for testing)
    pub fn wait_for_cooldown(&self) {
        let mut last = self.last_request.lock().unwrap();
        if let Some(instant) = *last {
            let elapsed = instant.elapsed();
            if elapsed < self.min_cooldown {
                let wait = self.min_cooldown - elapsed;
                thread::sleep(wait);
            }
        }
        *last = Some(Instant::now());
    }

    /// Returns the backoff duration for a given attempt (for testing)
    pub fn backoff_duration(attempt: u32) -> Duration {
        let seconds = 3u64 * 2u64.pow(attempt);
        let capped = seconds.min(60);
        Duration::from_secs(capped)
    }
}

/// Loads OSM data for a chunk with caching
///
/// Pipeline:
/// 1. Check cache for chunk data
/// 2. If miss, determine chunk bounds
/// 3. Fetch OSM data from Overpass API
/// 4. Parse and assign to chunks
/// 5. Write to cache
/// 6. Return data for requested chunk
pub fn load_chunk_osm_data(
    chunk_id: &ChunkId,
    depth: u8,
    client: &OverpassClient,
    cache: &DiskCache,
) -> Result<OsmData, Box<dyn std::error::Error>> {
    // Generate cache key from chunk ID
    let path_str = chunk_id.path.iter().map(|q| q.to_string()).collect::<Vec<_>>().join("");
    let cache_key = format!("chunk_d{}_f{}_{}", depth, chunk_id.face, path_str);
    
    // Try to read from cache
    if let Ok(cached_bytes) = cache.read_osm(&cache_key) {
        // Deserialize cached data
        if let Ok(data) = serde_json::from_slice::<OsmData>(&cached_bytes) {
            return Ok(data);
        }
        // If deserialization fails, fall through to fetch fresh data
    }
    
    // Cache miss - fetch from API
    // Get chunk bounds in GPS coordinates
    let (min_gps, max_gps) = chunk_bounds_gps(chunk_id)?;
    
    // Query Overpass API
    let json = client.query_bbox(
        min_gps.lat_deg.min(max_gps.lat_deg),
        min_gps.lon_deg.min(max_gps.lon_deg),
        min_gps.lat_deg.max(max_gps.lat_deg),
        min_gps.lon_deg.max(max_gps.lon_deg),
    )?;
    
    // Parse response
    let all_data = parse_overpass_response(&json)?;
    
    // Assign to chunks
    let chunks = assign_osm_to_chunks(&all_data, depth);
    
    // Get data for this specific chunk
    let chunk_data = chunks.get(chunk_id).cloned().unwrap_or_default();
    
    // Write to cache
    let serialized = serde_json::to_vec(&chunk_data)?;
    let _ = cache.write_osm(&cache_key, &serialized); // Ignore cache write errors
    
    Ok(chunk_data)
}

