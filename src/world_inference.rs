//! World-object inference from OSM data.
//!
//! Reads OSM features for a chunk and converts them into [`PlacedObject`]s
//! with deterministic IDs so every client produces the same scene without
//! any central authority.  Objects are anchored to the terrain surface by
//! scanning the octree top-down at the object's voxel (X, Z) position.
//!
//! # Rules implemented
//! | OSM tag              | Model name      |
//! |----------------------|-----------------|
//! | amenity=bench        | bench           |
//! | amenity=waste_basket | bin             |
//! | amenity=recycling    | bin             |
//! | amenity=post_box     | letterbox       |
//! | amenity=traffic_signals | traffic_light |
//! | highway=street_lamp  | streetlight     |
//! | highway=traffic_signals | traffic_light |
//! | power=pole           | streetlight     |
//! | every 30 m on paved roads | streetlight |

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::chunk::ChunkId;
use crate::coordinates::GPS;
use crate::materials::MaterialId;
use crate::osm::OsmData;
use crate::voxel::{Octree, VoxelCoord};
use crate::world_objects::{ObjectType, PlacedObject};

// Spacing between auto-placed streetlights along road centrelines (metres).
const STREETLIGHT_SPACING_M: f64 = 30.0;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Deterministic 16-hex-char ID from (type_name, lat×10⁶, lon×10⁶).
fn inferred_id(type_name: &str, lat: f64, lon: f64) -> String {
    let mut h = DefaultHasher::new();
    type_name.hash(&mut h);
    ((lat * 1_000_000.0) as i64).hash(&mut h);
    ((lon * 1_000_000.0) as i64).hash(&mut h);
    format!("inf_{:016x}", h.finish())
}

/// GPS → voxel (X, Z) column (altitude ignored — we scan Y independently).
fn gps_to_voxel_xz(lat: f64, lon: f64) -> (i64, i64) {
    let ecef = GPS::new(lat, lon, 0.0).to_ecef();
    let v = VoxelCoord::from_ecef(&ecef);
    (v.x, v.z)
}

/// Scan the octree from `max_y - 1` down to `min_y` for the first solid
/// (non-AIR, non-WATER) voxel at (vx, vz).  Returns the Y coordinate of the
/// voxel *surface* (one above the solid), in **absolute voxel space**.
fn find_surface_y(octree: &Octree, vx: i64, vz: i64, min_y: i64, max_y: i64) -> Option<i64> {
    for vy in (min_y..max_y).rev() {
        let mat = octree.get_voxel(VoxelCoord::new(vx, vy, vz));
        if mat != MaterialId::AIR && mat != MaterialId::WATER {
            return Some(vy + 1); // on top of this solid voxel
        }
    }
    None
}

/// Try to build one [`PlacedObject`] anchored to the terrain at (lat, lon).
/// Returns `None` if the GPS position falls outside this chunk's X/Z range
/// or no solid surface was found beneath it.
fn make_placed(
    type_name: &str,
    lat: f64,
    lon: f64,
    octree: &Octree,
    chunk_id: &ChunkId,
    origin_voxel: &VoxelCoord,
) -> Option<PlacedObject> {
    let (vx, vz) = gps_to_voxel_xz(lat, lon);
    let min_v = chunk_id.min_voxel();
    let max_v = chunk_id.max_voxel();

    // Reject positions outside this chunk's X/Z footprint.
    if vx < min_v.x || vx >= max_v.x || vz < min_v.z || vz >= max_v.z {
        return None;
    }

    let vy = find_surface_y(octree, vx, vz, min_v.y, max_v.y)?;

    let pos = [
        (vx - origin_voxel.x) as f32,
        (vy - origin_voxel.y) as f32,
        (vz - origin_voxel.z) as f32,
    ];

    Some(PlacedObject {
        id: inferred_id(type_name, lat, lon),
        object_type: ObjectType::Custom(type_name.to_string()),
        position: pos,
        rotation_y: 0.0,
        scale: 1.0,
        content_key: String::new(),
        label: String::new(),
        placed_by: "world_inference".to_string(),
        placed_at: 0,
    })
}

/// Haversine distance in metres between two GPS points.
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Walk a GPS polyline and return one point every `spacing_m` metres.
/// The first point is placed half a spacing from the start so lamps don't
/// cluster at intersections when two roads share an endpoint.
fn road_lamp_positions(nodes: &[GPS], spacing_m: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    if nodes.len() < 2 {
        return out;
    }

    let mut seg_start_dist = 0.0f64;
    // Start half-interval in — staggers placements at intersections.
    let mut next_lamp_dist = spacing_m / 2.0;

    for i in 0..nodes.len() - 1 {
        let a = &nodes[i];
        let b = &nodes[i + 1];
        let seg_len = haversine_m(a.lat, a.lon, b.lat, b.lon);
        if seg_len < 0.1 {
            seg_start_dist += seg_len;
            continue;
        }
        let seg_end_dist = seg_start_dist + seg_len;

        while next_lamp_dist < seg_end_dist {
            let t = (next_lamp_dist - seg_start_dist) / seg_len;
            let lat = a.lat + t * (b.lat - a.lat);
            let lon = a.lon + t * (b.lon - a.lon);
            out.push((lat, lon));
            next_lamp_dist += spacing_m;
        }

        seg_start_dist = seg_end_dist;
    }
    out
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Infer world objects for one chunk from its OSM data and terrain octree.
///
/// Returns a list of [`PlacedObject`]s with deterministic IDs — every client
/// running the same OSM + terrain data will produce identical results.
/// The returned objects use positions **relative to `origin_voxel`** (local
/// world-space metres), matching the coordinate system used by the renderer.
pub fn infer_objects_for_chunk(
    chunk_id: &ChunkId,
    osm: &OsmData,
    octree: &Octree,
    origin_voxel: &VoxelCoord,
) -> Vec<PlacedObject> {
    let mut result: Vec<PlacedObject> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut add = |obj: PlacedObject| {
        if seen.insert(obj.id.clone()) {
            result.push(obj);
        }
    };

    // ── Amenity nodes ────────────────────────────────────────────────────────
    for amenity in &osm.amenities {
        let model = match amenity.amenity.as_str() {
            "bench" => "bench",
            "waste_basket" | "recycling" => "bin",
            "post_box" => "letterbox",
            "traffic_signals" => "traffic_light",
            _ => continue,
        };
        if let Some(obj) = make_placed(
            model,
            amenity.lat,
            amenity.lon,
            octree,
            chunk_id,
            origin_voxel,
        ) {
            add(obj);
        }
    }

    // ── Generic OSM nodes (all-tags import) ──────────────────────────────────
    for node in &osm.extra_nodes {
        let model = if node.tag("highway") == Some("street_lamp") {
            "streetlight"
        } else if node.tag("highway") == Some("traffic_signals")
            || node.tag("amenity") == Some("traffic_signals")
        {
            "traffic_light"
        } else if node.tag("power") == Some("pole") {
            "streetlight"
        } else {
            continue;
        };

        if let Some(obj) = make_placed(model, node.lat, node.lon, octree, chunk_id, origin_voxel) {
            add(obj);
        }
    }

    // ── Streetlights along paved road centrelines ────────────────────────────
    // Only fires when the OSM data has no explicit street_lamp nodes for this
    // area — most regions outside Europe don't map individual lamps.
    for road in &osm.roads {
        if !road.road_type.is_paved() {
            continue;
        }
        for (lat, lon) in road_lamp_positions(&road.nodes, STREETLIGHT_SPACING_M) {
            if let Some(obj) = make_placed("streetlight", lat, lon, octree, chunk_id, origin_voxel)
            {
                add(obj);
            }
        }
    }

    result
}

/// Apply a set of signed admin overrides to an inferred object list in-place.
///
/// Overrides with invalid signatures are silently skipped.  Applied in
/// timestamp order so the most recent override for any given object wins.
pub fn apply_overrides(
    objects: &mut Vec<PlacedObject>,
    overrides: &[crate::world_objects::ObjectOverride],
) {
    use crate::world_objects::OverrideAction;

    // Sort by timestamp ascending — later overrides win.
    let mut sorted: Vec<_> = overrides.iter().collect();
    sorted.sort_by_key(|o| o.timestamp);

    for ov in sorted {
        if !ov.verify() {
            eprintln!(
                "⚠️  [Override] Invalid signature for target '{}' — skipped",
                ov.target_id
            );
            continue;
        }
        match &ov.action {
            OverrideAction::Remove => {
                objects.retain(|o| o.id != ov.target_id);
            }
            OverrideAction::Move {
                position,
                rotation_y,
            } => {
                if let Some(obj) = objects.iter_mut().find(|o| o.id == ov.target_id) {
                    obj.position = *position;
                    obj.rotation_y = *rotation_y;
                }
            }
            OverrideAction::Replace { new_type } => {
                if let Some(obj) = objects.iter_mut().find(|o| o.id == ov.target_id) {
                    obj.object_type = crate::world_objects::ObjectType::Custom(new_type.clone());
                }
            }
            OverrideAction::Scale { scale } => {
                if let Some(obj) = objects.iter_mut().find(|o| o.id == ov.target_id) {
                    obj.scale = *scale;
                }
            }
        }
    }
}
