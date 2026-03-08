/// World Inference — lazy object materialisation from real-world data
///
/// When a chunk is loaded for the first time, this module runs inference rules
/// over the chunk's GPS bounds and produces a list of world objects that
/// "should" exist there based on known real-world data patterns.
///
/// Rules are deterministic: same chunk coords → same objects → same IDs.
/// No central registry needed. Objects emerge as the world is explored.
///
/// IDs are content-addressed: SHA-256(lat_deg_x1e6 || lon_deg_x1e6 || type_tag || instance_idx)
/// truncated to 16 hex chars. Collision-resistant at this scale.
///
/// Discovered objects are written to the world-object DHT so every subsequent
/// visitor gets the same set without re-running inference.

use crate::chunk::ChunkId;
use crate::osm::{OsmData, RoadType};
use crate::world_objects::{PlacedObject, ObjectType};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// ── Deterministic ID ──────────────────────────────────────────────────────────

/// Generate a stable, deterministic object ID from its position and type.
/// Two nodes inferring the same object at the same coordinates get the same ID.
fn infer_id(lat_micro: i64, lon_micro: i64, type_tag: &str, instance: u32) -> String {
    let mut h = DefaultHasher::new();
    lat_micro.hash(&mut h);
    lon_micro.hash(&mut h);
    type_tag.hash(&mut h);
    instance.hash(&mut h);
    // Mix in a second pass for better avalanche
    let v1 = h.finish();
    let mut h2 = DefaultHasher::new();
    v1.hash(&mut h2);
    lat_micro.wrapping_add(lon_micro).hash(&mut h2);
    type_tag.len().hash(&mut h2);
    let v2 = h2.finish();
    format!("inf_{:016x}{:016x}", v1, v2)
}

// ── Inference Rules ───────────────────────────────────────────────────────────

/// The biome/terrain context inferred for a chunk — used to select rules.
#[derive(Debug, Clone, PartialEq)]
pub enum ChunkBiome {
    Urban,          // dense road network, buildings
    Suburban,       // sparse roads, residential buildings
    Rural,          // fields, farms, few roads
    Forest,
    Water,          // ocean, lake, river
    Mountain,
    Unknown,
}

/// Surface tags inferred from real-world data available at the time.
/// When `osm` is Some, inference uses real OSM features.
/// When `osm` is None, GPS heuristics provide degraded-but-functional inference.
pub struct ChunkContext {
    pub chunk_id: ChunkId,
    /// Centre of the chunk in degrees.
    pub lat: f64,
    pub lon: f64,
    /// Approximate biome.
    pub biome: ChunkBiome,
    /// OSM features for this chunk's bounding box. None = offline or not yet fetched.
    pub osm: Option<OsmData>,
    /// Whether the chunk has previously been inferred (fetched from DHT).
    pub already_discovered: bool,
}

impl ChunkContext {
    pub fn from_chunk(chunk_id: ChunkId) -> Self {
        let gps = chunk_id.center_gps();
        let biome = biome_heuristic(gps.lat, gps.lon);
        Self {
            chunk_id,
            lat: gps.lat,
            lon: gps.lon,
            biome,
            osm: None,
            already_discovered: false,
        }
    }

    pub fn with_osm(mut self, osm: OsmData) -> Self {
        // Upgrade biome estimate using real OSM data
        if !osm.roads.is_empty() || !osm.buildings.is_empty() {
            let dense = osm.buildings.len() > 5 || osm.roads.len() > 8;
            self.biome = if dense { ChunkBiome::Urban } else { ChunkBiome::Suburban };
        } else if !osm.water.is_empty() {
            self.biome = ChunkBiome::Water;
        } else if !osm.parks.is_empty() {
            self.biome = ChunkBiome::Forest;
        }
        self.osm = Some(osm);
        self
    }
}

/// Very rough biome heuristic from GPS coordinates alone.
/// Replace with OSM data lookup when available.
fn biome_heuristic(lat: f64, lon: f64) -> ChunkBiome {
    // Ocean approximation: very rough — any coord more than ~300km from any
    // coastline will be ocean. For now use lat/lon range heuristics.
    // Pacific ocean centre
    if lon < -120.0 && (lat < 60.0 && lat > -60.0) && lon > -180.0 {
        return ChunkBiome::Water;
    }
    // Atlantic
    if lon < -20.0 && lon > -60.0 && lat > 5.0 && lat < 65.0 {
        return ChunkBiome::Water;
    }
    // High latitude = tundra/mountain, treat as Rural for now
    if lat.abs() > 65.0 {
        return ChunkBiome::Rural;
    }
    // Default: Unknown (will improve with OSM)
    ChunkBiome::Unknown
}

// ── The Inference Engine ──────────────────────────────────────────────────────

/// A single inferred object — position is world-local metres from origin.
/// The caller is responsible for converting GPS → world coordinates.
#[derive(Debug, Clone)]
pub struct InferredObject {
    pub id: String,
    pub object_type: ObjectType,
    /// GPS position (lat, lon) so the caller can project to world coords.
    pub lat: f64,
    pub lon: f64,
    /// World-space Y (height above terrain) — 0.0 = ground level.
    pub y_offset: f32,
    /// Rotation around Y axis in radians (0 = +Z facing).
    pub rotation_y: f32,
    /// Human-readable label.
    pub label: String,
    /// Optional config payload (JSON string). Empty = use type defaults.
    pub config: String,
}

/// Run all inference rules for a chunk and return every object that should exist.
/// Returns an empty Vec if the chunk is ocean, ice cap, or otherwise empty.
pub fn infer_chunk_objects(ctx: &ChunkContext) -> Vec<InferredObject> {
    if ctx.already_discovered {
        return vec![];
    }

    // If we have real OSM data, use it — much more accurate than heuristics.
    if let Some(osm) = &ctx.osm {
        return infer_from_osm(ctx, osm);
    }

    // Fallback: GPS heuristics only (offline mode, no OSM data yet)
    let mut objects = Vec::new();
    match ctx.biome {
        ChunkBiome::Water => { infer_buoys(ctx, &mut objects); }
        ChunkBiome::Urban => {
            infer_streetlights(ctx, &mut objects);
            infer_traffic_lights(ctx, &mut objects);
            infer_benches(ctx, &mut objects);
            infer_bins(ctx, &mut objects);
            infer_letterboxes(ctx, &mut objects);
        }
        ChunkBiome::Suburban => {
            infer_streetlights(ctx, &mut objects);
            infer_letterboxes(ctx, &mut objects);
        }
        ChunkBiome::Rural => { infer_rural_markers(ctx, &mut objects); }
        ChunkBiome::Forest | ChunkBiome::Mountain => {}
        ChunkBiome::Unknown => { infer_streetlights(ctx, &mut objects); }
    }
    objects
}

// ── OSM-based inference ───────────────────────────────────────────────────────

/// Infer world objects from real OSM data.
///
/// - Buildings → one InferredObject at centroid (type = "building_residential", etc.)
/// - Roads → streetlights every ~40m along paved roads
/// - Road intersections → traffic lights at junction nodes
/// - Amenity nodes → direct placement (bench, bin, etc.)
fn infer_from_osm(ctx: &ChunkContext, osm: &OsmData) -> Vec<InferredObject> {
    let mut out = Vec::new();

    // Buildings
    for building in &osm.buildings {
        let c = building.centroid();
        let lat_micro = (c.lat * 1_000_000.0) as i64;
        let lon_micro = (c.lon * 1_000_000.0) as i64;
        let btype = building_model_type(&building.building_type, building.levels);
        let id = infer_id(lat_micro, lon_micro, &btype, 0);

        // Bounding box footprint in metres
        let (lat_min, lat_max, lon_min, lon_max) = building.polygon.iter().fold(
            (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
            |(la, lb, loa, lob), g| (la.min(g.lat), lb.max(g.lat), loa.min(g.lon), lob.max(g.lon)),
        );
        let cos_lat = c.lat.to_radians().cos();
        let width_m  = ((lon_max - lon_min) * 111_320.0 * cos_lat).max(3.0);
        let depth_m  = ((lat_max - lat_min) * 111_320.0).max(3.0);

        out.push(InferredObject {
            id,
            object_type: ObjectType::Custom(btype.clone()),
            lat: c.lat,
            lon: c.lon,
            y_offset: 0.0,
            rotation_y: 0.0,
            label: format!("Building ({})", building.building_type),
            config: format!(r#"{{"height":{:.1},"levels":{},"width":{:.1},"depth":{:.1}}}"#,
                            building.height_m, building.levels, width_m, depth_m),
        });
        // Letterbox for residential buildings
        if matches!(building.building_type.as_str(),
                    "house" | "residential" | "yes" | "detached" | "semi")
        {
            let box_lat = c.lat + 0.00005;
            let box_lon = c.lon;
            let blat_micro = (box_lat * 1_000_000.0) as i64;
            let blon_micro = (box_lon * 1_000_000.0) as i64;
            let lid = infer_id(blat_micro, blon_micro, "letterbox", 0);
            out.push(InferredObject {
                id: lid,
                object_type: ObjectType::Custom("letterbox".into()),
                lat: box_lat, lon: box_lon,
                y_offset: 0.0, rotation_y: 0.0,
                label: "Letterbox".into(),
                config: String::new(),
            });
        }
    }

    // Streetlights along paved roads (~40m spacing)
    let light_spacing_deg = 0.0004;
    for road in &osm.roads {
        if !road.road_type.is_paved() { continue; }
        let mut last_light_lat = f64::NAN;
        let mut last_light_lon = f64::NAN;
        for (i, node) in road.nodes.iter().enumerate() {
            if last_light_lat.is_nan()
                || dist_deg(last_light_lat, last_light_lon, node.lat, node.lon)
                    >= light_spacing_deg
            {
                let lat_micro = (node.lat * 1_000_000.0) as i64;
                let lon_micro = (node.lon * 1_000_000.0) as i64;
                let id = infer_id(lat_micro, lon_micro, "streetlight", 0);
                out.push(InferredObject {
                    id,
                    object_type: ObjectType::Custom("streetlight".into()),
                    lat: node.lat, lon: node.lon,
                    y_offset: 0.0, rotation_y: 0.0,
                    label: "Street Light".into(),
                    config: String::new(),
                });
                last_light_lat = node.lat;
                last_light_lon = node.lon;
            }
            // Traffic light at first road node that appears to be an intersection
            // (heuristic: primary/secondary node at start or end of way)
            if (i == 0 || i == road.nodes.len() - 1)
                && matches!(road.road_type,
                            RoadType::Primary | RoadType::Secondary | RoadType::Tertiary)
            {
                let lat_micro = (node.lat * 1_000_000.0) as i64;
                let lon_micro = (node.lon * 1_000_000.0) as i64;
                let id = infer_id(lat_micro, lon_micro, "traffic_light", 0);
                out.push(InferredObject {
                    id,
                    object_type: ObjectType::Custom("traffic_light".into()),
                    lat: node.lat, lon: node.lon,
                    y_offset: 0.0, rotation_y: 0.0,
                    label: "Traffic Light".into(),
                    config: r#"{"phase_ms":[30000,3000,27000],"mode":"normal"}"#.into(),
                });
            }
        }
    }

    // OSM amenity nodes
    for amenity in &osm.amenities {
        let object_type = amenity_to_object_type(&amenity.amenity);
        if object_type.is_empty() { continue; }
        let lat_micro = (amenity.lat * 1_000_000.0) as i64;
        let lon_micro = (amenity.lon * 1_000_000.0) as i64;
        let id = infer_id(lat_micro, lon_micro, &object_type, 0);
        out.push(InferredObject {
            id,
            object_type: ObjectType::Custom(object_type.clone()),
            lat: amenity.lat, lon: amenity.lon,
            y_offset: 0.0, rotation_y: 0.0,
            label: amenity.name.clone().unwrap_or_else(|| {
                object_type.replace('_', " ")
                           .split_whitespace()
                           .map(|w| {
                               let mut c = w.chars();
                               match c.next() {
                                   None => String::new(),
                                   Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                               }
                           })
                           .collect::<Vec<_>>()
                           .join(" ")
            }),
            config: String::new(),
        });
    }

    // Water buoys
    if !osm.water.is_empty() {
        infer_buoys(ctx, &mut out);
    }

    out
}

/// Map OSM building type to model type string.
fn building_model_type(osm_type: &str, levels: u8) -> String {
    match osm_type {
        "commercial" | "retail" | "shop" => "building_commercial".into(),
        "industrial" | "warehouse" | "factory" => "building_industrial".into(),
        "apartments" | "block" => {
            if levels > 8 { "building_skyscraper".into() }
            else { "building_residential".into() }
        }
        "office" | "civic" => {
            if levels > 6 { "building_skyscraper".into() }
            else { "building_commercial".into() }
        }
        _ => "building_residential".into(),
    }
}

/// Map OSM amenity tag to object type string. Empty string = skip.
fn amenity_to_object_type(amenity: &str) -> String {
    match amenity {
        "bench"                 => "bench".into(),
        "waste_basket" | "bin"  => "bin".into(),
        "traffic_signals"       => "traffic_light".into(),
        "street_lamp"           => "streetlight".into(),
        "post_box"              => "letterbox".into(),
        _                       => String::new(),
    }
}

/// Approximate degree distance between two points (no trig — for spacing comparison only).
fn dist_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    (dlat * dlat + dlon * dlon).sqrt()
}

// ── Heuristic rule implementations ───────────────────────────────────────────

/// Place streetlights in a grid across the chunk.
/// Real spacing: ~40m apart on urban roads. We approximate with a sparse grid.
fn infer_streetlights(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    // One streetlight per ~40m. Chunk is ~30 voxels = ~30m.
    // So 1-2 per chunk depending on density.
    let spacing_deg = 0.0004; // ~44m at equator
    let lat_start = (ctx.lat / spacing_deg).floor() * spacing_deg;
    let lon_start = (ctx.lon / spacing_deg).floor() * spacing_deg;

    for di in 0..2i64 {
        for dj in 0..2i64 {
            let lat = lat_start + di as f64 * spacing_deg;
            let lon = lon_start + dj as f64 * spacing_deg;
            let lat_micro = (lat * 1_000_000.0) as i64;
            let lon_micro = (lon * 1_000_000.0) as i64;
            let id = infer_id(lat_micro, lon_micro, "streetlight", 0);
            out.push(InferredObject {
                id,
                object_type: ObjectType::Custom("streetlight".into()),
                lat, lon,
                y_offset: 0.0,
                rotation_y: 0.0,
                label: "Street Light".into(),
                config: String::new(),
            });
        }
    }
}

/// Traffic lights at intersection-like positions.
/// Real intersections ~100-200m apart in urban grids.
fn infer_traffic_lights(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    let spacing_deg = 0.0009; // ~100m
    let lat_micro = ((ctx.lat / spacing_deg).round() * spacing_deg * 1_000_000.0) as i64;
    let lon_micro = ((ctx.lon / spacing_deg).round() * spacing_deg * 1_000_000.0) as i64;
    let id = infer_id(lat_micro, lon_micro, "traffic_light", 0);
    out.push(InferredObject {
        id,
        object_type: ObjectType::Custom("traffic_light".into()),
        lat: lat_micro as f64 / 1_000_000.0,
        lon: lon_micro as f64 / 1_000_000.0,
        y_offset: 0.0,
        rotation_y: 0.0,
        label: "Traffic Light".into(),
        config: r#"{"phase_ms":[30000,3000,27000],"mode":"normal"}"#.into(),
    });
}

fn infer_benches(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    let spacing_deg = 0.0006;
    let lat = (ctx.lat / spacing_deg).round() * spacing_deg + spacing_deg * 0.3;
    let lon = (ctx.lon / spacing_deg).round() * spacing_deg + spacing_deg * 0.1;
    let lat_micro = (lat * 1_000_000.0) as i64;
    let lon_micro = (lon * 1_000_000.0) as i64;
    let id = infer_id(lat_micro, lon_micro, "bench", 0);
    out.push(InferredObject {
        id,
        object_type: ObjectType::Custom("bench".into()),
        lat, lon,
        y_offset: 0.0,
        rotation_y: std::f32::consts::FRAC_PI_2,
        label: "Bench".into(),
        config: String::new(),
    });
}

fn infer_bins(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    let spacing_deg = 0.0005;
    let lat = (ctx.lat / spacing_deg).round() * spacing_deg + spacing_deg * 0.15;
    let lon = (ctx.lon / spacing_deg).round() * spacing_deg - spacing_deg * 0.2;
    let lat_micro = (lat * 1_000_000.0) as i64;
    let lon_micro = (lon * 1_000_000.0) as i64;
    let id = infer_id(lat_micro, lon_micro, "bin", 0);
    out.push(InferredObject {
        id,
        object_type: ObjectType::Custom("bin".into()),
        lat, lon,
        y_offset: 0.0,
        rotation_y: 0.0,
        label: "Rubbish Bin".into(),
        config: String::new(),
    });
}

fn infer_letterboxes(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    // One letterbox per ~15m — residential density
    let spacing_deg = 0.00015;
    let lat_start = (ctx.lat / spacing_deg).floor() * spacing_deg;
    let lon_start = (ctx.lon / spacing_deg).floor() * spacing_deg;
    for i in 0..3i64 {
        let lat = lat_start + i as f64 * spacing_deg;
        let lon = lon_start;
        let lat_micro = (lat * 1_000_000.0) as i64;
        let lon_micro = (lon * 1_000_000.0) as i64;
        let id = infer_id(lat_micro, lon_micro, "letterbox", i as u32);
        out.push(InferredObject {
            id,
            object_type: ObjectType::Custom("letterbox".into()),
            lat, lon,
            y_offset: 0.0,
            rotation_y: 0.0,
            label: format!("Letterbox #{}", i + 1),
            config: String::new(),
        });
    }
}

fn infer_buoys(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    let spacing_deg = 0.005; // ~550m — sparse in open water
    let lat = (ctx.lat / spacing_deg).round() * spacing_deg;
    let lon = (ctx.lon / spacing_deg).round() * spacing_deg;
    // Only place if the snapped position is within this chunk's bounds
    if (lat - ctx.lat).abs() > spacing_deg * 0.6 { return; }
    let lat_micro = (lat * 1_000_000.0) as i64;
    let lon_micro = (lon * 1_000_000.0) as i64;
    let id = infer_id(lat_micro, lon_micro, "buoy", 0);
    out.push(InferredObject {
        id,
        object_type: ObjectType::Custom("buoy".into()),
        lat, lon,
        y_offset: 0.0,
        rotation_y: 0.0,
        label: "Navigation Buoy".into(),
        config: String::new(),
    });
}

fn infer_rural_markers(ctx: &ChunkContext, out: &mut Vec<InferredObject>) {
    let spacing_deg = 0.003;
    let lat = (ctx.lat / spacing_deg).round() * spacing_deg;
    let lon = (ctx.lon / spacing_deg).round() * spacing_deg;
    if (lat - ctx.lat).abs() > spacing_deg * 0.6 { return; }
    let lat_micro = (lat * 1_000_000.0) as i64;
    let lon_micro = (lon * 1_000_000.0) as i64;
    let id = infer_id(lat_micro, lon_micro, "gate_post", 0);
    out.push(InferredObject {
        id,
        object_type: ObjectType::Custom("gate_post".into()),
        lat, lon,
        y_offset: 0.0,
        rotation_y: 0.0,
        label: "Gate Post".into(),
        config: String::new(),
    });
}

// ── PlacedObject conversion ───────────────────────────────────────────────────

/// Convert an InferredObject to a PlacedObject for DHT storage.
/// `world_pos` is the world-space (x, y, z) position in metres from origin.
pub fn to_placed_object(obj: &InferredObject, world_pos: [f32; 3]) -> PlacedObject {
    PlacedObject {
        id: obj.id.clone(),
        object_type: obj.object_type.clone(),
        position: world_pos,
        rotation_y: obj.rotation_y,
        scale: 1.0,
        content_key: obj.config.clone(),
        label: obj.label.clone(),
        placed_at: 0, // 0 = inferred (not player-placed)
        placed_by: "world_inference".into(),
    }
}

/// A road segment expressed as two GPS endpoints plus road width.
/// Callers project both ends to world coords and draw quads.
pub struct RoadSegmentGps {
    pub a_lat: f64,
    pub a_lon: f64,
    pub b_lat: f64,
    pub b_lon: f64,
    pub width_m: f32,
    pub road_type: crate::osm::RoadType,
    pub name: Option<String>,
    pub is_bridge: bool,
    pub is_tunnel: bool,
}

/// Extract all road segments from the OSM data for a chunk.
pub fn infer_road_segments(ctx: &ChunkContext) -> Vec<RoadSegmentGps> {
    let osm = match &ctx.osm { Some(o) => o, None => return vec![] };
    let mut out = Vec::new();
    for road in &osm.roads {
        let w = road.road_type.width_m() as f32;
        for pair in road.nodes.windows(2) {
            out.push(RoadSegmentGps {
                a_lat: pair[0].lat, a_lon: pair[0].lon,
                b_lat: pair[1].lat, b_lon: pair[1].lon,
                width_m: w,
                road_type: road.road_type.clone(),
                name: road.name.clone(),
                is_bridge: road.is_bridge,
                is_tunnel: road.is_tunnel,
            });
        }
    }
    out
}

// ── DHT key for chunk inference status ───────────────────────────────────────

/// DHT key that records whether a chunk has been through inference.
/// Presence of this key = inference already ran, fetch objects from chunk DHT key instead.
pub fn inference_status_key(chunk_id: &ChunkId) -> Vec<u8> {
    format!("world/inferred/{}/{}/{}", chunk_id.x, chunk_id.y, chunk_id.z).into_bytes()
}

// ── Railway segments ──────────────────────────────────────────────────────────

/// A railway segment for rendering (dual rails + sleepers).
pub struct RailwaySegmentGps {
    pub a_lat: f64,
    pub a_lon: f64,
    pub b_lat: f64,
    pub b_lon: f64,
    pub railway_type: String,
    pub is_bridge: bool,
    pub is_tunnel: bool,
    pub layer: i8,
}

/// Extract railway segments from OSM data for a chunk.
pub fn infer_railway_segments(ctx: &ChunkContext) -> Vec<RailwaySegmentGps> {
    let osm = match &ctx.osm { Some(o) => o, None => return vec![] };
    let mut out = Vec::new();
    for rail in &osm.railways {
        if rail.railway_type == "disused" || rail.railway_type == "razed" { continue; }
        for pair in rail.nodes.windows(2) {
            out.push(RailwaySegmentGps {
                a_lat: pair[0].lat, a_lon: pair[0].lon,
                b_lat: pair[1].lat, b_lon: pair[1].lon,
                railway_type: rail.railway_type.clone(),
                is_bridge: rail.is_bridge,
                is_tunnel: rail.is_tunnel,
                layer: rail.layer,
            });
        }
    }
    out
}

// ── Waterway segments (for rendering centrelines) ─────────────────────────────

/// A waterway centreline segment with type info for rendering.
pub struct WaterwaySegmentGps {
    pub a_lat: f64,
    pub a_lon: f64,
    pub b_lat: f64,
    pub b_lon: f64,
    pub waterway_type: String,
    pub width_m: f32,
    pub name: Option<String>,
}

/// Extract waterway centreline segments for rendering as water surface overlays.
pub fn infer_waterway_segments(ctx: &ChunkContext) -> Vec<WaterwaySegmentGps> {
    let osm = match &ctx.osm { Some(o) => o, None => return vec![] };
    let mut out = Vec::new();
    for wl in &osm.waterway_lines {
        let width_m = (wl.half_width_m() * 2.0) as f32;
        for pair in wl.nodes.windows(2) {
            out.push(WaterwaySegmentGps {
                a_lat: pair[0].lat, a_lon: pair[0].lon,
                b_lat: pair[1].lat, b_lon: pair[1].lon,
                waterway_type: wl.waterway_type.clone(),
                width_m,
                name: wl.name.clone(),
            });
        }
    }
    out
}

// ── Park and land area polygons ───────────────────────────────────────────────

/// A park or land-area polygon for rendering as a coloured surface overlay.
pub struct LandPolygonGps {
    pub polygon: Vec<crate::coordinates::GPS>,
    pub area_type: String,  // "park", "forest", "wood", "farmland", "grass", "meadow", etc.
    pub category: String,   // "leisure", "landuse", "natural"
    pub name: Option<String>,
}

/// Collect park and land-area polygons for the chunk.
pub fn infer_land_polygons(ctx: &ChunkContext) -> Vec<LandPolygonGps> {
    let osm = match &ctx.osm { Some(o) => o, None => return vec![] };
    let mut out = Vec::new();

    // Explicit parks
    for p in &osm.parks {
        if p.polygon.len() < 3 { continue; }
        out.push(LandPolygonGps {
            polygon: p.polygon.clone(),
            area_type: "park".into(),
            category: "leisure".into(),
            name: p.name.clone(),
        });
    }

    // Land use / natural areas (avoid duplicating parks already above)
    for la in &osm.land_areas {
        if la.polygon.len() < 3 { continue; }
        if la.area_type == "park" { continue; } // already added from parks vec
        out.push(LandPolygonGps {
            polygon: la.polygon.clone(),
            area_type: la.area_type.clone(),
            category: la.category.clone(),
            name: la.name.clone(),
        });
    }

    out
}
