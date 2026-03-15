//! Offline OSM feature baking for the worldgen pipeline.
//!
//! This module applies OpenStreetMap features (waterways, water polygons) to
//! pre-generated terrain chunks.  It runs *after* `TerrainGenerator::generate_chunk`
//! so the octree already contains SRTM-derived STONE/DIRT/GRASS voxels.
//!
//! # Architecture
//! - `OsmProcessor` holds a reference to the shared OSM disk cache and origin coords.
//! - `apply_to_chunk` fetches OSM data for the chunk's GPS bbox, then applies each
//!   feature type in order: water polygons first, then waterway channels.
//! - When a `RiverProfileCache` is provided, waterway channels are carved using
//!   SRTM-derived water surface elevations, accurate widths, and depths.
//! - All coordinate conversions use the same ECEF ↔ voxel math as `terrain.rs`.

use std::sync::Arc;

use crate::chunk::ChunkId;
use crate::coordinates::GPS;
use crate::materials::MaterialId;
use crate::osm::{OsmDiskCache, OsmWater, WaterwayLine};
use crate::terrain_analysis::TerrainAnalysis;
use crate::voxel::{Octree, VoxelCoord, WORLD_MIN_METERS};
use crate::worldgen_river::RiverProfileCache;

#[derive(Debug, Clone, Copy)]
struct WaterwayCarveProfile {
    water_surface_m: f32,
    half_width_m: f32,
    center_depth_m: f32,
    carve_depth_m: f32,
    dist_to_center_m: f32,
    polygon_backed: bool,
}

#[derive(Debug, Clone, Copy)]
struct ChannelCol {
    grid_x: usize,
    grid_z: usize,
    vx: i64,
    vz: i64,
    terrain_y: i64,
    water_surface_y: i64,
    wetted_depth_vox: Option<i64>,
    dist_to_center_m: f32,
    half_width_m: f32,
    center_depth_m: f32,
    polygon_backed: bool,
    inside_backed_channel_polygon: bool,
}

/// Applies OSM features (waterways, water polygons) to pre-generated terrain chunks.
pub struct OsmProcessor {
    osm_cache: Arc<OsmDiskCache>,
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
    /// Optional terrain analysis for TWI/slope-aware feature placement.
    pub analysis: Option<Arc<TerrainAnalysis>>,
    /// Pre-computed river profiles for the whole region.
    /// When present, waterway carving uses SRTM-derived elevations and widths.
    pub river_profiles: Option<Arc<RiverProfileCache>>,
}

impl OsmProcessor {
    pub fn new(
        osm_cache: Arc<OsmDiskCache>,
        origin_gps: GPS,
        origin_voxel: VoxelCoord,
        analysis: Option<Arc<TerrainAnalysis>>,
    ) -> Self {
        Self {
            osm_cache,
            origin_gps,
            origin_voxel,
            analysis,
            river_profiles: None,
        }
    }

    pub fn with_river_profiles(mut self, profiles: Arc<RiverProfileCache>) -> Self {
        self.river_profiles = Some(profiles);
        self
    }

    /// Apply all implemented OSM features to a pre-generated chunk.
    ///
    /// Currently implements: water polygons, waterway channels.
    /// The octree must already contain terrain voxels from `TerrainGenerator`.
    pub fn apply_to_chunk(&self, chunk_id: &ChunkId, octree: &mut Octree) {
        let (lat_min, lat_max, lon_min, lon_max) = chunk_id.gps_bounds();
        let osm = crate::osm::fetch_osm_for_chunk_with_cache(
            lat_min,
            lat_max,
            lon_min,
            lon_max,
            &self.osm_cache,
        );
        if osm.is_empty() {
            return;
        }

        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();

        if !osm.water.is_empty() {
            self.apply_water_polygons(octree, &min_v, &max_v, &osm.water, &osm.waterway_lines);
        }
        if !osm.waterway_lines.is_empty() {
            self.apply_waterway_channels(
                octree,
                &min_v,
                &max_v,
                &osm.water,
                &osm.waterway_lines,
                self.river_profiles.as_deref(),
            );
        }
    }

    // ── Water polygon fill ────────────────────────────────────────────────────

    fn apply_water_polygons(
        &self,
        octree: &mut Octree,
        min_v: &VoxelCoord,
        max_v: &VoxelCoord,
        osm_water: &[OsmWater],
        waterways: &[WaterwayLine],
    ) {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};

        // Compute the ellipsoid ECEF Y for the WGS-84 formula (same as terrain.rs).
        const WGS84_A: f64 = 6_378_137.0;
        const WGS84_B: f64 = 6_356_752.3142;
        let origin_ecef_y = (self.origin_voxel.y as f64 + 0.5) + WORLD_MIN_METERS;
        let flat_water: Vec<&OsmWater> = osm_water
            .iter()
            .filter(|water| !should_skip_flat_polygon_fill(water, waterways))
            .collect();

        if flat_water.is_empty() {
            return;
        }

        // First pass: find all columns inside a water polygon and record their GPS.
        struct WaterCol {
            vx: i64,
            vz: i64,
            surface_y: i64,
        }

        let mut water_cols: Vec<WaterCol> = Vec::new();

        for i in 0..CHUNK_SIZE_X {
            for k in 0..CHUNK_SIZE_Z {
                let vx = min_v.x + i;
                let vz = min_v.z + k;

                let (lat, lon) = voxel_to_gps(vx, vz, origin_ecef_y, WGS84_A, WGS84_B);

                let in_water = flat_water.iter().any(|w| {
                    crate::osm::point_in_polygon(lat, lon, &w.polygon)
                        && !w
                            .holes
                            .iter()
                            .any(|hole| crate::osm::point_in_polygon(lat, lon, hole))
                });
                if !in_water {
                    continue;
                }

                let surface_y = surface_voxel_y(octree, vx, vz, min_v.y, max_v.y)
                    .unwrap_or(self.origin_voxel.y);

                water_cols.push(WaterCol { vx, vz, surface_y });
            }
        }

        if water_cols.is_empty() {
            return;
        }

        // Use the minimum surface_y among all water columns as the flat water plane.
        // Clamp: never lower the water level more than MAX_CARVE below any column's
        // own terrain surface — this prevents cliff-edge columns (bank columns that
        // happen to sit inside the water polygon boundary) from dragging the water
        // level down by 50-100m and hollowing out the entire chunk.
        const WATER_DEPTH: i64 = 5; // river bed depth below water surface (voxels)
        const MAX_CARVE: i64 = 15; // never lower water > 15m below any column's terrain
        let raw_min_y = water_cols.iter().map(|c| c.surface_y).min().unwrap_or(0);
        let water_level_y = water_cols
            .iter()
            .map(|c| c.surface_y - MAX_CARVE)
            .max()
            .map(|floor| raw_min_y.max(floor))
            .unwrap_or(raw_min_y);

        // Fill each water column.
        for col in &water_cols {
            // Only touch columns at or near the water plane — skip high bank columns.
            if col.surface_y > water_level_y + MAX_CARVE {
                continue;
            }
            let col_bot = water_level_y - WATER_DEPTH;
            let bedrock = (water_level_y - WATER_DEPTH - 2).max(min_v.y);

            // Carve the channel bed to WATER/STONE below water surface
            for vy in bedrock..water_level_y.min(max_v.y) {
                let pos = VoxelCoord::new(col.vx, vy, col.vz);
                let depth_below = water_level_y - vy;
                let mat = if depth_below <= WATER_DEPTH {
                    MaterialId::WATER
                } else {
                    MaterialId::STONE
                };
                octree.set_voxel(pos, mat);
            }
            // Ensure voxels above water surface are AIR
            for vy in water_level_y.max(min_v.y)..col.surface_y.max(water_level_y).min(max_v.y) {
                octree.set_voxel(VoxelCoord::new(col.vx, vy, col.vz), MaterialId::AIR);
            }
            // Ensure water surface voxel is WATER
            if water_level_y >= min_v.y && water_level_y < max_v.y {
                octree.set_voxel(
                    VoxelCoord::new(col.vx, water_level_y, col.vz),
                    MaterialId::WATER,
                );
            }
            // Ensure column below col_bot is STONE down to bedrock
            for vy in bedrock.max(min_v.y)..col_bot.min(max_v.y) {
                octree.set_voxel(VoxelCoord::new(col.vx, vy, col.vz), MaterialId::STONE);
            }
        }
    }

    // ── Waterway channel carving ──────────────────────────────────────────────

    fn apply_waterway_channels(
        &self,
        octree: &mut Octree,
        min_v: &VoxelCoord,
        max_v: &VoxelCoord,
        osm_water: &[OsmWater],
        waterways: &[WaterwayLine],
        river_profiles: Option<&RiverProfileCache>,
    ) {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};

        const WGS84_A: f64 = 6_378_137.0;
        const WGS84_B: f64 = 6_356_752.3142;
        let origin_ecef_y = (self.origin_voxel.y as f64 + 0.5) + WORLD_MIN_METERS;
        let backed_channel_polygons: Vec<&OsmWater> = osm_water
            .iter()
            .filter(|water| should_skip_flat_polygon_fill(water, waterways))
            .collect();
        let chunk_width = CHUNK_SIZE_X as usize;
        let chunk_depth = CHUNK_SIZE_Z as usize;
        let mut channel_grid = vec![None; chunk_width * chunk_depth];
        let mut polygon_mask = vec![false; chunk_width * chunk_depth];
        let mut surface_grid = vec![None; chunk_width * chunk_depth];
        let mut terrain_grid = vec![None; chunk_width * chunk_depth];

        for i in 0..CHUNK_SIZE_X {
            for k in 0..CHUNK_SIZE_Z {
                let vx = min_v.x + i;
                let vz = min_v.z + k;
                let (lat, lon) = voxel_to_gps(vx, vz, origin_ecef_y, WGS84_A, WGS84_B);
                let surface_idx = k as usize * chunk_width + i as usize;
                let terrain_y = surface_voxel_y(octree, vx, vz, min_v.y, max_v.y)
                    .unwrap_or(self.origin_voxel.y);
                terrain_grid[surface_idx] = Some(terrain_y);
                let inside_backed_channel_polygon = backed_channel_polygons
                    .iter()
                    .any(|water| point_in_water_polygon(lat, lon, water));
                polygon_mask[surface_idx] = inside_backed_channel_polygon;

                let Some(profile) = waterway_carve_profile_at(
                    lat,
                    lon,
                    waterways,
                    river_profiles,
                    &backed_channel_polygons,
                ) else {
                    continue;
                };

                // ── Determine water surface voxel Y ──────────────────────────
                // River profiles are orthometric heights, so convert them with the
                // same geoid-aware datum bridge as terrain.rs and clamp to terrain.
                let water_surface_y = resolve_channel_water_surface_y(
                    self.origin_gps,
                    self.origin_voxel,
                    lat,
                    lon,
                    terrain_y,
                    (profile.water_surface_m > 1.0).then_some(profile.water_surface_m as f64),
                );

                let bank_zone_width_m =
                    waterway_bank_zone_width_m(profile.half_width_m, profile.center_depth_m);
                let inside_influence = inside_backed_channel_polygon
                    || profile.dist_to_center_m <= profile.half_width_m + bank_zone_width_m;
                if !inside_influence {
                    continue;
                }

                let wetted_depth_vox = channel_wetted_depth_vox(&profile);
                surface_grid[surface_idx] = Some(water_surface_y);
                channel_grid[surface_idx] = Some(ChannelCol {
                    grid_x: i as usize,
                    grid_z: k as usize,
                    vx,
                    vz,
                    terrain_y,
                    water_surface_y,
                    wetted_depth_vox,
                    dist_to_center_m: profile.dist_to_center_m,
                    half_width_m: profile.half_width_m,
                    center_depth_m: profile.center_depth_m,
                    polygon_backed: profile.polygon_backed,
                    inside_backed_channel_polygon,
                });
            }
        }

        if channel_grid.iter().all(|col| col.is_none()) {
            return;
        }

        stabilize_channel_surface_grid(&mut surface_grid, chunk_width, chunk_depth);
        let polygon_wetted_mask = dilate_bool_grid(&polygon_mask, chunk_width, chunk_depth, 1);
        bridge_channel_wetted_gaps(
            &mut channel_grid,
            &mut surface_grid,
            &terrain_grid,
            &polygon_wetted_mask,
            min_v.x,
            min_v.z,
            chunk_width,
            chunk_depth,
        );
        stabilize_channel_surface_grid(&mut surface_grid, chunk_width, chunk_depth);
        let channel_cols: Vec<ChannelCol> = channel_grid.iter().flatten().copied().collect();
        let mut wetted_depth_grid = vec![None; chunk_width * chunk_depth];
        let mut bank_surface_grid = vec![None; chunk_width * chunk_depth];
        let mut wet_mask = vec![false; chunk_width * chunk_depth];

        for col in &channel_cols {
            let surface_idx = col.grid_z * chunk_width + col.grid_x;
            let water_surface_y = surface_grid[surface_idx].unwrap_or(col.water_surface_y);
            let wetted_depth_vox = constrain_polygon_backed_wetting(
                col.wetted_depth_vox,
                col.polygon_backed,
                col.inside_backed_channel_polygon,
                polygon_wetted_mask[surface_idx],
            );
            wetted_depth_grid[surface_idx] = wetted_depth_vox;
            wet_mask[surface_idx] = wetted_depth_vox.is_some();
            if wetted_depth_vox.is_none() {
                bank_surface_grid[surface_idx] = tapered_bank_surface_y(
                    col.terrain_y,
                    water_surface_y,
                    col.dist_to_center_m,
                    col.half_width_m,
                    col.center_depth_m,
                );
            }
        }

        stabilize_bank_surface_grid(
            &mut bank_surface_grid,
            &surface_grid,
            &wet_mask,
            &terrain_grid,
            chunk_width,
            chunk_depth,
        );

        for col in channel_cols {
            let surface_idx = col.grid_z * chunk_width + col.grid_x;
            let water_surface_y = surface_grid[surface_idx].unwrap_or(col.water_surface_y);
            if let Some(wetted_depth_vox) = wetted_depth_grid[surface_idx] {
                carve_waterway_column(
                    octree,
                    col.vx,
                    col.vz,
                    min_v.y,
                    max_v.y,
                    col.terrain_y,
                    water_surface_y,
                    Some(wetted_depth_vox),
                    col.dist_to_center_m,
                    col.half_width_m,
                    col.center_depth_m,
                );
            } else {
                carve_bank_column(
                    octree,
                    col.vx,
                    col.vz,
                    min_v.y,
                    max_v.y,
                    col.terrain_y,
                    bank_surface_grid[surface_idx],
                );
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert voxel (vx, vz) to (lat, lon) using the same ellipsoid ECEF formula as terrain.rs.
///
/// `origin_ecef_y` is `(origin_voxel.y + 0.5) + WORLD_MIN_METERS`.
pub(crate) fn voxel_to_gps(
    vx: i64,
    vz: i64,
    origin_ecef_y: f64,
    wgs84_a: f64,
    wgs84_b: f64,
) -> (f64, f64) {
    let ecef_x = (vx as f64 + 0.5) + WORLD_MIN_METERS;
    let ecef_z = (vz as f64 + 0.5) + WORLD_MIN_METERS;
    let y_sq = wgs84_a * wgs84_a * (1.0 - (ecef_z / wgs84_b).powi(2)) - ecef_x * ecef_x;
    let ecef_y = if y_sq > 0.0 {
        y_sq.sqrt() * origin_ecef_y.signum()
    } else {
        origin_ecef_y
    };
    let gps = crate::coordinates::ECEF::new(ecef_x, ecef_y, ecef_z).to_gps();
    (gps.lat, gps.lon)
}

/// Scan downward from `max_vy` to find the highest non-AIR voxel in column (vx, vz).
pub(crate) fn surface_voxel_y(
    octree: &Octree,
    vx: i64,
    vz: i64,
    min_vy: i64,
    max_vy: i64,
) -> Option<i64> {
    for vy in (min_vy..max_vy).rev() {
        if octree.get_voxel(VoxelCoord::new(vx, vy, vz)) != MaterialId::AIR {
            return Some(vy);
        }
    }
    None
}

/// Flat-earth distance from point (lat, lon) to segment (a→b).
///
/// Returns `(distance_m, t_along_segment, closest_lat, closest_lon)`.
pub(crate) fn point_to_segment_dist_m(
    lat: f64,
    lon: f64,
    seg_lat1: f64,
    seg_lon1: f64,
    seg_lat2: f64,
    seg_lon2: f64,
) -> (f64, f64, f64, f64) {
    let cos_lat = lat.to_radians().cos();
    let scale_x = 111_320.0_f64;
    let scale_z = 111_320.0_f64 * cos_lat;

    let px = (lat - seg_lat1) * scale_x;
    let pz = (lon - seg_lon1) * scale_z;
    let dx = (seg_lat2 - seg_lat1) * scale_x;
    let dz = (seg_lon2 - seg_lon1) * scale_z;
    let seg_len2 = dx * dx + dz * dz;

    if seg_len2 < 1e-10 {
        let dist = (px * px + pz * pz).sqrt();
        return (dist, 0.0, seg_lat1, seg_lon1);
    }

    let t = ((px * dx + pz * dz) / seg_len2).clamp(0.0, 1.0);
    let cx = dx * t - px;
    let cz = dz * t - pz;
    let dist = (cx * cx + cz * cz).sqrt();
    (
        dist,
        t,
        seg_lat1 + t * (seg_lat2 - seg_lat1),
        seg_lon1 + t * (seg_lon2 - seg_lon1),
    )
}

fn waterway_carve_profile_at(
    lat: f64,
    lon: f64,
    waterways: &[WaterwayLine],
    river_profiles: Option<&RiverProfileCache>,
    backed_channel_polygons: &[&OsmWater],
) -> Option<WaterwayCarveProfile> {
    if let Some(cache) = river_profiles {
        if let Some((t, seg)) = cache.nearest(lat, lon, 300.0) {
            let (water_surface_m, half_width_m, center_depth_m) = seg.at_t(t);
            let mut best_dist_m = f64::MAX;
            for pair in seg.nodes.windows(2) {
                let (dist_m, _, _, _) = crate::worldgen_river::point_to_segment_dist(
                    lat,
                    lon,
                    pair[0].lat,
                    pair[0].lon,
                    pair[1].lat,
                    pair[1].lon,
                );
                if dist_m < best_dist_m {
                    best_dist_m = dist_m;
                }
            }
            if best_dist_m.is_finite() {
                let half_width_m = half_width_m.max(1.0);
                let bank_t = (best_dist_m / half_width_m as f64).clamp(0.0, 1.0) as f32;
                let polygon_backed = polyline_has_backing(&seg.nodes, backed_channel_polygons);
                return Some(WaterwayCarveProfile {
                    water_surface_m,
                    half_width_m,
                    center_depth_m: center_depth_m.max(1.0),
                    carve_depth_m: (center_depth_m * (1.0 - bank_t * bank_t)).max(0.0),
                    dist_to_center_m: best_dist_m as f32,
                    polygon_backed,
                });
            }
        }
    }

    let mut best_match: Option<(f64, f32, f32, f32, bool)> = None;
    for wl in waterways {
        let half_width_m = wl.half_width_m() as f32;
        let center_depth_m = wl.channel_depth_vox() as f32;
        let polygon_backed = polyline_has_backing(&wl.nodes, backed_channel_polygons);
        for pair in wl.nodes.windows(2) {
            let (dist_m, t, _, _) = point_to_segment_dist_m(
                lat,
                lon,
                pair[0].lat,
                pair[0].lon,
                pair[1].lat,
                pair[1].lon,
            );
            if best_match
                .map(|(best_dist_m, _, _, _, _)| dist_m < best_dist_m)
                .unwrap_or(true)
            {
                let elev0 = pair[0].alt as f32;
                let elev1 = pair[1].alt as f32;
                let water_surface_m = elev0 + (elev1 - elev0) * t as f32;
                best_match = Some((
                    dist_m,
                    water_surface_m,
                    half_width_m,
                    center_depth_m,
                    polygon_backed,
                ));
            }
        }
    }

    best_match.map(
        |(dist_to_center_m, water_surface_m, half_width_m, center_depth_m, polygon_backed)| {
            let half_width_m = half_width_m.max(1.0);
            let bank_t = (dist_to_center_m / half_width_m as f64).clamp(0.0, 1.0) as f32;
            WaterwayCarveProfile {
                water_surface_m,
                half_width_m,
                center_depth_m: center_depth_m.max(1.0),
                carve_depth_m: (center_depth_m * (1.0 - bank_t * bank_t)).max(0.0),
                dist_to_center_m: dist_to_center_m as f32,
                polygon_backed,
            }
        },
    )
}

fn is_channelized_water_type(water_type: &str) -> bool {
    matches!(
        water_type,
        "river" | "stream" | "canal" | "drain" | "ditch" | "riverbank"
    )
}

fn point_in_water_polygon(lat: f64, lon: f64, water: &OsmWater) -> bool {
    crate::osm::point_in_polygon(lat, lon, &water.polygon)
        && !water
            .holes
            .iter()
            .any(|hole| crate::osm::point_in_polygon(lat, lon, hole))
}

fn polyline_has_backbone_inside_polygon(nodes: &[GPS], polygon: &[GPS]) -> bool {
    nodes.windows(2).any(|pair| {
        crate::osm::point_in_polygon(pair[0].lat, pair[0].lon, polygon)
            || crate::osm::point_in_polygon(
                (pair[0].lat + pair[1].lat) * 0.5,
                (pair[0].lon + pair[1].lon) * 0.5,
                polygon,
            )
    })
}

fn polyline_has_backing(nodes: &[GPS], backed_channel_polygons: &[&OsmWater]) -> bool {
    backed_channel_polygons
        .iter()
        .any(|water| polyline_has_backbone_inside_polygon(nodes, &water.polygon))
}

fn line_has_backbone_inside_polygon(line: &WaterwayLine, polygon: &[GPS]) -> bool {
    polyline_has_backbone_inside_polygon(&line.nodes, polygon)
}

fn should_skip_flat_polygon_fill(water: &OsmWater, waterways: &[WaterwayLine]) -> bool {
    is_channelized_water_type(&water.water_type)
        && waterways
            .iter()
            .filter(|line| is_channelized_water_type(&line.waterway_type))
            .any(|line| line_has_backbone_inside_polygon(line, &water.polygon))
}

fn waterway_bank_zone_width_m(half_width_m: f32, center_depth_m: f32) -> f32 {
    (half_width_m * 0.75)
        .max(center_depth_m * 2.0)
        .clamp(4.0, 24.0)
}

fn channel_wetted_depth_vox(profile: &WaterwayCarveProfile) -> Option<i64> {
    (profile.dist_to_center_m <= profile.half_width_m)
        .then_some(profile.carve_depth_m.max(1.0).round() as i64)
}

const MAX_FLOW_SURFACE_STEP_VOX: i64 = 1;
const MAX_BANK_SURFACE_STEP_VOX: i64 = 2;
const MAX_CHANNEL_CONTINUITY_GAP_CELLS: usize = 3;
const MAX_CHANNEL_CONTINUITY_OVERBURDEN_VOX: i64 = 4;

fn stabilize_channel_surface_grid(surfaces: &mut [Option<i64>], width: usize, height: usize) {
    if surfaces.is_empty() {
        return;
    }

    let snapshot = surfaces.to_vec();
    for z in 0..height {
        for x in 0..width {
            let idx = z * width + x;
            let Some(surface_y) = snapshot[idx] else {
                continue;
            };

            let mut min_neighbor = i64::MAX;
            let mut max_neighbor = i64::MIN;
            let mut neighbor_count = 0usize;

            for dz in -1isize..=1 {
                for dx in -1isize..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }

                    let nx = x as isize + dx;
                    let nz = z as isize + dz;
                    if nx < 0 || nz < 0 || nx >= width as isize || nz >= height as isize {
                        continue;
                    }

                    let neighbor_idx = nz as usize * width + nx as usize;
                    let Some(neighbor_y) = snapshot[neighbor_idx] else {
                        continue;
                    };

                    min_neighbor = min_neighbor.min(neighbor_y);
                    max_neighbor = max_neighbor.max(neighbor_y);
                    neighbor_count += 1;
                }
            }

            if neighbor_count < 2 {
                continue;
            }

            surfaces[idx] = Some(surface_y.clamp(
                min_neighbor - MAX_FLOW_SURFACE_STEP_VOX,
                max_neighbor + MAX_FLOW_SURFACE_STEP_VOX,
            ));
        }
    }
}

fn stabilize_bank_surface_grid(
    bank_surfaces: &mut [Option<i64>],
    water_surfaces: &[Option<i64>],
    wet_mask: &[bool],
    terrain_grid: &[Option<i64>],
    width: usize,
    height: usize,
) {
    if bank_surfaces.is_empty() {
        return;
    }

    let snapshot = bank_surfaces.to_vec();
    for z in 0..height {
        for x in 0..width {
            let idx = z * width + x;
            let Some(bank_surface_y) = snapshot[idx] else {
                continue;
            };
            let Some(terrain_y) = terrain_grid[idx] else {
                continue;
            };
            let min_bank_top_y = water_surfaces[idx]
                .map(|surface_y| (surface_y + 1).min(terrain_y))
                .unwrap_or(terrain_y);

            let mut min_anchor = i64::MAX;
            let mut max_anchor = i64::MIN;
            let mut neighbor_count = 0usize;

            for dz in -1isize..=1 {
                for dx in -1isize..=1 {
                    if dx == 0 && dz == 0 {
                        continue;
                    }

                    let nx = x as isize + dx;
                    let nz = z as isize + dz;
                    if nx < 0 || nz < 0 || nx >= width as isize || nz >= height as isize {
                        continue;
                    }

                    let neighbor_idx = nz as usize * width + nx as usize;
                    if let Some(neighbor_bank_y) = snapshot[neighbor_idx] {
                        min_anchor = min_anchor.min(neighbor_bank_y);
                        max_anchor = max_anchor.max(neighbor_bank_y);
                        neighbor_count += 1;
                    } else if wet_mask[neighbor_idx] {
                        let Some(neighbor_water_y) = water_surfaces[neighbor_idx] else {
                            continue;
                        };
                        let neighbor_bank_floor = terrain_grid[neighbor_idx]
                            .map(|terrain_y| (neighbor_water_y + 1).min(terrain_y));
                        let Some(anchor_y) = neighbor_bank_floor else {
                            continue;
                        };
                        min_anchor = min_anchor.min(anchor_y);
                        max_anchor = max_anchor.max(anchor_y);
                        neighbor_count += 1;
                    }
                }
            }

            if neighbor_count < 2 {
                continue;
            }

            bank_surfaces[idx] = Some(
                bank_surface_y
                    .clamp(
                        min_anchor - MAX_BANK_SURFACE_STEP_VOX,
                        max_anchor + MAX_BANK_SURFACE_STEP_VOX,
                    )
                    .clamp(min_bank_top_y, terrain_y),
            );
        }
    }
}

fn dilate_bool_grid(mask: &[bool], width: usize, height: usize, radius: usize) -> Vec<bool> {
    let mut dilated = vec![false; mask.len()];
    for z in 0..height {
        for x in 0..width {
            let idx = z * width + x;
            if mask[idx] {
                dilated[idx] = true;
                continue;
            }

            'neighbors: for dz in -(radius as isize)..=(radius as isize) {
                for dx in -(radius as isize)..=(radius as isize) {
                    let nx = x as isize + dx;
                    let nz = z as isize + dz;
                    if nx < 0 || nz < 0 || nx >= width as isize || nz >= height as isize {
                        continue;
                    }
                    let neighbor_idx = nz as usize * width + nx as usize;
                    if mask[neighbor_idx] {
                        dilated[idx] = true;
                        break 'neighbors;
                    }
                }
            }
        }
    }
    dilated
}

fn constrain_polygon_backed_wetting(
    wetted_depth_vox: Option<i64>,
    polygon_backed: bool,
    inside_polygon: bool,
    inside_polygon_wet_mask: bool,
) -> Option<i64> {
    if inside_polygon {
        return if inside_polygon_wet_mask {
            Some(wetted_depth_vox.unwrap_or(1))
        } else {
            None
        };
    }

    if polygon_backed && !inside_polygon_wet_mask {
        None
    } else {
        wetted_depth_vox
    }
}

fn channel_col_is_wet(col: &ChannelCol, inside_polygon_wet_mask: bool) -> bool {
    constrain_polygon_backed_wetting(
        col.wetted_depth_vox,
        col.polygon_backed,
        col.inside_backed_channel_polygon,
        inside_polygon_wet_mask,
    )
    .is_some()
}

fn interpolate_i64(a: i64, b: i64, numer: usize, denom: usize) -> i64 {
    if denom == 0 {
        return a;
    }
    let t = numer as f32 / denom as f32;
    ((a as f32) + (b - a) as f32 * t).round() as i64
}

fn interpolate_f32(a: f32, b: f32, numer: usize, denom: usize) -> f32 {
    if denom == 0 {
        return a;
    }
    let t = numer as f32 / denom as f32;
    a + (b - a) * t
}

fn find_wet_channel_anchor(
    x: usize,
    z: usize,
    dx: isize,
    dz: isize,
    width: usize,
    height: usize,
    channel_grid: &[Option<ChannelCol>],
    surface_grid: &[Option<i64>],
    polygon_wetted_mask: &[bool],
) -> Option<(usize, ChannelCol, i64)> {
    for steps in 1..=(MAX_CHANNEL_CONTINUITY_GAP_CELLS + 1) {
        let nx = x as isize + dx * steps as isize;
        let nz = z as isize + dz * steps as isize;
        if nx < 0 || nz < 0 || nx >= width as isize || nz >= height as isize {
            break;
        }
        let idx = nz as usize * width + nx as usize;
        let Some(col) = channel_grid[idx] else {
            continue;
        };
        let Some(surface_y) = surface_grid[idx] else {
            continue;
        };
        if channel_col_is_wet(&col, polygon_wetted_mask[idx]) {
            return Some((steps, col, surface_y));
        }
    }
    None
}

fn bridge_channel_wetted_gaps(
    channel_grid: &mut [Option<ChannelCol>],
    surface_grid: &mut [Option<i64>],
    terrain_grid: &[Option<i64>],
    polygon_wetted_mask: &[bool],
    min_vx: i64,
    min_vz: i64,
    width: usize,
    height: usize,
) {
    let snapshot_channels = channel_grid.to_vec();
    let snapshot_surfaces = surface_grid.to_vec();
    let axes = [
        (-1isize, 0isize, 1isize, 0isize),
        (0, -1, 0, 1),
        (-1, -1, 1, 1),
        (-1, 1, 1, -1),
    ];

    for z in 0..height {
        for x in 0..width {
            let idx = z * width + x;
            if snapshot_channels[idx]
                .as_ref()
                .map(|col| channel_col_is_wet(col, polygon_wetted_mask[idx]))
                .unwrap_or(false)
            {
                continue;
            }

            let Some(terrain_y) = terrain_grid[idx] else {
                continue;
            };
            let mut best: Option<(usize, i64, ChannelCol)> = None;

            for (neg_dx, neg_dz, pos_dx, pos_dz) in axes {
                let Some((neg_steps, neg_col, neg_surface_y)) = find_wet_channel_anchor(
                    x,
                    z,
                    neg_dx,
                    neg_dz,
                    width,
                    height,
                    &snapshot_channels,
                    &snapshot_surfaces,
                    polygon_wetted_mask,
                ) else {
                    continue;
                };
                let Some((pos_steps, pos_col, pos_surface_y)) = find_wet_channel_anchor(
                    x,
                    z,
                    pos_dx,
                    pos_dz,
                    width,
                    height,
                    &snapshot_channels,
                    &snapshot_surfaces,
                    polygon_wetted_mask,
                ) else {
                    continue;
                };

                let gap_cells = neg_steps + pos_steps - 1;
                if gap_cells > MAX_CHANNEL_CONTINUITY_GAP_CELLS {
                    continue;
                }

                let allowed_surface_delta = MAX_FLOW_SURFACE_STEP_VOX * (gap_cells as i64 + 1);
                let surface_delta = (pos_surface_y - neg_surface_y).abs();
                if surface_delta > allowed_surface_delta {
                    continue;
                }

                let span = neg_steps + pos_steps;
                let polygon_backed = snapshot_channels[idx]
                    .map(|col| col.polygon_backed)
                    .unwrap_or(neg_col.polygon_backed || pos_col.polygon_backed);
                if polygon_backed && !polygon_wetted_mask[idx] {
                    continue;
                }

                let water_surface_y = snapshot_surfaces[idx].unwrap_or_else(|| {
                    interpolate_i64(neg_surface_y, pos_surface_y, neg_steps, span)
                });
                let wetted_depth_vox = interpolate_i64(
                    neg_col.wetted_depth_vox.unwrap_or(1).max(1),
                    pos_col.wetted_depth_vox.unwrap_or(1).max(1),
                    neg_steps,
                    span,
                )
                .max(1);
                let overburden_limit =
                    MAX_CHANNEL_CONTINUITY_OVERBURDEN_VOX.max(wetted_depth_vox + 1);
                if terrain_y > water_surface_y + overburden_limit {
                    continue;
                }

                let candidate = ChannelCol {
                    grid_x: x,
                    grid_z: z,
                    vx: snapshot_channels[idx]
                        .map(|col| col.vx)
                        .unwrap_or(min_vx + x as i64),
                    vz: snapshot_channels[idx]
                        .map(|col| col.vz)
                        .unwrap_or(min_vz + z as i64),
                    terrain_y,
                    water_surface_y,
                    wetted_depth_vox: Some(wetted_depth_vox),
                    dist_to_center_m: snapshot_channels[idx]
                        .map(|col| col.dist_to_center_m)
                        .unwrap_or(0.0),
                    half_width_m: snapshot_channels[idx]
                        .map(|col| col.half_width_m)
                        .unwrap_or_else(|| {
                            interpolate_f32(
                                neg_col.half_width_m,
                                pos_col.half_width_m,
                                neg_steps,
                                span,
                            )
                        })
                        .max(1.0),
                    center_depth_m: snapshot_channels[idx]
                        .map(|col| col.center_depth_m)
                        .unwrap_or_else(|| {
                            interpolate_f32(
                                neg_col.center_depth_m,
                                pos_col.center_depth_m,
                                neg_steps,
                                span,
                            )
                        })
                        .max(1.0),
                    polygon_backed,
                    inside_backed_channel_polygon: snapshot_channels[idx]
                        .map(|col| col.inside_backed_channel_polygon)
                        .unwrap_or(false),
                };

                let score = (gap_cells, surface_delta);
                if best
                    .map(|(best_gap, best_surface, _)| score < (best_gap, best_surface))
                    .unwrap_or(true)
                {
                    best = Some((gap_cells, surface_delta, candidate));
                }
            }

            if let Some((_, _, bridged_col)) = best {
                channel_grid[idx] = Some(bridged_col);
                surface_grid[idx] = Some(bridged_col.water_surface_y);
            }
        }
    }
}

fn smoothstep01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn tapered_bank_surface_y(
    terrain_y: i64,
    water_surface_y: i64,
    dist_to_center_m: f32,
    half_width_m: f32,
    center_depth_m: f32,
) -> Option<i64> {
    if terrain_y <= water_surface_y {
        return None;
    }
    let bank_zone_width_m = waterway_bank_zone_width_m(half_width_m, center_depth_m);
    let bank_dist_m = (dist_to_center_m - half_width_m).max(0.0);
    if bank_dist_m >= bank_zone_width_m {
        return None;
    }
    let eased = smoothstep01(bank_dist_m / bank_zone_width_m);
    let rise = ((terrain_y - water_surface_y) as f32 * eased).round() as i64;
    let min_bank_top_y = (water_surface_y + 1).min(terrain_y);
    Some((water_surface_y + rise).clamp(min_bank_top_y, terrain_y))
}

fn clear_column_to_air(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    min_vy: i64,
    max_vy: i64,
    from_y: i64,
    to_y_inclusive: i64,
) {
    let start_y = from_y.max(min_vy);
    let end_y = (to_y_inclusive + 1).min(max_vy);
    if start_y >= end_y {
        return;
    }
    for vy in start_y..end_y {
        octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::AIR);
    }
}

fn carve_bank_column(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    min_vy: i64,
    max_vy: i64,
    terrain_y: i64,
    bank_surface_y: Option<i64>,
) {
    let Some(bank_surface_y) = bank_surface_y else {
        return;
    };
    clear_column_to_air(
        octree,
        vx,
        vz,
        min_vy,
        max_vy,
        bank_surface_y + 1,
        terrain_y,
    );
}

fn carve_waterway_column(
    octree: &mut Octree,
    vx: i64,
    vz: i64,
    min_vy: i64,
    max_vy: i64,
    terrain_y: i64,
    water_surface_y: i64,
    wetted_depth_vox: Option<i64>,
    dist_to_center_m: f32,
    half_width_m: f32,
    center_depth_m: f32,
) {
    if let Some(depth_vox) = wetted_depth_vox {
        let channel_bot = water_surface_y - depth_vox.max(1);
        let bedrock = water_surface_y - 200;

        for vy in bedrock.max(min_vy)..channel_bot.max(min_vy) {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
        }
        for vy in channel_bot.max(min_vy)..(water_surface_y + 1).min(max_vy) {
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
        }
        clear_column_to_air(
            octree,
            vx,
            vz,
            min_vy,
            max_vy,
            water_surface_y + 1,
            terrain_y,
        );
        return;
    }

    if let Some(bank_surface_y) = tapered_bank_surface_y(
        terrain_y,
        water_surface_y,
        dist_to_center_m,
        half_width_m,
        center_depth_m,
    ) {
        clear_column_to_air(
            octree,
            vx,
            vz,
            min_vy,
            max_vy,
            bank_surface_y + 1,
            terrain_y,
        );
    }
}

fn orthometric_height_to_voxel_y(
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
    lat: f64,
    lon: f64,
    orthometric_height_m: f64,
) -> i64 {
    let undulation_m = crate::elevation::egm96_undulation(lat, lon);
    let surface_delta = orthometric_height_m + undulation_m - origin_gps.alt;
    origin_voxel.y + surface_delta as i64
}

#[cfg(test)]
fn resolve_water_surface_y(
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
    lat: f64,
    lon: f64,
    terrain_y: i64,
    profile_surface_ortho_m: Option<f64>,
) -> i64 {
    profile_surface_ortho_m
        .map(|surface_m| {
            orthometric_height_to_voxel_y(origin_gps, origin_voxel, lat, lon, surface_m)
                .min(terrain_y)
        })
        .unwrap_or(terrain_y)
}

fn resolve_channel_water_surface_y(
    origin_gps: GPS,
    origin_voxel: VoxelCoord,
    lat: f64,
    lon: f64,
    terrain_y: i64,
    profile_surface_ortho_m: Option<f64>,
) -> i64 {
    profile_surface_ortho_m
        .map(|surface_m| {
            orthometric_height_to_voxel_y(origin_gps, origin_voxel, lat, lon, surface_m)
        })
        .unwrap_or(terrain_y)
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelCol, WaterwayCarveProfile, bridge_channel_wetted_gaps, carve_waterway_column,
        channel_wetted_depth_vox, constrain_polygon_backed_wetting, dilate_bool_grid,
        orthometric_height_to_voxel_y, resolve_channel_water_surface_y, resolve_water_surface_y,
        should_skip_flat_polygon_fill, stabilize_bank_surface_grid, stabilize_channel_surface_grid,
        surface_voxel_y,
    };
    use crate::coordinates::GPS;
    use crate::elevation::egm96_undulation;
    use crate::materials::MaterialId;
    use crate::osm::{OsmWater, WaterwayLine};
    use crate::voxel::{Octree, VoxelCoord};

    fn test_channel_col(
        grid_x: usize,
        wetted_depth_vox: Option<i64>,
        water_surface_y: i64,
        terrain_y: i64,
        polygon_backed: bool,
        inside_backed_channel_polygon: bool,
    ) -> ChannelCol {
        ChannelCol {
            grid_x,
            grid_z: 0,
            vx: grid_x as i64,
            vz: 0,
            terrain_y,
            water_surface_y,
            wetted_depth_vox,
            dist_to_center_m: 0.0,
            half_width_m: 8.0,
            center_depth_m: 3.0,
            polygon_backed,
            inside_backed_channel_polygon,
        }
    }

    #[test]
    fn orthometric_height_to_voxel_y_matches_terrain_datum_bridge() {
        let origin_gps = GPS::new(-26.1901, 152.6660, 150.0);
        let origin_voxel = VoxelCoord::new(1_312_300, 9_029_900, 3_602_100);
        let lat = -26.1906;
        let lon = 152.6653;
        let water_surface_m = 112.4;

        let expected =
            origin_voxel.y + (water_surface_m + egm96_undulation(lat, lon) - origin_gps.alt) as i64;

        assert_eq!(
            orthometric_height_to_voxel_y(origin_gps, origin_voxel, lat, lon, water_surface_m),
            expected
        );
    }

    #[test]
    fn resolve_water_surface_y_clamps_profile_to_terrain_surface() {
        let origin_gps = GPS::new(-26.1901, 152.6660, 150.0);
        let origin_voxel = VoxelCoord::new(1_312_300, 9_029_900, 3_602_100);
        let terrain_y = origin_voxel.y - 15;

        let resolved = resolve_water_surface_y(
            origin_gps,
            origin_voxel,
            -26.1906,
            152.6653,
            terrain_y,
            Some(150.0),
        );

        assert_eq!(resolved, terrain_y);
    }

    #[test]
    fn resolve_water_surface_y_falls_back_to_terrain_without_profile() {
        let origin_gps = GPS::new(-26.1901, 152.6660, 150.0);
        let origin_voxel = VoxelCoord::new(1_312_300, 9_029_900, 3_602_100);
        let terrain_y = origin_voxel.y - 23;

        let resolved = resolve_water_surface_y(
            origin_gps,
            origin_voxel,
            -26.1906,
            152.6653,
            terrain_y,
            None,
        );

        assert_eq!(resolved, terrain_y);
    }

    #[test]
    fn resolve_channel_water_surface_y_preserves_profile_surface_over_low_terrain() {
        let origin_gps = GPS::new(-26.1901, 152.6660, 150.0);
        let origin_voxel = VoxelCoord::new(1_312_300, 9_029_900, 3_602_100);
        let terrain_y = origin_voxel.y - 15;
        let profile_surface = 150.0;
        let expected = orthometric_height_to_voxel_y(
            origin_gps,
            origin_voxel,
            -26.1906,
            152.6653,
            profile_surface,
        );

        let resolved = resolve_channel_water_surface_y(
            origin_gps,
            origin_voxel,
            -26.1906,
            152.6653,
            terrain_y,
            Some(profile_surface),
        );

        assert_eq!(resolved, expected);
        assert!(resolved > terrain_y);
    }

    #[test]
    fn skip_flat_fill_for_channelized_river_polygon_with_centerline() {
        let river_polygon = OsmWater {
            osm_id: 1,
            polygon: vec![
                GPS::new(-26.0, 152.0, 0.0),
                GPS::new(-26.0, 152.01, 0.0),
                GPS::new(-26.01, 152.01, 0.0),
                GPS::new(-26.01, 152.0, 0.0),
            ],
            holes: vec![],
            name: Some("Test River".into()),
            water_type: "river".into(),
        };
        let lines = vec![WaterwayLine {
            nodes: vec![
                GPS::new(-26.005, 152.002, 0.0),
                GPS::new(-26.005, 152.008, 0.0),
            ],
            waterway_type: "river".into(),
            name: Some("Test River".into()),
        }];

        assert!(should_skip_flat_polygon_fill(&river_polygon, &lines));
    }

    #[test]
    fn keep_flat_fill_for_lake_polygon_even_with_inlet_line() {
        let lake_polygon = OsmWater {
            osm_id: 2,
            polygon: vec![
                GPS::new(-26.0, 152.0, 0.0),
                GPS::new(-26.0, 152.01, 0.0),
                GPS::new(-26.01, 152.01, 0.0),
                GPS::new(-26.01, 152.0, 0.0),
            ],
            holes: vec![],
            name: Some("Test Lake".into()),
            water_type: "lake".into(),
        };
        let inlet = vec![WaterwayLine {
            nodes: vec![
                GPS::new(-26.005, 152.002, 0.0),
                GPS::new(-26.005, 152.008, 0.0),
            ],
            waterway_type: "stream".into(),
            name: Some("Inlet".into()),
        }];

        assert!(!should_skip_flat_polygon_fill(&lake_polygon, &inlet));
    }

    #[test]
    fn dilate_bool_grid_expands_single_polygon_cell() {
        let mask = vec![false, false, false, false, true, false, false, false, false];

        let dilated = dilate_bool_grid(&mask, 3, 3, 1);

        assert!(dilated.into_iter().all(|v| v));
    }

    #[test]
    fn constrain_polygon_backed_wetting_drops_water_outside_polygon_mask() {
        assert_eq!(
            constrain_polygon_backed_wetting(Some(3), true, false, false),
            None
        );
    }

    #[test]
    fn constrain_polygon_backed_wetting_keeps_unbacked_or_in_mask_water() {
        assert_eq!(
            constrain_polygon_backed_wetting(Some(3), true, true, true),
            Some(3)
        );
        assert_eq!(
            constrain_polygon_backed_wetting(Some(3), false, false, false),
            Some(3)
        );
    }

    #[test]
    fn constrain_polygon_backed_wetting_keeps_minimum_water_inside_polygon() {
        assert_eq!(
            constrain_polygon_backed_wetting(None, true, true, true),
            Some(1)
        );
        assert_eq!(
            constrain_polygon_backed_wetting(None, false, true, true),
            Some(1)
        );
        assert_eq!(
            constrain_polygon_backed_wetting(None, true, false, true),
            None
        );
    }

    #[test]
    fn stabilize_channel_surface_grid_clamps_isolated_spike() {
        let mut surfaces = vec![
            Some(10),
            Some(10),
            Some(10),
            Some(10),
            Some(13),
            Some(10),
            Some(10),
            Some(10),
            Some(10),
        ];

        stabilize_channel_surface_grid(&mut surfaces, 3, 3);

        assert_eq!(surfaces[4], Some(11));
    }

    #[test]
    fn stabilize_channel_surface_grid_clamps_isolated_pit() {
        let mut surfaces = vec![
            Some(10),
            Some(10),
            Some(10),
            Some(10),
            Some(7),
            Some(10),
            Some(10),
            Some(10),
            Some(10),
        ];

        stabilize_channel_surface_grid(&mut surfaces, 3, 3);

        assert_eq!(surfaces[4], Some(9));
    }

    #[test]
    fn stabilize_channel_surface_grid_preserves_gradual_slope() {
        let mut surfaces = vec![
            Some(10),
            Some(10),
            Some(10),
            Some(11),
            Some(11),
            Some(11),
            Some(12),
            Some(12),
            Some(12),
        ];
        let original = surfaces.clone();

        stabilize_channel_surface_grid(&mut surfaces, 3, 3);

        assert_eq!(surfaces, original);
    }

    #[test]
    fn bridge_channel_wetted_gaps_fills_missing_column_between_wet_endpoints() {
        let mut channels = vec![
            Some(test_channel_col(0, Some(3), 10, 12, false, false)),
            None,
            Some(test_channel_col(2, Some(3), 10, 12, false, false)),
        ];
        let mut surfaces = vec![Some(10), None, Some(10)];
        let terrain = vec![Some(12), Some(12), Some(12)];
        let polygon_wetted_mask = vec![false; 3];

        bridge_channel_wetted_gaps(
            &mut channels,
            &mut surfaces,
            &terrain,
            &polygon_wetted_mask,
            0,
            0,
            3,
            1,
        );

        assert_eq!(channels[1].and_then(|col| col.wetted_depth_vox), Some(3));
        assert_eq!(surfaces[1], Some(10));
    }

    #[test]
    fn bridge_channel_wetted_gaps_promotes_existing_bank_gap_to_water() {
        let mut channels = vec![
            Some(test_channel_col(0, Some(2), 10, 13, false, false)),
            Some(test_channel_col(1, None, 10, 12, false, false)),
            Some(test_channel_col(2, Some(2), 10, 13, false, false)),
        ];
        let mut surfaces = vec![Some(10), Some(10), Some(10)];
        let terrain = vec![Some(13), Some(12), Some(13)];
        let polygon_wetted_mask = vec![false; 3];

        bridge_channel_wetted_gaps(
            &mut channels,
            &mut surfaces,
            &terrain,
            &polygon_wetted_mask,
            0,
            0,
            3,
            1,
        );

        assert_eq!(channels[1].and_then(|col| col.wetted_depth_vox), Some(2));
    }

    #[test]
    fn bridge_channel_wetted_gaps_respects_polygon_owned_wet_mask() {
        let mut channels = vec![
            Some(test_channel_col(0, Some(3), 10, 12, true, true)),
            None,
            Some(test_channel_col(2, Some(3), 10, 12, true, true)),
        ];
        let mut surfaces = vec![Some(10), None, Some(10)];
        let terrain = vec![Some(12), Some(12), Some(12)];
        let polygon_wetted_mask = vec![true, false, true];

        bridge_channel_wetted_gaps(
            &mut channels,
            &mut surfaces,
            &terrain,
            &polygon_wetted_mask,
            0,
            0,
            3,
            1,
        );

        assert!(channels[1].is_none());
        assert_eq!(surfaces[1], None);
    }

    #[test]
    fn bridge_channel_wetted_gaps_rejects_high_ridge_between_endpoints() {
        let mut channels = vec![
            Some(test_channel_col(0, Some(2), 10, 12, false, false)),
            None,
            Some(test_channel_col(2, Some(2), 10, 12, false, false)),
        ];
        let mut surfaces = vec![Some(10), None, Some(10)];
        let terrain = vec![Some(12), Some(18), Some(12)];
        let polygon_wetted_mask = vec![false; 3];

        bridge_channel_wetted_gaps(
            &mut channels,
            &mut surfaces,
            &terrain,
            &polygon_wetted_mask,
            0,
            0,
            3,
            1,
        );

        assert!(channels[1].is_none());
        assert_eq!(surfaces[1], None);
    }

    #[test]
    fn stabilize_bank_surface_grid_clamps_isolated_bank_spike() {
        let mut bank_surfaces = vec![
            Some(12),
            Some(12),
            Some(12),
            Some(12),
            Some(18),
            Some(12),
            Some(12),
            Some(12),
            Some(12),
        ];
        let water_surfaces = vec![Some(10); 9];
        let wet_mask = vec![false; 9];
        let terrain_grid = vec![Some(20); 9];

        stabilize_bank_surface_grid(
            &mut bank_surfaces,
            &water_surfaces,
            &wet_mask,
            &terrain_grid,
            3,
            3,
        );

        assert_eq!(bank_surfaces[4], Some(14));
    }

    #[test]
    fn stabilize_bank_surface_grid_clamps_bank_pit_above_water() {
        let mut bank_surfaces = vec![
            Some(12),
            Some(12),
            Some(12),
            Some(12),
            Some(9),
            Some(12),
            Some(12),
            Some(12),
            Some(12),
        ];
        let water_surfaces = vec![Some(10); 9];
        let wet_mask = vec![false; 9];
        let terrain_grid = vec![Some(20); 9];

        stabilize_bank_surface_grid(
            &mut bank_surfaces,
            &water_surfaces,
            &wet_mask,
            &terrain_grid,
            3,
            3,
        );

        assert_eq!(bank_surfaces[4], Some(11));
    }

    #[test]
    fn stabilize_bank_surface_grid_uses_wet_neighbors_as_bank_floor() {
        let mut bank_surfaces = vec![None, None, None, None, Some(17), None, None, None, None];
        let water_surfaces = vec![Some(10); 9];
        let wet_mask = vec![true, true, true, true, false, true, true, true, true];
        let terrain_grid = vec![Some(20); 9];

        stabilize_bank_surface_grid(
            &mut bank_surfaces,
            &water_surfaces,
            &wet_mask,
            &terrain_grid,
            3,
            3,
        );

        assert_eq!(bank_surfaces[4], Some(13));
    }

    #[test]
    fn stabilize_bank_surface_grid_preserves_gradual_bank_slope() {
        let mut bank_surfaces = vec![
            Some(11),
            Some(12),
            Some(13),
            Some(11),
            Some(12),
            Some(13),
            Some(11),
            Some(12),
            Some(13),
        ];
        let original = bank_surfaces.clone();
        let water_surfaces = vec![Some(10); 9];
        let wet_mask = vec![false; 9];
        let terrain_grid = vec![Some(20); 9];

        stabilize_bank_surface_grid(
            &mut bank_surfaces,
            &water_surfaces,
            &wet_mask,
            &terrain_grid,
            3,
            3,
        );

        assert_eq!(bank_surfaces, original);
    }

    #[test]
    fn carve_waterway_column_clears_overhang_above_water_surface() {
        let mut octree = Octree::new();
        let vx = 10;
        let vz = 12;
        for vy in 0..15 {
            let mat = if vy == 14 {
                MaterialId::GRASS
            } else {
                MaterialId::STONE
            };
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), mat);
        }

        carve_waterway_column(&mut octree, vx, vz, 0, 20, 14, 10, Some(3), 0.0, 6.0, 5.0);

        for vy in 0..7 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::STONE
            );
        }
        for vy in 7..=10 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::WATER
            );
        }
        for vy in 11..15 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::AIR
            );
        }
        assert_eq!(surface_voxel_y(&octree, vx, vz, 0, 20), Some(10));
    }

    #[test]
    fn carve_waterway_column_tapers_bank_outside_wetted_channel() {
        let mut octree = Octree::new();
        let vx = 20;
        let vz = 24;
        for vy in 0..21 {
            let mat = if vy == 20 {
                MaterialId::GRASS
            } else {
                MaterialId::STONE
            };
            octree.set_voxel(VoxelCoord::new(vx, vy, vz), mat);
        }

        carve_waterway_column(&mut octree, vx, vz, 0, 32, 20, 10, None, 10.0, 6.0, 4.0);

        let new_surface_y = surface_voxel_y(&octree, vx, vz, 0, 32).expect("bank surface");
        assert!(new_surface_y > 10);
        assert!(new_surface_y < 20);
        for vy in (new_surface_y + 1)..21 {
            assert_eq!(
                octree.get_voxel(VoxelCoord::new(vx, vy, vz)),
                MaterialId::AIR
            );
        }
    }

    #[test]
    fn channel_wetted_depth_only_applies_inside_channel_core() {
        let bank_profile = WaterwayCarveProfile {
            water_surface_m: 100.0,
            half_width_m: 8.0,
            center_depth_m: 3.0,
            carve_depth_m: 0.0,
            dist_to_center_m: 11.0,
            polygon_backed: false,
        };
        assert_eq!(channel_wetted_depth_vox(&bank_profile), None);

        let wetted_profile = WaterwayCarveProfile {
            water_surface_m: 100.0,
            half_width_m: 8.0,
            center_depth_m: 3.0,
            carve_depth_m: 2.4,
            dist_to_center_m: 3.0,
            polygon_backed: false,
        };
        assert_eq!(channel_wetted_depth_vox(&wetted_profile), Some(2));

        let shallow_edge_profile = WaterwayCarveProfile {
            water_surface_m: 100.0,
            half_width_m: 8.0,
            center_depth_m: 3.0,
            carve_depth_m: 0.2,
            dist_to_center_m: 7.9,
            polygon_backed: false,
        };
        assert_eq!(channel_wetted_depth_vox(&shallow_edge_profile), Some(1));
    }
}
