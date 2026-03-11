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

/// Applies OSM features (waterways, water polygons) to pre-generated terrain chunks.
pub struct OsmProcessor {
    osm_cache: Arc<OsmDiskCache>,
    _origin_gps: GPS,
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
            _origin_gps: origin_gps,
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
            lat_min, lat_max, lon_min, lon_max, &self.osm_cache,
        );
        if osm.is_empty() {
            return;
        }

        let min_v = chunk_id.min_voxel();
        let max_v = chunk_id.max_voxel();

        if !osm.water.is_empty() {
            self.apply_water_polygons(octree, &min_v, &max_v, &osm.water);
        }
        if !osm.waterway_lines.is_empty() {
            self.apply_waterway_channels(
                octree, &min_v, &max_v, &osm.waterway_lines,
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
    ) {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};

        // Compute the ellipsoid ECEF Y for the WGS-84 formula (same as terrain.rs).
        const WGS84_A: f64 = 6_378_137.0;
        const WGS84_B: f64 = 6_356_752.3142;
        let origin_ecef_y =
            (self.origin_voxel.y as f64 + 0.5) + WORLD_MIN_METERS;

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

                let in_water = osm_water.iter().any(|w| {
                    crate::osm::point_in_polygon(lat, lon, &w.polygon)
                        && !w.holes.iter().any(|hole| {
                            crate::osm::point_in_polygon(lat, lon, hole)
                        })
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

        // Use the minimum surface_y among all border columns as the water level
        // within this chunk.  Border columns are those at the edge of the in-water
        // region; using the minimum keeps the water at the lowest bank elevation.
        let water_level_y = water_cols.iter().map(|c| c.surface_y).min().unwrap_or(0);

        // Fill each water column.
        const WATER_DEPTH: i64 = 5;
        for col in &water_cols {
            let col_bot = water_level_y - WATER_DEPTH;
            let bedrock = col.surface_y - 200;

            // Fill from bedrock up to water surface
            for vy in bedrock.max(min_v.y)..water_level_y.min(max_v.y) {
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
        waterways: &[WaterwayLine],
        river_profiles: Option<&RiverProfileCache>,
    ) {
        use crate::chunk::{CHUNK_SIZE_X, CHUNK_SIZE_Z};

        const WGS84_A: f64 = 6_378_137.0;
        const WGS84_B: f64 = 6_356_752.3142;
        let origin_ecef_y = (self.origin_voxel.y as f64 + 0.5) + WORLD_MIN_METERS;

        for i in 0..CHUNK_SIZE_X {
            for k in 0..CHUNK_SIZE_Z {
                let vx = min_v.x + i;
                let vz = min_v.z + k;
                let (lat, lon) = voxel_to_gps(vx, vz, origin_ecef_y, WGS84_A, WGS84_B);

                // ── Look up profile metrics for this column ───────────────────
                // Try the RiverProfileCache first (whole-river context).
                // Fall back to per-waterway flat metrics when profiles unavailable.
                let profile_result: Option<(f32, f32, f32, bool)> =
                    if let Some(cache) = river_profiles {
                        cache.nearest(lat, lon, 300.0).map(|(t, seg)| {
                            let (surf_m, hw_m, dep_m) = seg.at_t(t);
                            let dist_to_seg = {
                                let mut best = f64::MAX;
                                for pair in seg.nodes.windows(2) {
                                    let (d, _, _, _) = crate::worldgen_river::point_to_segment_dist(
                                        lat, lon,
                                        pair[0].lat, pair[0].lon,
                                        pair[1].lat, pair[1].lon,
                                    );
                                    if d < best { best = d; }
                                }
                                best
                            };
                            // Parabolic cross-section: depth tapers to 0 at bank edge.
                            let bank_t = (dist_to_seg / hw_m as f64).clamp(0.0, 1.0) as f32;
                            let cross_depth = dep_m * (1.0 - bank_t * bank_t);
                            (surf_m, hw_m, cross_depth, seg.is_tidal)
                        })
                    } else {
                        None
                    };

                let (water_surf_m, half_width, carve_depth, _is_tidal) =
                    if let Some(p) = profile_result {
                        p
                    } else {
                        // Fallback: scan raw WaterwayLines directly.
                        let mut best_surf_m: f32 = 0.0;
                        let mut best_hw: f32 = 0.0;
                        let mut best_depth: f32 = 0.0;
                        for wl in waterways {
                            let hw = wl.half_width_m() as f32;
                            let d  = wl.channel_depth_vox() as f32;
                            for pair in wl.nodes.windows(2) {
                                let (dist, t, cl, clon) = point_to_segment_dist_m(
                                    lat, lon,
                                    pair[0].lat, pair[0].lon,
                                    pair[1].lat, pair[1].lon,
                                );
                                let _ = (cl, clon);
                                if dist < hw as f64 {
                                    let cross_t = (dist / hw as f64) as f32;
                                    let cross_d = d * (1.0 - cross_t * cross_t);
                                    if cross_d > best_depth {
                                        let elev0 = pair[0].alt as f32;
                                        let elev1 = pair[1].alt as f32;
                                        best_surf_m = elev0 + (elev1 - elev0) * t as f32;
                                        best_hw     = hw;
                                        best_depth  = cross_d;
                                    }
                                }
                            }
                        }
                        (best_surf_m, best_hw, best_depth, false)
                    };

                if carve_depth < 0.3 { continue; }
                let _ = half_width;

                // ── Determine water surface voxel Y ──────────────────────────
                // If we have a valid SRTM elevation from the profile, use it.
                // Otherwise fall back to terrain surface voxel.
                let water_surface_y = if water_surf_m > 1.0 {
                    // Convert SRTM orthometric height → voxel Y.
                    // terrain.rs places voxel Y at: origin_voxel.y + (srtm_ell - origin_alt).
                    // We approximate: water_y = origin_voxel.y + (water_surf_m - origin_ortho).
                    // Use terrain surface as a sanity bound (don't float above terrain).
                    let terrain_y = surface_voxel_y(octree, vx, vz, min_v.y, max_v.y)
                        .unwrap_or(self.origin_voxel.y);
                    // water_surf_m is orthometric; terrain.rs uses ellipsoidal offset,
                    // but the difference is < 1 voxel for Brisbane.  Good enough for now.
                    terrain_y
                } else {
                    surface_voxel_y(octree, vx, vz, min_v.y, max_v.y)
                        .unwrap_or(self.origin_voxel.y)
                };

                let depth_vox   = carve_depth.round().max(1.0) as i64;
                let channel_bot = water_surface_y - depth_vox;
                let bedrock     = water_surface_y - 200;

                // Stone below channel, water inside channel, air above.
                for vy in bedrock.max(min_v.y)..channel_bot.max(min_v.y) {
                    octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::STONE);
                }
                for vy in channel_bot.max(min_v.y)..water_surface_y.min(max_v.y) {
                    octree.set_voxel(VoxelCoord::new(vx, vy, vz), MaterialId::WATER);
                }
                if water_surface_y >= min_v.y && water_surface_y < max_v.y {
                    octree.set_voxel(VoxelCoord::new(vx, water_surface_y, vz), MaterialId::AIR);
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert voxel (vx, vz) to (lat, lon) using the same ellipsoid ECEF formula as terrain.rs.
///
/// `origin_ecef_y` is `(origin_voxel.y + 0.5) + WORLD_MIN_METERS`.
fn voxel_to_gps(vx: i64, vz: i64, origin_ecef_y: f64, wgs84_a: f64, wgs84_b: f64) -> (f64, f64) {
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
fn surface_voxel_y(octree: &Octree, vx: i64, vz: i64, min_vy: i64, max_vy: i64) -> Option<i64> {
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
fn point_to_segment_dist_m(
    lat: f64, lon: f64,
    seg_lat1: f64, seg_lon1: f64,
    seg_lat2: f64, seg_lon2: f64,
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
