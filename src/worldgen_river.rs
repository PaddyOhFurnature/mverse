//! Tier 4a — River profile computation.
//!
//! Collects all waterway lines from the OSM cache for a region, determines
//! flow direction from SRTM, and computes per-segment profile metrics:
//!   * Water surface elevation (SRTM at each node, adjusted by substrate)
//!   * Channel half-width (OSM type + flow-accumulation proxy via TWI)
//!   * Channel depth (type + local gradient)
//!   * Tidal zone flag (near coast + low gradient + low elevation)
//!
//! The resulting `RiverProfileCache` is queried per-column in
//! `OsmProcessor::apply_waterway_channels` to replace the flat-uniform
//! placeholder with physically grounded carving.

use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;
use crate::osm::{OsmDiskCache, WaterwayLine};
use crate::terrain_analysis::TerrainAnalysis;

const TILE_SIZE: f64 = 0.01; // OSM cache tile size in degrees

// ── Public profile types ──────────────────────────────────────────────────────

/// Per-node profile metrics along a waterway segment.
#[derive(Debug, Clone)]
pub struct SegmentProfile {
    /// Nodes in upstream → downstream order.
    pub nodes: Vec<GPS>,
    /// SRTM water-surface elevation at each node (metres, orthometric height).
    pub water_surface_m: Vec<f32>,
    /// Channel half-width at each node (metres).
    pub half_width_m: Vec<f32>,
    /// Channel depth at each node (metres below water surface).
    pub depth_m: Vec<f32>,
    /// True when the whole segment is within the tidal zone.
    pub is_tidal: bool,
    /// OSM `waterway` tag value.
    pub waterway_type: String,
}

impl SegmentProfile {
    /// Interpolate (water_surface_m, half_width_m, depth_m) at parameter `t ∈ [0, 1]`
    /// where 0 = upstream end, 1 = downstream end.
    pub fn at_t(&self, t: f64) -> (f32, f32, f32) {
        if self.nodes.is_empty() {
            return (0.0, 6.0, 2.0);
        }
        let n = self.nodes.len() - 1;
        let fi = (t * n as f64).clamp(0.0, n as f64);
        let lo = fi.floor() as usize;
        let hi = (lo + 1).min(n);
        let frac = (fi - lo as f64) as f32;

        let lerp = |a: f32, b: f32| a + (b - a) * frac;
        (
            lerp(self.water_surface_m[lo], self.water_surface_m[hi]),
            lerp(self.half_width_m[lo], self.half_width_m[hi]),
            lerp(self.depth_m[lo], self.depth_m[hi]),
        )
    }
}

/// Region-wide cache of river profiles, built once before the parallel chunk loop.
pub struct RiverProfileCache {
    pub segments: Vec<SegmentProfile>,
}

impl RiverProfileCache {
    /// Build profiles for all waterway lines in the given region.
    ///
    /// Scans every 0.01° OSM tile that overlaps `[lat_min..lat_max] × [lon_min..lon_max]`,
    /// then computes a `SegmentProfile` for each unique waterway line found.
    pub fn build(
        lat_min: f64,
        lat_max: f64,
        lon_min: f64,
        lon_max: f64,
        osm_cache: &OsmDiskCache,
        elevation: &std::sync::RwLock<ElevationPipeline>,
        analysis: Option<&TerrainAnalysis>,
    ) -> Self {
        let lines = collect_waterway_lines(lat_min, lat_max, lon_min, lon_max, osm_cache);
        eprintln!(
            "[river] collected {} waterway lines for region",
            lines.len()
        );

        let elev_guard = elevation.read().expect("elevation lock");
        let segments = lines
            .into_iter()
            .filter_map(|wl| build_segment_profile(wl, &elev_guard, analysis))
            .collect();

        Self { segments }
    }

    /// Find the closest matching profile segment and return `(t, &SegmentProfile)` for a
    /// given GPS location.  `t` is the fractional position along the segment.
    ///
    /// Returns `None` when the location is more than `max_search_m` metres from any segment.
    pub fn nearest(&self, lat: f64, lon: f64, max_search_m: f64) -> Option<(f64, &SegmentProfile)> {
        let mut best_dist = max_search_m;
        let mut best_t = 0.0f64;
        let mut best_seg: Option<&SegmentProfile> = None;

        for seg in &self.segments {
            let seg_count = (seg.nodes.len().saturating_sub(1)).max(1) as f64;
            for (seg_idx, pair) in seg.nodes.windows(2).enumerate() {
                let (dist, t, _, _) = point_to_segment_dist(
                    lat,
                    lon,
                    pair[0].lat,
                    pair[0].lon,
                    pair[1].lat,
                    pair[1].lon,
                );
                if dist < best_dist {
                    best_dist = dist;
                    best_t = (seg_idx as f64 + t) / seg_count;
                    best_seg = Some(seg);
                }
            }
        }

        best_seg.map(|seg| (best_t, seg))
    }
}

// ── Collection phase ──────────────────────────────────────────────────────────

/// Scan all OSM tiles covering `[lat_min..lat_max] × [lon_min..lon_max]` and
/// return deduplicated waterway lines.
fn collect_waterway_lines(
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
    cache: &OsmDiskCache,
) -> Vec<WaterwayLine> {
    let s_tiles = (lat_min / TILE_SIZE).floor() as i64;
    let n_tiles = (lat_max / TILE_SIZE).ceil() as i64;
    let w_tiles = (lon_min / TILE_SIZE).floor() as i64;
    let e_tiles = (lon_max / TILE_SIZE).ceil() as i64;

    // Dedup by comparing first node GPS (snap to 5 decimal places).
    let mut seen: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
    let mut lines: Vec<WaterwayLine> = Vec::new();

    for ts in s_tiles..n_tiles {
        for tw in w_tiles..e_tiles {
            let tile_s = ts as f64 * TILE_SIZE;
            let tile_w = tw as f64 * TILE_SIZE;
            let tile_n = tile_s + TILE_SIZE;
            let tile_e = tile_w + TILE_SIZE;

            if let Some(osm) = cache.load(tile_s, tile_w, tile_n, tile_e) {
                for wl in osm.waterway_lines {
                    // Skip lines with fewer than 2 nodes.
                    if wl.nodes.len() < 2 {
                        continue;
                    }
                    // Dedup: key on first node snapped to 5 dp.
                    let key = snap_key(wl.nodes[0].lat, wl.nodes[0].lon);
                    if seen.insert(key) {
                        lines.push(wl);
                    }
                }
            }
        }
    }
    lines
}

#[inline]
fn snap_key(lat: f64, lon: f64) -> (i64, i64) {
    (
        (lat * 100_000.0).round() as i64,
        (lon * 100_000.0).round() as i64,
    )
}

// ── Profile computation ───────────────────────────────────────────────────────

/// Compute a `SegmentProfile` for a single waterway line.
///
/// Returns `None` if the line has no valid elevation data.
fn build_segment_profile(
    mut wl: WaterwayLine,
    elevation: &ElevationPipeline,
    analysis: Option<&TerrainAnalysis>,
) -> Option<SegmentProfile> {
    let n = wl.nodes.len();
    if n < 2 {
        return None;
    }

    // ── Sample SRTM elevation at each node ───────────────────────────────────
    let mut elev_m: Vec<f32> = wl
        .nodes
        .iter()
        .map(|gps| {
            elevation
                .query_with_fill(gps)
                .map(|e| e.meters as f32)
                .unwrap_or(0.0)
        })
        .collect();

    // ── Determine flow direction: orient nodes upstream → downstream ──────────
    // River flows from high to low.  Compare average of first quarter vs last quarter.
    let q = (n / 4).max(1);
    let elev_first: f32 = elev_m[..q].iter().sum::<f32>() / q as f32;
    let elev_last: f32 = elev_m[n - q..].iter().sum::<f32>() / q as f32;

    if elev_first < elev_last {
        // First end is lower → reverse so that node[0] is the source (upstream).
        wl.nodes.reverse();
        elev_m.reverse();
    }

    // ── Smooth elevation (rivers cannot flow uphill — fix minor data artefacts) ─
    for i in 1..n {
        if elev_m[i] > elev_m[i - 1] {
            elev_m[i] = elev_m[i - 1]; // enforce monotone downhill
        }
    }

    // ── Compute cumulative downstream distance ────────────────────────────────
    let mut cum_dist_m = vec![0.0f64; n];
    for i in 1..n {
        let d = haversine_m(
            wl.nodes[i - 1].lat,
            wl.nodes[i - 1].lon,
            wl.nodes[i].lat,
            wl.nodes[i].lon,
        );
        cum_dist_m[i] = cum_dist_m[i - 1] + d;
    }
    let total_len = cum_dist_m[n - 1].max(1.0);

    // ── Tidal zone detection ──────────────────────────────────────────────────
    // Tidal if: mouth elevation ≤ 5 m AND coastal dist < 15 km AND overall
    // gradient < 0.01% (flat).
    let mouth_elev = elev_m[n - 1];
    let overall_drop = (elev_m[0] - mouth_elev).max(0.0) as f64;
    let gradient_pct = overall_drop / total_len * 100.0;
    let mouth_coastal = analysis
        .map(|a| a.coastal_dist_at(wl.nodes[n - 1].lat, wl.nodes[n - 1].lon))
        .unwrap_or(100_000.0);

    let is_tidal = mouth_elev < 5.0
        && gradient_pct < 0.01
        && mouth_coastal < 15_000.0
        && wl.waterway_type == "river";

    // ── Per-node width and depth ──────────────────────────────────────────────
    // Base values from OSM type; scaled by downstream distance fraction.
    let (base_half_w, base_depth_m) = type_base_metrics(&wl.waterway_type);

    let mut half_width_m = Vec::with_capacity(n);
    let mut depth_m = Vec::with_capacity(n);

    // Flow-accumulation proxy: use TWI if available, else estimate from
    // downstream fraction (water gets wider/deeper towards the mouth).
    for i in 0..n {
        let frac = (cum_dist_m[i] / total_len) as f32; // 0 = source, 1 = mouth
        let twi = analysis
            .map(|a| a.twi_at(wl.nodes[i].lat, wl.nodes[i].lon))
            .unwrap_or(6.0);

        // Width: grows downstream; TWI-boosted in valley floors.
        let twi_factor = (twi / 8.0).clamp(0.5, 3.0) as f32;
        let dist_factor = 1.0 + 2.0 * frac; // 1× at source, 3× at mouth
        let hw = base_half_w * twi_factor * dist_factor;

        // Depth: grows downstream more slowly; capped by reasonable limits.
        let slope_deg = analysis
            .map(|a| a.slope_at(wl.nodes[i].lat, wl.nodes[i].lon))
            .unwrap_or(2.0);
        // Steep gradient → shallower (cascade); flat → deeper (wide lazy river).
        let slope_factor = (1.0 - (slope_deg / 30.0).clamp(0.0, 0.9)) as f32;
        let d = base_depth_m * (1.0 + 1.5 * frac) * slope_factor;

        // Extra depth at tidal reach.
        let d = if is_tidal { d * 1.5 } else { d };

        half_width_m.push(hw.clamp(0.5, 200.0));
        depth_m.push(d.clamp(0.3, 25.0));
    }

    Some(SegmentProfile {
        nodes: wl.nodes,
        water_surface_m: elev_m,
        half_width_m,
        depth_m,
        is_tidal,
        waterway_type: wl.waterway_type,
    })
}

/// Base (half_width_m, depth_m) for each OSM waterway type at the source end.
fn type_base_metrics(wtype: &str) -> (f32, f32) {
    match wtype {
        "river" => (8.0, 2.0),
        "canal" => (6.0, 2.5),
        "stream" => (2.0, 0.8),
        "drain" => (1.5, 0.6),
        "ditch" => (0.8, 0.4),
        _ => (1.0, 0.5),
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

/// Haversine distance between two GPS points in metres.
pub fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Distance from point (lat, lon) to segment (a→b) in metres.
///
/// Returns `(distance_m, t_along_segment [0,1], closest_lat, closest_lon)`.
pub fn point_to_segment_dist(
    lat: f64,
    lon: f64,
    a_lat: f64,
    a_lon: f64,
    b_lat: f64,
    b_lon: f64,
) -> (f64, f64, f64, f64) {
    let cos_lat = lat.to_radians().cos();
    let sx = 111_320.0_f64;
    let sz = 111_320.0_f64 * cos_lat;

    let px = (lat - a_lat) * sx;
    let pz = (lon - a_lon) * sz;
    let dx = (b_lat - a_lat) * sx;
    let dz = (b_lon - a_lon) * sz;
    let len2 = dx * dx + dz * dz;

    if len2 < 1e-10 {
        return ((px * px + pz * pz).sqrt(), 0.0, a_lat, a_lon);
    }

    let t = ((px * dx + pz * dz) / len2).clamp(0.0, 1.0);
    let cx = dx * t - px;
    let cz = dz * t - pz;
    let dist = (cx * cx + cz * cz).sqrt();
    let c_lat = a_lat + (b_lat - a_lat) * t;
    let c_lon = a_lon + (b_lon - a_lon) * t;
    (dist, t, c_lat, c_lon)
}

#[cfg(test)]
mod tests {
    use super::{RiverProfileCache, SegmentProfile};
    use crate::coordinates::GPS;

    #[test]
    fn nearest_returns_global_fraction_along_segment() {
        let seg = SegmentProfile {
            nodes: vec![
                GPS::new(0.0, 0.0, 0.0),
                GPS::new(0.0, 0.001, 0.0),
                GPS::new(0.0, 0.002, 0.0),
                GPS::new(0.0, 0.003, 0.0),
            ],
            water_surface_m: vec![100.0, 90.0, 80.0, 70.0],
            half_width_m: vec![1.0, 2.0, 3.0, 4.0],
            depth_m: vec![0.5, 1.0, 1.5, 2.0],
            is_tidal: false,
            waterway_type: "river".into(),
        };
        let cache = RiverProfileCache {
            segments: vec![seg],
        };

        let (t, seg) = cache.nearest(0.0, 0.0024, 1000.0).expect("nearest");
        let (surface_m, half_width_m, depth_m) = seg.at_t(t);

        assert!(t > 0.7 && t < 0.85, "unexpected t={t}");
        assert!(
            surface_m < 80.0 && surface_m > 70.0,
            "unexpected surface={surface_m}"
        );
        assert!(
            half_width_m > 3.0 && half_width_m < 4.0,
            "unexpected width={half_width_m}"
        );
        assert!(depth_m > 1.5 && depth_m < 2.0, "unexpected depth={depth_m}");
    }
}
