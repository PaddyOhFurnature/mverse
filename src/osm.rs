/// OpenStreetMap data fetching and per-chunk caching.
///
/// Queries the Overpass API for buildings, roads, waterways, and parks
/// within a chunk's GPS bounds. Results are cached to disk as JSON so
/// subsequent loads are instant.
///
/// ## Why NOT the old approach
/// Old code used `GpsPos { lat_deg, lon_deg }` — wrong field names for the
/// current `GPS { lat, lon }` type. All coordinates were silently zero.
/// Buildings were voxelized INTO terrain instead of placed ON it as objects.
///
/// ## Correct approach
/// 1. Per-chunk Overpass query using `ChunkId::gps_bounds()`
/// 2. Parse → `OsmData` using `GPS { lat, lon }` correctly
/// 3. Feed into `world_inference` → `InferredObject` list
/// 4. Object elevation = terrain height at that GPS (queried from ElevationPipeline)
/// 5. Objects placed ON terrain surface as PlacedObjects, NOT voxelized

use crate::coordinates::GPS;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::sync::Mutex;

// ── Data types ────────────────────────────────────────────────────────────────

/// Road classification from OSM highway tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    pub fn from_highway_tag(tag: &str) -> Self {
        match tag {
            "motorway" | "motorway_link"        => Self::Motorway,
            "trunk" | "trunk_link"              => Self::Trunk,
            "primary" | "primary_link"          => Self::Primary,
            "secondary" | "secondary_link"      => Self::Secondary,
            "tertiary" | "tertiary_link"        => Self::Tertiary,
            "residential" | "living_street"
            | "unclassified"                    => Self::Residential,
            "service"                           => Self::Service,
            "footway" | "path" | "pedestrian"   => Self::Path,
            "cycleway"                          => Self::Cycleway,
            other                               => Self::Other(other.to_string()),
        }
    }

    /// Nominal carriageway width in metres.
    pub fn width_m(&self) -> f64 {
        match self {
            Self::Motorway    => 12.0,
            Self::Trunk       => 10.0,
            Self::Primary     => 8.0,
            Self::Secondary   => 7.0,
            Self::Tertiary    => 6.0,
            Self::Residential => 6.0,
            Self::Service     => 4.0,
            Self::Path        => 2.0,
            Self::Cycleway    => 2.0,
            Self::Other(_)    => 5.0,
        }
    }

    /// Is this road type paved (gets streetlights)?
    pub fn is_paved(&self) -> bool {
        !matches!(self, Self::Path | Self::Cycleway | Self::Other(_))
    }
}

/// A building footprint from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmBuilding {
    pub osm_id: u64,
    /// Polygon vertices in GPS coords (lat, lon).
    pub polygon: Vec<GPS>,
    /// Estimated height in metres (from height or building:levels tags, or default).
    pub height_m: f64,
    /// OSM building tag value: "yes", "house", "commercial", "industrial", etc.
    pub building_type: String,
    pub levels: u8,
}

impl OsmBuilding {
    /// Centroid GPS position.
    pub fn centroid(&self) -> GPS {
        let n = self.polygon.len() as f64;
        if n == 0.0 { return GPS::new(0.0, 0.0, 0.0); }
        let lat = self.polygon.iter().map(|g| g.lat).sum::<f64>() / n;
        let lon = self.polygon.iter().map(|g| g.lon).sum::<f64>() / n;
        GPS::new(lat, lon, 0.0)
    }
}

/// A road way from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmRoad {
    pub osm_id: u64,
    /// Centreline node positions.
    pub nodes: Vec<GPS>,
    pub road_type: RoadType,
    pub name: Option<String>,
    pub is_bridge: bool,
    pub is_tunnel: bool,
    /// OSM layer tag (-5..+5, default 0).
    pub layer: i8,
}

/// A water polygon (lake, river, ocean) from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmWater {
    pub osm_id: u64,
    pub polygon: Vec<GPS>,
    pub name: Option<String>,
    pub water_type: String, // "lake", "river", "ocean", "reservoir", etc.
}

/// A park or leisure area from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmPark {
    pub osm_id: u64,
    pub polygon: Vec<GPS>,
    pub name: Option<String>,
}

/// An amenity node from OSM (bench, bin, traffic_signal, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmAmenity {
    pub osm_id: u64,
    pub lat: f64,
    pub lon: f64,
    pub amenity: String,    // "bench", "waste_basket", "traffic_signals", etc.
    pub name: Option<String>,
}

/// All OSM features for a geographic bounding box.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsmData {
    pub buildings: Vec<OsmBuilding>,
    pub roads: Vec<OsmRoad>,
    pub water: Vec<OsmWater>,
    pub parks: Vec<OsmPark>,
    pub amenities: Vec<OsmAmenity>,
}

impl OsmData {
    pub fn is_empty(&self) -> bool {
        self.buildings.is_empty()
            && self.roads.is_empty()
            && self.water.is_empty()
            && self.parks.is_empty()
            && self.amenities.is_empty()
    }
}

// ── Overpass client ───────────────────────────────────────────────────────────

/// Global shared rate limiter — one Instant shared across ALL chunk fetch calls.
/// Without this, every call gets a fresh client and the 2s cooldown does nothing.
static LAST_REQUEST: std::sync::OnceLock<Mutex<Option<Instant>>> = std::sync::OnceLock::new();

fn wait_global_cooldown() {
    let cell = LAST_REQUEST.get_or_init(|| Mutex::new(None));
    let mut last = cell.lock().unwrap();
    if let Some(t) = *last {
        let elapsed = t.elapsed();
        let cooldown = Duration::from_secs(2);
        if elapsed < cooldown {
            std::thread::sleep(cooldown - elapsed);
        }
    }
    *last = Some(Instant::now());
}

/// Fetch features in a bounding box from Overpass API.
/// Query is intentionally minimal — buildings and main roads only.
/// Water / parks / amenities are skipped to keep each packet small.
pub fn query_overpass(south: f64, west: f64, north: f64, east: f64)
    -> Result<String, String>
{
    wait_global_cooldown();

    // Only query buildings and navigable roads — 2 feature types, small response.
    let query = format!(
        "[out:json][timeout:25];\n(\
          way[\"building\"]({s},{w},{n},{e});\
          way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary|residential|service|living_street|unclassified)$\"]({s},{w},{n},{e});\
          node[\"amenity\"~\"^(bench|waste_basket|street_lamp|traffic_signals|post_box)$\"]({s},{w},{n},{e});\
        );\nout body;\n>;\nout skel qt;",
        s = south, w = west, n = north, e = east,
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("metaverse-core/0.1 (planet-scale metaverse)")
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post("https://overpass-api.de/api/interpreter")
        .body(query)
        .send()
        .map_err(|e| format!("Overpass request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Overpass HTTP {}", resp.status()));
    }
    resp.text().map_err(|e| e.to_string())
}

// ── JSON parser ───────────────────────────────────────────────────────────────

/// Parse a raw Overpass JSON response into `OsmData`.
pub fn parse_overpass_json(json_str: &str) -> Result<OsmData, String> {
    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    let elements = v["elements"].as_array()
        .ok_or("Missing 'elements' array")?;

    // First pass: build node id → GPS lookup
    let mut node_map: std::collections::HashMap<u64, GPS> = std::collections::HashMap::new();
    for elem in elements {
        if elem["type"].as_str() == Some("node") {
            let id  = elem["id"].as_u64().unwrap_or(0);
            let lat = elem["lat"].as_f64().unwrap_or(0.0);
            let lon = elem["lon"].as_f64().unwrap_or(0.0);
            node_map.insert(id, GPS::new(lat, lon, 0.0));

            // Amenity nodes
            if let Some(amenity) = elem["tags"]["amenity"].as_str() {
                // collected in second pass via same element
                let _ = (id, amenity);
            }
        }
    }

    let mut data = OsmData::default();

    // Second pass: ways and amenity nodes
    for elem in elements {
        let elem_type = elem["type"].as_str().unwrap_or("");
        let id = elem["id"].as_u64().unwrap_or(0);
        let tags = &elem["tags"];

        match elem_type {
            "node" => {
                if let Some(amenity) = tags["amenity"].as_str() {
                    let lat = elem["lat"].as_f64().unwrap_or(0.0);
                    let lon = elem["lon"].as_f64().unwrap_or(0.0);
                    data.amenities.push(OsmAmenity {
                        osm_id: id,
                        lat, lon,
                        amenity: amenity.to_string(),
                        name: tags["name"].as_str().map(|s| s.to_string()),
                    });
                }
            }
            "way" => {
                let node_ids: Vec<u64> = elem["nodes"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
                    .unwrap_or_default();

                let nodes: Vec<GPS> = node_ids.iter()
                    .filter_map(|nid| node_map.get(nid).copied())
                    .collect();

                if tags["building"].as_str().is_some() {
                    // Building
                    let height_m = tags["height"].as_str()
                        .and_then(|h| h.trim_end_matches('m').trim().parse::<f64>().ok())
                        .unwrap_or_else(|| {
                            let levels = tags["building:levels"].as_str()
                                .and_then(|l| l.parse::<f64>().ok())
                                .unwrap_or(2.0);
                            levels * 3.0
                        });
                    let levels = tags["building:levels"].as_str()
                        .and_then(|l| l.parse::<u8>().ok())
                        .unwrap_or(2);
                    data.buildings.push(OsmBuilding {
                        osm_id: id,
                        polygon: nodes,
                        height_m,
                        building_type: tags["building"].as_str().unwrap_or("yes").to_string(),
                        levels,
                    });
                } else if let Some(highway) = tags["highway"].as_str() {
                    // Road
                    data.roads.push(OsmRoad {
                        osm_id: id,
                        nodes,
                        road_type: RoadType::from_highway_tag(highway),
                        name: tags["name"].as_str().map(|s| s.to_string()),
                        is_bridge: tags["bridge"].as_str() == Some("yes"),
                        is_tunnel: tags["tunnel"].as_str() == Some("yes"),
                        layer: tags["layer"].as_str()
                            .and_then(|l| l.parse::<i8>().ok())
                            .unwrap_or(0),
                    });
                } else if tags["natural"].as_str() == Some("water")
                    || tags["waterway"].as_str().is_some()
                {
                    data.water.push(OsmWater {
                        osm_id: id,
                        polygon: nodes,
                        name: tags["name"].as_str().map(|s| s.to_string()),
                        water_type: tags["water"].as_str()
                            .or(tags["waterway"].as_str())
                            .unwrap_or("water")
                            .to_string(),
                    });
                } else if tags["leisure"].as_str() == Some("park") {
                    data.parks.push(OsmPark {
                        osm_id: id,
                        polygon: nodes,
                        name: tags["name"].as_str().map(|s| s.to_string()),
                    });
                }
            }
            _ => {}
        }
    }

    Ok(data)
}

// ── Disk cache ────────────────────────────────────────────────────────────────

/// Cache OSM query results to disk as JSON files.
/// Key is derived from the bounding box rounded to 3 decimal places.
pub struct OsmDiskCache {
    dir: PathBuf,
}

impl OsmDiskCache {
    pub fn new(dir: &Path) -> Self {
        let _ = fs::create_dir_all(dir);
        Self { dir: dir.to_owned() }
    }

    fn path(&self, s: f64, w: f64, n: f64, e: f64) -> PathBuf {
        let name = format!("osm_{:.4}_{:.4}_{:.4}_{:.4}.json", s, w, n, e);
        self.dir.join(name)
    }

    pub fn load(&self, s: f64, w: f64, n: f64, e: f64) -> Option<OsmData> {
        let p = self.path(s, w, n, e);
        let bytes = fs::read(&p).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, s: f64, w: f64, n: f64, e: f64, data: &OsmData) {
        let p = self.path(s, w, n, e);
        if let Ok(json) = serde_json::to_vec_pretty(data) {
            let _ = fs::write(p, json);
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Fetch OSM data for a bounding box. Checks disk cache first.
/// `cache_dir` is typically `world_data/osm/`.
///
/// Overpass API is only queried if the env var `METAVERSE_OVERPASS=1` is set.
/// Default: return empty (use heuristics) if no local tile cache.
/// This avoids blocking the game thread on 30s network timeouts.
pub fn fetch_osm_for_bounds(
    south: f64, west: f64, north: f64, east: f64,
    cache_dir: &Path,
) -> Result<OsmData, String> {
    let cache = OsmDiskCache::new(cache_dir);

    // Cache hit — instant
    if let Some(cached) = cache.load(south, west, north, east) {
        return Ok(cached);
    }

    // No local tile — only hit Overpass if explicitly enabled.
    // Without this guard the game thread blocks 30s per chunk on a timeout.
    if std::env::var("METAVERSE_OVERPASS").as_deref() != Ok("1") {
        return Err("no local tile (place PBF at world_data/map.osm.pbf)".into());
    }

    // Overpass path (opt-in only)
    println!("🗺️  Fetching OSM ({:.4},{:.4})→({:.4},{:.4})…", south, west, north, east);
    let json = query_overpass(south, west, north, east)?;
    let data = parse_overpass_json(&json)?;
    if !data.is_empty() {
        println!("   b:{} r:{} a:{}", data.buildings.len(), data.roads.len(), data.amenities.len());
    }
    cache.save(south, west, north, east, &data);
    Ok(data)
}

/// Fetch OSM data for a chunk using its GPS bounds.
/// Returns empty OsmData if the query fails (graceful degradation).
///
/// ## Tile strategy — why we don't query per-chunk
/// Each game chunk is ~150m × 300m. Querying Overpass once per chunk hammers
/// the public API and causes 504s. Instead we snap to a 0.01° tile (~1km²)
/// that covers ~25 chunks, fetch and cache that once, then clip to the
/// specific chunk bounds. One network request covers a whole neighbourhood.
pub fn fetch_osm_for_chunk(
    chunk_lat_min: f64, chunk_lat_max: f64,
    chunk_lon_min: f64, chunk_lon_max: f64,
    cache_dir: &Path,
) -> OsmData {
    // Snap the chunk centre to the nearest 0.01° tile
    let chunk_lat_centre = (chunk_lat_min + chunk_lat_max) * 0.5;
    let chunk_lon_centre = (chunk_lon_min + chunk_lon_max) * 0.5;
    let tile_size = 0.01; // ~1.1km — covers ~25 game chunks per tile
    let tile_s = (chunk_lat_centre / tile_size).floor() * tile_size;
    let tile_w = (chunk_lon_centre / tile_size).floor() * tile_size;
    let tile_n = tile_s + tile_size;
    let tile_e = tile_w + tile_size;

    // Fetch the whole tile (instant if cached; empty if no local data)
    let tile = match fetch_osm_for_bounds(tile_s, tile_w, tile_n, tile_e, cache_dir) {
        Ok(data) => data,
        Err(_) => {
            // No local tile — inference will use GPS heuristics instead.
            // Not a warning: this is normal when the PBF hasn't been indexed yet.
            return OsmData::default();
        }
    };

    // Clip tile features to this chunk's bounds (+ small margin)
    let margin = 0.0003;
    let lat_min = chunk_lat_min - margin;
    let lat_max = chunk_lat_max + margin;
    let lon_min = chunk_lon_min - margin;
    let lon_max = chunk_lon_max + margin;
    clip_osm_to_bounds(tile, lat_min, lat_max, lon_min, lon_max)
}

/// Filter an OsmData to only features that intersect a bounding box.
fn clip_osm_to_bounds(
    data: OsmData,
    lat_min: f64, lat_max: f64,
    lon_min: f64, lon_max: f64,
) -> OsmData {
    let in_box = |lat: f64, lon: f64| {
        lat >= lat_min && lat <= lat_max && lon >= lon_min && lon <= lon_max
    };

    OsmData {
        buildings: data.buildings.into_iter().filter(|b| {
            let c = b.centroid();
            in_box(c.lat, c.lon)
        }).collect(),

        roads: data.roads.into_iter().filter(|r| {
            r.nodes.iter().any(|n| in_box(n.lat, n.lon))
        }).collect(),

        water: data.water.into_iter().filter(|w| {
            w.polygon.iter().any(|n| in_box(n.lat, n.lon))
        }).collect(),

        parks: data.parks.into_iter().filter(|p| {
            p.polygon.iter().any(|n| in_box(n.lat, n.lon))
        }).collect(),

        amenities: data.amenities.into_iter().filter(|a| {
            in_box(a.lat, a.lon)
        }).collect(),
    }
}
