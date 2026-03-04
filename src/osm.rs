/// OpenStreetMap data fetching and per-chunk caching.
///
/// Queries the Overpass API for buildings, roads, waterways, and parks
/// within a chunk's GPS bounds. Results are cached to disk in binary
/// (bincode) format so subsequent loads are instant.
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
    /// Inner rings (holes) — land masses inside river bends / islands.
    /// A point is water only if inside `polygon` AND outside all `holes`.
    #[serde(default)]
    pub holes: Vec<Vec<GPS>>,
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

/// A railway line from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmRailway {
    pub osm_id: u64,
    pub nodes: Vec<GPS>,
    pub railway_type: String, // "rail", "tram", "subway", "light_rail", "monorail", "narrow_gauge", "funicular", "disused", etc.
    pub name: Option<String>,
    pub is_bridge: bool,
    pub is_tunnel: bool,
    pub layer: i8,
}

/// A land area from OSM (landuse, natural areas, leisure areas).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmLandArea {
    pub osm_id: u64,
    pub polygon: Vec<GPS>,
    pub name: Option<String>,
    pub area_type: String, // "forest", "wood", "farmland", "residential", "industrial", "commercial",
                           // "grass", "meadow", "scrub", "heath", "orchard", "vineyard", "cemetery",
                           // "military", "quarry", "beach", "sand", "bare_rock", "cliff", "wetland",
                           // "glacier", "pitch", "golf_course", "playground", "stadium", "sports_centre", etc.
    pub category: String,  // "landuse", "natural", "leisure", "tourism"
}

/// A barrier from OSM (walls, fences, hedges).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmBarrier {
    pub osm_id: u64,
    pub nodes: Vec<GPS>,
    pub barrier_type: String, // "wall", "fence", "hedge", "retaining_wall", "kerb", "guard_rail", "city_wall"
}

/// A power infrastructure element from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmPower {
    pub osm_id: u64,
    pub nodes: Vec<GPS>,   // line nodes or single point (tower/pole)
    pub power_type: String, // "line", "cable", "tower", "pole", "substation", "plant"
    pub lat: f64,  // for nodes/points (tower, pole) — 0 for ways
    pub lon: f64,
}

/// An aeroway feature from OSM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmAeroway {
    pub osm_id: u64,
    pub polygon: Vec<GPS>,  // area features
    pub nodes: Vec<GPS>,    // line features (runway centreline, taxiway)
    pub aeroway_type: String, // "aerodrome", "runway", "taxiway", "apron", "hangar", "terminal", "helipad"
    pub name: Option<String>,
    pub is_area: bool,
}

/// All OSM features for a geographic bounding box.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsmData {
    pub buildings: Vec<OsmBuilding>,
    pub roads: Vec<OsmRoad>,
    #[serde(default)]
    pub railways: Vec<OsmRailway>,
    pub water: Vec<OsmWater>,
    /// Open (non-closed) waterway centrelines — river/canal/etc. ways that are
    /// mapped as lines rather than area polygons.  Used as a fallback water
    /// detector when no polygon covers a column: a column within
    /// `WATERWAY_HALF_WIDTH_DEG` of any centreline segment is treated as water.
    #[serde(default)]
    pub waterway_lines: Vec<Vec<GPS>>,
    pub parks: Vec<OsmPark>,
    #[serde(default)]
    pub land_areas: Vec<OsmLandArea>,
    #[serde(default)]
    pub barriers: Vec<OsmBarrier>,
    #[serde(default)]
    pub power: Vec<OsmPower>,
    #[serde(default)]
    pub aeroways: Vec<OsmAeroway>,
    pub amenities: Vec<OsmAmenity>,
}

impl OsmData {
    pub fn is_empty(&self) -> bool {
        self.buildings.is_empty()
            && self.roads.is_empty()
            && self.railways.is_empty()
            && self.water.is_empty()
            && self.waterway_lines.is_empty()
            && self.parks.is_empty()
            && self.land_areas.is_empty()
            && self.barriers.is_empty()
            && self.power.is_empty()
            && self.aeroways.is_empty()
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
        let cooldown = Duration::from_secs(5);
        if elapsed < cooldown {
            std::thread::sleep(cooldown - elapsed);
        }
    }
    *last = Some(Instant::now());
}

/// Overpass API mirror endpoints — tried in order on failure.
const OVERPASS_ENDPOINTS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
];

/// Fetch features in a bounding box from Overpass API.
/// Query is intentionally minimal — buildings and main roads only.
/// Water / parks / amenities are skipped to keep each packet small.
pub fn query_overpass(south: f64, west: f64, north: f64, east: f64, endpoints: &[String])
    -> Result<String, String>
{
    wait_global_cooldown();

    // Water queries use an expanded bbox (buf=0.005°) so large river/bay polygons whose
    // member-way nodes lie just outside the strict tile boundary are still captured.
    let buf = 0.005_f64;
    let ws = south - buf;
    let ww = west - buf;
    let wn = north + buf;
    let we = east + buf;
    let query = format!(
        "[out:json][timeout:60];\n(\
          way[\"building\"]({s},{w},{n},{e});\
          way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary|residential|service|living_street|unclassified)$\"]({s},{w},{n},{e});\
          node[\"amenity\"]({s},{w},{n},{e});\
          way[\"natural\"=\"water\"]({ws},{ww},{wn},{we});\
          way[\"natural\"=\"riverbank\"]({ws},{ww},{wn},{we});\
          way[\"waterway\"~\"^(river|canal|reservoir|dock|riverbank)$\"]({ws},{ww},{wn},{we});\
          relation[\"type\"=\"multipolygon\"][\"natural\"=\"water\"]({ws},{ww},{wn},{we});\
          relation[\"type\"=\"multipolygon\"][\"waterway\"=\"river\"]({ws},{ww},{wn},{we});\
          relation[\"type\"=\"multipolygon\"][\"natural\"=\"riverbank\"]({ws},{ww},{wn},{we});\
          way[\"railway\"]({s},{w},{n},{e});\
          way[\"barrier\"~\"^(wall|fence|hedge|retaining_wall|guard_rail|city_wall)$\"]({s},{w},{n},{e});\
          way[\"power\"~\"^(line|cable|minor_line)$\"]({s},{w},{n},{e});\
          node[\"power\"~\"^(tower|pole)$\"]({s},{w},{n},{e});\
          way[\"aeroway\"]({s},{w},{n},{e});\
          way[\"landuse\"~\"^(forest|farmland|residential|industrial|commercial|retail|grass|meadow|scrub|heath|orchard|vineyard|cemetery|military|quarry)$\"]({ws},{ww},{wn},{we});\
          way[\"natural\"~\"^(wood|scrub|heath|grassland|beach|sand|bare_rock|cliff|wetland|glacier)$\"]({ws},{ww},{wn},{we});\
          way[\"leisure\"~\"^(pitch|golf_course|playground|stadium|sports_centre|marina|swimming_pool)$\"]({s},{w},{n},{e});\
        );\nout body;\n>;\nout skel qt;",
        s = south, w = west, n = north, e = east,
        ws = ws, ww = ww, wn = wn, we = we,
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("metaverse-core/0.1 (planet-scale metaverse)")
        .build()
        .map_err(|e| e.to_string())?;

    let effective: Vec<&str> = if endpoints.is_empty() {
        OVERPASS_ENDPOINTS.to_vec()
    } else {
        endpoints.iter().map(|s| s.as_str()).collect()
    };

    let mut last_err = String::new();
    for endpoint in &effective {
        let result = client
            .post(*endpoint)
            .body(query.clone())
            .send();
        match result {
            Ok(resp) if resp.status().is_success() => {
                return resp.text().map_err(|e| e.to_string());
            }
            Ok(resp) => {
                last_err = format!("Overpass {} HTTP {}", endpoint, resp.status());
            }
            Err(e) => {
                last_err = format!("Overpass {} failed: {}", endpoint, e);
            }
        }
    }
    Err(last_err)
}

// ── JSON parser ───────────────────────────────────────────────────────────────

/// Parse a raw Overpass JSON response into `OsmData`.
pub fn parse_overpass_json(json_str: &str) -> Result<OsmData, String> {
    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse error: {}", e))?;

    let elements = v["elements"].as_array()
        .ok_or("Missing 'elements' array")?;

    // Pass 1: build node id → GPS and way id → node list
    let mut node_map: std::collections::HashMap<u64, GPS> = std::collections::HashMap::new();
    let mut way_nodes_map: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
    for elem in elements {
        match elem["type"].as_str() {
            Some("node") => {
                let id  = elem["id"].as_u64().unwrap_or(0);
                let lat = elem["lat"].as_f64().unwrap_or(0.0);
                let lon = elem["lon"].as_f64().unwrap_or(0.0);
                node_map.insert(id, GPS::new(lat, lon, 0.0));
            }
            Some("way") => {
                let id = elem["id"].as_u64().unwrap_or(0);
                let node_ids: Vec<u64> = elem["nodes"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
                    .unwrap_or_default();
                way_nodes_map.insert(id, node_ids);
            }
            _ => {}
        }
    }

    let mut data = OsmData::default();

    // Pass 2: ways and amenity nodes
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
                if let Some(ptype) = tags["power"].as_str() {
                    if matches!(ptype, "tower" | "pole") {
                        let lat = elem["lat"].as_f64().unwrap_or(0.0);
                        let lon = elem["lon"].as_f64().unwrap_or(0.0);
                        data.power.push(OsmPower {
                            osm_id: id, nodes: vec![], power_type: ptype.to_string(), lat, lon,
                        });
                    }
                }
            }
            "way" => {
                let node_ids = way_nodes_map.get(&id).cloned().unwrap_or_default();
                let nodes: Vec<GPS> = node_ids.iter()
                    .filter_map(|nid| node_map.get(nid).copied())
                    .collect();

                if tags["building"].as_str().is_some() {
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
                    || tags["natural"].as_str() == Some("riverbank")
                    || tags["waterway"].as_str().is_some()
                {
                    // Closed way → area polygon; open way → centreline
                    let is_closed = nodes.len() >= 3
                        && nodes.first().map(|p| (p.lat, p.lon))
                            == nodes.last().map(|p| (p.lat, p.lon));
                    if is_closed || tags["natural"].as_str().is_some() {
                        data.water.push(OsmWater {
                            osm_id: id,
                            polygon: nodes,
                            holes: vec![],
                            name: tags["name"].as_str().map(|s| s.to_string()),
                            water_type: tags["water"].as_str()
                                .or(tags["waterway"].as_str())
                                .or(tags["natural"].as_str())
                                .unwrap_or("water")
                                .to_string(),
                        });
                    } else {
                        // Open waterway centreline — kept as a buffer fallback
                        data.waterway_lines.push(nodes);
                    }
                } else if tags["leisure"].as_str() == Some("park") {
                    data.parks.push(OsmPark {
                        osm_id: id,
                        polygon: nodes,
                        name: tags["name"].as_str().map(|s| s.to_string()),
                    });
                } else if let Some(rtype) = tags["railway"].as_str() {
                    data.railways.push(OsmRailway {
                        osm_id: id, nodes,
                        railway_type: rtype.to_string(),
                        name: tags["name"].as_str().map(|s| s.to_string()),
                        is_bridge: tags["bridge"].as_str().map(|s| s != "no").unwrap_or(false),
                        is_tunnel: tags["tunnel"].as_str().map(|s| s != "no").unwrap_or(false),
                        layer: tags["layer"].as_str().and_then(|l| l.parse().ok()).unwrap_or(0),
                    });
                } else if let Some(btype) = tags["barrier"].as_str() {
                    data.barriers.push(OsmBarrier {
                        osm_id: id, nodes, barrier_type: btype.to_string(),
                    });
                } else if let Some(ptype) = tags["power"].as_str() {
                    data.power.push(OsmPower {
                        osm_id: id, nodes, power_type: ptype.to_string(), lat: 0.0, lon: 0.0,
                    });
                } else if let Some(atype) = tags["aeroway"].as_str() {
                    let is_closed = !nodes.is_empty()
                        && nodes.first().map(|p| (p.lat, p.lon)) == nodes.last().map(|p| (p.lat, p.lon));
                    let is_area = is_closed;
                    let (polygon, line_nodes) = if is_area { (nodes, vec![]) } else { (vec![], nodes) };
                    data.aeroways.push(OsmAeroway {
                        osm_id: id, polygon, nodes: line_nodes,
                        aeroway_type: atype.to_string(),
                        name: tags["name"].as_str().map(|s| s.to_string()),
                        is_area,
                    });
                } else if let Some(luse) = tags["landuse"].as_str() {
                    data.land_areas.push(OsmLandArea {
                        osm_id: id, polygon: nodes,
                        name: tags["name"].as_str().map(|s| s.to_string()),
                        area_type: luse.to_string(), category: "landuse".to_string(),
                    });
                } else if let Some(nat) = tags["natural"].as_str() {
                    if matches!(nat, "wood" | "scrub" | "heath" | "grassland" | "beach" | "sand" | "bare_rock" | "cliff" | "wetland" | "glacier") {
                        data.land_areas.push(OsmLandArea {
                            osm_id: id, polygon: nodes,
                            name: tags["name"].as_str().map(|s| s.to_string()),
                            area_type: nat.to_string(), category: "natural".to_string(),
                        });
                    }
                } else if let Some(leis) = tags["leisure"].as_str() {
                    if matches!(leis, "pitch" | "golf_course" | "playground" | "stadium" | "sports_centre" | "marina" | "swimming_pool") {
                        data.land_areas.push(OsmLandArea {
                            osm_id: id, polygon: nodes,
                            name: tags["name"].as_str().map(|s| s.to_string()),
                            area_type: leis.to_string(), category: "leisure".to_string(),
                        });
                    }
                }
            }
            "relation" => {
                // Handle multipolygon water relations (e.g. Brisbane River)
                if tags["type"].as_str() == Some("multipolygon")
                    && (tags["natural"].as_str() == Some("water")
                        || tags["waterway"].as_str().is_some())
                {
                    let members = match elem["members"].as_array() {
                        Some(m) => m,
                        None => continue,
                    };
                    // Collect outer ways (stitched into one polygon) and inner ways
                    // (each inner way = one hole; they're typically already closed rings).
                    let mut outer_ways: Vec<Vec<GPS>> = Vec::new();
                    let mut holes: Vec<Vec<GPS>> = Vec::new();
                    for member in members {
                        let role = member["role"].as_str().unwrap_or("");
                        if role != "outer" && role != "inner" { continue; }
                        if let Some(way_id) = member["ref"].as_u64() {
                            if let Some(nids) = way_nodes_map.get(&way_id) {
                                let pts: Vec<GPS> = nids.iter()
                                    .filter_map(|nid| node_map.get(nid).copied())
                                    .collect();
                                if pts.len() >= 3 {
                                    if role == "outer" {
                                        outer_ways.push(pts);
                                    } else {
                                        holes.push(pts);
                                    }
                                }
                            }
                        }
                    }
                    // Stitch outer ways into a single polygon ring
                    let polygon = stitch_ways(outer_ways);
                    if polygon.len() >= 3 {
                        data.water.push(OsmWater {
                            osm_id: id,
                            polygon,
                            holes,
                            name: tags["name"].as_str().map(|s| s.to_string()),
                            water_type: tags["water"].as_str()
                                .or(tags["waterway"].as_str())
                                .unwrap_or("water")
                                .to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(data)
}

/// Stitch multiple ordered way chains into a single closed polygon ring.
/// Ways are chained end-to-start; if needed a way is reversed to connect.
fn stitch_ways(ways: Vec<Vec<GPS>>) -> Vec<GPS> {
    if ways.is_empty() { return vec![]; }
    let mut ring = ways[0].clone();
    for next_way in ways.into_iter().skip(1) {
        if next_way.is_empty() { continue; }
        let tail = ring.last().unwrap();
        let head = &next_way[0];
        let head_rev = next_way.last().unwrap();
        let dist_head = (tail.lat - head.lat).abs() + (tail.lon - head.lon).abs();
        let dist_rev  = (tail.lat - head_rev.lat).abs() + (tail.lon - head_rev.lon).abs();
        if dist_rev < dist_head {
            ring.extend(next_way.into_iter().rev().skip(1));
        } else {
            ring.extend(next_way.into_iter().skip(1));
        }
    }
    ring
}

// ── Disk cache ────────────────────────────────────────────────────────────────

const OSM_CACHE_VERSION: u32 = 6;

/// Cache OSM tile data on disk in binary (bincode) format.
/// Key is derived from the bounding box rounded to 4 decimal places.
pub struct OsmDiskCache {
    dir: PathBuf,
}

impl OsmDiskCache {
    pub fn new(dir: &Path) -> Self {
        let _ = fs::create_dir_all(dir);
        Self { dir: dir.to_owned() }
    }

    fn path(&self, s: f64, w: f64, n: f64, e: f64) -> PathBuf {
        let name = format!("osm_{:.4}_{:.4}_{:.4}_{:.4}.bin", s, w, n, e);
        self.dir.join(name)
    }

    pub fn exists(&self, s: f64, w: f64, n: f64, e: f64) -> bool {
        self.path(s, w, n, e).exists()
    }

    pub fn load(&self, s: f64, w: f64, n: f64, e: f64) -> Option<OsmData> {
        let bytes = fs::read(self.path(s, w, n, e)).ok()?;
        if bytes.len() < 4 { return None; }
        let version = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if version != OSM_CACHE_VERSION { return None; }
        bincode::deserialize(&bytes[4..]).ok()
    }

    pub fn save(&self, s: f64, w: f64, n: f64, e: f64, data: &OsmData) {
        if let Ok(payload) = bincode::serialize(data) {
            let mut bytes = OSM_CACHE_VERSION.to_le_bytes().to_vec();
            bytes.extend_from_slice(&payload);
            let _ = fs::write(self.path(s, w, n, e), bytes);
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Compute a stable DHT announce key for an OSM tile bounding box.
pub fn osm_dht_key(s: f64, w: f64, n: f64, e: f64) -> Vec<u8> {
    let s = format!("osm:{:.4}:{:.4}:{:.4}:{:.4}", s, w, n, e);
    sha2_stable_hash(s.as_bytes())
}

/// Stable non-cryptographic hash used for DHT keys (8 bytes).
pub fn sha2_stable_hash(data: &[u8]) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    h.finish().to_le_bytes().to_vec()
}

/// Fetch OSM data for a bounding box. Checks disk cache first.
/// `cache_dir` is typically `world_data/osm/`.
///
/// Overpass API is only queried if the env var `METAVERSE_OVERPASS=1` is set.
/// Default: return empty (use heuristics) if no local tile cache.
/// This avoids blocking the game thread on 30s network timeouts.
pub fn fetch_osm_for_bounds(
    south: f64, west: f64, north: f64, east: f64,
    cache_dir: &Path,
    overpass_endpoints: &[String],
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
    let json = query_overpass(south, west, north, east, overpass_endpoints)?;
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
///
/// ## Cross-tile water polygon fix
/// Large water polygons (rivers, bays) are stored only in the tile whose Overpass
/// query captured them. A polygon stored in tile N can extend far into tile N+1.
/// Chunks inside tile N+1 would miss the polygon entirely.
/// Fix: after loading the primary tile, merge water polygons from the four
/// neighbouring tiles (N/S/E/W). Buildings/roads/parks are NOT merged (they are
/// duplicated across tiles already and the primary tile is sufficient).
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

    // Clip bounds (+ small margin) used for all filtering below
    let margin = 0.0003;
    let lat_min = chunk_lat_min - margin;
    let lat_max = chunk_lat_max + margin;
    let lon_min = chunk_lon_min - margin;
    let lon_max = chunk_lon_max + margin;

    // Fetch the whole tile (instant if cached; empty if no local data)
    let tile = match fetch_osm_for_bounds(tile_s, tile_w, tile_n, tile_e, cache_dir, &[]) {
        Ok(data) => data,
        Err(_) => {
            // No local tile — inference will use GPS heuristics instead.
            // Not a warning: this is normal when the PBF hasn't been indexed yet.
            return OsmData::default();
        }
    };

    // Clip primary tile — buildings, roads, parks, amenities come only from here
    let mut result = clip_osm_to_bounds(tile, lat_min, lat_max, lon_min, lon_max);

    // Gather extra water polygons from the eight neighbouring tiles (cardinal + diagonal).
    // Large river/bay polygons are stored in whichever tile captured them; they
    // may extend far across tile boundaries so neighbouring tiles must be checked.
    let neighbour_offsets: [(f64, f64); 8] = [
        (-tile_size,  0.0),        // south
        ( tile_size,  0.0),        // north
        ( 0.0,       -tile_size),  // west
        ( 0.0,        tile_size),  // east
        (-tile_size, -tile_size),  // SW
        (-tile_size,  tile_size),  // SE
        ( tile_size, -tile_size),  // NW
        ( tile_size,  tile_size),  // NE
    ];
    for (dlat, dlon) in neighbour_offsets {
        let ns = tile_s + dlat;
        let nw = tile_w + dlon;
        let nn = ns + tile_size;
        let ne = nw + tile_size;
        if let Ok(nb_tile) = fetch_osm_for_bounds(ns, nw, nn, ne, cache_dir, &[]) {
            let poly_intersects = |pts: &[crate::coordinates::GPS]| -> bool {
                if pts.iter().any(|p| p.lat >= lat_min && p.lat <= lat_max
                                  && p.lon >= lon_min && p.lon <= lon_max) {
                    return true;
                }
                let p_lat_min = pts.iter().map(|p| p.lat).fold(f64::INFINITY, f64::min);
                let p_lat_max = pts.iter().map(|p| p.lat).fold(f64::NEG_INFINITY, f64::max);
                let p_lon_min = pts.iter().map(|p| p.lon).fold(f64::INFINITY, f64::min);
                let p_lon_max = pts.iter().map(|p| p.lon).fold(f64::NEG_INFINITY, f64::max);
                p_lat_min <= lat_max && p_lat_max >= lat_min
                    && p_lon_min <= lon_max && p_lon_max >= lon_min
            };
            for w in nb_tile.water {
                // Skip if already present (matched by osm_id)
                if result.water.iter().any(|x| x.osm_id == w.osm_id) {
                    continue;
                }
                if poly_intersects(&w.polygon) {
                    result.water.push(w);
                }
            }
            for line in nb_tile.waterway_lines {
                if poly_intersects(&line) {
                    result.waterway_lines.push(line);
                }
            }
        }
    }

    result
}

/// For chunks in the OSM data gap (no polygon water, has centreline), find the 4 "bracket"
/// points that define the water corridor:
///   ⟦ = 2 bank points where polygon coverage ENDS (upstream side)
///   ⟧ = 2 bank points where polygon coverage BEGINS (downstream side)
/// Returns a 4-point synthetic bridge polygon for PIP water detection in gap chunks.
///
/// Algorithm:
///   1. Compute dominant river direction from waterway centreline points.
///   2. Walk tiles in both directions until a tile with polygon water is found (up to 6 tiles).
///   3. At each end, collect polygon points and find the gap-facing boundary layer.
///   4. From that layer pick left bank (max perp) and right bank (min perp).
///   5. Return [left_up, right_up, right_down, left_down] as a convex bridge quad.
pub fn find_gap_bridge_polygon(
    chunk_lat_min: f64, chunk_lat_max: f64,
    chunk_lon_min: f64, chunk_lon_max: f64,
    waterway_lines: &[Vec<GPS>],
    cache_dir: &std::path::Path,
) -> Option<Vec<GPS>> {
    if waterway_lines.is_empty() { return None; }

    let tile_size = 0.01_f64;
    let chunk_lat_centre = (chunk_lat_min + chunk_lat_max) * 0.5;
    let chunk_lon_centre = (chunk_lon_min + chunk_lon_max) * 0.5;

    // Find dominant river direction from all centreline points (first→last overall vector)
    let all_pts: Vec<GPS> = waterway_lines.iter().flat_map(|l| l.iter().cloned()).collect();
    if all_pts.len() < 2 { return None; }
    let first = &all_pts[0];
    let last  = &all_pts[all_pts.len() - 1];
    let dlat = last.lat - first.lat;
    let dlon = last.lon - first.lon;
    let len  = (dlat * dlat + dlon * dlon).sqrt();
    if len < 1e-10 { return None; }

    let dir_lat  =  dlat / len;   // unit vector along river
    let dir_lon  =  dlon / len;
    let perp_lat = -dir_lon;      // unit vector perpendicular (cross-river)
    let perp_lon =  dir_lat;

    let project_along = |lat: f64, lon: f64| lat * dir_lat + lon * dir_lon;
    let project_perp  = |lat: f64, lon: f64| lat * perp_lat + lon * perp_lon;
    let chunk_proj = project_along(chunk_lat_centre, chunk_lon_centre);

    // Walk tiles in one river direction until polygon water found.
    // Returns (left_bank_GPS, right_bank_GPS) at the gap-facing edge of that tile.
    let find_bracket = |dir_sign: f64| -> Option<(GPS, GPS)> {
        let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
        let chunk_tile_s = (chunk_lat_centre / tile_size).floor() as i64;
        let chunk_tile_w = (chunk_lon_centre / tile_size).floor() as i64;
        seen.insert((chunk_tile_s, chunk_tile_w));

        for step in 1..=8usize {
            // Overshoot slightly so we always cross tile boundaries
            let offset = step as f64 * tile_size * 1.2;
            let check_lat = chunk_lat_centre + dir_lat * offset * dir_sign;
            let check_lon = chunk_lon_centre + dir_lon * offset * dir_sign;
            let snapped_s = (check_lat / tile_size).floor() * tile_size;
            let snapped_w = (check_lon / tile_size).floor() * tile_size;
            let tile_key  = (
                (snapped_s / tile_size).round() as i64,
                (snapped_w / tile_size).round() as i64,
            );
            if !seen.insert(tile_key) { continue; }   // already tried this tile

            if let Ok(tile_data) = fetch_osm_for_bounds(
                snapped_s, snapped_w,
                snapped_s + tile_size, snapped_w + tile_size,
                cache_dir, &[],
            ) {
                if tile_data.water.is_empty() { continue; }

                // Collect all polygon points from this tile
                let poly_pts: Vec<GPS> = tile_data.water.iter()
                    .flat_map(|w| w.polygon.iter().cloned())
                    .collect();
                if poly_pts.is_empty() { continue; }

                // Find the gap-facing boundary layer:
                // the cluster of points whose along-river projection is closest to the gap chunk.
                let min_dist = poly_pts.iter()
                    .map(|p| (project_along(p.lat, p.lon) - chunk_proj).abs())
                    .fold(f64::INFINITY, f64::min);

                // Keep all points within one tile-width of the closest group
                let boundary_pts: Vec<&GPS> = poly_pts.iter()
                    .filter(|p| {
                        (project_along(p.lat, p.lon) - chunk_proj).abs() <= min_dist + tile_size
                    })
                    .collect();
                if boundary_pts.is_empty() { continue; }

                // Left bank = max perpendicular projection, right bank = min
                let left = boundary_pts.iter()
                    .max_by(|a, b|
                        project_perp(a.lat, a.lon)
                            .partial_cmp(&project_perp(b.lat, b.lon))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    )?;
                let right = boundary_pts.iter()
                    .min_by(|a, b|
                        project_perp(a.lat, a.lon)
                            .partial_cmp(&project_perp(b.lat, b.lon))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    )?;

                return Some((**left, **right));
            }
        }
        None
    };

    let (left_a, right_a) = find_bracket(-1.0)?;  // ⟦ — upstream bracket
    let (left_b, right_b) = find_bracket( 1.0)?;  // ⟧ — downstream bracket

    // Bridge polygon: four corners in winding order for a valid PIP test
    Some(vec![left_a, right_a, right_b, left_b])
}

/// Ray-casting point-in-polygon test for GPS coordinates.
pub fn point_in_polygon(lat: f64, lon: f64, polygon: &[crate::coordinates::GPS]) -> bool {
    let n = polygon.len();
    if n < 3 { return false; }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (lat_i, lon_i) = (polygon[i].lat, polygon[i].lon);
        let (lat_j, lon_j) = (polygon[j].lat, polygon[j].lon);
        if ((lon_i > lon) != (lon_j > lon))
            && (lat < (lat_j - lat_i) * (lon - lon_i) / (lon_j - lon_i) + lat_i)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Half-width in degrees used as the buffer around open waterway centrelines
/// when doing neighbour-tile intersection filtering.
/// 0.002° ≈ 222m at Brisbane latitude.
pub const WATERWAY_HALF_WIDTH_DEG: f64 = 0.002;

/// Returns true if (lat, lon) is within `hw` degrees of any
/// segment of an open waterway centreline polyline.
/// Uses a squared perpendicular-distance test in lat/lon space.
pub fn point_near_waterway_line(lat: f64, lon: f64, line: &[GPS], hw: f64) -> bool {
    let hw2 = hw * hw;
    for seg in line.windows(2) {
        let (ax, ay) = (seg[0].lat, seg[0].lon);
        let (bx, by) = (seg[1].lat, seg[1].lon);
        let dx = bx - ax;
        let dy = by - ay;
        let len2 = dx * dx + dy * dy;
        // squared distance from (lat,lon) to segment AB
        let dist2 = if len2 < 1e-14 {
            // degenerate segment — just check distance to point A
            let ex = lat - ax; let ey = lon - ay;
            ex * ex + ey * ey
        } else {
            let t = ((lat - ax) * dx + (lon - ay) * dy) / len2;
            let t = t.clamp(0.0, 1.0);
            let px = ax + t * dx - lat;
            let py = ay + t * dy - lon;
            px * px + py * py
        };
        if dist2 <= hw2 { return true; }
    }
    false
}


fn clip_osm_to_bounds(
    data: OsmData,
    lat_min: f64, lat_max: f64,
    lon_min: f64, lon_max: f64,
) -> OsmData {
    let in_box = |lat: f64, lon: f64| {
        lat >= lat_min && lat <= lat_max && lon >= lon_min && lon <= lon_max
    };

    // A polygon/bbox intersects our box if any vertex is inside, OR the polygon
    // bbox overlaps ours (handles large river polygons that fully cover the chunk).
    let poly_intersects = |pts: &[GPS]| -> bool {
        if pts.iter().any(|n| in_box(n.lat, n.lon)) {
            return true;
        }
        // Check if polygon bbox overlaps chunk bbox
        let p_lat_min = pts.iter().map(|n| n.lat).fold(f64::INFINITY, f64::min);
        let p_lat_max = pts.iter().map(|n| n.lat).fold(f64::NEG_INFINITY, f64::max);
        let p_lon_min = pts.iter().map(|n| n.lon).fold(f64::INFINITY, f64::min);
        let p_lon_max = pts.iter().map(|n| n.lon).fold(f64::NEG_INFINITY, f64::max);
        p_lat_min <= lat_max && p_lat_max >= lat_min && p_lon_min <= lon_max && p_lon_max >= lon_min
    };

    OsmData {
        buildings: data.buildings.into_iter().filter(|b| {
            let c = b.centroid();
            in_box(c.lat, c.lon)
        }).collect(),

        roads: data.roads.into_iter().filter(|r| {
            r.nodes.iter().any(|n| in_box(n.lat, n.lon))
        }).collect(),

        railways: data.railways.into_iter().filter(|r| {
            r.nodes.iter().any(|n| in_box(n.lat, n.lon))
        }).collect(),

        // Water: include full polygon (unclipped) for any polygon whose bbox overlaps.
        // Clipping vertices breaks point_in_polygon tests (open polygon = wrong results).
        // Water is now rendered as voxels, so polygon vertex count per-chunk doesn't matter.
        water: data.water.into_iter().filter(|w| {
            poly_intersects(&w.polygon)
        }).collect(),

        // Waterway centrelines: keep any that have at least one node near the chunk.
        waterway_lines: data.waterway_lines.into_iter().filter(|pts| {
            poly_intersects(pts)
        }).collect(),

        parks: data.parks.into_iter().filter(|p| {
            poly_intersects(&p.polygon)
        }).collect(),

        land_areas: data.land_areas.into_iter().filter(|a| {
            poly_intersects(&a.polygon)
        }).collect(),

        barriers: data.barriers.into_iter().filter(|b| {
            b.nodes.iter().any(|n| in_box(n.lat, n.lon))
        }).collect(),

        power: data.power.into_iter().filter(|p| {
            if p.nodes.is_empty() { in_box(p.lat, p.lon) } else { p.nodes.iter().any(|n| in_box(n.lat, n.lon)) }
        }).collect(),

        aeroways: data.aeroways.into_iter().filter(|a| {
            if a.is_area { poly_intersects(&a.polygon) } else { a.nodes.iter().any(|n| in_box(n.lat, n.lon)) }
        }).collect(),

        amenities: data.amenities.into_iter().filter(|a| {
            in_box(a.lat, a.lon)
        }).collect(),
    }
}

/// Import OSM data from a PBF file into the tile cache.
///
/// Streams through the PBF, collects all nodes/ways/relations, then tiles them
/// into 0.01°×0.01° grid cells and saves each non-empty tile as a bincode cache file.
/// Already-cached tiles are skipped (idempotent).
/// Memory-bounded two-pass algorithm:
///   Pass 1: collect node IDs referenced by relevant ways only (~5-10% of all nodes).
///   Pass 2: load only those node coords, tile features immediately, flush every 2M features.
/// Returns the number of tiles written.
pub fn import_pbf_to_cache(pbf_path: &std::path::Path, cache_dir: &std::path::Path) -> Result<usize, String> {
    use std::collections::HashMap;
    use osmpbfreader::{OsmPbfReader, OsmObj};

    const TILE_SIZE: f64 = 0.01;
    const FLUSH_THRESHOLD: usize = 2_000_000; // flush tile accumulator every 2M pushed features

    fn snap(v: f64) -> f64 { (v / TILE_SIZE).floor() * TILE_SIZE }

    fn tile_key(lat: f64, lon: f64) -> (i64, i64) {
        ((snap(lat) / TILE_SIZE).round() as i64, (snap(lon) / TILE_SIZE).round() as i64)
    }

    fn bbox_tile_keys(coords: &[(f64, f64)]) -> Vec<(i64, i64)> {
        if coords.is_empty() { return vec![]; }
        let min_lat = coords.iter().map(|c| c.0).fold(f64::INFINITY, f64::min);
        let max_lat = coords.iter().map(|c| c.0).fold(f64::NEG_INFINITY, f64::max);
        let min_lon = coords.iter().map(|c| c.1).fold(f64::INFINITY, f64::min);
        let max_lon = coords.iter().map(|c| c.1).fold(f64::NEG_INFINITY, f64::max);
        let mut out = Vec::new();
        let mut s = (snap(min_lat) / TILE_SIZE).round() as i64;
        let s_max = (snap(max_lat) / TILE_SIZE).round() as i64;
        while s <= s_max {
            let mut w = (snap(min_lon) / TILE_SIZE).round() as i64;
            let w_max = (snap(max_lon) / TILE_SIZE).round() as i64;
            while w <= w_max {
                out.push((s, w));
                w += 1;
            }
            s += 1;
        }
        out
    }

    fn to_gps(coords: &[(f64, f64)]) -> Vec<crate::coordinates::GPS> {
        coords.iter().map(|&(lat, lon)| crate::coordinates::GPS::new(lat, lon, 0.0)).collect()
    }

    let cache = OsmDiskCache::new(cache_dir);

    let mut tiles: HashMap<(i64, i64), OsmData> = HashMap::new();
    let mut total_pushed: usize = 0;
    let mut written = 0usize;

    // Flush accumulated tiles to disk and clear the accumulator.
    let flush = |tiles: &mut HashMap<(i64,i64), OsmData>, written: &mut usize| {
        for ((s_i, w_i), data) in tiles.drain() {
            if data.is_empty() { continue; }
            let s = s_i as f64 * TILE_SIZE;
            let w = w_i as f64 * TILE_SIZE;
            let n = s + TILE_SIZE;
            let e = w + TILE_SIZE;
            if !cache.exists(s, w, n, e) {
                cache.save(s, w, n, e, &data);
                *written += 1;
            }
        }
    };

    // ── PASS 1: collect only the node IDs referenced by ways we care about ──────
    // Use a HashSet to deduplicate on insert (avoids collecting duplicates then sorting).
    let needed_ids: std::collections::HashSet<i64> = {
        let file = std::fs::File::open(pbf_path).map_err(|e| e.to_string())?;
        let mut reader = OsmPbfReader::new(file);
        let mut ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        for obj in reader.iter().filter_map(|r| r.ok()) {
            if let OsmObj::Way(w) = obj {
                if w.tags.contains_key("building")
                    || w.tags.contains_key("highway")
                    || w.tags.contains_key("natural")
                    || w.tags.contains_key("waterway")
                    || w.tags.contains_key("leisure")
                    || w.tags.contains_key("landuse")
                    || w.tags.contains_key("railway")
                    || w.tags.contains_key("barrier")
                    || w.tags.contains_key("power")
                    || w.tags.contains_key("aeroway")
                {
                    for nr in &w.nodes { ids.insert(nr.0); }
                }
            }
        }
        ids
    };

    // ── PASS 2a: load only needed node coords into a sorted Vec; also collect point features ──
    // Vec<(id, lat, lon)> sorted by id uses ~3x less RAM than HashMap<i64,(f32,f32)>
    // because HashMap has ~40 bytes of overhead per entry beyond the key+value.
    let mut node_coords: Vec<(i64, f32, f32)> = Vec::with_capacity(needed_ids.len());
    {
        let file = std::fs::File::open(pbf_path).map_err(|e| e.to_string())?;
        let mut reader = OsmPbfReader::new(file);
        for obj in reader.iter().filter_map(|r| r.ok()) {
            match obj {
                OsmObj::Node(n) => {
                    if needed_ids.contains(&n.id.0) {
                        node_coords.push((n.id.0, n.lat() as f32, n.lon() as f32));
                    }
                    // Point features: tile directly (lat/lon available without lookup)
                    if let Some(amenity) = n.tags.get("amenity") {
                        let tk = tile_key(n.lat(), n.lon());
                        tiles.entry(tk).or_default().amenities.push(OsmAmenity {
                            osm_id: n.id.0 as u64, lat: n.lat(), lon: n.lon(),
                            amenity: amenity.as_str().to_string(),
                            name: n.tags.get("name").map(|s| s.to_string()),
                        });
                        total_pushed += 1;
                    }
                    if let Some(power) = n.tags.get("power") {
                        let ptype = power.as_str();
                        if matches!(ptype, "tower" | "pole") {
                            let tk = tile_key(n.lat(), n.lon());
                            tiles.entry(tk).or_default().power.push(OsmPower {
                                osm_id: n.id.0 as u64, nodes: vec![],
                                power_type: ptype.to_string(), lat: n.lat(), lon: n.lon(),
                            });
                            total_pushed += 1;
                        }
                    }
                    if total_pushed >= FLUSH_THRESHOLD {
                        flush(&mut tiles, &mut written);
                        total_pushed = 0;
                    }
                }
                OsmObj::Way(_) | OsmObj::Relation(_) => break, // nodes always come first in PBF
            }
        }
    }
    // Sort by id for O(log n) binary_search lookup; drop needed_ids to free its RAM.
    node_coords.sort_unstable_by_key(|t| t.0);
    drop(needed_ids);

    // Lookup helper: binary search into sorted node_coords.
    let lookup = |id: i64| -> Option<(f64, f64)> {
        node_coords.binary_search_by_key(&id, |t| t.0)
            .ok()
            .map(|i| (node_coords[i].1 as f64, node_coords[i].2 as f64))
    };

    // ── PASS 2b: read ways only; look up node coords from sorted Vec ─────────────
    {
        let file = std::fs::File::open(pbf_path).map_err(|e| e.to_string())?;
        let mut reader = OsmPbfReader::new(file);

        for obj in reader.iter().filter_map(|r| r.ok()) {
            match obj {
                OsmObj::Node(_) => {} // already handled in pass 2a

                OsmObj::Way(w) => {
                    // Resolve node coords (f32 → f64 for GPS)
                    let coords: Vec<(f64, f64)> = w.nodes.iter()
                        .filter_map(|id| lookup(id.0))
                        .collect();
                    if coords.len() < 2 { continue; }

                    let tks = bbox_tile_keys(&coords);
                    if tks.is_empty() { continue; }
                    let gps = to_gps(&coords);
                    let n_tiles = tks.len();

                    if w.tags.contains_key("building") {
                        let btype = w.tags.get("building").map(|s| s.as_str()).unwrap_or("yes").to_string();
                        let levels: u8 = w.tags.get("building:levels").and_then(|s| s.parse().ok()).unwrap_or(2);
                        let height_m = levels as f64 * 3.0;
                        for tk in tks {
                            tiles.entry(tk).or_default().buildings.push(OsmBuilding {
                                osm_id: w.id.0 as u64, polygon: gps.clone(),
                                height_m, building_type: btype.clone(), levels,
                            });
                        }
                    } else if let Some(highway) = w.tags.get("highway") {
                        let hval = highway.as_str();
                        if !matches!(hval,
                            "motorway" | "motorway_link" | "trunk" | "trunk_link" |
                            "primary" | "primary_link" | "secondary" | "secondary_link" |
                            "tertiary" | "tertiary_link" | "residential" | "living_street" |
                            "unclassified" | "service" | "footway" | "path" | "pedestrian" | "cycleway"
                        ) { continue; }
                        let name = w.tags.get("name").map(|s| s.to_string());
                        let is_bridge = w.tags.get("bridge").map(|s| s != "no").unwrap_or(false);
                        let is_tunnel = w.tags.get("tunnel").map(|s| s != "no").unwrap_or(false);
                        let layer: i8 = w.tags.get("layer").and_then(|s| s.parse().ok()).unwrap_or(0);
                        let road_type = RoadType::from_highway_tag(hval);
                        for tk in tks {
                            tiles.entry(tk).or_default().roads.push(OsmRoad {
                                osm_id: w.id.0 as u64, nodes: gps.clone(), road_type: road_type.clone(),
                                name: name.clone(), is_bridge, is_tunnel, layer,
                            });
                        }
                    } else if let Some(natural) = w.tags.get("natural") {
                        let nval = natural.as_str();
                        if nval == "water" || nval == "riverbank" {
                            let name = w.tags.get("name").map(|s| s.to_string());
                            for tk in tks {
                                tiles.entry(tk).or_default().water.push(OsmWater {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(), holes: vec![],
                                    name: name.clone(), water_type: nval.to_string(),
                                });
                            }
                        } else if matches!(nval, "wood" | "scrub" | "heath" | "grassland" | "beach" | "sand" | "bare_rock" | "cliff" | "wetland" | "glacier") {
                            let name = w.tags.get("name").map(|s| s.to_string());
                            for tk in &tks {
                                tiles.entry(*tk).or_default().land_areas.push(OsmLandArea {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(),
                                    name: name.clone(), area_type: nval.to_string(), category: "natural".to_string(),
                                });
                            }
                        } else { continue; }
                    } else if let Some(waterway) = w.tags.get("waterway") {
                        let wval = waterway.as_str();
                        if !matches!(wval, "river" | "canal" | "stream" | "drain") { continue; }
                        let closed = coords.first() == coords.last() && coords.len() > 3;
                        if closed {
                            let name = w.tags.get("name").map(|s| s.to_string());
                            for tk in tks {
                                tiles.entry(tk).or_default().water.push(OsmWater {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(), holes: vec![],
                                    name: name.clone(), water_type: wval.to_string(),
                                });
                            }
                        } else {
                            for tk in tks {
                                tiles.entry(tk).or_default().waterway_lines.push(gps.clone());
                            }
                        }
                    } else if let Some(leisure) = w.tags.get("leisure") {
                        let lval = leisure.as_str();
                        let name = w.tags.get("name").map(|s| s.to_string());
                        if lval == "park" {
                            for tk in &tks {
                                tiles.entry(*tk).or_default().parks.push(OsmPark {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(), name: name.clone(),
                                });
                                tiles.entry(*tk).or_default().land_areas.push(OsmLandArea {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(),
                                    name: name.clone(), area_type: "park".to_string(), category: "leisure".to_string(),
                                });
                            }
                        } else if matches!(lval, "pitch" | "golf_course" | "playground" | "stadium" | "sports_centre" | "marina" | "swimming_pool") {
                            for tk in tks {
                                tiles.entry(tk).or_default().land_areas.push(OsmLandArea {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(),
                                    name: name.clone(), area_type: lval.to_string(), category: "leisure".to_string(),
                                });
                            }
                        } else { continue; }
                    } else if let Some(landuse) = w.tags.get("landuse") {
                        let lval = landuse.as_str();
                        let name = w.tags.get("name").map(|s| s.to_string());
                        let is_grass = lval == "grass";
                        if is_grass {
                            for tk in &tks {
                                tiles.entry(*tk).or_default().parks.push(OsmPark {
                                    osm_id: w.id.0 as u64, polygon: gps.clone(), name: name.clone(),
                                });
                            }
                        }
                        for tk in tks {
                            tiles.entry(tk).or_default().land_areas.push(OsmLandArea {
                                osm_id: w.id.0 as u64, polygon: gps.clone(),
                                name: name.clone(), area_type: lval.to_string(), category: "landuse".to_string(),
                            });
                        }
                    } else if let Some(railway) = w.tags.get("railway") {
                        let name = w.tags.get("name").map(|s| s.to_string());
                        let is_bridge = w.tags.get("bridge").map(|s| s != "no").unwrap_or(false);
                        let is_tunnel = w.tags.get("tunnel").map(|s| s != "no").unwrap_or(false);
                        let layer: i8 = w.tags.get("layer").and_then(|s| s.parse().ok()).unwrap_or(0);
                        let rtype = railway.as_str().to_string();
                        for tk in tks {
                            tiles.entry(tk).or_default().railways.push(OsmRailway {
                                osm_id: w.id.0 as u64, nodes: gps.clone(), railway_type: rtype.clone(),
                                name: name.clone(), is_bridge, is_tunnel, layer,
                            });
                        }
                    } else if let Some(barrier) = w.tags.get("barrier") {
                        let btype = barrier.as_str();
                        if !matches!(btype, "wall" | "fence" | "hedge" | "retaining_wall" | "guard_rail" | "city_wall") { continue; }
                        let btype = btype.to_string();
                        for tk in tks {
                            tiles.entry(tk).or_default().barriers.push(OsmBarrier {
                                osm_id: w.id.0 as u64, nodes: gps.clone(), barrier_type: btype.clone(),
                            });
                        }
                    } else if let Some(power) = w.tags.get("power") {
                        let ptype = power.as_str();
                        if !matches!(ptype, "line" | "cable" | "minor_line") { continue; }
                        let ptype = ptype.to_string();
                        for tk in tks {
                            tiles.entry(tk).or_default().power.push(OsmPower {
                                osm_id: w.id.0 as u64, nodes: gps.clone(),
                                power_type: ptype.clone(), lat: 0.0, lon: 0.0,
                            });
                        }
                    } else if let Some(aeroway) = w.tags.get("aeroway") {
                        let atype = aeroway.as_str().to_string();
                        let name = w.tags.get("name").map(|s| s.to_string());
                        let is_closed = coords.len() > 3 && coords.first() == coords.last();
                        let (polygon, nodes) = if is_closed { (gps.clone(), vec![]) } else { (vec![], gps.clone()) };
                        for tk in tks {
                            tiles.entry(tk).or_default().aeroways.push(OsmAeroway {
                                osm_id: w.id.0 as u64, polygon: polygon.clone(), nodes: nodes.clone(),
                                aeroway_type: atype.clone(), name: name.clone(), is_area: is_closed,
                            });
                        }
                    } else { continue; }

                    total_pushed += n_tiles;
                    if total_pushed >= FLUSH_THRESHOLD {
                        flush(&mut tiles, &mut written);
                        total_pushed = 0;
                    }
                }

                OsmObj::Relation(_) => {}
            }
        }
    }

    flush(&mut tiles, &mut written);
    Ok(written)
}
