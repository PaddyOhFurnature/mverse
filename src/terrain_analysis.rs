//! Tier 2 SRTM terrain analysis — derived rasters from elevation data.
//!
//! Computes flow direction (D8), flow accumulation, TWI, aspect, TRI, slope,
//! and coastal distance from a 2D DEM grid sampled from the elevation pipeline.
//! Works globally at SRTM resolution (~30 m); results are later interpolated
//! to voxel scale.

use std::io::{self, BufReader, Read};
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
    /// Distance to nearest coastline in metres.
    /// Empty when GSHHG data is not provided; `coastal_dist_at` returns 100 000 m.
    pub coastal_dist: Vec<f32>,
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
            coastal_dist: Vec::new(),
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
        self.coastal_dist = bfs_coastal_dist(self.dem.rows, self.dem.cols, &on_coast, cell_size_m);
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
    let mut seg_count  = 0usize;

    loop {
        // ── Read 44-byte header (11 × i32 big-endian) ───────────────────────
        let _id = match read_i32_be(&mut reader) {
            Ok(v) => v,
            Err(_) => break, // clean EOF
        };
        let n_pts    = read_i32_be(&mut reader).unwrap_or(0);
        let flag     = read_i32_be(&mut reader).unwrap_or(0);
        let west_raw = read_i32_be(&mut reader).unwrap_or(0);
        let east_raw = read_i32_be(&mut reader).unwrap_or(0);
        let south_us = read_i32_be(&mut reader).unwrap_or(0);
        let north_us = read_i32_be(&mut reader).unwrap_or(0);
        // area, area_full, container, ancestor — not needed
        let _ = skip_bytes(&mut reader, 16);

        let level      = (flag & 0xFF) as u8;
        let poly_south = south_us as f64 / 1_000_000.0;
        let poly_north = north_us as f64 / 1_000_000.0;
        let poly_west  = gshhg_lon(west_raw);
        let poly_east  = gshhg_lon(east_raw);

        let in_region = level == 1
            && poly_east  >= dem.lon_min
            && poly_west  <= dem.lon_max
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
    let c1     = col_f(lon1).round() as i64;
    let r1     = row_f(lat1).round() as i64;

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
        if c0 == c1 && r0 == r1 { break; }
        let e2 = 2 * err;
        if e2 > -dr { err -= dr; c0 += sc; }
        if e2 <  dc { err += dc; r0 += sr; }
    }
    marked
}

/// Multi-source BFS (4-connected) distance transform from all coastline cells.
///
/// Returns a flat `f32` grid where each value is the approximate distance to
/// the nearest coastline cell in metres (Manhattan metric scaled by
/// `cell_size_m`).  Cells beyond `max_dist_m` are capped.
fn bfs_coastal_dist(
    rows: usize,
    cols: usize,
    on_coast: &[bool],
    cell_size_m: f32,
) -> Vec<f32> {
    const MAX_DIST: f32 = 50_000.0; // 50 km cap — beyond this we don't care
    let n = rows * cols;
    let mut dist = vec![f32::MAX; n];
    let mut queue = std::collections::VecDeque::with_capacity(on_coast.iter().filter(|&&v| v).count() * 4);

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
        if cur >= MAX_DIST { continue; }
        let r = idx / cols;
        let c = idx % cols;
        let nd = cur + cell_size_m;
        for d in 0..4 {
            let nr = r as i32 + DR[d];
            let nc = c as i32 + DC[d];
            if nr < 0 || nr >= rows as i32 || nc < 0 || nc >= cols as i32 { continue; }
            let ni = nr as usize * cols + nc as usize;
            if nd < dist[ni] {
                dist[ni] = nd;
                queue.push_back(ni);
            }
        }
    }

    // Cap and replace MAX sentinels.
    dist.iter_mut().for_each(|v| { if *v == f32::MAX { *v = MAX_DIST; } });
    dist
}
