//! Road geometry carving for the worldgen pipeline.
//!
//! Applies OpenStreetMap road data to pre-generated terrain chunks.
//! Runs after `TerrainGenerator::generate_chunk` and `OsmProcessor::apply_to_chunk`
//! (so water is already carved), replacing terrain surface voxels with
//! ASPHALT, CONCRETE, or GRAVEL.
//!
//! # Algorithm
//! For each 1m² column in the chunk, we find the nearest road segment.
//! The perpendicular distance from the column centre to that segment determines
//! which zone the column falls in:
//!
//! ```text
//! [footpath][carriageway half][CL][carriageway half][footpath]
//! ```
//!
//! - **Carriageway**: distance ≤ `road_type.width_m() / 2`  → ASPHALT or GRAVEL
//! - **Footpath**:    distance ≤ carriageway edge + `footpath_width()`  → CONCRETE
//!
//! # Vertical treatment
//! - **At-grade** (default): road surface replaces terrain surface voxel.
//!   One GRAVEL subbase voxel is placed directly below.
//!   Cut/fill: if road_y < terrain_y → spare AIR above; if road_y > terrain_y → GRAVEL embankment.
//!
//! - **Bridge** (`is_bridge=true`): road surface sits `max(1, abs(layer)) × LAYER_HEIGHT_M`
//!   above terrain. A 2-voxel CONCRETE deck is placed below the road surface;
//!   everything between the deck bottom and the terrain stays AIR.
//!
//! - **Tunnel** (`is_tunnel=true`): a `TUNNEL_HEIGHT`-voxel AIR bore is carved
//!   below terrain, with road floor and GRAVEL subbase at the bottom. If a
//!   tunnel column sits under exposed surface water, the bore is pushed down to
//!   keep a solid roof below the water; if that is impossible in the chunk, the
//!   column falls back to bridge treatment rather than deleting the water.

use std::sync::Arc;

use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z, ChunkId};
use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;
use crate::materials::MaterialId;
use crate::osm::{OsmData, OsmDiskCache, OsmRoad, RoadType};
use crate::voxel::{Octree, VoxelCoord, WORLD_MIN_METERS};

/// Vertical metres per OSM `layer` tag unit for bridges/underpasses.
const LAYER_HEIGHT_M: i64 = 5;

/// Interior headroom carved for a road tunnel (voxels above road floor).
const TUNNEL_HEIGHT: i64 = 5;

/// Minimum bridge surface offset above exposed water so a 2-voxel deck still leaves
/// one voxel of air above the water surface.
const MIN_BRIDGE_SURFACE_ABOVE_WATER: i64 = 4;

/// Minimum solid voxels to leave between tunnel headroom and the bottom of a
/// surface-water column.
const MIN_TUNNEL_ROOF_BELOW_WATER: i64 = 1;

/// How far to search across a road embankment for opposite-side surface water
/// when deciding whether to preserve an explicit culvert path.
const MAX_CULVERT_SEARCH_RADIUS: i64 = 4;

/// How far to extend a detected road/water crossing along the road centerline so
/// adjacent approach columns keep one coherent crossing treatment.
const MAX_CROSSING_SPAN_RADIUS: i64 = 4;

const WGS84_A: f64 = 6_378_137.0;
const WGS84_B: f64 = 6_356_752.3142;

// ── Road Zone ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum RoadZone {
    Carriageway,
    Footpath,
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Applies OSM road geometry to a pre-generated terrain chunk.
pub struct RoadProcessor {
    osm_cache: Arc<OsmDiskCache>,
    origin_voxel: crate::voxel::VoxelCoord,
    origin_gps: GPS,
    /// Elevation pipeline for grade smoothing — samples SRTM at segment endpoints.
    elevation: Option<Arc<std::sync::RwLock<ElevationPipeline>>>,
}

impl RoadProcessor {
    pub fn new(
        osm_cache: Arc<OsmDiskCache>,
        origin_voxel: crate::voxel::VoxelCoord,
        origin_gps: GPS,
    ) -> Self {
        Self {
            osm_cache,
            origin_voxel,
            origin_gps,
            elevation: None,
        }
    }

    pub fn with_elevation(mut self, elev: Arc<std::sync::RwLock<ElevationPipeline>>) -> Self {
        self.elevation = Some(elev);
        self
    }

    /// Apply road geometry to a pre-generated chunk octree.
    ///
    /// Must be called *after* `TerrainGenerator::generate_chunk` and
    /// `OsmProcessor::apply_to_chunk`.
    pub fn apply_to_chunk(&self, chunk_id: &ChunkId, octree: &mut Octree) {
        let origin_ecef_y = (self.origin_voxel.y as f64 + 0.5) + WORLD_MIN_METERS;
        let (lat_min, lat_max, lon_min, lon_max) =
            crate::worldgen_osm::chunk_gps_bounds(chunk_id, origin_ecef_y);
        let osm = crate::osm::fetch_osm_for_chunk_with_cache(
            lat_min,
            lat_max,
            lon_min,
            lon_max,
            &self.osm_cache,
        );
        if osm.roads.is_empty() {
            return;
        }

        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();
        let source_octree = octree.clone();

        for i in 0..CHUNK_SIZE_X {
            for k in 0..CHUNK_SIZE_Z {
                let vx = min_v.x + i;
                let vz = min_v.z + k;

                let (lat, lon) =
                    crate::worldgen_osm::voxel_to_gps(vx, vz, origin_ecef_y, WGS84_A, WGS84_B);

                let hit = nearest_road_hit(lat, lon, &osm.roads);
                let (road, dist_m, seg_t, seg_idx) = match hit {
                    Some(h) => h,
                    None => continue,
                };

                let hw = road.road_type.width_m() / 2.0;
                let fp = footpath_width(&road.road_type);

                let zone = if dist_m <= hw {
                    RoadZone::Carriageway
                } else if fp > 0.0 && dist_m <= hw + fp {
                    RoadZone::Footpath
                } else {
                    continue;
                };

                let terrain_y = match crate::worldgen_osm::surface_voxel_y(
                    &source_octree,
                    vx,
                    vz,
                    min_v.y,
                    max_v.y,
                ) {
                    Some(y) => y,
                    None => continue,
                };
                let terrain_surface_mat =
                    source_octree.get_voxel(VoxelCoord::new(vx, terrain_y, vz));

                let road_y = grade_smoothed_y(
                    road,
                    seg_idx,
                    seg_t,
                    terrain_y,
                    &self.origin_voxel,
                    &self.origin_gps,
                    self.elevation.as_ref(),
                );

                let surface_mat = match zone {
                    RoadZone::Carriageway => carriageway_material(&road.road_type),
                    RoadZone::Footpath => MaterialId::CONCRETE,
                };

                let corridor_mode = if road.is_bridge || road.is_tunnel {
                    None
                } else {
                    resolve_road_corridor_mode(
                        &source_octree,
                        road,
                        seg_idx,
                        seg_t,
                        road_y,
                        origin_ecef_y,
                        &osm,
                        &min_v,
                        &max_v,
                    )
                };

                apply_road_column(
                    octree,
                    &source_octree,
                    vx,
                    vz,
                    terrain_y,
                    road_y,
                    terrain_surface_mat,
                    surface_mat,
                    road.is_bridge,
                    road.is_tunnel,
                    road.layer,
                    corridor_mode,
                    origin_ecef_y,
                    &osm,
                    &min_v,
                    &max_v,
                );
            }
        }
    }
}

// ── Road geometry helpers ────────────────────────────────────────────────────

/// Find the closest road segment to (lat, lon) within reachable range.
///
/// Returns `(road, perpendicular_distance_m, t_along_segment, segment_index)`.
fn nearest_road_hit<'a>(
    lat: f64,
    lon: f64,
    roads: &'a [OsmRoad],
) -> Option<(&'a OsmRoad, f64, f64, usize)> {
    let mut best_road: Option<&OsmRoad> = None;
    let mut best_dist = f64::MAX;
    let mut best_t = 0.0_f64;
    let mut best_seg = 0usize;

    for road in roads {
        if road.nodes.len() < 2 {
            continue;
        }
        // Maximum distance from centreline that matters for this road type.
        let max_reach = road.road_type.width_m() / 2.0 + footpath_width(&road.road_type) + 2.0; // 2m margin so we don't miss edge columns

        for (seg_idx, pair) in road.nodes.windows(2).enumerate() {
            let (dist, t, _, _) = crate::worldgen_osm::point_to_segment_dist_m(
                lat,
                lon,
                pair[0].lat,
                pair[0].lon,
                pair[1].lat,
                pair[1].lon,
            );
            if dist < best_dist && dist <= max_reach {
                best_dist = dist;
                best_road = Some(road);
                best_t = t;
                best_seg = seg_idx;
            }
        }
    }

    best_road.map(|r| (r, best_dist, best_t, best_seg))
}

/// Compute the road surface voxel Y at this column via grade smoothing.
///
/// Samples SRTM elevation at both segment endpoints (A and B), then linearly
/// interpolates at parameter `seg_t` along the segment. This gives a smooth
/// grade instead of the stairstepped per-column terrain height.
///
/// Falls back to `terrain_y` if the elevation pipeline is unavailable or
/// if node altitudes are zero (no data).
fn grade_smoothed_y(
    road: &OsmRoad,
    seg_idx: usize,
    seg_t: f64,
    terrain_y: i64,
    origin_voxel: &VoxelCoord,
    origin_gps: &GPS,
    elevation: Option<&Arc<std::sync::RwLock<ElevationPipeline>>>,
) -> i64 {
    let base_y = if let Some(elev_arc) = elevation {
        if let Ok(pipe) = elev_arc.read() {
            let nodes = &road.nodes;
            if seg_idx + 1 < nodes.len() {
                let a = &nodes[seg_idx];
                let b = &nodes[seg_idx + 1];

                let elev_a = pipe
                    .query_with_fill(&GPS::new(a.lat, a.lon, 0.0))
                    .map(|e| e.meters)
                    .unwrap_or(origin_gps.alt);
                let elev_b = pipe
                    .query_with_fill(&GPS::new(b.lat, b.lon, 0.0))
                    .map(|e| e.meters)
                    .unwrap_or(origin_gps.alt);

                // Add geoid undulation to match the datum used in terrain.rs.
                let na = crate::elevation::egm96_undulation(a.lat, a.lon);
                let nb = crate::elevation::egm96_undulation(b.lat, b.lon);
                let delta_a = elev_a + na - origin_gps.alt;
                let delta_b = elev_b + nb - origin_gps.alt;

                // Lerp between the two endpoint voxel Ys.
                let ya = origin_voxel.y + delta_a as i64;
                let yb = origin_voxel.y + delta_b as i64;
                ya + ((yb - ya) as f64 * seg_t) as i64
            } else {
                terrain_y
            }
        } else {
            terrain_y
        }
    } else {
        terrain_y
    };

    let layer_offset: i64 = if road.is_bridge {
        LAYER_HEIGHT_M * (road.layer.unsigned_abs() as i64).max(1)
    } else if road.is_tunnel {
        -(LAYER_HEIGHT_M * (road.layer.unsigned_abs() as i64).max(1))
    } else {
        0
    };
    base_y + layer_offset
}

/// Surface material for the carriageway based on road type.
fn carriageway_material(rt: &RoadType) -> MaterialId {
    match rt {
        RoadType::Cycleway => MaterialId::CONCRETE, // bike lanes are usually concrete/asphalt
        RoadType::Path => MaterialId::GRAVEL,       // footpaths/unpaved tracks
        RoadType::Other(_) => MaterialId::GRAVEL,   // unknown → unpaved
        _ => MaterialId::ASPHALT,                   // all named road types → asphalt
    }
}

/// Extra width on each side of carriageway reserved for footpaths (metres).
/// Returns 0 if this road type has no footpath.
fn footpath_width(rt: &RoadType) -> f64 {
    match rt {
        RoadType::Motorway => 0.0, // no pedestrian access on motorways
        RoadType::Trunk => 0.0,    // controlled access — no footpath
        RoadType::Primary | RoadType::Secondary | RoadType::Tertiary => 1.5,
        RoadType::Residential => 1.0,
        _ => 0.0,
    }
}

// ── Voxel carving ────────────────────────────────────────────────────────────

fn bridge_surface_y(road_y: i64, terrain_y: i64, terrain_surface_mat: MaterialId) -> i64 {
    if terrain_surface_mat == MaterialId::WATER {
        road_y.max(terrain_y + MIN_BRIDGE_SURFACE_ABOVE_WATER)
    } else {
        road_y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SurfaceWaterRange {
    floor_y: i64,
    top_y: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceWaterSignal {
    Range(SurfaceWaterRange),
    Continuation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoadCorridorMode {
    Bridge(i64),
    Culvert(SurfaceWaterRange),
}

fn road_node_xz(node: &GPS) -> (i64, i64) {
    let voxel = VoxelCoord::from_ecef(&node.to_ecef());
    (voxel.x, voxel.z)
}

fn road_centerline_voxel(road: &OsmRoad, seg_idx: usize, seg_t: f64) -> Option<(i64, i64)> {
    let a = road.nodes.get(seg_idx)?;
    let b = road.nodes.get(seg_idx + 1)?;
    let (ax, az) = road_node_xz(a);
    let (bx, bz) = road_node_xz(b);
    let t = seg_t.clamp(0.0, 1.0);
    Some((
        (ax as f64 + (bx - ax) as f64 * t).round() as i64,
        (az as f64 + (bz - az) as f64 * t).round() as i64,
    ))
}

fn road_section_axes(road: &OsmRoad, seg_idx: usize) -> Option<((i64, i64), (i64, i64))> {
    let a = road.nodes.get(seg_idx)?;
    let b = road.nodes.get(seg_idx + 1)?;
    let (ax, az) = road_node_xz(a);
    let (bx, bz) = road_node_xz(b);
    let dx = bx - ax;
    let dz = bz - az;

    if dx == 0 && dz == 0 {
        return None;
    }

    if dx.abs() > dz.abs() * 2 {
        Some(((dx.signum(), 0), (0, 1)))
    } else if dz.abs() > dx.abs() * 2 {
        Some(((0, dz.signum()), (1, 0)))
    } else {
        let along_x = dx.signum();
        let along_z = dz.signum();
        if along_x == 0 && along_z == 0 {
            Some(((1, 0), (0, 1)))
        } else {
            Some(((along_x, along_z), (-along_z, along_x)))
        }
    }
}

fn road_along_step(road: &OsmRoad, seg_idx: usize) -> Option<(i64, i64)> {
    road_section_axes(road, seg_idx).map(|(along, _)| along)
}

fn road_cross_section_step(road: &OsmRoad, seg_idx: usize) -> Option<(i64, i64)> {
    road_section_axes(road, seg_idx).map(|(_, cross)| cross)
}

fn road_footprint_half_width_vox(road: &OsmRoad) -> i64 {
    (road.road_type.width_m() / 2.0 + footpath_width(&road.road_type)).ceil() as i64
}

fn surface_water_floor_y(
    octree: &Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    terrain_surface_mat: MaterialId,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<i64> {
    surface_water_range_at_column(octree, vx, vz, terrain_y, terrain_surface_mat, min_v, max_v)
        .map(|range| range.floor_y)
}

fn surface_water_range_at_column(
    octree: &Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    terrain_surface_mat: MaterialId,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<SurfaceWaterRange> {
    let top_y = if terrain_surface_mat == MaterialId::WATER {
        terrain_y
    } else {
        let surface_y = crate::worldgen_osm::surface_voxel_y(octree, vx, vz, min_v.y, max_v.y)?;
        if octree.get_voxel(VoxelCoord::new(vx, surface_y, vz)) != MaterialId::WATER {
            return None;
        }
        surface_y
    };

    let mut floor_y = top_y;
    while floor_y > min_v.y
        && octree.get_voxel(VoxelCoord::new(vx, floor_y - 1, vz)) == MaterialId::WATER
    {
        floor_y -= 1;
    }
    Some(SurfaceWaterRange { floor_y, top_y })
}

fn osm_has_surface_water_at(osm: &OsmData, lat: f64, lon: f64) -> bool {
    osm.water.iter().any(|water| {
        crate::osm::point_in_polygon(lat, lon, &water.polygon)
            && !water
                .holes
                .iter()
                .any(|hole| crate::osm::point_in_polygon(lat, lon, hole))
    }) || osm.waterway_lines.iter().any(|line| {
        line.nodes.windows(2).any(|pair| {
            let (dist, _, _, _) = crate::worldgen_osm::point_to_segment_dist_m(
                lat,
                lon,
                pair[0].lat,
                pair[0].lon,
                pair[1].lat,
                pair[1].lon,
            );
            dist <= line.half_width_m()
        })
    })
}

fn surface_water_signal_at_sample(
    octree: &Octree,
    vx: i64,
    vz: i64,
    origin_ecef_y: f64,
    osm: &OsmData,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<SurfaceWaterSignal> {
    if vx >= min_v.x && vx < max_v.x && vz >= min_v.z && vz < max_v.z {
        return surface_water_range_at_column(
            octree,
            vx,
            vz,
            min_v.y,
            MaterialId::AIR,
            min_v,
            max_v,
        )
        .map(SurfaceWaterSignal::Range);
    }

    let (lat, lon) = crate::worldgen_osm::voxel_to_gps(vx, vz, origin_ecef_y, WGS84_A, WGS84_B);
    osm_has_surface_water_at(osm, lat, lon).then_some(SurfaceWaterSignal::Continuation)
}

fn resolve_corridor_bridge_surface_y(
    octree: &Octree,
    center_vx: i64,
    center_vz: i64,
    step_x: i64,
    step_z: i64,
    half_width: i64,
    road_y: i64,
    origin_ecef_y: f64,
    osm: &OsmData,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<i64> {
    let mut max_water_top_y = None;

    for offset in -half_width..=half_width {
        let sample_vx = center_vx + step_x * offset;
        let sample_vz = center_vz + step_z * offset;
        if let Some(SurfaceWaterSignal::Range(range)) = surface_water_signal_at_sample(
            octree,
            sample_vx,
            sample_vz,
            origin_ecef_y,
            osm,
            min_v,
            max_v,
        ) {
            max_water_top_y = Some(
                max_water_top_y
                    .map(|current: i64| current.max(range.top_y))
                    .unwrap_or(range.top_y),
            );
        }
    }

    max_water_top_y.map(|top_y| road_y.max(top_y + MIN_BRIDGE_SURFACE_ABOVE_WATER))
}

fn resolve_tunnel_road_y(
    octree: &Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    terrain_surface_mat: MaterialId,
    road_y: i64,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<i64> {
    let Some(water_floor_y) =
        surface_water_floor_y(octree, vx, vz, terrain_y, terrain_surface_mat, min_v, max_v)
    else {
        return Some(road_y);
    };

    let max_safe_road_y = water_floor_y - TUNNEL_HEIGHT - MIN_TUNNEL_ROOF_BELOW_WATER - 1;
    let min_tunnel_road_y = min_v.y + 1;
    if max_safe_road_y < min_tunnel_road_y {
        None
    } else {
        Some(road_y.min(max_safe_road_y))
    }
}

fn culvert_range_for_axis(
    octree: &Octree,
    vx: i64,
    vz: i64,
    road_y: i64,
    origin_ecef_y: f64,
    osm: &OsmData,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
    dx: i64,
    dz: i64,
    search_radius: i64,
) -> Option<SurfaceWaterRange> {
    let max_culvert_top = road_y - 2;
    if max_culvert_top < min_v.y {
        return None;
    }

    for radius in 1..=search_radius {
        let ax = vx + dx * radius;
        let az = vz + dz * radius;
        let bx = vx - dx * radius;
        let bz = vz - dz * radius;
        let a = surface_water_signal_at_sample(octree, ax, az, origin_ecef_y, osm, min_v, max_v);
        let b = surface_water_signal_at_sample(octree, bx, bz, origin_ecef_y, osm, min_v, max_v);

        match (a, b) {
            (Some(SurfaceWaterSignal::Range(a)), Some(SurfaceWaterSignal::Range(b))) => {
                let floor_y = a.floor_y.max(b.floor_y);
                let top_y = a.top_y.min(b.top_y).min(max_culvert_top);
                if top_y >= floor_y {
                    return Some(SurfaceWaterRange { floor_y, top_y });
                }
            }
            (Some(SurfaceWaterSignal::Range(range)), Some(SurfaceWaterSignal::Continuation))
            | (Some(SurfaceWaterSignal::Continuation), Some(SurfaceWaterSignal::Range(range))) => {
                let top_y = range.top_y.min(max_culvert_top);
                if top_y >= range.floor_y {
                    return Some(SurfaceWaterRange {
                        floor_y: range.floor_y,
                        top_y,
                    });
                }
            }
            _ => {}
        }
    }

    None
}

fn resolve_road_corridor_mode(
    octree: &Octree,
    road: &OsmRoad,
    seg_idx: usize,
    seg_t: f64,
    road_y: i64,
    origin_ecef_y: f64,
    osm: &OsmData,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<RoadCorridorMode> {
    let (center_vx, center_vz) = road_centerline_voxel(road, seg_idx, seg_t)?;
    let (along_x, along_z) = road_along_step(road, seg_idx)?;
    let (step_x, step_z) = road_cross_section_step(road, seg_idx)?;
    let half_width = road_footprint_half_width_vox(road);
    let search_radius = half_width + MAX_CULVERT_SEARCH_RADIUS;
    let mut bridge_surface_y: Option<i64> = None;
    let mut culvert_range: Option<SurfaceWaterRange> = None;

    for offset in -MAX_CROSSING_SPAN_RADIUS..=MAX_CROSSING_SPAN_RADIUS {
        let span_center_vx = center_vx + along_x * offset;
        let span_center_vz = center_vz + along_z * offset;

        if let Some(surface_y) = resolve_corridor_bridge_surface_y(
            octree,
            span_center_vx,
            span_center_vz,
            step_x,
            step_z,
            half_width,
            road_y,
            origin_ecef_y,
            osm,
            min_v,
            max_v,
        ) {
            bridge_surface_y = Some(
                bridge_surface_y
                    .map(|current: i64| current.max(surface_y))
                    .unwrap_or(surface_y),
            );
        }

        if let Some(range) = culvert_range_for_axis(
            octree,
            span_center_vx,
            span_center_vz,
            road_y,
            origin_ecef_y,
            osm,
            min_v,
            max_v,
            step_x,
            step_z,
            search_radius,
        ) {
            culvert_range = match culvert_range {
                Some(current)
                    if current.top_y > range.top_y
                        || (current.top_y == range.top_y && current.floor_y <= range.floor_y) =>
                {
                    Some(current)
                }
                _ => Some(range),
            };
        }
    }

    if let Some(surface_y) = bridge_surface_y {
        Some(RoadCorridorMode::Bridge(surface_y))
    } else {
        culvert_range.map(RoadCorridorMode::Culvert)
    }
}

fn resolve_culvert_water_range(
    octree: &Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    road_y: i64,
    origin_ecef_y: f64,
    osm: &OsmData,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) -> Option<SurfaceWaterRange> {
    if road_y <= terrain_y {
        return None;
    }

    let x_range = culvert_range_for_axis(
        octree,
        vx,
        vz,
        road_y,
        origin_ecef_y,
        osm,
        min_v,
        max_v,
        1,
        0,
        MAX_CULVERT_SEARCH_RADIUS,
    );
    let z_range = culvert_range_for_axis(
        octree,
        vx,
        vz,
        road_y,
        origin_ecef_y,
        osm,
        min_v,
        max_v,
        0,
        1,
        MAX_CULVERT_SEARCH_RADIUS,
    );
    match (x_range, z_range) {
        (Some(x), Some(z)) => {
            if x.top_y > z.top_y || (x.top_y == z.top_y && x.floor_y <= z.floor_y) {
                Some(x)
            } else {
                Some(z)
            }
        }
        (Some(x), None) => Some(x),
        (None, Some(z)) => Some(z),
        (None, None) => None,
    }
}

fn apply_road_column(
    octree: &mut Octree,
    source_octree: &Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    road_y: i64,
    terrain_surface_mat: MaterialId,
    surface_mat: MaterialId,
    is_bridge: bool,
    is_tunnel: bool,
    layer: i8,
    corridor_mode: Option<RoadCorridorMode>,
    origin_ecef_y: f64,
    osm: &OsmData,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) {
    if is_tunnel {
        if let Some(resolved_road_y) = resolve_tunnel_road_y(
            source_octree,
            vx,
            vz,
            terrain_y,
            terrain_surface_mat,
            road_y,
            min_v,
            max_v,
        ) {
            apply_tunnel(
                octree,
                vx,
                vz,
                terrain_y,
                resolved_road_y,
                surface_mat,
                layer,
                min_v,
                max_v,
            );
        } else {
            apply_bridge(
                octree,
                vx,
                vz,
                terrain_y,
                match corridor_mode {
                    Some(RoadCorridorMode::Bridge(surface_y)) => surface_y,
                    _ => bridge_surface_y(road_y, terrain_y, terrain_surface_mat),
                },
                surface_mat,
                layer,
                min_v,
                max_v,
            );
        }
        return;
    }

    let corridor_bridge_surface_y = match corridor_mode {
        Some(RoadCorridorMode::Bridge(surface_y)) => Some(surface_y),
        _ => None,
    };

    if is_bridge || terrain_surface_mat == MaterialId::WATER || corridor_bridge_surface_y.is_some()
    {
        apply_bridge(
            octree,
            vx,
            vz,
            terrain_y,
            corridor_bridge_surface_y
                .unwrap_or_else(|| bridge_surface_y(road_y, terrain_y, terrain_surface_mat)),
            surface_mat,
            layer,
            min_v,
            max_v,
        );
    } else {
        apply_at_grade(
            octree,
            vx,
            vz,
            terrain_y,
            road_y,
            surface_mat,
            match corridor_mode {
                Some(RoadCorridorMode::Culvert(range)) => Some(range),
                _ => resolve_culvert_water_range(
                    source_octree,
                    vx,
                    vz,
                    terrain_y,
                    road_y,
                    origin_ecef_y,
                    osm,
                    min_v,
                    max_v,
                ),
            },
            min_v,
            max_v,
        );
    }
}

/// At-grade road: road surface sits at or near terrain surface.
///
/// Handles both cut (road lower than terrain) and fill (road higher).
fn apply_at_grade(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    road_y: i64,
    surface_mat: MaterialId,
    culvert_range: Option<SurfaceWaterRange>,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) {
    let road_y = road_y.clamp(min_v.y, max_v.y - 1);

    // Road surface.
    octree.set_voxel(VoxelCoord::new(vx, road_y, vz), surface_mat);

    // Gravel subbase one voxel below road surface.
    if road_y - 1 >= min_v.y {
        octree.set_voxel(VoxelCoord::new(vx, road_y - 1, vz), MaterialId::GRAVEL);
    }

    // Cut: clear material above road surface up to old terrain surface.
    for vy in (road_y + 1)..=terrain_y.min(max_v.y - 1) {
        octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::AIR);
    }

    // Fill: build up a gravel embankment from terrain surface to road surface.
    for vy in (terrain_y + 1)..road_y {
        if vy >= min_v.y && vy < max_v.y {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::GRAVEL);
        }
    }

    if let Some(range) = culvert_range {
        for vy in range.floor_y.max(min_v.y)..=range.top_y.min(max_v.y - 1) {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }
        let culvert_void_top = (road_y - 1).min(max_v.y);
        for vy in (range.top_y + 1).max(min_v.y)..culvert_void_top {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::AIR);
        }
        if road_y - 1 >= min_v.y && road_y - 1 < max_v.y {
            octree.set_voxel(VoxelCoord::new(vx, road_y - 1, vz), MaterialId::GRAVEL);
        }
    }
}

/// Bridge: road surface is elevated above terrain by `layer × LAYER_HEIGHT_M`.
///
/// ```text
///   road_y    → ASPHALT/CONCRETE
///   road_y-1  → CONCRETE  (deck)
///   road_y-2  → CONCRETE  (deck soffit)
///   ...AIR... (void under bridge)
///   terrain_y → unchanged terrain voxel
/// ```
fn apply_bridge(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    road_y: i64,
    surface_mat: MaterialId,
    _layer: i8,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) {
    if road_y <= terrain_y {
        // Guard: bridge elevation didn't rise above terrain — treat as at-grade.
        apply_at_grade(
            octree,
            vx,
            vz,
            terrain_y,
            road_y,
            surface_mat,
            None,
            min_v,
            max_v,
        );
        return;
    }

    // Road surface.
    let surf = road_y.clamp(min_v.y, max_v.y - 1);
    octree.set_voxel(VoxelCoord::new(vx, surf, vz), surface_mat);

    // Concrete bridge deck (2 voxels thick below road surface).
    for vy in [(surf - 1), (surf - 2)] {
        if vy >= min_v.y && vy < max_v.y && vy > terrain_y {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::CONCRETE);
        }
    }

    // Void under the deck down to terrain.
    let deck_bot = (surf - 2).max(terrain_y + 1);
    for vy in (terrain_y + 1)..deck_bot {
        if vy >= min_v.y && vy < max_v.y {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::AIR);
        }
    }
}

/// Tunnel: an AIR bore is carved through terrain, with road floor at the bottom.
///
/// ```text
///   terrain_y   → unchanged
///   ...STONE... (mountain above tunnel)
///   road_y+TUNNEL_HEIGHT → STONE (tunnel ceiling, already there)
///   road_y+1 .. road_y+TUNNEL_HEIGHT-1 → AIR (headroom)
///   road_y      → ASPHALT/CONCRETE (road floor)
///   road_y-1    → STONE  (subbase / tunnel invert)
/// ```
fn apply_tunnel(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    _terrain_y: i64,
    road_y: i64,
    surface_mat: MaterialId,
    _layer: i8,
    min_v: &VoxelCoord,
    max_v: &VoxelCoord,
) {
    let road_y = road_y.clamp(min_v.y + 1, max_v.y - TUNNEL_HEIGHT - 1);

    // Road floor.
    octree.set_voxel(VoxelCoord::new(vx, road_y, vz), surface_mat);

    // Stone subbase/invert below floor.
    if road_y - 1 >= min_v.y {
        octree.set_voxel(VoxelCoord::new(vx, road_y - 1, vz), MaterialId::STONE);
    }

    // Air headroom inside tunnel.
    for vy in (road_y + 1)..=(road_y + TUNNEL_HEIGHT).min(max_v.y - 1) {
        octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::AIR);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RoadCorridorMode, SurfaceWaterRange, apply_bridge, apply_road_column, bridge_surface_y,
        resolve_culvert_water_range, resolve_road_corridor_mode, resolve_tunnel_road_y,
        surface_water_floor_y, surface_water_range_at_column,
    };
    use crate::coordinates::GPS;
    use crate::materials::MaterialId;
    use crate::osm::{OsmData, OsmRoad, OsmWater, RoadType};
    use crate::voxel::{Octree, VoxelCoord, WORLD_MIN_METERS};

    fn test_origin_ecef_y() -> f64 {
        WORLD_MIN_METERS + 0.5
    }

    fn chunk_edge_water_polygon(vx: i64, vz: i64, origin_ecef_y: f64) -> OsmWater {
        let (lat, lon) = crate::worldgen_osm::voxel_to_gps(
            vx,
            vz,
            origin_ecef_y,
            super::WGS84_A,
            super::WGS84_B,
        );
        let d_lat = 0.00005;
        let d_lon = 0.00005;
        OsmWater {
            osm_id: 1,
            polygon: vec![
                GPS::new(lat - d_lat, lon - d_lon, 0.0),
                GPS::new(lat - d_lat, lon + d_lon, 0.0),
                GPS::new(lat + d_lat, lon + d_lon, 0.0),
                GPS::new(lat + d_lat, lon - d_lon, 0.0),
                GPS::new(lat - d_lat, lon - d_lon, 0.0),
            ],
            holes: Vec::new(),
            name: None,
            water_type: "river".to_string(),
        }
    }

    fn gps_at_voxel(vx: i64, vz: i64, _origin_ecef_y: f64) -> GPS {
        // Preserve altitude so GPS -> ECEF -> voxel roundtrips in the current helpers.
        VoxelCoord::new(vx, 0, vz).to_ecef().to_gps()
    }

    fn test_road(vx0: i64, vz0: i64, vx1: i64, vz1: i64, road_type: RoadType) -> OsmRoad {
        let origin_ecef_y = test_origin_ecef_y();
        OsmRoad {
            osm_id: 1,
            nodes: vec![
                gps_at_voxel(vx0, vz0, origin_ecef_y),
                gps_at_voxel(vx1, vz1, origin_ecef_y),
            ],
            road_type,
            name: None,
            is_bridge: false,
            is_tunnel: false,
            layer: 0,
        }
    }

    #[test]
    fn fill_road_column_carves_culvert_between_opposite_water_banks() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 10;
        let vz = 10;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(vx - 1, vy, vz), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(vx + 1, vy, vz), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(vx - 1, vy, vz), MaterialId::WATER);
            octree.set_voxel(VoxelCoord::new(vx + 1, vy, vz), MaterialId::WATER);
        }
        let source_octree = octree.clone();

        apply_road_column(
            &mut octree,
            &source_octree,
            vx,
            vz,
            8,
            12,
            MaterialId::STONE,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            None,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 6..=10 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::WATER
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 11, vz)),
            MaterialId::GRAVEL
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 12, vz)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn fill_road_column_without_opposite_water_keeps_embankment_solid() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 10;
        let vz = 10;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(vx - 1, vy, vz), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(vx - 1, vy, vz), MaterialId::WATER);
        }
        let source_octree = octree.clone();

        apply_road_column(
            &mut octree,
            &source_octree,
            vx,
            vz,
            8,
            12,
            MaterialId::STONE,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            None,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 10, vz)),
            MaterialId::GRAVEL
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 11, vz)),
            MaterialId::GRAVEL
        );
    }

    #[test]
    fn culvert_resolution_prefers_opposite_surface_water_overlap() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 10;
        let vz = 10;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(vx - 2, vy, vz), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(vx + 2, vy, vz), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(vx, vy, vz - 1), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(vx, vy, vz + 1), MaterialId::STONE);
        }
        for vy in 5..=9 {
            octree.set_voxel(VoxelCoord::new(vx - 2, vy, vz), MaterialId::WATER);
            octree.set_voxel(VoxelCoord::new(vx + 2, vy, vz), MaterialId::WATER);
        }
        for vy in 6..=8 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz - 1), MaterialId::WATER);
            octree.set_voxel(VoxelCoord::new(vx, vy, vz + 1), MaterialId::WATER);
        }

        assert_eq!(
            surface_water_range_at_column(&octree, vx - 2, vz, 0, MaterialId::AIR, &min_v, &max_v),
            Some(SurfaceWaterRange {
                floor_y: 5,
                top_y: 9
            })
        );
        assert_eq!(
            resolve_culvert_water_range(
                &octree,
                vx,
                vz,
                8,
                12,
                origin_ecef_y,
                &osm,
                &min_v,
                &max_v,
            ),
            Some(SurfaceWaterRange {
                floor_y: 5,
                top_y: 9
            })
        );
    }

    #[test]
    fn culvert_resolution_uses_boundary_osm_water_signal() {
        let mut octree = Octree::new();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 0;
        let vz = 10;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(vx + 1, vy, vz), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(vx + 1, vy, vz), MaterialId::WATER);
        }

        let mut osm = OsmData::default();
        osm.water
            .push(chunk_edge_water_polygon(vx - 1, vz, origin_ecef_y));

        assert_eq!(
            resolve_culvert_water_range(
                &octree,
                vx,
                vz,
                8,
                12,
                origin_ecef_y,
                &osm,
                &min_v,
                &max_v,
            ),
            Some(SurfaceWaterRange {
                floor_y: 6,
                top_y: 10,
            })
        );
    }

    #[test]
    fn culvert_resolution_does_not_use_boundary_osm_without_inside_water() {
        let octree = Octree::new();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 0;
        let vz = 10;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        let mut osm = OsmData::default();
        osm.water
            .push(chunk_edge_water_polygon(vx - 1, vz, origin_ecef_y));

        assert_eq!(
            resolve_culvert_water_range(
                &octree,
                vx,
                vz,
                8,
                12,
                origin_ecef_y,
                &osm,
                &min_v,
                &max_v,
            ),
            None
        );
    }

    #[test]
    fn wet_surface_tunnel_lowers_floor_below_water_column() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 8;
        let vz = 9;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..9 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
        }
        for vy in 9..=12 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }
        let source_octree = octree.clone();

        apply_road_column(
            &mut octree,
            &source_octree,
            vx,
            vz,
            12,
            8,
            MaterialId::WATER,
            MaterialId::ASPHALT,
            false,
            true,
            -1,
            None,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 2, vz)),
            MaterialId::ASPHALT
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 1, vz)),
            MaterialId::STONE
        );
        for vy in 3..=7 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::AIR
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 8, vz)),
            MaterialId::STONE
        );
        for vy in 9..=12 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::WATER
            );
        }
    }

    #[test]
    fn shallow_wet_tunnel_falls_back_to_bridge() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 8;
        let vz = 9;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..2 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
        }
        for vy in 2..=5 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }
        let source_octree = octree.clone();

        apply_road_column(
            &mut octree,
            &source_octree,
            vx,
            vz,
            5,
            1,
            MaterialId::WATER,
            MaterialId::ASPHALT,
            false,
            true,
            -1,
            None,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 2..=5 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::WATER
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 6, vz)),
            MaterialId::AIR
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 7, vz)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 8, vz)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 9, vz)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn water_floor_and_tunnel_resolution_track_surface_water_columns() {
        let mut octree = Octree::new();
        let vx = 3;
        let vz = 4;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..6 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
        }
        for vy in 6..=8 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }

        assert_eq!(
            surface_water_floor_y(&octree, vx, vz, 8, MaterialId::WATER, &min_v, &max_v),
            Some(6)
        );
        assert_eq!(
            resolve_tunnel_road_y(&octree, vx, vz, 8, MaterialId::WATER, 7, &min_v, &max_v),
            None
        );
        assert_eq!(
            resolve_tunnel_road_y(&octree, vx, vz, 8, MaterialId::STONE, 7, &min_v, &max_v),
            None
        );
    }

    #[test]
    fn wet_surface_promotes_at_grade_column_to_bridge() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let vx = 8;
        let vz = 9;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..7 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
        }
        for vy in 7..=10 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }
        let source_octree = octree.clone();

        apply_road_column(
            &mut octree,
            &source_octree,
            vx,
            vz,
            10,
            10,
            MaterialId::WATER,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            None,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 7..=10 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::WATER
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 11, vz)),
            MaterialId::AIR
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 12, vz)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 13, vz)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 14, vz)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn corridor_bridge_mode_promotes_dry_bank_columns_to_bridge() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let road = test_road(10, 16, 22, 16, RoadType::Residential);
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(16, vy, 16), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(16, vy, 20), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(16, vy, 16), MaterialId::WATER);
        }

        let source_octree = octree.clone();
        let corridor_mode = resolve_road_corridor_mode(
            &source_octree,
            &road,
            0,
            0.5,
            10,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );
        assert_eq!(corridor_mode, Some(RoadCorridorMode::Bridge(14)));

        apply_road_column(
            &mut octree,
            &source_octree,
            16,
            20,
            8,
            10,
            MaterialId::STONE,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            corridor_mode,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 9..=11 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(16, vy, 20)),
                MaterialId::AIR
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(16, 12, 20)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(16, 13, 20)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(16, 14, 20)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn corridor_culvert_mode_uses_centerline_cross_section() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let road = test_road(10, 16, 22, 16, RoadType::Service);
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(16, vy, 13), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(16, vy, 18), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(16, vy, 19), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(16, vy, 13), MaterialId::WATER);
            octree.set_voxel(VoxelCoord::new(16, vy, 19), MaterialId::WATER);
        }

        let source_octree = octree.clone();
        let corridor_mode = resolve_road_corridor_mode(
            &source_octree,
            &road,
            0,
            0.5,
            12,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );
        assert_eq!(
            corridor_mode,
            Some(RoadCorridorMode::Culvert(SurfaceWaterRange {
                floor_y: 6,
                top_y: 10,
            }))
        );

        apply_road_column(
            &mut octree,
            &source_octree,
            16,
            18,
            8,
            12,
            MaterialId::STONE,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            corridor_mode,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 6..=10 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(16, vy, 18)),
                MaterialId::WATER
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(16, 11, 18)),
            MaterialId::GRAVEL
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(16, 12, 18)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn longitudinal_bridge_mode_extends_to_nearby_approach_columns() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let road = test_road(8, 16, 24, 16, RoadType::Residential);
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(16, vy, 16), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(20, vy, 16), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(21, vy, 16), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(16, vy, 16), MaterialId::WATER);
        }

        let source_octree = octree.clone();
        let corridor_mode = resolve_road_corridor_mode(
            &source_octree,
            &road,
            0,
            0.75,
            10,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );
        assert_eq!(corridor_mode, Some(RoadCorridorMode::Bridge(14)));
        assert_eq!(
            resolve_road_corridor_mode(
                &source_octree,
                &road,
                0,
                0.8125,
                10,
                origin_ecef_y,
                &osm,
                &min_v,
                &max_v,
            ),
            None
        );

        apply_road_column(
            &mut octree,
            &source_octree,
            20,
            16,
            8,
            10,
            MaterialId::STONE,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            corridor_mode,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 9..=11 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(20, vy, 16)),
                MaterialId::AIR
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(20, 12, 16)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(20, 13, 16)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(20, 14, 16)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn longitudinal_culvert_mode_extends_to_nearby_approach_columns() {
        let mut octree = Octree::new();
        let osm = OsmData::default();
        let origin_ecef_y = test_origin_ecef_y();
        let road = test_road(8, 16, 24, 16, RoadType::Service);
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..=8 {
            octree.set_voxel(VoxelCoord::new(16, vy, 13), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(16, vy, 19), MaterialId::STONE);
            octree.set_voxel(VoxelCoord::new(20, vy, 16), MaterialId::STONE);
        }
        for vy in 6..=10 {
            octree.set_voxel(VoxelCoord::new(16, vy, 13), MaterialId::WATER);
            octree.set_voxel(VoxelCoord::new(16, vy, 19), MaterialId::WATER);
        }

        let source_octree = octree.clone();
        let corridor_mode = resolve_road_corridor_mode(
            &source_octree,
            &road,
            0,
            0.75,
            12,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );
        assert_eq!(
            corridor_mode,
            Some(RoadCorridorMode::Culvert(SurfaceWaterRange {
                floor_y: 6,
                top_y: 10,
            }))
        );

        apply_road_column(
            &mut octree,
            &source_octree,
            20,
            16,
            8,
            12,
            MaterialId::STONE,
            MaterialId::ASPHALT,
            false,
            false,
            0,
            corridor_mode,
            origin_ecef_y,
            &osm,
            &min_v,
            &max_v,
        );

        for vy in 6..=10 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(20, vy, 16)),
                MaterialId::WATER
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(20, 11, 16)),
            MaterialId::GRAVEL
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(20, 12, 16)),
            MaterialId::ASPHALT
        );
    }

    #[test]
    fn bridge_surface_keeps_clearance_above_water() {
        assert_eq!(bridge_surface_y(10, 10, MaterialId::WATER), 14);
        assert_eq!(bridge_surface_y(18, 10, MaterialId::WATER), 18);
        assert_eq!(bridge_surface_y(10, 10, MaterialId::STONE), 10);
    }

    #[test]
    fn bridge_preserves_waterway_below_deck() {
        let mut octree = Octree::new();
        let vx = 8;
        let vz = 9;
        let min_v = VoxelCoord::new(0, 0, 0);
        let max_v = VoxelCoord::new(32, 32, 32);

        for vy in 0..7 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
        }
        for vy in 7..=10 {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }

        apply_bridge(
            &mut octree,
            vx,
            vz,
            10,
            18,
            MaterialId::ASPHALT,
            1,
            &min_v,
            &max_v,
        );

        for vy in 7..=10 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::WATER
            );
        }
        for vy in 11..16 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::AIR
            );
        }
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 16, vz)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 17, vz)),
            MaterialId::CONCRETE
        );
        assert_eq!(
            octree.get_voxel(VoxelCoord::new(vx, 18, vz)),
            MaterialId::ASPHALT
        );
    }
}
