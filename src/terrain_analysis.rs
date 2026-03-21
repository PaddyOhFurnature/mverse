//! Tier 2 SRTM terrain analysis — derived rasters from elevation data.
//!
//! Computes flow direction (D8), flow accumulation, TWI, aspect, TRI, slope,
//! and coastal distance from a 2D DEM grid sampled from the elevation pipeline.
//! Works globally at SRTM resolution (~30 m); results are later interpolated
//! to voxel scale.

use crate::biome::OsmLanduse;
use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;
use crate::osm::{OsmAeroway, OsmLandArea, OsmWater};
use std::io::{self, BufReader, Read};

// ── RegionDem ────────────────────────────────────────────────────────────────

/// A 2D elevation grid for a geographic region, stored in row-major order.
///
/// Row 0 corresponds to `lat_min` (southernmost), and column 0 to `lon_min`
/// (westernmost).  Row index increases northward; column index increases eastward.
pub struct RegionDem {
    /// Elevation values in row-major order: `elevations[row * cols + col]`
    pub elevations: Vec<f32>,
    pub rows: usize,
    pub cols: usize,
    /// Geographic bounds (degrees)
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
    /// Cell size in degrees (same for lat and lon)
    pub cell_size_deg: f64,
}

impl RegionDem {
    pub fn get(&self, row: usize, col: usize) -> f32 {
        self.elevations[row * self.cols + col]
    }

    /// Bilinear interpolation at an arbitrary geographic coordinate.
    ///
    /// Clamps to grid edges when the coordinate falls outside the bounds.
    pub fn at_latlon(&self, lat: f64, lon: f64) -> f32 {
        let col_f = (lon - self.lon_min) / self.cell_size_deg;
        let row_f = (lat - self.lat_min) / self.cell_size_deg;

        let c0 = col_f.floor() as isize;
        let r0 = row_f.floor() as isize;
        let c1 = c0 + 1;
        let r1 = r0 + 1;

        let tc = col_f - c0 as f64;
        let tr = row_f - r0 as f64;

        let clamp_r = |r: isize| r.clamp(0, self.rows as isize - 1) as usize;
        let clamp_c = |c: isize| c.clamp(0, self.cols as isize - 1) as usize;

        let e00 = self.get(clamp_r(r0), clamp_c(c0)) as f64;
        let e01 = self.get(clamp_r(r0), clamp_c(c1)) as f64;
        let e10 = self.get(clamp_r(r1), clamp_c(c0)) as f64;
        let e11 = self.get(clamp_r(r1), clamp_c(c1)) as f64;

        (e00 * (1.0 - tc) * (1.0 - tr)
            + e01 * tc * (1.0 - tr)
            + e10 * (1.0 - tc) * tr
            + e11 * tc * tr) as f32
    }

    /// Sample a DEM from an `ElevationPipeline` over the given bounds.
    ///
    /// `step_deg` controls both the spatial resolution and the number of samples.
    /// Caller is responsible for locking the pipeline (pass a read-guard deref).
    pub fn sample_region(
        pipeline: &ElevationPipeline,
        lat_min: f64,
        lat_max: f64,
        lon_min: f64,
        lon_max: f64,
        step_deg: f64,
    ) -> Self {
        let rows = ((lat_max - lat_min) / step_deg).ceil() as usize + 1;
        let cols = ((lon_max - lon_min) / step_deg).ceil() as usize + 1;

        eprintln!(
            "[terrain_analysis] Sampling DEM {}×{} = {} cells at {:.4}°/cell …",
            rows,
            cols,
            rows * cols,
            step_deg
        );

        let mut elevations = Vec::with_capacity(rows * cols);

        for r in 0..rows {
            let lat = lat_min + r as f64 * step_deg;
            for c in 0..cols {
                let lon = lon_min + c as f64 * step_deg;
                let gps = GPS::new(lat, lon, 0.0);
                let h = pipeline.query(&gps).map(|e| e.meters as f32).unwrap_or(0.0);
                elevations.push(h);
            }
        }

        Self {
            elevations,
            rows,
            cols,
            lat_min,
            lat_max,
            lon_min,
            lon_max,
            cell_size_deg: step_deg,
        }
    }
}

// ── TerrainAnalysis ──────────────────────────────────────────────────────────

/// Per-cell derived terrain values computed from a `RegionDem`.
pub struct TerrainAnalysis {
    pub dem: RegionDem,
    /// D8 flow direction: 0-7 (N, NE, E, SE, S, SW, W, NW clockwise), 8 = sink/flat
    pub flow_dir: Vec<u8>,
    /// Flow accumulation: number of upstream cells that drain through each cell
    pub flow_accum: Vec<f32>,
    /// Topographic Wetness Index: ln(accum / tan(slope))
    pub twi: Vec<f32>,
    /// Aspect: compass direction of steepest descent, 0-360° (0=N, 90=E)
    pub aspect: Vec<f32>,
    /// Terrain Ruggedness Index: mean absolute elevation difference from 8 neighbours
    pub tri: Vec<f32>,
    /// Slope: gradient magnitude in degrees
    pub slope_deg: Vec<f32>,
    /// Distance to nearest coastline in metres.
    /// Empty when GSHHG data is not provided; `coastal_dist_at` returns 100 000 m.
    pub coastal_dist: Vec<f32>,
    /// True when the DEM cell center falls outside all nearby level-1 GSHHG
    /// land polygons, i.e. the cell is open ocean rather than land.
    pub ocean_mask: Vec<bool>,
    /// Reservoir water surface elevation in metres per cell.
    /// 0.0 means "not a reservoir".  Populated by `compute_reservoirs`.
    pub reservoir_mask: Vec<f32>,
    /// Encoded OSM landuse override per cell. 0 = none.
    pub osm_landuse_mask: Vec<u8>,
    /// Target orthometric elevation for engineered flat ground such as recreation
    /// grounds and pitches.  Valid only where `engineered_ground_strength > 0`.
    pub engineered_ground_level: Vec<f32>,
    /// Blend strength for engineered ground flattening, 0.0-1.0.
    pub engineered_ground_strength: Vec<f32>,
}

/// Moisture classification derived from TWI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MoistureClass {
    Dry,
    Moderate,
    Wet,
    Waterlogged,
}

// D8 neighbour offsets (row, col) for directions 0=N … 7=NW.
// Row increases northward, col increases eastward.
const DR: [i32; 8] = [1, 1, 0, -1, -1, -1, 0, 1];
const DC: [i32; 8] = [0, 1, 1, 1, 0, -1, -1, -1];
// Distance factors: 1.0 for cardinal, √2 for diagonal
const D8_DIST: [f64; 8] = [
    1.0,
    std::f64::consts::SQRT_2,
    1.0,
    std::f64::consts::SQRT_2,
    1.0,
    std::f64::consts::SQRT_2,
    1.0,
    std::f64::consts::SQRT_2,
];

#[derive(Debug, Clone, Copy)]
struct EngineeredGroundProfile {
    priority: u8,
    boundary_strength: f32,
    interior_strength: f32,
}

fn encode_osm_landuse(landuse: OsmLanduse) -> u8 {
    match landuse {
        OsmLanduse::Residential => 1,
        OsmLanduse::Commercial => 2,
        OsmLanduse::Industrial => 3,
        OsmLanduse::Retail => 4,
        OsmLanduse::Forest => 5,
        OsmLanduse::Farmland => 6,
        OsmLanduse::Meadow => 7,
        OsmLanduse::Water => 8,
    }
}

fn decode_osm_landuse(code: u8) -> Option<OsmLanduse> {
    match code {
        1 => Some(OsmLanduse::Residential),
        2 => Some(OsmLanduse::Commercial),
        3 => Some(OsmLanduse::Industrial),
        4 => Some(OsmLanduse::Retail),
        5 => Some(OsmLanduse::Forest),
        6 => Some(OsmLanduse::Farmland),
        7 => Some(OsmLanduse::Meadow),
        8 => Some(OsmLanduse::Water),
        _ => None,
    }
}

fn osm_landuse_priority(landuse: OsmLanduse) -> u8 {
    match landuse {
        OsmLanduse::Water => 5,
        OsmLanduse::Residential
        | OsmLanduse::Commercial
        | OsmLanduse::Industrial
        | OsmLanduse::Retail => 4,
        OsmLanduse::Farmland | OsmLanduse::Meadow => 3,
        OsmLanduse::Forest => 2,
    }
}

fn landuse_override_for_area(area: &OsmLandArea) -> Option<OsmLanduse> {
    match area.area_type.as_str() {
        "residential" => Some(OsmLanduse::Residential),
        "commercial" => Some(OsmLanduse::Commercial),
        "industrial" => Some(OsmLanduse::Industrial),
        "retail" => Some(OsmLanduse::Retail),
        "forest" | "wood" => Some(OsmLanduse::Forest),
        "farmland" | "orchard" | "vineyard" => Some(OsmLanduse::Farmland),
        "meadow" | "grass" | "grassland" => Some(OsmLanduse::Meadow),
        _ => None,
    }
}

fn engineered_ground_profile(area_type: &str) -> Option<EngineeredGroundProfile> {
    match area_type {
        "aerodrome" => Some(EngineeredGroundProfile {
            priority: 1,
            boundary_strength: 0.9,
            interior_strength: 0.95,
        }),
        "recreation_ground" => Some(EngineeredGroundProfile {
            priority: 1,
            boundary_strength: 0.85,
            interior_strength: 1.0,
        }),
        "pitch" | "stadium" | "sports_centre" | "playground" => Some(EngineeredGroundProfile {
            priority: 2,
            boundary_strength: 1.0,
            interior_strength: 1.0,
        }),
        "taxiway" | "apron" | "helipad" => Some(EngineeredGroundProfile {
            priority: 3,
            boundary_strength: 0.95,
            interior_strength: 1.0,
        }),
        "runway" => Some(EngineeredGroundProfile {
            priority: 4,
            boundary_strength: 1.0,
            interior_strength: 1.0,
        }),
        _ => None,
    }
}

fn engineered_ground_support_profile(area_type: &str) -> Option<EngineeredGroundProfile> {
    match area_type {
        // Broader park/grass polygons often wrap explicit sports grounds in OSM.
        // We only activate these as weaker support surfaces when they spatially
        // intersect an explicit engineered-ground polygon.
        "park" | "grass" => Some(EngineeredGroundProfile {
            priority: 0,
            boundary_strength: 0.75,
            interior_strength: 0.9,
        }),
        _ => None,
    }
}

fn engineered_ground_support_seed(area_type: &str) -> bool {
    matches!(
        area_type,
        "recreation_ground"
            | "pitch"
            | "stadium"
            | "sports_centre"
            | "runway"
            | "taxiway"
            | "apron"
            | "helipad"
    )
}

fn engineered_ground_line_half_width_m(area_type: &str) -> Option<f64> {
    match area_type {
        "runway" => Some(35.0),
        "taxiway" => Some(18.0),
        "apron" => Some(25.0),
        "helipad" => Some(12.0),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct EngineeredGroundHaloProfile {
    ring_strengths: [f32; 2],
}

fn engineered_ground_halo_profile(area_type: &str) -> Option<EngineeredGroundHaloProfile> {
    match area_type {
        // Recreation grounds often have short unmapped shoulders or gaps around
        // the formal polygon boundary that still belong to the levelled complex.
        "recreation_ground" => Some(EngineeredGroundHaloProfile {
            ring_strengths: [0.85, 0.65],
        }),
        // Supporting park/grass polygons can carry that flattened edge a little
        // further, but more weakly than the explicit recreation-ground owner.
        "park" | "grass" => Some(EngineeredGroundHaloProfile {
            ring_strengths: [0.75, 0.5],
        }),
        _ => None,
    }
}

const ENGINEERED_GROUND_HALO_MAX_DELTA_M: f32 = 8.0;

fn polygon_bounds(polygon: &[GPS]) -> Option<(f64, f64, f64, f64)> {
    if polygon.len() < 3 {
        return None;
    }
    let lat_min = polygon.iter().map(|p| p.lat).fold(f64::INFINITY, f64::min);
    let lat_max = polygon
        .iter()
        .map(|p| p.lat)
        .fold(f64::NEG_INFINITY, f64::max);
    let lon_min = polygon.iter().map(|p| p.lon).fold(f64::INFINITY, f64::min);
    let lon_max = polygon
        .iter()
        .map(|p| p.lon)
        .fold(f64::NEG_INFINITY, f64::max);
    Some((lat_min, lat_max, lon_min, lon_max))
}

fn bounds_to_polygon(bounds: (f64, f64, f64, f64)) -> Vec<GPS> {
    let (lat_min, lat_max, lon_min, lon_max) = bounds;
    vec![
        GPS::new(lat_min, lon_min, 0.0),
        GPS::new(lat_min, lon_max, 0.0),
        GPS::new(lat_max, lon_max, 0.0),
        GPS::new(lat_max, lon_min, 0.0),
        GPS::new(lat_min, lon_min, 0.0),
    ]
}

fn polyline_bounds(nodes: &[GPS], pad_m: f64) -> Option<(f64, f64, f64, f64)> {
    if nodes.len() < 2 {
        return None;
    }
    let lat_min = nodes.iter().map(|p| p.lat).fold(f64::INFINITY, f64::min);
    let lat_max = nodes
        .iter()
        .map(|p| p.lat)
        .fold(f64::NEG_INFINITY, f64::max);
    let lon_min = nodes.iter().map(|p| p.lon).fold(f64::INFINITY, f64::min);
    let lon_max = nodes
        .iter()
        .map(|p| p.lon)
        .fold(f64::NEG_INFINITY, f64::max);
    let mid_lat = 0.5 * (lat_min + lat_max);
    let lat_pad = pad_m / 111_320.0;
    let lon_pad = pad_m / (111_320.0 * mid_lat.to_radians().cos().abs().max(0.1));
    Some((
        lat_min - lat_pad,
        lat_max + lat_pad,
        lon_min - lon_pad,
        lon_max + lon_pad,
    ))
}

fn point_to_polyline_segment_distance_m(lat: f64, lon: f64, a: &GPS, b: &GPS) -> f64 {
    let scale_y = 111_320.0;
    let scale_x = 111_320.0 * lat.to_radians().cos().abs().max(0.1);

    let ax = (a.lon - lon) * scale_x;
    let ay = (a.lat - lat) * scale_y;
    let bx = (b.lon - lon) * scale_x;
    let by = (b.lat - lat) * scale_y;
    let abx = bx - ax;
    let aby = by - ay;
    let denom = abx * abx + aby * aby;
    if denom <= 1.0e-12 {
        return (ax * ax + ay * ay).sqrt();
    }
    let t = (-(ax * abx + ay * aby) / denom).clamp(0.0, 1.0);
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    (cx * cx + cy * cy).sqrt()
}

fn point_to_polyline_distance_m(lat: f64, lon: f64, nodes: &[GPS]) -> Option<f64> {
    if nodes.len() < 2 {
        return None;
    }
    let mut best = f64::INFINITY;
    for segment in nodes.windows(2) {
        best = best.min(point_to_polyline_segment_distance_m(
            lat,
            lon,
            &segment[0],
            &segment[1],
        ));
    }
    Some(best)
}

fn rasterize_polyline_buffer_cells(
    dem: &RegionDem,
    nodes: &[GPS],
    half_width_m: f64,
) -> Vec<(usize, usize)> {
    let Some(bounds) = polyline_bounds(nodes, half_width_m) else {
        return Vec::new();
    };
    let (lat_min, lat_max, lon_min, lon_max) = bounds;
    if lat_max < dem.lat_min
        || lat_min > dem.lat_max
        || lon_max < dem.lon_min
        || lon_min > dem.lon_max
    {
        return Vec::new();
    }

    let r0 = ((lat_min - dem.lat_min) / dem.cell_size_deg)
        .floor()
        .max(0.0) as usize;
    let r1 = ((lat_max - dem.lat_min) / dem.cell_size_deg)
        .ceil()
        .min(dem.rows as f64 - 1.0) as usize;
    let c0 = ((lon_min - dem.lon_min) / dem.cell_size_deg)
        .floor()
        .max(0.0) as usize;
    let c1 = ((lon_max - dem.lon_min) / dem.cell_size_deg)
        .ceil()
        .min(dem.cols as f64 - 1.0) as usize;

    let cell_size_lat_m = 111_320.0 * dem.cell_size_deg;
    let cell_size_lon_m =
        111_320.0 * dem.cell_size_deg * (0.5 * (lat_min + lat_max)).to_radians().cos().abs().max(0.1);
    let cell_radius_m = 0.5 * (cell_size_lat_m * cell_size_lat_m + cell_size_lon_m * cell_size_lon_m).sqrt();

    let mut inside_cells = Vec::new();
    for r in r0..=r1 {
        let center_lat = dem.lat_min + (r as f64 + 0.5) * dem.cell_size_deg;
        for c in c0..=c1 {
            let center_lon = dem.lon_min + (c as f64 + 0.5) * dem.cell_size_deg;
            let Some(dist_m) = point_to_polyline_distance_m(center_lat, center_lon, nodes) else {
                continue;
            };
            if dist_m <= half_width_m + cell_radius_m {
                inside_cells.push((r, c));
            }
        }
    }
    inside_cells
}

fn bounds_overlap(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    !(a.1 < b.0 || a.0 > b.1 || a.3 < b.2 || a.2 > b.3)
}

fn rasterize_polygon_cells(
    dem: &RegionDem,
    polygon: &[GPS],
    bounds: (f64, f64, f64, f64),
) -> Vec<(usize, usize)> {
    let (lat_min, lat_max, lon_min, lon_max) = bounds;
    if lat_max < dem.lat_min
        || lat_min > dem.lat_max
        || lon_max < dem.lon_min
        || lon_min > dem.lon_max
    {
        return Vec::new();
    }

    let r0 = ((lat_min - dem.lat_min) / dem.cell_size_deg)
        .floor()
        .max(0.0) as usize;
    let r1 = ((lat_max - dem.lat_min) / dem.cell_size_deg)
        .ceil()
        .min(dem.rows as f64 - 1.0) as usize;
    let c0 = ((lon_min - dem.lon_min) / dem.cell_size_deg)
        .floor()
        .max(0.0) as usize;
    let c1 = ((lon_max - dem.lon_min) / dem.cell_size_deg)
        .ceil()
        .min(dem.cols as f64 - 1.0) as usize;

    let mut inside_cells = Vec::new();
    for r in r0..=r1 {
        let cell_lat_min = dem.lat_min + r as f64 * dem.cell_size_deg;
        let cell_lat_max = (cell_lat_min + dem.cell_size_deg).min(dem.lat_max);
        for c in c0..=c1 {
            let cell_lon_min = dem.lon_min + c as f64 * dem.cell_size_deg;
            let cell_lon_max = (cell_lon_min + dem.cell_size_deg).min(dem.lon_max);
            if polygon_intersects_cell(
                polygon,
                cell_lat_min,
                cell_lat_max,
                cell_lon_min,
                cell_lon_max,
            ) {
                inside_cells.push((r, c));
            }
        }
    }
    inside_cells
}

fn rasterize_polygon_center_cells(
    dem: &RegionDem,
    polygon: &[GPS],
    bounds: (f64, f64, f64, f64),
) -> Vec<(usize, usize)> {
    let (lat_min, lat_max, lon_min, lon_max) = bounds;
    if lat_max < dem.lat_min
        || lat_min > dem.lat_max
        || lon_max < dem.lon_min
        || lon_min > dem.lon_max
    {
        return Vec::new();
    }

    let r0 = ((lat_min - dem.lat_min) / dem.cell_size_deg)
        .floor()
        .max(0.0) as usize;
    let r1 = ((lat_max - dem.lat_min) / dem.cell_size_deg)
        .ceil()
        .min(dem.rows as f64 - 1.0) as usize;
    let c0 = ((lon_min - dem.lon_min) / dem.cell_size_deg)
        .floor()
        .max(0.0) as usize;
    let c1 = ((lon_max - dem.lon_min) / dem.cell_size_deg)
        .ceil()
        .min(dem.cols as f64 - 1.0) as usize;

    let mut inside_cells = Vec::new();
    for r in r0..=r1 {
        let center_lat = dem.lat_min + (r as f64 + 0.5) * dem.cell_size_deg;
        for c in c0..=c1 {
            let center_lon = dem.lon_min + (c as f64 + 0.5) * dem.cell_size_deg;
            if point_in_polygon(center_lat, center_lon, polygon) {
                inside_cells.push((r, c));
            }
        }
    }
    inside_cells
}

fn engineered_ground_target_for_cells(dem: &RegionDem, cells: &[(usize, usize)]) -> Option<f32> {
    if cells.is_empty() {
        return None;
    }
    let mut elevations: Vec<f32> = cells.iter().map(|&(r, c)| dem.get(r, c)).collect();
    elevations.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
    // Using the raw polygon median on a sloped sports/park complex tends to drag the
    // whole surface uphill into artificial terraces. Bias the target toward the lower
    // stable bench by taking the median of the lower half of sampled elevations.
    let lower_half_len = ((elevations.len() + 1) / 2).max(1);
    Some(elevations[lower_half_len / 2])
}

fn outward_ring_cells(
    rows: usize,
    cols: usize,
    inside_cells: &[(usize, usize)],
    max_distance: usize,
) -> Vec<(usize, usize, usize)> {
    use std::collections::VecDeque;

    if inside_cells.is_empty() || max_distance == 0 {
        return Vec::new();
    }

    let mut distance = vec![usize::MAX; rows * cols];
    let mut queue = VecDeque::new();
    for &(r, c) in inside_cells {
        let idx = r * cols + c;
        if distance[idx] == 0 {
            continue;
        }
        distance[idx] = 0;
        queue.push_back((r, c));
    }

    while let Some((r, c)) = queue.pop_front() {
        let idx = r * cols + c;
        let current_distance = distance[idx];
        if current_distance >= max_distance {
            continue;
        }

        for dr in -1isize..=1 {
            for dc in -1isize..=1 {
                if dr == 0 && dc == 0 {
                    continue;
                }
                let rr = r as isize + dr;
                let cc = c as isize + dc;
                if rr < 0 || cc < 0 || rr >= rows as isize || cc >= cols as isize {
                    continue;
                }
                let rr = rr as usize;
                let cc = cc as usize;
                let nidx = rr * cols + cc;
                let next_distance = current_distance + 1;
                if next_distance >= distance[nidx] {
                    continue;
                }
                distance[nidx] = next_distance;
                queue.push_back((rr, cc));
            }
        }
    }

    let mut ring_cells = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let dist = distance[r * cols + c];
            if dist > 0 && dist <= max_distance {
                ring_cells.push((r, c, dist));
            }
        }
    }
    ring_cells
}

impl TerrainAnalysis {
    /// Compute all derived rasters from `dem` in a single pass.
    pub fn compute(dem: RegionDem) -> Self {
        let n = dem.rows * dem.cols;
        // Approximate metres per degree at mid-latitude (good enough for gradient calcs).
        let cell_size_m = 111_320.0 * dem.cell_size_deg;

        // ── Slope and aspect (Horn's method) ─────────────────────────────────
        let mut slope_deg = vec![0.0f32; n];
        let mut aspect = vec![0.0f32; n];
        let mut dz_dx_grid = vec![0.0f64; n];
        let mut dz_dy_grid = vec![0.0f64; n];

        for r in 0..dem.rows {
            for c in 0..dem.cols {
                let idx = r * dem.cols + c;

                let rm1 = r.saturating_sub(1);
                let rp1 = (r + 1).min(dem.rows - 1);
                let cm1 = c.saturating_sub(1);
                let cp1 = (c + 1).min(dem.cols - 1);

                let e = |rr: usize, cc: usize| dem.get(rr, cc) as f64;

                // East–West gradient (positive = East is higher)
                let dz_dx = (e(rm1, cp1) + 2.0 * e(r, cp1) + e(rp1, cp1)
                    - e(rm1, cm1)
                    - 2.0 * e(r, cm1)
                    - e(rp1, cm1))
                    / (8.0 * cell_size_m);

                // North–South gradient (positive = North is higher; r+1 = North in our grid)
                let dz_dy = (e(rp1, cm1) + 2.0 * e(rp1, c) + e(rp1, cp1)
                    - e(rm1, cm1)
                    - 2.0 * e(rm1, c)
                    - e(rm1, cp1))
                    / (8.0 * cell_size_m);

                let grad = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt();
                slope_deg[idx] = grad.atan2(1.0).to_degrees() as f32;

                // atan2(dz_dx, dz_dy): 0° = N, 90° = E
                let asp = dz_dx.atan2(dz_dy).to_degrees();
                aspect[idx] = ((asp + 360.0) % 360.0) as f32;

                dz_dx_grid[idx] = dz_dx;
                dz_dy_grid[idx] = dz_dy;
            }
        }

        // ── D8 flow direction ─────────────────────────────────────────────────
        let mut flow_dir = vec![8u8; n];

        for r in 0..dem.rows {
            for c in 0..dem.cols {
                let idx = r * dem.cols + c;
                let center = dem.get(r, c) as f64;
                let mut best_slope = 0.0f64;
                let mut best_dir = 8u8;

                for d in 0..8usize {
                    let nr = r as i32 + DR[d];
                    let nc = c as i32 + DC[d];
                    if nr < 0 || nr >= dem.rows as i32 || nc < 0 || nc >= dem.cols as i32 {
                        continue;
                    }
                    let nb_elev = dem.get(nr as usize, nc as usize) as f64;
                    let s = (center - nb_elev) / (D8_DIST[d] * cell_size_m);
                    if s > best_slope {
                        best_slope = s;
                        best_dir = d as u8;
                    }
                }
                flow_dir[idx] = best_dir;
            }
        }

        // ── Flow accumulation (D8 priority-flood style) ───────────────────────
        // Sort cells high-to-low, then each cell drains into its D8 neighbour.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_unstable_by(|&a, &b| {
            dem.elevations[b]
                .partial_cmp(&dem.elevations[a])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut flow_accum = vec![1.0f32; n]; // Each cell counts itself
        for &idx in &order {
            let d = flow_dir[idx];
            if d == 8 {
                continue; // local sink — nothing to propagate
            }
            let r = idx / dem.cols;
            let c = idx % dem.cols;
            let nr = r as i32 + DR[d as usize];
            let nc = c as i32 + DC[d as usize];
            if nr >= 0 && nr < dem.rows as i32 && nc >= 0 && nc < dem.cols as i32 {
                let nb = nr as usize * dem.cols + nc as usize;
                flow_accum[nb] += flow_accum[idx];
            }
        }

        // ── TWI ───────────────────────────────────────────────────────────────
        let mut twi = vec![0.0f32; n];
        for i in 0..n {
            let slope_rad = (slope_deg[i] as f64).to_radians().max(0.001);
            let accum = (flow_accum[i] as f64).max(1.0);
            twi[i] = (accum / slope_rad.tan()).ln() as f32;
        }

        // ── TRI ───────────────────────────────────────────────────────────────
        let mut tri = vec![0.0f32; n];
        for r in 0..dem.rows {
            for c in 0..dem.cols {
                let idx = r * dem.cols + c;
                let center = dem.get(r, c);
                let mut sum = 0.0f32;
                let mut count = 0u32;
                for d in 0..8usize {
                    let nr = r as i32 + DR[d];
                    let nc = c as i32 + DC[d];
                    if nr >= 0 && nr < dem.rows as i32 && nc >= 0 && nc < dem.cols as i32 {
                        sum += (center - dem.get(nr as usize, nc as usize)).abs();
                        count += 1;
                    }
                }
                tri[idx] = if count > 0 { sum / count as f32 } else { 0.0 };
            }
        }

        Self {
            dem,
            flow_dir,
            flow_accum,
            twi,
            aspect,
            tri,
            slope_deg,
            coastal_dist: Vec::new(),
            ocean_mask: Vec::new(),
            reservoir_mask: Vec::new(),
            osm_landuse_mask: Vec::new(),
            engineered_ground_level: Vec::new(),
            engineered_ground_strength: Vec::new(),
        }
    }

    // ── Index helpers ─────────────────────────────────────────────────────────

    fn idx(&self, row: usize, col: usize) -> usize {
        row * self.dem.cols + col
    }

    fn row_col(&self, lat: f64, lon: f64) -> (usize, usize) {
        let col_f = (lon - self.dem.lon_min) / self.dem.cell_size_deg;
        let row_f = (lat - self.dem.lat_min) / self.dem.cell_size_deg;
        let row = (row_f.round() as isize).clamp(0, self.dem.rows as isize - 1) as usize;
        let col = (col_f.round() as isize).clamp(0, self.dem.cols as isize - 1) as usize;
        (row, col)
    }

    fn row_col_centered(&self, lat: f64, lon: f64) -> (usize, usize) {
        let col_f = (lon - self.dem.lon_min) / self.dem.cell_size_deg - 0.5;
        let row_f = (lat - self.dem.lat_min) / self.dem.cell_size_deg - 0.5;
        let row = (row_f.round() as isize).clamp(0, self.dem.rows as isize - 1) as usize;
        let col = (col_f.round() as isize).clamp(0, self.dem.cols as isize - 1) as usize;
        (row, col)
    }

    // ── Public query API ──────────────────────────────────────────────────────

    pub fn twi_at(&self, lat: f64, lon: f64) -> f32 {
        let (r, c) = self.row_col(lat, lon);
        self.twi[self.idx(r, c)]
    }

    pub fn tri_at(&self, lat: f64, lon: f64) -> f32 {
        let (r, c) = self.row_col(lat, lon);
        self.tri[self.idx(r, c)]
    }

    pub fn aspect_at(&self, lat: f64, lon: f64) -> f32 {
        let (r, c) = self.row_col(lat, lon);
        self.aspect[self.idx(r, c)]
    }

    pub fn slope_at(&self, lat: f64, lon: f64) -> f32 {
        let (r, c) = self.row_col(lat, lon);
        self.slope_deg[self.idx(r, c)]
    }

    pub fn flow_accum_at(&self, lat: f64, lon: f64) -> f32 {
        let (r, c) = self.row_col(lat, lon);
        self.flow_accum[self.idx(r, c)]
    }

    pub fn osm_landuse_at(&self, lat: f64, lon: f64) -> Option<OsmLanduse> {
        if self.osm_landuse_mask.is_empty() {
            return None;
        }
        let (r, c) = self.row_col_centered(lat, lon);
        decode_osm_landuse(self.osm_landuse_mask[self.idx(r, c)])
    }

    pub fn engineered_ground_control_at(&self, lat: f64, lon: f64) -> Option<(f32, f32)> {
        if self.engineered_ground_level.is_empty() || self.engineered_ground_strength.is_empty() {
            return None;
        }

        // Engineered-ground ownership is rasterized onto coarse (~30 m) DEM cells.
        // Bilinear blending weakens that ownership inside mapped sports grounds and
        // leaks flattening into adjacent unmarked cells, so query the owning cell directly.
        let (r, c) = self.row_col_centered(lat, lon);
        let idx = self.idx(r, c);
        let strength = self.engineered_ground_strength[idx].clamp(0.0, 1.0);
        if strength <= 0.05 {
            None
        } else {
            Some((self.engineered_ground_level[idx], strength))
        }
    }

    /// Returns `true` if the cell is likely a river or stream.
    ///
    /// Criterion: high upstream drainage area (flow_accum > 500 cells) and
    /// gentle slope (< 15°) to exclude steep cascades and waterfalls.
    pub fn is_likely_river(&self, lat: f64, lon: f64) -> bool {
        self.flow_accum_at(lat, lon) > 500.0 && self.slope_at(lat, lon) < 15.0
    }

    /// Returns `true` if the cell is likely a cliff or very rugged feature.
    ///
    /// Criterion: steep slope (> 45°) and high local relief (TRI > 20 m).
    pub fn is_likely_cliff(&self, lat: f64, lon: f64) -> bool {
        self.slope_at(lat, lon) > 45.0 && self.tri_at(lat, lon) > 20.0
    }

    /// Classify the moisture regime at a location based on TWI.
    ///
    /// | TWI    | Class        | Interpretation            |
    /// |--------|--------------|---------------------------|
    /// | > 12   | Waterlogged  | Valley floor / bog        |
    /// | 8–12   | Wet          | Lower slopes / riparian   |
    /// | 5–8    | Moderate     | Mid-slope                 |
    /// | < 5    | Dry          | Ridge / exposed crest     |
    pub fn moisture_class(&self, lat: f64, lon: f64) -> MoistureClass {
        let twi = self.twi_at(lat, lon);
        match twi {
            t if t > 12.0 => MoistureClass::Waterlogged,
            t if t > 8.0 => MoistureClass::Wet,
            t if t > 5.0 => MoistureClass::Moderate,
            _ => MoistureClass::Dry,
        }
    }

    // ── Coastal distance ──────────────────────────────────────────────────────

    /// Compute and store coastal distance from a GSHHG binary coastline file.
    ///
    /// Parses level-1 (land/ocean) polygon edges from `gshhg_path`, rasterises
    /// them onto the DEM grid, then runs a multi-source BFS to produce per-cell
    /// distances in metres.  Results are capped at `max_dist_m` (use 50 000 for
    /// normal substrate classification; far-inland cells all get 50 km).
    ///
    /// Safe to call multiple times; each call replaces the previous result.
    pub fn compute_coastal_dist(&mut self, gshhg_path: &std::path::Path) {
        eprintln!("[terrain_analysis] Computing coastal distance from GSHHG …");
        let on_coast = rasterise_coastline(&self.dem, gshhg_path);
        let coastline_cells: usize = on_coast.iter().filter(|&&v| v).count();
        eprintln!(
            "[terrain_analysis] Coastline cells rasterised: {} / {}",
            coastline_cells,
            self.dem.rows * self.dem.cols,
        );
        let cell_size_m = (111_320.0 * self.dem.cell_size_deg) as f32;
        self.coastal_dist = if coastline_cells > 0 {
            bfs_coastal_dist(self.dem.rows, self.dem.cols, &on_coast, cell_size_m)
        } else {
            direct_coastal_dist_fallback(&self.dem, gshhg_path, 50_000.0)
        };
        self.ocean_mask = rasterise_ocean_mask(&self.dem, gshhg_path);
        let ocean_cells = self.ocean_mask.iter().filter(|&&v| v).count();
        eprintln!(
            "[terrain_analysis] Ocean mask raster complete: {} / {} ocean cells",
            ocean_cells,
            self.dem.rows * self.dem.cols,
        );
        eprintln!("[terrain_analysis] Coastal distance raster complete.");
    }

    /// Distance to the nearest coastline in metres at `(lat, lon)`.
    ///
    /// Returns 100 000.0 (100 km) when coastal distance data is not available
    /// (i.e. `compute_coastal_dist` was never called), making all locations
    /// "far from coast" — a safe conservative default.
    pub fn coastal_dist_at(&self, lat: f64, lon: f64) -> f32 {
        if self.coastal_dist.is_empty() {
            return 100_000.0;
        }
        let (r, c) = self.row_col(lat, lon);
        self.coastal_dist[self.idx(r, c)]
    }

    /// Whether the queried cell is classified as open ocean by the GSHHG mask.
    pub fn ocean_at(&self, lat: f64, lon: f64) -> bool {
        if self.ocean_mask.is_empty() {
            return false;
        }
        let (r, c) = self.row_col_centered(lat, lon);
        self.ocean_mask[self.idx(r, c)]
    }

    /// Rasterise conservative OSM landuse overrides onto the DEM grid.
    pub fn compute_osm_landuse(&mut self, land_areas: &[OsmLandArea]) {
        let n = self.dem.rows * self.dem.cols;
        if self.osm_landuse_mask.len() != n {
            self.osm_landuse_mask = vec![0u8; n];
        }

        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for area in land_areas {
            if !seen.insert(area.osm_id) {
                continue;
            }
            let Some(landuse) = landuse_override_for_area(area) else {
                continue;
            };
            if area.polygon.len() < 3 {
                continue;
            }

            let lat_min = area
                .polygon
                .iter()
                .map(|p| p.lat)
                .fold(f64::INFINITY, f64::min);
            let lat_max = area
                .polygon
                .iter()
                .map(|p| p.lat)
                .fold(f64::NEG_INFINITY, f64::max);
            let lon_min = area
                .polygon
                .iter()
                .map(|p| p.lon)
                .fold(f64::INFINITY, f64::min);
            let lon_max = area
                .polygon
                .iter()
                .map(|p| p.lon)
                .fold(f64::NEG_INFINITY, f64::max);

            if lat_max < self.dem.lat_min
                || lat_min > self.dem.lat_max
                || lon_max < self.dem.lon_min
                || lon_min > self.dem.lon_max
            {
                continue;
            }

            let r0 = ((lat_min - self.dem.lat_min) / self.dem.cell_size_deg)
                .floor()
                .max(0.0) as usize;
            let r1 = ((lat_max - self.dem.lat_min) / self.dem.cell_size_deg)
                .ceil()
                .min(self.dem.rows as f64 - 1.0) as usize;
            let c0 = ((lon_min - self.dem.lon_min) / self.dem.cell_size_deg)
                .floor()
                .max(0.0) as usize;
            let c1 = ((lon_max - self.dem.lon_min) / self.dem.cell_size_deg)
                .ceil()
                .min(self.dem.cols as f64 - 1.0) as usize;

            for r in r0..=r1 {
                let cell_lat_min = self.dem.lat_min + r as f64 * self.dem.cell_size_deg;
                let cell_lat_max = (cell_lat_min + self.dem.cell_size_deg).min(self.dem.lat_max);
                for c in c0..=c1 {
                    let cell_lon_min = self.dem.lon_min + c as f64 * self.dem.cell_size_deg;
                    let cell_lon_max =
                        (cell_lon_min + self.dem.cell_size_deg).min(self.dem.lon_max);
                    if !polygon_intersects_cell(
                        &area.polygon,
                        cell_lat_min,
                        cell_lat_max,
                        cell_lon_min,
                        cell_lon_max,
                    ) {
                        continue;
                    }

                    let idx = self.idx(r, c);
                    let encoded = encode_osm_landuse(landuse);
                    let existing = decode_osm_landuse(self.osm_landuse_mask[idx]);
                    let replace = existing
                        .map(|current| {
                            osm_landuse_priority(landuse) >= osm_landuse_priority(current)
                        })
                        .unwrap_or(true);
                    if replace {
                        self.osm_landuse_mask[idx] = encoded;
                    }
                }
            }
        }
    }

    /// Rasterise engineered flat-ground controls for strongly man-made open ground.
    ///
    /// This is intentionally conservative: explicit recreation/sports polygons
    /// are flattened directly, while broader `park` / `grass` polygons only
    /// receive a weaker support profile when they spatially wrap or touch those
    /// explicit engineered-ground polygons.
    pub fn compute_engineered_ground(&mut self, land_areas: &[OsmLandArea]) {
        self.compute_engineered_ground_with_aeroways(land_areas, &[]);
    }

    /// Rasterise engineered flat-ground controls from land areas plus aeroway polygons.
    ///
    /// Aeroway polygons let airport surfaces (runways, aprons, taxiways, helipads)
    /// participate in the same coarse DEM flattening pass as recreation/sports grounds.
    pub fn compute_engineered_ground_with_aeroways(
        &mut self,
        land_areas: &[OsmLandArea],
        aeroways: &[OsmAeroway],
    ) {
        #[derive(Clone)]
        struct EngineeredGroundHaloSource {
            cells: Vec<(usize, usize)>,
            target: f32,
            halo_profile: EngineeredGroundHaloProfile,
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        enum EngineeredGroundAreaKind {
            Explicit,
            Support,
        }

        #[derive(Clone)]
        struct EngineeredGroundArea {
            kind: EngineeredGroundAreaKind,
            polygon: Vec<GPS>,
            bounds: (f64, f64, f64, f64),
            cells: Vec<(usize, usize)>,
            target_cells: Vec<(usize, usize)>,
            profile: EngineeredGroundProfile,
            halo_profile: Option<EngineeredGroundHaloProfile>,
            allows_support_extensions: bool,
        }

        let n = self.dem.rows * self.dem.cols;
        if self.engineered_ground_level.len() != n {
            self.engineered_ground_level = vec![0.0f32; n];
        }
        if self.engineered_ground_strength.len() != n {
            self.engineered_ground_strength = vec![0.0f32; n];
        }

        let mut priority_mask = vec![0u8; n];
        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut areas: Vec<EngineeredGroundArea> = Vec::new();
        let mut halo_sources: Vec<EngineeredGroundHaloSource> = Vec::new();

        let mut register_area = |osm_id: u64, polygon: &[GPS], area_type: &str| {
            if !seen.insert(osm_id) {
                return;
            }
            if polygon.len() < 3 {
                return;
            }
            let Some(bounds) = polygon_bounds(polygon) else {
                return;
            };
            let inside_cells = rasterize_polygon_cells(&self.dem, polygon, bounds);
            if inside_cells.is_empty() {
                return;
            }

            if let Some(profile) = engineered_ground_profile(area_type) {
                let target_cells = {
                    let cells = rasterize_polygon_center_cells(&self.dem, polygon, bounds);
                    if cells.is_empty() {
                        inside_cells.clone()
                    } else {
                        cells
                    }
                };
                areas.push(EngineeredGroundArea {
                    kind: EngineeredGroundAreaKind::Explicit,
                    polygon: polygon.to_vec(),
                    bounds,
                    cells: inside_cells,
                    target_cells,
                    profile,
                    halo_profile: engineered_ground_halo_profile(area_type),
                    allows_support_extensions: engineered_ground_support_seed(area_type),
                });
            } else if let Some(profile) = engineered_ground_support_profile(area_type) {
                areas.push(EngineeredGroundArea {
                    kind: EngineeredGroundAreaKind::Support,
                    polygon: polygon.to_vec(),
                    bounds,
                    cells: inside_cells,
                    target_cells: Vec::new(),
                    profile,
                    halo_profile: engineered_ground_halo_profile(area_type),
                    allows_support_extensions: false,
                });
            }
        };

        for area in land_areas {
            register_area(area.osm_id, &area.polygon, &area.area_type);
        }
        for aeroway in aeroways {
            if !aeroway.is_area {
                continue;
            }
            register_area(aeroway.osm_id, &aeroway.polygon, &aeroway.aeroway_type);
        }
        for aeroway in aeroways {
            if aeroway.is_area {
                continue;
            }
            if !seen.insert(aeroway.osm_id) {
                continue;
            }
            let Some(profile) = engineered_ground_profile(&aeroway.aeroway_type) else {
                continue;
            };
            let Some(half_width_m) = engineered_ground_line_half_width_m(&aeroway.aeroway_type)
            else {
                continue;
            };
            let Some(bounds) = polyline_bounds(&aeroway.nodes, half_width_m) else {
                continue;
            };
            let inside_cells =
                rasterize_polyline_buffer_cells(&self.dem, &aeroway.nodes, half_width_m);
            if inside_cells.is_empty() {
                continue;
            }
            areas.push(EngineeredGroundArea {
                kind: EngineeredGroundAreaKind::Explicit,
                polygon: bounds_to_polygon(bounds),
                bounds,
                cells: inside_cells.clone(),
                target_cells: inside_cells,
                profile,
                halo_profile: engineered_ground_halo_profile(&aeroway.aeroway_type),
                allows_support_extensions: engineered_ground_support_seed(&aeroway.aeroway_type),
            });
        }

        let mut visited = vec![false; areas.len()];
        for start_idx in 0..areas.len() {
            if visited[start_idx] {
                continue;
            }

            let mut component = Vec::new();
            let mut stack = vec![start_idx];
            visited[start_idx] = true;

            while let Some(area_idx) = stack.pop() {
                component.push(area_idx);
                for next_idx in 0..areas.len() {
                    if visited[next_idx] {
                        continue;
                    }
                    if !bounds_overlap(areas[area_idx].bounds, areas[next_idx].bounds) {
                        continue;
                    }
                    if !polygon_intersects_polygon(
                        &areas[area_idx].polygon,
                        &areas[next_idx].polygon,
                    ) {
                        continue;
                    }
                    visited[next_idx] = true;
                    stack.push(next_idx);
                }
            }

            let Some(target) = component
                .iter()
                .filter_map(|&area_idx| {
                    let area = &areas[area_idx];
                    if area.kind != EngineeredGroundAreaKind::Explicit {
                        return None;
                    }
                    let full_target = engineered_ground_target_for_cells(&self.dem, &area.cells)?;
                    let center_target =
                        engineered_ground_target_for_cells(&self.dem, &area.target_cells)
                            .unwrap_or(full_target);
                    Some(full_target.min(center_target))
                })
                .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            else {
                continue;
            };
            let component_support_enabled = component.iter().any(|&area_idx| {
                areas[area_idx].kind == EngineeredGroundAreaKind::Explicit
                    && areas[area_idx].allows_support_extensions
            });

            for &area_idx in &component {
                let area = &areas[area_idx];
                if area.kind == EngineeredGroundAreaKind::Support && !component_support_enabled {
                    continue;
                }
                let inside_set: std::collections::HashSet<(usize, usize)> =
                    area.cells.iter().copied().collect();

                for &(r, c) in &area.cells {
                    let is_boundary = (-1isize..=1).any(|dr| {
                        (-1isize..=1).any(|dc| {
                            if dr == 0 && dc == 0 {
                                return false;
                            }
                            let rr = r as isize + dr;
                            let cc = c as isize + dc;
                            if rr < 0
                                || cc < 0
                                || rr >= self.dem.rows as isize
                                || cc >= self.dem.cols as isize
                            {
                                return true;
                            }
                            !inside_set.contains(&(rr as usize, cc as usize))
                        })
                    });

                    let strength = if is_boundary {
                        area.profile.boundary_strength
                    } else {
                        area.profile.interior_strength
                    };
                    let idx = self.idx(r, c);
                    if area.profile.priority > priority_mask[idx] {
                        self.engineered_ground_level[idx] = target;
                        self.engineered_ground_strength[idx] = strength;
                        priority_mask[idx] = area.profile.priority;
                    } else if area.profile.priority == priority_mask[idx]
                        && strength >= self.engineered_ground_strength[idx]
                    {
                        self.engineered_ground_level[idx] = target;
                        self.engineered_ground_strength[idx] = strength;
                    }
                }

                let allow_halo = area.kind == EngineeredGroundAreaKind::Explicit
                    || component_support_enabled;
                if allow_halo {
                    if let Some(halo_profile) = area.halo_profile {
                    halo_sources.push(EngineeredGroundHaloSource {
                        cells: area.cells.clone(),
                        target,
                        halo_profile,
                    });
                    }
                }
            }
        }

        for source in halo_sources {
            for (r, c, dist) in outward_ring_cells(
                self.dem.rows,
                self.dem.cols,
                &source.cells,
                source.halo_profile.ring_strengths.len(),
            ) {
                let idx = self.idx(r, c);
                if self.engineered_ground_strength[idx] > 0.0 {
                    continue;
                }
                let strength = source.halo_profile.ring_strengths[dist - 1];
                if strength <= 0.0 {
                    continue;
                }
                if (self.dem.get(r, c) - source.target).abs() > ENGINEERED_GROUND_HALO_MAX_DELTA_M {
                    continue;
                }
                self.engineered_ground_level[idx] = source.target;
                self.engineered_ground_strength[idx] = strength;
            }
        }

        let count = self
            .engineered_ground_strength
            .iter()
            .filter(|&&strength| strength > 0.0)
            .count();
        if count > 0 {
            eprintln!(
                "[terrain_analysis] Engineered ground mask: {} cells marked",
                count
            );
        }
    }

    // ── Reservoir detection ───────────────────────────────────────────────────

    /// Detect reservoirs from OSM water polygons and mark their cells in the
    /// reservoir mask.
    ///
    /// For each polygon whose water type indicates a reservoir (or is a large
    /// elevated water body), samples SRTM elevations for cells inside it,
    /// derives the water surface elevation from the distribution, and marks
    /// those cells.  The worldgen should call this after `compute_coastal_dist`.
    ///
    /// `water_polygons` — all `OsmWater` features collected from OSM tiles in
    /// the region (may contain duplicates; we deduplicate by osm_id).
    pub fn compute_reservoirs(&mut self, water_polygons: &[OsmWater]) {
        let n = self.dem.rows * self.dem.cols;
        if self.reservoir_mask.len() != n {
            self.reservoir_mask = vec![0.0f32; n];
        }

        // Deduplicate by osm_id — the same polygon appears in every tile it touches.
        let mut seen: std::collections::HashSet<u64> = std::collections::HashSet::new();

        for poly in water_polygons {
            if !seen.insert(poly.osm_id) {
                continue;
            }

            if is_channelized_water_type(poly.water_type.as_str()) {
                continue;
            }

            // Only process polygons that look like reservoirs:
            // - explicitly tagged water=reservoir/basin/pond
            // - OR large elevated standing water body (centroid > 5 m ASL and area > 0.1 km²)
            let is_explicit = matches!(
                poly.water_type.as_str(),
                "reservoir" | "basin" | "pond" | "lagoon"
            );
            if !is_explicit && poly.polygon.len() < 4 {
                continue;
            }

            // Bounding box of polygon
            let lat_min = poly
                .polygon
                .iter()
                .map(|p| p.lat)
                .fold(f64::INFINITY, f64::min);
            let lat_max = poly
                .polygon
                .iter()
                .map(|p| p.lat)
                .fold(f64::NEG_INFINITY, f64::max);
            let lon_min = poly
                .polygon
                .iter()
                .map(|p| p.lon)
                .fold(f64::INFINITY, f64::min);
            let lon_max = poly
                .polygon
                .iter()
                .map(|p| p.lon)
                .fold(f64::NEG_INFINITY, f64::max);

            // Skip if polygon bbox doesn't intersect DEM
            if lat_max < self.dem.lat_min
                || lat_min > self.dem.lat_max
                || lon_max < self.dem.lon_min
                || lon_min > self.dem.lon_max
            {
                continue;
            }

            // Collect elevations of DEM cells whose centres fall inside the polygon
            let r0 = ((lat_min - self.dem.lat_min) / self.dem.cell_size_deg)
                .floor()
                .max(0.0) as usize;
            let r1 = ((lat_max - self.dem.lat_min) / self.dem.cell_size_deg)
                .ceil()
                .min(self.dem.rows as f64 - 1.0) as usize;
            let c0 = ((lon_min - self.dem.lon_min) / self.dem.cell_size_deg)
                .floor()
                .max(0.0) as usize;
            let c1 = ((lon_max - self.dem.lon_min) / self.dem.cell_size_deg)
                .ceil()
                .min(self.dem.cols as f64 - 1.0) as usize;

            let mut inside_elevs: Vec<f32> = Vec::new();
            let mut inside_cells: Vec<(usize, usize)> = Vec::new();

            for r in r0..=r1 {
                let cell_lat = self.dem.lat_min + (r as f64 + 0.5) * self.dem.cell_size_deg;
                for c in c0..=c1 {
                    let cell_lon = self.dem.lon_min + (c as f64 + 0.5) * self.dem.cell_size_deg;
                    if point_in_polygon(cell_lat, cell_lon, &poly.polygon) {
                        let elev = self.dem.get(r, c);
                        inside_elevs.push(elev);
                        inside_cells.push((r, c));
                    }
                }
            }

            if inside_elevs.len() < 5 {
                continue;
            }

            // For non-explicitly-tagged polygons, require elevated (not sea/tidal)
            // and large enough to plausibly be a reservoir rather than a puddle.
            if !is_explicit {
                let centroid_elev =
                    inside_elevs.iter().copied().sum::<f32>() / inside_elevs.len() as f32;
                // Rough area: degrees² × (111320 m/°)²
                let area_km2 =
                    (lat_max - lat_min) * (lon_max - lon_min) * 111_320.0 * 111_320.0 / 1_000_000.0;
                if centroid_elev < 5.0 || area_km2 < 0.05 {
                    continue;
                }
            }

            // Water surface = 40th-percentile elevation inside polygon.
            // SRTM measures actual water surface; the distribution is narrow
            // (flat water) with some high outliers at the banks.  P40 is below
            // the banks but above any nodata spikes.
            inside_elevs.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
            let p40_idx = (inside_elevs.len() as f32 * 0.40) as usize;
            let water_surface = inside_elevs[p40_idx].max(0.1);

            // Mark all cells inside polygon where DEM is below the water surface.
            for (r, c) in inside_cells {
                let elev = self.dem.get(r, c);
                if elev <= water_surface + 2.0 {
                    // +2m tolerance for SRTM noise
                    let idx = self.idx(r, c);
                    // Use the max water level if multiple reservoirs overlap
                    if self.reservoir_mask[idx] < water_surface {
                        self.reservoir_mask[idx] = water_surface;
                    }
                }
            }
        }

        let count = self.reservoir_mask.iter().filter(|&&v| v > 0.0).count();
        if count > 0 {
            eprintln!("[terrain_analysis] Reservoir mask: {} cells marked", count);
        }
    }

    /// Returns the water surface elevation in metres if this cell is inside a
    /// known reservoir, or `None` if it is dry terrain.
    pub fn reservoir_level_at(&self, lat: f64, lon: f64) -> Option<f32> {
        if self.reservoir_mask.is_empty() {
            return None;
        }
        let (r, c) = self.row_col(lat, lon);
        let v = self.reservoir_mask[self.idx(r, c)];
        if v > 0.0 { Some(v) } else { None }
    }
}

// ── Point-in-polygon ─────────────────────────────────────────────────────────

/// Ray-casting point-in-polygon test for geographic coordinates.
/// Returns `true` if `(lat, lon)` is inside `polygon` (closed ring).
fn point_in_polygon(lat: f64, lon: f64, polygon: &[GPS]) -> bool {
    let n = polygon.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let xi = polygon[i].lon;
        let yi = polygon[i].lat;
        let xj = polygon[j].lon;
        let yj = polygon[j].lat;
        if ((yi > lat) != (yj > lat)) && (lon < (xj - xi) * (lat - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn point_in_rect(
    lat: f64,
    lon: f64,
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> bool {
    lat >= lat_min && lat <= lat_max && lon >= lon_min && lon <= lon_max
}

fn orientation(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

fn on_segment(ax: f64, ay: f64, bx: f64, by: f64, px: f64, py: f64) -> bool {
    const EPS: f64 = 1.0e-12;
    px >= ax.min(bx) - EPS
        && px <= ax.max(bx) + EPS
        && py >= ay.min(by) - EPS
        && py <= ay.max(by) + EPS
}

fn segments_intersect(a0: (f64, f64), a1: (f64, f64), b0: (f64, f64), b1: (f64, f64)) -> bool {
    const EPS: f64 = 1.0e-12;
    let o1 = orientation(a0.0, a0.1, a1.0, a1.1, b0.0, b0.1);
    let o2 = orientation(a0.0, a0.1, a1.0, a1.1, b1.0, b1.1);
    let o3 = orientation(b0.0, b0.1, b1.0, b1.1, a0.0, a0.1);
    let o4 = orientation(b0.0, b0.1, b1.0, b1.1, a1.0, a1.1);

    let crosses = ((o1 > EPS && o2 < -EPS) || (o1 < -EPS && o2 > EPS))
        && ((o3 > EPS && o4 < -EPS) || (o3 < -EPS && o4 > EPS));
    if crosses {
        return true;
    }

    (o1.abs() <= EPS && on_segment(a0.0, a0.1, a1.0, a1.1, b0.0, b0.1))
        || (o2.abs() <= EPS && on_segment(a0.0, a0.1, a1.0, a1.1, b1.0, b1.1))
        || (o3.abs() <= EPS && on_segment(b0.0, b0.1, b1.0, b1.1, a0.0, a0.1))
        || (o4.abs() <= EPS && on_segment(b0.0, b0.1, b1.0, b1.1, a1.0, a1.1))
}

fn polygon_intersects_cell(
    polygon: &[GPS],
    cell_lat_min: f64,
    cell_lat_max: f64,
    cell_lon_min: f64,
    cell_lon_max: f64,
) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    let center_lat = 0.5 * (cell_lat_min + cell_lat_max);
    let center_lon = 0.5 * (cell_lon_min + cell_lon_max);
    if point_in_polygon(center_lat, center_lon, polygon) {
        return true;
    }

    let corners = [
        (cell_lat_min, cell_lon_min),
        (cell_lat_min, cell_lon_max),
        (cell_lat_max, cell_lon_max),
        (cell_lat_max, cell_lon_min),
    ];
    if corners
        .iter()
        .any(|&(lat, lon)| point_in_polygon(lat, lon, polygon))
    {
        return true;
    }

    if polygon.iter().any(|p| {
        point_in_rect(
            p.lat,
            p.lon,
            cell_lat_min,
            cell_lat_max,
            cell_lon_min,
            cell_lon_max,
        )
    }) {
        return true;
    }

    let cell_edges = [
        ((cell_lon_min, cell_lat_min), (cell_lon_max, cell_lat_min)),
        ((cell_lon_max, cell_lat_min), (cell_lon_max, cell_lat_max)),
        ((cell_lon_max, cell_lat_max), (cell_lon_min, cell_lat_max)),
        ((cell_lon_min, cell_lat_max), (cell_lon_min, cell_lat_min)),
    ];

    let segment_count = if polygon.first() == polygon.last() {
        polygon.len().saturating_sub(1)
    } else {
        polygon.len()
    };
    for i in 0..segment_count {
        let a = &polygon[i];
        let b = &polygon[(i + 1) % polygon.len()];
        let seg = ((a.lon, a.lat), (b.lon, b.lat));
        if cell_edges
            .iter()
            .any(|&(c0, c1)| segments_intersect(seg.0, seg.1, c0, c1))
        {
            return true;
        }
    }

    false
}

fn polygon_intersects_polygon(a: &[GPS], b: &[GPS]) -> bool {
    let Some(a_bounds) = polygon_bounds(a) else {
        return false;
    };
    let Some(b_bounds) = polygon_bounds(b) else {
        return false;
    };
    if !bounds_overlap(a_bounds, b_bounds) {
        return false;
    }

    if a.iter().any(|p| point_in_polygon(p.lat, p.lon, b))
        || b.iter().any(|p| point_in_polygon(p.lat, p.lon, a))
    {
        return true;
    }

    for a_edge in a.windows(2) {
        for b_edge in b.windows(2) {
            if segments_intersect(
                (a_edge[0].lon, a_edge[0].lat),
                (a_edge[1].lon, a_edge[1].lat),
                (b_edge[0].lon, b_edge[0].lat),
                (b_edge[1].lon, b_edge[1].lat),
            ) {
                return true;
            }
        }
    }

    false
}

fn is_channelized_water_type(water_type: &str) -> bool {
    matches!(
        water_type,
        "river" | "riverbank" | "stream" | "canal" | "drain" | "ditch"
    )
}

// ── GSHHG coastal distance helpers ───────────────────────────────────────────

/// Read a big-endian i32 from a `Read` source.
fn read_i32_be<R: Read>(r: &mut R) -> io::Result<i32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(i32::from_be_bytes(buf))
}

/// Skip exactly `n` bytes from a `Read` source.
fn skip_bytes<R: Read>(r: &mut R, n: usize) -> io::Result<()> {
    let mut buf = [0u8; 512];
    let mut remaining = n;
    while remaining > 0 {
        let chunk = remaining.min(buf.len());
        r.read_exact(&mut buf[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

/// Convert GSHHG's 0-360° longitude range to standard -180..180°.
#[inline]
fn gshhg_lon(micro_deg: i32) -> f64 {
    let deg = micro_deg as f64 / 1_000_000.0;
    if deg > 180.0 { deg - 360.0 } else { deg }
}

/// Parse all level-1 (ocean/land boundary = coastline) polygon edges from a
/// GSHHG binary file that intersect the DEM bounding box, and rasterise them
/// onto a boolean grid aligned to `dem`.
///
/// Returns a flat `bool` grid, same row/col layout as `dem.elevations`.
fn rasterise_coastline(dem: &RegionDem, path: &std::path::Path) -> Vec<bool> {
    let n = dem.rows * dem.cols;
    let mut on_coast = vec![false; n];

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[terrain_analysis] Cannot open GSHHG {:?}: {e}", path);
            return on_coast;
        }
    };
    let mut reader = BufReader::with_capacity(1 << 20, file);

    let mut poly_count = 0usize;
    let mut seg_count = 0usize;

    loop {
        // ── Read 44-byte header (11 × i32 big-endian) ───────────────────────
        let _id = match read_i32_be(&mut reader) {
            Ok(v) => v,
            Err(_) => break, // clean EOF
        };
        let n_pts = read_i32_be(&mut reader).unwrap_or(0);
        let flag = read_i32_be(&mut reader).unwrap_or(0);
        let west_raw = read_i32_be(&mut reader).unwrap_or(0);
        let east_raw = read_i32_be(&mut reader).unwrap_or(0);
        let south_us = read_i32_be(&mut reader).unwrap_or(0);
        let north_us = read_i32_be(&mut reader).unwrap_or(0);
        // area, area_full, container, ancestor — not needed
        let _ = skip_bytes(&mut reader, 16);

        let level = (flag & 0xFF) as u8;
        let poly_south = south_us as f64 / 1_000_000.0;
        let poly_north = north_us as f64 / 1_000_000.0;
        let poly_west = gshhg_lon(west_raw);
        let poly_east = gshhg_lon(east_raw);

        let in_region = level == 1
            && poly_east >= dem.lon_min
            && poly_west <= dem.lon_max
            && poly_north >= dem.lat_min
            && poly_south <= dem.lat_max;

        if !in_region || n_pts <= 0 {
            let _ = skip_bytes(&mut reader, n_pts as usize * 8);
            continue;
        }

        // ── Read polygon points ──────────────────────────────────────────────
        let mut pts: Vec<(f64, f64)> = Vec::with_capacity(n_pts as usize);
        for _ in 0..n_pts {
            let x = read_i32_be(&mut reader).unwrap_or(0);
            let y = read_i32_be(&mut reader).unwrap_or(0);
            let lon = gshhg_lon(x);
            let lat = y as f64 / 1_000_000.0;
            pts.push((lat, lon));
        }

        // ── Rasterise each edge onto the DEM grid ────────────────────────────
        poly_count += 1;
        for i in 0..pts.len() {
            let j = (i + 1) % pts.len();
            seg_count += rasterise_segment(dem, pts[i], pts[j], &mut on_coast);
        }
    }

    eprintln!(
        "[terrain_analysis] GSHHG: parsed {poly_count} level-1 polygons, \
         rasterised {seg_count} edge segments"
    );
    on_coast
}

fn collect_level1_land_polygons(dem: &RegionDem, path: &std::path::Path) -> Vec<Vec<GPS>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[terrain_analysis] Cannot open GSHHG {:?}: {e}", path);
            return Vec::new();
        }
    };
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let mid_lon = (dem.lon_min + dem.lon_max) * 0.5;
    let mut polygons = Vec::new();

    loop {
        let _id = match read_i32_be(&mut reader) {
            Ok(v) => v,
            Err(_) => break,
        };
        let n_pts = read_i32_be(&mut reader).unwrap_or(0);
        let flag = read_i32_be(&mut reader).unwrap_or(0);
        let west_raw = read_i32_be(&mut reader).unwrap_or(0);
        let east_raw = read_i32_be(&mut reader).unwrap_or(0);
        let south_us = read_i32_be(&mut reader).unwrap_or(0);
        let north_us = read_i32_be(&mut reader).unwrap_or(0);
        let _ = skip_bytes(&mut reader, 16);

        let level = (flag & 0xFF) as u8;
        let poly_south = south_us as f64 / 1_000_000.0;
        let poly_north = north_us as f64 / 1_000_000.0;
        let mut poly_west = wrap_lon_near(gshhg_lon(west_raw), mid_lon);
        let mut poly_east = wrap_lon_near(gshhg_lon(east_raw), mid_lon);
        if poly_west > poly_east {
            std::mem::swap(&mut poly_west, &mut poly_east);
        }

        let in_region = level == 1
            && poly_east >= dem.lon_min
            && poly_west <= dem.lon_max
            && poly_north >= dem.lat_min
            && poly_south <= dem.lat_max;

        if n_pts <= 0 {
            continue;
        }

        let mut polygon = Vec::with_capacity(n_pts as usize);
        for _ in 0..n_pts {
            let x = read_i32_be(&mut reader).unwrap_or(0);
            let y = read_i32_be(&mut reader).unwrap_or(0);
            if in_region {
                polygon.push(GPS::new(
                    y as f64 / 1_000_000.0,
                    wrap_lon_near(gshhg_lon(x), mid_lon),
                    0.0,
                ));
            }
        }

        if in_region && polygon.len() >= 3 {
            polygons.push(polygon);
        }
    }

    polygons
}

fn rasterise_ocean_mask(dem: &RegionDem, path: &std::path::Path) -> Vec<bool> {
    let n = dem.rows * dem.cols;
    let mut land_mask = vec![false; n];

    for polygon in collect_level1_land_polygons(dem, path) {
        let Some(bounds) = polygon_bounds(&polygon) else {
            continue;
        };
        for (r, c) in rasterize_polygon_center_cells(dem, &polygon, bounds) {
            land_mask[r * dem.cols + c] = true;
        }
    }

    land_mask.into_iter().map(|is_land| !is_land).collect()
}

/// Rasterise a single great-circle segment `(lat0,lon0)→(lat1,lon1)` onto the
/// boolean grid using Bresenham's line algorithm in grid-index space.
/// Returns the number of cells marked.
fn rasterise_segment(
    dem: &RegionDem,
    (lat0, lon0): (f64, f64),
    (lat1, lon1): (f64, f64),
    grid: &mut [bool],
) -> usize {
    // Convert geographic coords to continuous grid indices.
    let col_f = |lon: f64| (lon - dem.lon_min) / dem.cell_size_deg;
    let row_f = |lat: f64| (lat - dem.lat_min) / dem.cell_size_deg;

    let mut c0 = col_f(lon0).round() as i64;
    let mut r0 = row_f(lat0).round() as i64;
    let c1 = col_f(lon1).round() as i64;
    let r1 = row_f(lat1).round() as i64;

    let dc = (c1 - c0).abs();
    let dr = (r1 - r0).abs();
    let sc = if c0 < c1 { 1i64 } else { -1i64 };
    let sr = if r0 < r1 { 1i64 } else { -1i64 };
    let mut err = dc - dr;
    let mut marked = 0usize;

    loop {
        // Mark cell if inside grid bounds.
        if c0 >= 0 && c0 < dem.cols as i64 && r0 >= 0 && r0 < dem.rows as i64 {
            let idx = r0 as usize * dem.cols + c0 as usize;
            if !grid[idx] {
                grid[idx] = true;
                marked += 1;
            }
        }
        if c0 == c1 && r0 == r1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dr {
            err -= dr;
            c0 += sc;
        }
        if e2 < dc {
            err += dc;
            r0 += sr;
        }
    }
    marked
}

#[derive(Clone, Copy)]
struct CoastSegment {
    lat0: f64,
    lon0: f64,
    lat1: f64,
    lon1: f64,
}

#[inline]
fn wrap_lon_near(lon: f64, reference_lon: f64) -> f64 {
    let mut wrapped = lon;
    while wrapped - reference_lon > 180.0 {
        wrapped -= 360.0;
    }
    while wrapped - reference_lon < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

fn collect_nearby_coast_segments(
    dem: &RegionDem,
    path: &std::path::Path,
    max_dist_m: f32,
) -> Vec<CoastSegment> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[terrain_analysis] Cannot open GSHHG {:?}: {e}", path);
            return Vec::new();
        }
    };
    let mut reader = BufReader::with_capacity(1 << 20, file);

    let mid_lat = (dem.lat_min + dem.lat_max) * 0.5;
    let mid_lon = (dem.lon_min + dem.lon_max) * 0.5;
    let lat_pad = max_dist_m as f64 / 111_320.0;
    let lon_pad = max_dist_m as f64 / (111_320.0 * mid_lat.to_radians().cos().abs().max(0.1));
    let search_lat_min = dem.lat_min - lat_pad;
    let search_lat_max = dem.lat_max + lat_pad;
    let search_lon_min = dem.lon_min - lon_pad;
    let search_lon_max = dem.lon_max + lon_pad;

    let mut segments = Vec::new();

    loop {
        let _id = match read_i32_be(&mut reader) {
            Ok(v) => v,
            Err(_) => break,
        };
        let n_pts = read_i32_be(&mut reader).unwrap_or(0);
        let flag = read_i32_be(&mut reader).unwrap_or(0);
        let west_raw = read_i32_be(&mut reader).unwrap_or(0);
        let east_raw = read_i32_be(&mut reader).unwrap_or(0);
        let south_us = read_i32_be(&mut reader).unwrap_or(0);
        let north_us = read_i32_be(&mut reader).unwrap_or(0);
        let _ = skip_bytes(&mut reader, 16);

        let level = (flag & 0xFF) as u8;
        let greenwich = ((flag >> 16) & 1) != 0;
        let poly_south = south_us as f64 / 1_000_000.0;
        let poly_north = north_us as f64 / 1_000_000.0;
        let mut poly_west = gshhg_lon(west_raw);
        let mut poly_east = gshhg_lon(east_raw);
        poly_west = wrap_lon_near(poly_west, mid_lon);
        poly_east = wrap_lon_near(poly_east, mid_lon);
        let lon_overlap = if greenwich {
            true
        } else {
            poly_east.max(poly_west) >= search_lon_min && poly_east.min(poly_west) <= search_lon_max
        };
        let in_region = level == 1
            && poly_north >= search_lat_min
            && poly_south <= search_lat_max
            && lon_overlap;

        if !in_region || n_pts <= 1 {
            let _ = skip_bytes(&mut reader, n_pts as usize * 8);
            continue;
        }

        let mut pts: Vec<(f64, f64)> = Vec::with_capacity(n_pts as usize);
        for _ in 0..n_pts {
            let x = read_i32_be(&mut reader).unwrap_or(0);
            let y = read_i32_be(&mut reader).unwrap_or(0);
            let lon = wrap_lon_near(gshhg_lon(x), mid_lon);
            let lat = y as f64 / 1_000_000.0;
            pts.push((lat, lon));
        }

        for i in 0..pts.len() {
            let j = (i + 1) % pts.len();
            let (lat0, lon0) = pts[i];
            let (lat1, lon1) = pts[j];
            let seg_lat_min = lat0.min(lat1);
            let seg_lat_max = lat0.max(lat1);
            let seg_lon_min = lon0.min(lon1);
            let seg_lon_max = lon0.max(lon1);
            if seg_lat_max < search_lat_min
                || seg_lat_min > search_lat_max
                || seg_lon_max < search_lon_min
                || seg_lon_min > search_lon_max
            {
                continue;
            }
            segments.push(CoastSegment {
                lat0,
                lon0,
                lat1,
                lon1,
            });
        }
    }

    eprintln!(
        "[terrain_analysis] GSHHG fallback: collected {} nearby coastline segments",
        segments.len()
    );
    segments
}

fn point_to_segment_distance_m(lat: f64, lon: f64, segment: CoastSegment) -> f32 {
    let lat_scale = 111_320.0_f64;
    let lon_scale = 111_320.0_f64 * lat.to_radians().cos().abs().max(0.1);

    let ax = (wrap_lon_near(segment.lon0, lon) - lon) * lon_scale;
    let ay = (segment.lat0 - lat) * lat_scale;
    let bx = (wrap_lon_near(segment.lon1, lon) - lon) * lon_scale;
    let by = (segment.lat1 - lat) * lat_scale;

    let abx = bx - ax;
    let aby = by - ay;
    let ab_len2 = abx * abx + aby * aby;
    if ab_len2 <= f64::EPSILON {
        return (ax * ax + ay * ay).sqrt() as f32;
    }

    let t = (-(ax * abx + ay * aby) / ab_len2).clamp(0.0, 1.0);
    let px = ax + t * abx;
    let py = ay + t * aby;
    (px * px + py * py).sqrt() as f32
}

fn direct_coastal_dist_fallback(dem: &RegionDem, path: &std::path::Path, max_dist_m: f32) -> Vec<f32> {
    let segments = collect_nearby_coast_segments(dem, path, max_dist_m);
    if segments.is_empty() {
        return vec![max_dist_m; dem.rows * dem.cols];
    }

    let mut dist = vec![max_dist_m; dem.rows * dem.cols];
    for r in 0..dem.rows {
        let lat = dem.lat_min + (r as f64 + 0.5) * dem.cell_size_deg;
        for c in 0..dem.cols {
            let lon = dem.lon_min + (c as f64 + 0.5) * dem.cell_size_deg;
            let mut best = max_dist_m;
            for segment in &segments {
                let d = point_to_segment_distance_m(lat, lon, *segment);
                if d < best {
                    best = d;
                }
            }
            dist[r * dem.cols + c] = best.min(max_dist_m);
        }
    }
    dist
}

/// Multi-source BFS (4-connected) distance transform from all coastline cells.
///
/// Returns a flat `f32` grid where each value is the approximate distance to
/// the nearest coastline cell in metres (Manhattan metric scaled by
/// `cell_size_m`).  Cells beyond `max_dist_m` are capped.
fn bfs_coastal_dist(rows: usize, cols: usize, on_coast: &[bool], cell_size_m: f32) -> Vec<f32> {
    const MAX_DIST: f32 = 50_000.0; // 50 km cap — beyond this we don't care
    let n = rows * cols;
    let mut dist = vec![f32::MAX; n];
    let mut queue =
        std::collections::VecDeque::with_capacity(on_coast.iter().filter(|&&v| v).count() * 4);

    // Seed all coastline cells at distance 0.
    for i in 0..n {
        if on_coast[i] {
            dist[i] = 0.0;
            queue.push_back(i);
        }
    }

    // 4-connected BFS: each step adds one `cell_size_m`.
    const DR: [i32; 4] = [0, 0, 1, -1];
    const DC: [i32; 4] = [1, -1, 0, 0];

    while let Some(idx) = queue.pop_front() {
        let cur = dist[idx];
        if cur >= MAX_DIST {
            continue;
        }
        let r = idx / cols;
        let c = idx % cols;
        let nd = cur + cell_size_m;
        for d in 0..4 {
            let nr = r as i32 + DR[d];
            let nc = c as i32 + DC[d];
            if nr < 0 || nr >= rows as i32 || nc < 0 || nc >= cols as i32 {
                continue;
            }
            let ni = nr as usize * cols + nc as usize;
            if nd < dist[ni] {
                dist[ni] = nd;
                queue.push_back(ni);
            }
        }
    }

    // Cap and replace MAX sentinels.
    dist.iter_mut().for_each(|v| {
        if *v == f32::MAX {
            *v = MAX_DIST;
        }
    });
    dist
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dem() -> RegionDem {
        RegionDem {
            elevations: vec![100.0; 9],
            rows: 3,
            cols: 3,
            lat_min: 0.0,
            lat_max: 0.03,
            lon_min: 0.0,
            lon_max: 0.03,
            cell_size_deg: 0.01,
        }
    }

    fn square_polygon() -> Vec<GPS> {
        vec![
            GPS::new(0.0, 0.0, 0.0),
            GPS::new(0.0, 0.03, 0.0),
            GPS::new(0.03, 0.03, 0.0),
            GPS::new(0.03, 0.0, 0.0),
            GPS::new(0.0, 0.0, 0.0),
        ]
    }

    #[test]
    fn compute_reservoirs_ignores_channelized_river_polygons() {
        let mut analysis = TerrainAnalysis::compute(test_dem());
        let water = OsmWater {
            osm_id: 1,
            polygon: square_polygon(),
            holes: vec![],
            name: Some("Mary River".into()),
            water_type: "river".into(),
        };

        analysis.compute_reservoirs(&[water]);

        assert!(analysis.reservoir_mask.iter().all(|&level| level == 0.0));
        assert_eq!(analysis.reservoir_level_at(0.015, 0.015), None);
    }

    #[test]
    fn compute_reservoirs_keeps_explicit_reservoir_polygons() {
        let mut analysis = TerrainAnalysis::compute(test_dem());
        let water = OsmWater {
            osm_id: 2,
            polygon: square_polygon(),
            holes: vec![],
            name: Some("Test Reservoir".into()),
            water_type: "reservoir".into(),
        };

        analysis.compute_reservoirs(&[water]);

        assert_eq!(analysis.reservoir_level_at(0.015, 0.015), Some(100.0));
    }

    #[test]
    fn compute_osm_landuse_marks_residential_cells() {
        let mut analysis = TerrainAnalysis::compute(test_dem());
        let area = OsmLandArea {
            osm_id: 10,
            polygon: square_polygon(),
            name: Some("Test Estate".into()),
            area_type: "residential".into(),
            category: "landuse".into(),
        };

        analysis.compute_osm_landuse(&[area]);

        assert_eq!(
            analysis.osm_landuse_at(0.015, 0.015),
            Some(OsmLanduse::Residential)
        );
    }

    #[test]
    fn compute_engineered_ground_flattens_recreation_ground_to_lower_bench() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 110.0, 120.0, 130.0, 100.0, //
                100.0, 110.0, 120.0, 130.0, 100.0, //
                100.0, 110.0, 120.0, 130.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let area = OsmLandArea {
            osm_id: 11,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Flat Reserve".into()),
            area_type: "recreation_ground".into(),
            category: "landuse".into(),
        };

        analysis.compute_engineered_ground(&[area]);

        let (target, strength) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("engineered ground control");
        assert!((target - 100.0).abs() < 0.01);
        assert!(strength > 0.95);
    }

    fn skinny_edge_polygon() -> Vec<GPS> {
        vec![
            GPS::new(0.001, 0.0085, 0.0),
            GPS::new(0.001, 0.0095, 0.0),
            GPS::new(0.029, 0.0095, 0.0),
            GPS::new(0.029, 0.0085, 0.0),
            GPS::new(0.001, 0.0085, 0.0),
        ]
    }

    #[test]
    fn compute_osm_landuse_marks_cells_when_polygon_only_clips_cell_edge() {
        let mut analysis = TerrainAnalysis::compute(test_dem());
        let area = OsmLandArea {
            osm_id: 12,
            polygon: skinny_edge_polygon(),
            name: Some("Edge Estate".into()),
            area_type: "residential".into(),
            category: "landuse".into(),
        };

        analysis.compute_osm_landuse(&[area]);

        assert_eq!(
            analysis.osm_landuse_at(0.015, 0.009),
            Some(OsmLanduse::Residential)
        );
    }

    #[test]
    fn compute_engineered_ground_marks_cells_when_polygon_only_clips_cell_edge() {
        let dem = RegionDem {
            elevations: vec![
                110.0, 100.0, 100.0, //
                120.0, 100.0, 100.0, //
                130.0, 100.0, 100.0,
            ],
            rows: 3,
            cols: 3,
            lat_min: 0.0,
            lat_max: 0.03,
            lon_min: 0.0,
            lon_max: 0.03,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let area = OsmLandArea {
            osm_id: 13,
            polygon: skinny_edge_polygon(),
            name: Some("Edge Pitch".into()),
            area_type: "pitch".into(),
            category: "leisure".into(),
        };

        analysis.compute_engineered_ground(&[area]);

        let (target, strength) = analysis
            .engineered_ground_control_at(0.015, 0.009)
            .expect("engineered ground control");
        assert!((target - 120.0).abs() < 0.01);
        assert!(strength > 0.95);
    }

    #[test]
    fn engineered_ground_control_does_not_bleed_into_adjacent_unmarked_cell() {
        let dem = RegionDem {
            elevations: vec![
                110.0, 100.0, 100.0, //
                120.0, 100.0, 100.0, //
                130.0, 100.0, 100.0,
            ],
            rows: 3,
            cols: 3,
            lat_min: 0.0,
            lat_max: 0.03,
            lon_min: 0.0,
            lon_max: 0.03,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let area = OsmLandArea {
            osm_id: 14,
            polygon: skinny_edge_polygon(),
            name: Some("Edge Pitch".into()),
            area_type: "pitch".into(),
            category: "leisure".into(),
        };

        analysis.compute_engineered_ground(&[area]);

        assert!(
            analysis
                .engineered_ground_control_at(0.015, 0.0115)
                .is_none()
        );
    }

    #[test]
    fn recreation_ground_halo_marks_adjacent_unmapped_cell() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let area = OsmLandArea {
            osm_id: 15,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Flat Reserve".into()),
            area_type: "recreation_ground".into(),
            category: "landuse".into(),
        };

        analysis.compute_engineered_ground(&[area]);

        let (target, strength) = analysis
            .engineered_ground_control_at(0.025, 0.045)
            .expect("adjacent fringe cell should inherit recreation-ground halo");
        assert!((target - 100.0).abs() < 0.01);
        assert!(strength >= 0.84);
    }

    #[test]
    fn recreation_ground_halo_rejects_large_height_jump() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 130.0, 130.0, //
                100.0, 100.0, 100.0, 130.0, 130.0, //
                100.0, 100.0, 100.0, 130.0, 130.0, //
                100.0, 100.0, 100.0, 130.0, 130.0, //
                100.0, 100.0, 100.0, 130.0, 130.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let area = OsmLandArea {
            osm_id: 16,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.03, 0.0),
                GPS::new(0.04, 0.03, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Raised Edge".into()),
            area_type: "recreation_ground".into(),
            category: "landuse".into(),
        };

        analysis.compute_engineered_ground(&[area]);

        assert!(
            analysis
                .engineered_ground_control_at(0.025, 0.045)
                .is_none()
        );
    }

    #[test]
    fn generic_park_without_nested_sports_remains_unmarked() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 101.0, 102.0, //
                103.0, 104.0, 105.0, //
                106.0, 107.0, 108.0,
            ],
            rows: 3,
            cols: 3,
            lat_min: 0.0,
            lat_max: 0.03,
            lon_min: 0.0,
            lon_max: 0.03,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let park = OsmLandArea {
            osm_id: 17,
            polygon: vec![
                GPS::new(0.0, 0.0, 0.0),
                GPS::new(0.0, 0.03, 0.0),
                GPS::new(0.03, 0.03, 0.0),
                GPS::new(0.03, 0.0, 0.0),
                GPS::new(0.0, 0.0, 0.0),
            ],
            name: Some("Generic Park".into()),
            area_type: "park".into(),
            category: "leisure".into(),
        };

        analysis.compute_engineered_ground(&[park]);

        assert!(
            analysis
                .engineered_ground_control_at(0.015, 0.015)
                .is_none()
        );
    }

    #[test]
    fn park_with_nested_pitch_inherits_support_ground_level() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 130.0, 135.0, 140.0, 100.0, //
                100.0, 125.0, 100.0, 138.0, 100.0, //
                100.0, 128.0, 134.0, 136.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let park = OsmLandArea {
            osm_id: 18,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Sports Park".into()),
            area_type: "park".into(),
            category: "leisure".into(),
        };
        let pitch = OsmLandArea {
            osm_id: 19,
            polygon: vec![
                GPS::new(0.02, 0.02, 0.0),
                GPS::new(0.02, 0.03, 0.0),
                GPS::new(0.03, 0.03, 0.0),
                GPS::new(0.03, 0.02, 0.0),
                GPS::new(0.02, 0.02, 0.0),
            ],
            name: Some("Central Pitch".into()),
            area_type: "pitch".into(),
            category: "leisure".into(),
        };

        analysis.compute_engineered_ground(&[park, pitch]);

        let (support_target, support_strength) = analysis
            .engineered_ground_control_at(0.025, 0.015)
            .expect("support cell should inherit park engineered ground");
        assert!((support_target - 100.0).abs() < 0.01);
        assert!(support_strength >= 0.74);

        let (pitch_target, pitch_strength) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("pitch cell should retain explicit engineered ground");
        assert!((pitch_target - 100.0).abs() < 0.01);
        assert!(pitch_strength > 0.95);
    }

    #[test]
    fn park_with_nested_playground_does_not_inherit_support_ground_level() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 130.0, 135.0, 140.0, 100.0, //
                100.0, 125.0, 100.0, 138.0, 100.0, //
                100.0, 128.0, 134.0, 136.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let park = OsmLandArea {
            osm_id: 30,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Play Park".into()),
            area_type: "park".into(),
            category: "leisure".into(),
        };
        let playground = OsmLandArea {
            osm_id: 31,
            polygon: vec![
                GPS::new(0.02, 0.02, 0.0),
                GPS::new(0.02, 0.03, 0.0),
                GPS::new(0.03, 0.03, 0.0),
                GPS::new(0.03, 0.02, 0.0),
                GPS::new(0.02, 0.02, 0.0),
            ],
            name: Some("Central Playground".into()),
            area_type: "playground".into(),
            category: "leisure".into(),
        };

        analysis.compute_engineered_ground(&[park, playground]);

        assert!(
            analysis
                .engineered_ground_control_at(0.025, 0.015)
                .is_none(),
            "park support should stay local when the only explicit owner is a playground"
        );

        let (playground_target, playground_strength) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("playground cell should retain explicit engineered ground");
        assert!((playground_target - 100.0).abs() < 0.01);
        assert!(playground_strength > 0.95);
    }

    #[test]
    fn aerodrome_with_nested_runway_inherits_support_ground_level() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 130.0, 135.0, 140.0, 100.0, //
                100.0, 125.0, 100.0, 138.0, 100.0, //
                100.0, 128.0, 134.0, 136.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let aerodrome = OsmAeroway {
            osm_id: 32,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            nodes: Vec::new(),
            aeroway_type: "aerodrome".into(),
            name: Some("Test Airport".into()),
            is_area: true,
        };
        let runway = OsmAeroway {
            osm_id: 33,
            polygon: vec![
                GPS::new(0.02, 0.02, 0.0),
                GPS::new(0.02, 0.03, 0.0),
                GPS::new(0.03, 0.03, 0.0),
                GPS::new(0.03, 0.02, 0.0),
                GPS::new(0.02, 0.02, 0.0),
            ],
            nodes: Vec::new(),
            aeroway_type: "runway".into(),
            name: Some("Runway 09/27".into()),
            is_area: true,
        };

        analysis.compute_engineered_ground_with_aeroways(&[], &[aerodrome, runway]);

        let (support_target, support_strength) = analysis
            .engineered_ground_control_at(0.025, 0.015)
            .expect("aerodrome support cell should inherit runway engineered ground");
        assert!((support_target - 100.0).abs() < 0.01);
        assert!(support_strength >= 0.89);

        let (runway_target, runway_strength) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("runway cell should retain explicit engineered ground");
        assert!((runway_target - 100.0).abs() < 0.01);
        assert!(runway_strength > 0.95);
    }

    #[test]
    fn aerodrome_with_nested_runway_centerline_inherits_support_ground_level() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 130.0, 135.0, 140.0, 100.0, //
                100.0, 125.0, 100.0, 138.0, 100.0, //
                100.0, 128.0, 134.0, 136.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let aerodrome = OsmAeroway {
            osm_id: 34,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            nodes: Vec::new(),
            aeroway_type: "aerodrome".into(),
            name: Some("Line Airport".into()),
            is_area: true,
        };
        let runway_centerline = OsmAeroway {
            osm_id: 35,
            polygon: Vec::new(),
            nodes: vec![GPS::new(0.025, 0.015, 0.0), GPS::new(0.025, 0.035, 0.0)],
            aeroway_type: "runway".into(),
            name: Some("Runway Centreline".into()),
            is_area: false,
        };

        analysis.compute_engineered_ground_with_aeroways(&[], &[aerodrome, runway_centerline]);

        let (support_target, support_strength) = analysis
            .engineered_ground_control_at(0.025, 0.015)
            .expect("aerodrome support cell should inherit centerline runway target");
        assert!((support_target - 100.0).abs() < 0.01);
        assert!(support_strength >= 0.89);

        let (runway_target, runway_strength) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("buffered runway centerline should seed explicit engineered ground");
        assert!((runway_target - 100.0).abs() < 0.01);
        assert!(runway_strength > 0.95);
    }

    #[test]
    fn standalone_aerodrome_marks_engineered_ground() {
        let dem = RegionDem {
            elevations: vec![
                25.0, 25.0, 25.0, 25.0, 25.0, //
                25.0, 27.0, 28.0, 27.0, 25.0, //
                25.0, 26.0, 29.0, 26.0, 25.0, //
                25.0, 27.0, 28.0, 27.0, 25.0, //
                25.0, 25.0, 25.0, 25.0, 25.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let aerodrome = OsmAeroway {
            osm_id: 36,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            nodes: Vec::new(),
            aeroway_type: "aerodrome".into(),
            name: Some("Standalone Airport".into()),
            is_area: true,
        };

        analysis.compute_engineered_ground_with_aeroways(&[], &[aerodrome]);

        let (target, strength) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("standalone aerodrome should mark engineered ground");
        assert!((target - 25.0).abs() < 0.01);
        assert!(strength >= 0.9);
    }

    #[test]
    fn connected_engineered_ground_component_shares_one_target() {
        let dem = RegionDem {
            elevations: vec![
                100.0, 100.0, 100.0, 100.0, 100.0, //
                100.0, 100.0, 115.0, 140.0, 100.0, //
                100.0, 100.0, 120.0, 140.0, 100.0, //
                100.0, 100.0, 118.0, 140.0, 100.0, //
                100.0, 100.0, 100.0, 100.0, 100.0,
            ],
            rows: 5,
            cols: 5,
            lat_min: 0.0,
            lat_max: 0.05,
            lon_min: 0.0,
            lon_max: 0.05,
            cell_size_deg: 0.01,
        };
        let mut analysis = TerrainAnalysis::compute(dem);
        let park = OsmLandArea {
            osm_id: 20,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Shared Sports Park".into()),
            area_type: "park".into(),
            category: "leisure".into(),
        };
        let low_pitch = OsmLandArea {
            osm_id: 21,
            polygon: vec![
                GPS::new(0.01, 0.01, 0.0),
                GPS::new(0.01, 0.02, 0.0),
                GPS::new(0.04, 0.02, 0.0),
                GPS::new(0.04, 0.01, 0.0),
                GPS::new(0.01, 0.01, 0.0),
            ],
            name: Some("Low Pitch".into()),
            area_type: "pitch".into(),
            category: "leisure".into(),
        };
        let high_pitch = OsmLandArea {
            osm_id: 22,
            polygon: vec![
                GPS::new(0.01, 0.03, 0.0),
                GPS::new(0.01, 0.04, 0.0),
                GPS::new(0.04, 0.04, 0.0),
                GPS::new(0.04, 0.03, 0.0),
                GPS::new(0.01, 0.03, 0.0),
            ],
            name: Some("High Pitch".into()),
            area_type: "pitch".into(),
            category: "leisure".into(),
        };

        analysis.compute_engineered_ground(&[park, low_pitch, high_pitch]);

        let (low_target, _) = analysis
            .engineered_ground_control_at(0.025, 0.015)
            .expect("low pitch should be engineered");
        let (high_target, _) = analysis
            .engineered_ground_control_at(0.025, 0.035)
            .expect("high pitch should be engineered");
        let (support_target, _) = analysis
            .engineered_ground_control_at(0.025, 0.025)
            .expect("support park should inherit shared engineered target");

        assert!((low_target - 100.0).abs() < 0.01);
        assert!((high_target - 100.0).abs() < 0.01);
        assert!((support_target - 100.0).abs() < 0.01);
    }
}
