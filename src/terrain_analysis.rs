//! Tier 2 SRTM terrain analysis — derived rasters from elevation data.
//!
//! Computes flow direction (D8), flow accumulation, TWI, aspect, TRI, and slope
//! from a 2D DEM grid sampled from the elevation pipeline.  Works globally at
//! SRTM resolution (~30 m); results are later interpolated to voxel scale.

use crate::coordinates::GPS;
use crate::elevation::ElevationPipeline;

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
                    - e(rm1, cm1) - 2.0 * e(r, cm1) - e(rp1, cm1))
                    / (8.0 * cell_size_m);

                // North–South gradient (positive = North is higher; r+1 = North in our grid)
                let dz_dy = (e(rp1, cm1) + 2.0 * e(rp1, c) + e(rp1, cp1)
                    - e(rm1, cm1) - 2.0 * e(rm1, c) - e(rm1, cp1))
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
                    if nr < 0
                        || nr >= dem.rows as i32
                        || nc < 0
                        || nc >= dem.cols as i32
                    {
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
                    if nr >= 0
                        && nr < dem.rows as i32
                        && nc >= 0
                        && nc < dem.cols as i32
                    {
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
}
