use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use image::{Rgb, RgbImage};
use metaverse_core::chunk::ChunkId;
use metaverse_core::coordinates::{ECEF, GPS};
use metaverse_core::elevation::{
    ElevationPipeline, egm96_undulation, make_offline_elevation_pipeline,
};
use metaverse_core::osm::{OsmDiskCache, OsmLandArea, OsmWater};
use metaverse_core::terrain::{SurfaceCache, TerrainGenerator};
use metaverse_core::terrain_analysis::{RegionDem, TerrainAnalysis};
use metaverse_core::voxel::{VoxelCoord, WORLD_MIN_METERS};
use serde::{Deserialize, Serialize};

const OSM_TILE_DEGREES: f64 = 0.01;

#[derive(Parser, Debug)]
#[command(name = "terrain_controls")]
#[command(about = "Sample category-based terrain controls and emit numeric + image diagnostics")]
struct Args {
    #[arg(
        long,
        default_value = "scripts/control_atlas/global_terrain_controls.json"
    )]
    atlas: PathBuf,
    #[arg(long, default_value = "screenshot/terrain-controls")]
    output: PathBuf,
    #[arg(long, default_value = "world_data/srtm")]
    srtm_dir: PathBuf,
    #[arg(long)]
    cop30_dir: Option<PathBuf>,
    #[arg(long, default_value = "world_data/osm")]
    osm_cache: PathBuf,
    #[arg(long)]
    coastline: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    categories: Vec<String>,
    #[arg(long)]
    max_per_category: Option<usize>,
    #[arg(long)]
    image_size: Option<u32>,
    #[arg(long)]
    skip_images: bool,
    #[arg(long)]
    chunk_sample_grid: Option<u32>,
}

fn default_window_m() -> f64 {
    240.0
}

fn default_image_size() -> u32 {
    96
}

fn default_expected_min_controls() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ControlAtlas {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_window_m")]
    default_window_m: f64,
    #[serde(default = "default_image_size")]
    default_image_size: u32,
    categories: Vec<ControlCategory>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ControlCategory {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_expected_min_controls")]
    expected_min_controls: usize,
    controls: Vec<ControlSite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ControlSite {
    id: String,
    label: String,
    lat: f64,
    lon: f64,
    #[serde(default)]
    window_m: Option<f64>,
    #[serde(default)]
    image_size: Option<u32>,
    #[serde(default)]
    reference_orthometric_alt_m: Option<f64>,
    #[serde(default)]
    reference_source: Option<String>,
    #[serde(default)]
    reference_status: Option<String>,
    #[serde(default)]
    reference_notes: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
struct ScalarStats {
    min: f64,
    max: f64,
    mean: f64,
    p95: f64,
}

#[derive(Debug, Clone, Serialize)]
struct GridMetrics {
    source_elevation_m: ScalarStats,
    generated_elevation_m: ScalarStats,
    generated_minus_source_m: ScalarStats,
    generated_minus_source_abs_p95_m: f64,
    generated_minus_source_abs_max_m: f64,
    generated_changed_cell_count: usize,
    generated_changed_fraction: f64,
    slope_deg: ScalarStats,
    tri_m: ScalarStats,
    twi: ScalarStats,
}

#[derive(Debug, Clone, Serialize)]
struct CenterMetrics {
    source_orthometric_alt_m: f64,
    generated_orthometric_alt_m: f64,
    generated_minus_source_m: f64,
    slope_deg: f32,
    tri_m: f32,
    twi: f32,
    coastal_dist_m: f32,
    osm_landuse: Option<String>,
    engineered_ground_target_m: Option<f32>,
    engineered_ground_strength: Option<f32>,
    reference_orthometric_alt_m: Option<f64>,
    source_minus_reference_m: Option<f64>,
    generated_minus_reference_m: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum SurfaceMode {
    DemOnly,
    OsmContextOnly,
    EngineeredGroundActive,
}

impl SurfaceMode {
    fn as_str(self) -> &'static str {
        match self {
            SurfaceMode::DemOnly => "dem_only",
            SurfaceMode::OsmContextOnly => "osm_context_only",
            SurfaceMode::EngineeredGroundActive => "engineered_ground_active",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct CoverageMetrics {
    surface_mode: SurfaceMode,
    osm_water_feature_count: usize,
    osm_land_area_feature_count: usize,
    osm_landuse_cell_count: usize,
    engineered_ground_cell_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ChunkSurfaceMetrics {
    sample_grid_size: u32,
    sample_count: usize,
    generated_chunk_count: usize,
    center_orthometric_alt_m: f64,
    center_column_lat: f64,
    center_column_lon: f64,
    center_sampled_source_orthometric_alt_m: f64,
    center_sampled_fast_surface_orthometric_alt_m: f64,
    center_minus_sampled_source_m: f64,
    center_minus_sampled_fast_surface_m: f64,
    center_requested_to_column_offset_m: f64,
    generated_minus_sampled_source_m: ScalarStats,
    generated_minus_sampled_source_abs_p95_m: f64,
    generated_minus_sampled_source_abs_max_m: f64,
    generated_minus_sampled_fast_surface_m: ScalarStats,
    generated_minus_sampled_fast_surface_abs_p95_m: f64,
    generated_minus_sampled_fast_surface_abs_max_m: f64,
    requested_to_column_offset_m: ScalarStats,
}

#[derive(Debug, Clone, Serialize)]
struct ControlArtifacts {
    source_height_png: Option<String>,
    generated_height_png: Option<String>,
    delta_png: Option<String>,
    slope_png: Option<String>,
    sheet_png: Option<String>,
    site_json: String,
}

#[derive(Debug, Clone, Serialize)]
struct ControlReport {
    id: String,
    label: String,
    category_id: String,
    category_name: String,
    lat: f64,
    lon: f64,
    window_m: f64,
    image_size: u32,
    tags: Vec<String>,
    reference_source: Option<String>,
    reference_status: Option<String>,
    reference_notes: Option<String>,
    coverage: CoverageMetrics,
    chunk_surface: Option<ChunkSurfaceMetrics>,
    center: CenterMetrics,
    grid: GridMetrics,
    artifacts: ControlArtifacts,
}

#[derive(Debug, Clone, Serialize)]
struct CategoryAggregate {
    control_count: usize,
    source_span_mean_m: f64,
    generated_span_mean_m: f64,
    delta_abs_p95_mean_m: f64,
    slope_p95_mean_deg: f64,
    controls_with_osm_context: usize,
    controls_with_engineered_ground: usize,
    controls_with_surface_delta: usize,
    dem_only_controls: usize,
}

#[derive(Debug, Clone, Serialize)]
struct CategoryReport {
    id: String,
    name: String,
    description: String,
    expected_min_controls: usize,
    aggregate: CategoryAggregate,
    controls: Vec<ControlReport>,
}

#[derive(Debug, Clone, Serialize)]
struct AtlasReport {
    atlas_name: String,
    atlas_description: String,
    generated_at_unix_ms: u128,
    args: Vec<String>,
    warnings: Vec<String>,
    categories: Vec<CategoryReport>,
}

fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn make_elevation_pipeline(srtm_dir: &Path, cop30_dir: Option<&Path>) -> ElevationPipeline {
    make_offline_elevation_pipeline(
        srtm_dir.to_path_buf(),
        cop30_dir.map(|dir| dir.to_path_buf()),
    )
}

fn control_origin(pipeline: &ElevationPipeline, lat: f64, lon: f64) -> (GPS, VoxelCoord) {
    let source_orthometric_m = pipeline
        .query_with_fill(&GPS::new(lat, lon, 0.0))
        .map(|e| e.meters)
        .unwrap_or(0.0);
    let origin_gps = GPS::new(lat, lon, source_orthometric_m + egm96_undulation(lat, lon));
    let origin_voxel = VoxelCoord::from_ecef(&origin_gps.to_ecef());
    (origin_gps, origin_voxel)
}

fn voxel_column_gps(origin_voxel: &VoxelCoord, voxel_x: i64, voxel_z: i64) -> GPS {
    const WGS84_A: f64 = 6_378_137.0;
    const WGS84_B: f64 = 6_356_752.3142;
    let origin_ecef_y = (origin_voxel.y as f64 + 0.5) + WORLD_MIN_METERS;
    let ecef_x = (voxel_x as f64 + 0.5) + WORLD_MIN_METERS;
    let ecef_z = (voxel_z as f64 + 0.5) + WORLD_MIN_METERS;
    let y_sq = WGS84_A * WGS84_A * (1.0 - (ecef_z / WGS84_B).powi(2)) - ecef_x * ecef_x;
    let ecef_y = if y_sq > 0.0 {
        y_sq.sqrt() * origin_ecef_y.signum()
    } else {
        origin_ecef_y
    };
    ECEF::new(ecef_x, ecef_y, ecef_z).to_gps()
}

fn gps_to_voxel_xz(lat: f64, lon: f64) -> (i64, i64) {
    let voxel = VoxelCoord::from_ecef(&GPS::new(lat, lon, 0.0).to_ecef());
    (voxel.x, voxel.z)
}

fn ecef_distance_m(a: GPS, b: GPS) -> f64 {
    a.to_ecef().distance_to(&b.to_ecef())
}

fn snap_gps_to_voxel_xz(origin_voxel: &VoxelCoord, lat: f64, lon: f64) -> (i64, i64, GPS, f64) {
    let requested = GPS::new(lat, lon, 0.0);
    let (base_vx, base_vz) = gps_to_voxel_xz(lat, lon);
    let (mut best_vx, mut best_vz) = (base_vx, base_vz);
    let mut best_gps = voxel_column_gps(origin_voxel, best_vx, best_vz);
    let mut best_dist = ecef_distance_m(requested, best_gps);

    // The X/Z -> GPS mapping gets locally ill-conditioned near meridians, so the
    // direct inverse is usually close but the distance surface is not smooth enough
    // for hill-climbing. A small exhaustive neighborhood is both stable and cheap.
    const SEARCH_RADIUS: i64 = 16;
    for dx in -SEARCH_RADIUS..=SEARCH_RADIUS {
        for dz in -SEARCH_RADIUS..=SEARCH_RADIUS {
            if dx == 0 && dz == 0 {
                continue;
            }
            let cand_vx = base_vx + dx;
            let cand_vz = base_vz + dz;
            let cand_gps = voxel_column_gps(origin_voxel, cand_vx, cand_vz);
            let cand_dist = ecef_distance_m(requested, cand_gps);
            if cand_dist + 1e-6 < best_dist {
                best_vx = cand_vx;
                best_vz = cand_vz;
                best_gps = cand_gps;
                best_dist = cand_dist;
            }
        }
    }
    (best_vx, best_vz, best_gps, best_dist)
}

fn orthometric_height_to_voxel_y(
    origin_gps: &GPS,
    origin_voxel: &VoxelCoord,
    lat: f64,
    lon: f64,
    orthometric_height_m: f64,
) -> i64 {
    origin_voxel.y + (orthometric_height_m + egm96_undulation(lat, lon) - origin_gps.alt) as i64
}

fn surface_cache_to_orthometric_height(
    origin_gps: &GPS,
    origin_voxel: &VoxelCoord,
    lat: f64,
    lon: f64,
    surface_y_f: f64,
) -> f64 {
    surface_y_f - origin_voxel.y as f64 + origin_gps.alt - egm96_undulation(lat, lon)
}

fn sample_axis_positions(min: f64, max: f64, samples: u32) -> Vec<f64> {
    if samples <= 1 {
        return vec![(min + max) * 0.5];
    }
    let span = max - min;
    let step = span / samples as f64;
    (0..samples)
        .map(|i| min + (i as f64 + 0.5) * step)
        .collect()
}

struct SampledChunkSurface {
    column_lat: f64,
    column_lon: f64,
    generated_orthometric_m: f64,
    direct_source_orthometric_m: f64,
    requested_to_column_offset_m: f64,
}

fn sample_dem_with_fill(
    pipeline: &ElevationPipeline,
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
    step_deg: f64,
) -> RegionDem {
    let rows = ((lat_max - lat_min) / step_deg).ceil() as usize + 1;
    let cols = ((lon_max - lon_min) / step_deg).ceil() as usize + 1;
    let mut elevations = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        let lat = lat_min + r as f64 * step_deg;
        for c in 0..cols {
            let lon = lon_min + c as f64 * step_deg;
            let gps = GPS::new(lat, lon, 0.0);
            let h = pipeline
                .query_with_fill(&gps)
                .map(|e| e.meters as f32)
                .unwrap_or(0.0);
            elevations.push(h);
        }
    }
    RegionDem {
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

fn bbox_for_site(lat: f64, lon: f64, window_m: f64) -> (f64, f64, f64, f64) {
    let meters_per_degree_lat = 111_320.0;
    let meters_per_degree_lon = 111_320.0 * lat.to_radians().cos().abs().max(0.1);
    let half_lat = window_m / meters_per_degree_lat;
    let half_lon = window_m / meters_per_degree_lon;
    (
        lat - half_lat,
        lat + half_lat,
        lon - half_lon,
        lon + half_lon,
    )
}

fn collect_osm_features(
    cache: Option<&OsmDiskCache>,
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> (Vec<OsmWater>, Vec<OsmLandArea>) {
    let Some(cache) = cache else {
        return (Vec::new(), Vec::new());
    };

    let tile_lat_start = (lat_min / OSM_TILE_DEGREES).floor() as i64;
    let tile_lat_end = (lat_max / OSM_TILE_DEGREES).floor() as i64;
    let tile_lon_start = (lon_min / OSM_TILE_DEGREES).floor() as i64;
    let tile_lon_end = (lon_max / OSM_TILE_DEGREES).floor() as i64;

    let mut water = Vec::new();
    let mut land_areas = Vec::new();
    for tile_lat in tile_lat_start..=tile_lat_end {
        let lat = tile_lat as f64 * OSM_TILE_DEGREES;
        for tile_lon in tile_lon_start..=tile_lon_end {
            let lon = tile_lon as f64 * OSM_TILE_DEGREES;
            if let Some(tile) = cache.load(lat, lon, lat + OSM_TILE_DEGREES, lon + OSM_TILE_DEGREES)
            {
                water.extend(tile.water);
                land_areas.extend(tile.land_areas);
            }
        }
    }
    (water, land_areas)
}

fn terrain_surface_height_m(analysis: &TerrainAnalysis, lat: f64, lon: f64, source_m: f64) -> f64 {
    let Some((target_level, strength)) = analysis.engineered_ground_control_at(lat, lon) else {
        return source_m;
    };
    let blend = strength.clamp(0.0, 1.0) as f64;
    if blend <= 0.0 {
        source_m
    } else {
        source_m + (target_level as f64 - source_m) * blend
    }
}

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = (((sorted.len().saturating_sub(1)) as f64) * p).round() as usize;
    sorted[idx]
}

fn summarize(values: &[f64]) -> ScalarStats {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    };
    ScalarStats {
        min,
        max,
        mean,
        p95: percentile(values, 0.95),
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

fn blend_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> Rgb<u8> {
    Rgb([
        lerp(a[0], b[0], t),
        lerp(a[1], b[1], t),
        lerp(a[2], b[2], t),
    ])
}

fn hypsometric_color(value: f64, min: f64, max: f64) -> Rgb<u8> {
    if !value.is_finite() {
        return Rgb([20, 20, 20]);
    }
    let t = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0) as f32
    } else {
        0.5
    };
    if t < 0.25 {
        blend_rgb([18, 60, 32], [72, 120, 48], t / 0.25)
    } else if t < 0.5 {
        blend_rgb([72, 120, 48], [156, 154, 88], (t - 0.25) / 0.25)
    } else if t < 0.75 {
        blend_rgb([156, 154, 88], [170, 118, 74], (t - 0.5) / 0.25)
    } else {
        blend_rgb([170, 118, 74], [238, 238, 238], (t - 0.75) / 0.25)
    }
}

fn delta_color(value: f64, max_abs: f64) -> Rgb<u8> {
    if !value.is_finite() || max_abs <= f64::EPSILON {
        return Rgb([110, 110, 110]);
    }
    let t = (value / max_abs).clamp(-1.0, 1.0) as f32;
    if t < 0.0 {
        blend_rgb([20, 60, 160], [220, 220, 220], (t + 1.0).clamp(0.0, 1.0))
    } else {
        blend_rgb([220, 220, 220], [180, 35, 25], t)
    }
}

fn slope_color(value: f64) -> Rgb<u8> {
    let t = (value / 45.0).clamp(0.0, 1.0) as f32;
    if t < 0.5 {
        blend_rgb([45, 135, 55], [255, 220, 40], t / 0.5)
    } else {
        blend_rgb([255, 220, 40], [185, 40, 25], (t - 0.5) / 0.5)
    }
}

fn draw_center_cross(image: &mut RgbImage) {
    let cx = image.width() / 2;
    let cy = image.height() / 2;
    let white = Rgb([255, 255, 255]);
    for dx in -2..=2 {
        let x = cx as i32 + dx;
        if x >= 0 && x < image.width() as i32 {
            image.put_pixel(x as u32, cy, white);
        }
    }
    for dy in -2..=2 {
        let y = cy as i32 + dy;
        if y >= 0 && y < image.height() as i32 {
            image.put_pixel(cx, y as u32, white);
        }
    }
}

fn render_field_image<F>(width: u32, height: u32, values: &[f64], color_for: F) -> RgbImage
where
    F: Fn(f64) -> Rgb<u8>,
{
    let mut img = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let idx = y as usize * width as usize + x as usize;
            img.put_pixel(x, height - 1 - y, color_for(values[idx]));
        }
    }
    draw_center_cross(&mut img);
    img
}

fn compose_sheet(
    source: &RgbImage,
    generated: &RgbImage,
    delta: &RgbImage,
    slope: &RgbImage,
) -> RgbImage {
    let mut sheet = RgbImage::new(source.width() * 2, source.height() * 2);
    blit(&mut sheet, source, 0, 0);
    blit(&mut sheet, generated, source.width(), 0);
    blit(&mut sheet, delta, 0, source.height());
    blit(&mut sheet, slope, source.width(), source.height());
    sheet
}

fn blit(dst: &mut RgbImage, src: &RgbImage, x0: u32, y0: u32) {
    for y in 0..src.height() {
        for x in 0..src.width() {
            let p = *src.get_pixel(x, y);
            dst.put_pixel(x0 + x, y0 + y, p);
        }
    }
}

fn write_image(path: &Path, image: &RgbImage) -> Result<(), String> {
    image
        .save(path)
        .map_err(|e| format!("failed to save {}: {}", path.display(), e))
}

fn sanitize_filename(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn relative_to_output(path: &Path, output_dir: &Path) -> String {
    path.strip_prefix(output_dir)
        .unwrap_or(path)
        .display()
        .to_string()
}

struct BuiltAnalysis {
    analysis: TerrainAnalysis,
    osm_water_feature_count: usize,
    osm_land_area_feature_count: usize,
    osm_landuse_cell_count: usize,
    engineered_ground_cell_count: usize,
}

fn surface_mode_for_coverage(
    osm_water_feature_count: usize,
    osm_land_area_feature_count: usize,
    osm_landuse_cell_count: usize,
    engineered_ground_cell_count: usize,
) -> SurfaceMode {
    if engineered_ground_cell_count > 0 {
        SurfaceMode::EngineeredGroundActive
    } else if osm_water_feature_count > 0
        || osm_land_area_feature_count > 0
        || osm_landuse_cell_count > 0
    {
        SurfaceMode::OsmContextOnly
    } else {
        SurfaceMode::DemOnly
    }
}

fn reference_supports_point_delta(reference_status: Option<&str>) -> bool {
    !matches!(reference_status, Some(status) if status.ends_with("_context"))
}

fn build_analysis(
    pipeline: &ElevationPipeline,
    osm_cache: Option<&OsmDiskCache>,
    coastline: Option<&Path>,
    lat: f64,
    lon: f64,
    window_m: f64,
    image_size: u32,
) -> BuiltAnalysis {
    let (lat_min, lat_max, lon_min, lon_max) = bbox_for_site(lat, lon, window_m);
    let step_deg = ((lat_max - lat_min) / (image_size.saturating_sub(1).max(1) as f64)).max(1e-6);
    let dem = sample_dem_with_fill(pipeline, lat_min, lat_max, lon_min, lon_max, step_deg);
    let mut analysis = TerrainAnalysis::compute(dem);
    if let Some(gshhg) = coastline {
        if gshhg.exists() {
            analysis.compute_coastal_dist(gshhg);
        }
    }
    let (water, land_areas) = collect_osm_features(osm_cache, lat_min, lat_max, lon_min, lon_max);
    if !water.is_empty() {
        analysis.compute_reservoirs(&water);
    }
    if !land_areas.is_empty() {
        analysis.compute_osm_landuse(&land_areas);
        analysis.compute_engineered_ground(&land_areas);
    }
    let osm_landuse_cell_count = analysis
        .osm_landuse_mask
        .iter()
        .filter(|&&value| value != 0)
        .count();
    let engineered_ground_cell_count = analysis
        .engineered_ground_strength
        .iter()
        .filter(|&&value| value > 0.0)
        .count();
    BuiltAnalysis {
        analysis,
        osm_water_feature_count: water.len(),
        osm_land_area_feature_count: land_areas.len(),
        osm_landuse_cell_count,
        engineered_ground_cell_count,
    }
}

fn sample_generated_surface_at(
    pipeline: &ElevationPipeline,
    generator: &TerrainGenerator,
    chunk_cache: &mut HashMap<ChunkId, SurfaceCache>,
    origin_gps: &GPS,
    origin_voxel: &VoxelCoord,
    lat: f64,
    lon: f64,
    source_orthometric_m: f64,
) -> Result<SampledChunkSurface, String> {
    let (voxel_x, voxel_z, column_gps, requested_to_column_offset_m) =
        snap_gps_to_voxel_xz(origin_voxel, lat, lon);
    let direct_source_orthometric_m = pipeline
        .query_with_fill(&GPS::new(column_gps.lat, column_gps.lon, 0.0))
        .map(|e| e.meters)
        .unwrap_or(source_orthometric_m);
    let approx_surface_voxel_y = orthometric_height_to_voxel_y(
        origin_gps,
        origin_voxel,
        column_gps.lat,
        column_gps.lon,
        direct_source_orthometric_m,
    );
    let chunk_id = ChunkId::from_voxel(&VoxelCoord::new(voxel_x, approx_surface_voxel_y, voxel_z));
    if !chunk_cache.contains_key(&chunk_id) {
        let (_, surface_cache) = generator
            .generate_chunk(&chunk_id)
            .map_err(|e| format!("failed to generate chunk {:?}: {}", chunk_id, e))?;
        chunk_cache.insert(chunk_id, surface_cache);
    }
    let surface_y_f = chunk_cache
        .get(&chunk_id)
        .and_then(|cache| cache.get(&(voxel_x, voxel_z)).copied())
        .ok_or_else(|| {
            format!(
                "generated chunk {:?} missing surface cache for column ({}, {})",
                chunk_id, voxel_x, voxel_z
            )
        })?;
    let generated_orthometric_m = surface_cache_to_orthometric_height(
        origin_gps,
        origin_voxel,
        column_gps.lat,
        column_gps.lon,
        surface_y_f,
    );
    Ok(SampledChunkSurface {
        column_lat: column_gps.lat,
        column_lon: column_gps.lon,
        generated_orthometric_m,
        direct_source_orthometric_m,
        requested_to_column_offset_m,
    })
}

fn evaluate_chunk_surface_sampling(
    analysis: &Arc<TerrainAnalysis>,
    site: &ControlSite,
    atlas: &ControlAtlas,
    args: &Args,
) -> Result<Option<ChunkSurfaceMetrics>, String> {
    let Some(sample_grid_size) = args.chunk_sample_grid.filter(|&n| n > 0) else {
        return Ok(None);
    };

    let window_m = site.window_m.unwrap_or(atlas.default_window_m);
    let (lat_min, lat_max, lon_min, lon_max) = bbox_for_site(site.lat, site.lon, window_m);
    let sample_lats = sample_axis_positions(lat_min, lat_max, sample_grid_size);
    let sample_lons = sample_axis_positions(lon_min, lon_max, sample_grid_size);

    if let Some(dir) = &args.cop30_dir {
        if !dir.exists() {
            return Err(format!("--cop30-dir {} does not exist", dir.display()));
        }
    }

    let pipeline = make_elevation_pipeline(&args.srtm_dir, args.cop30_dir.as_deref());
    let (origin_gps, origin_voxel) = control_origin(&pipeline, site.lat, site.lon);
    let generator = TerrainGenerator::new(pipeline, origin_gps, origin_voxel)
        .without_vegetation()
        .with_analysis(Arc::clone(analysis));
    let generator_pipeline = generator.elevation_pipeline();

    let mut chunk_cache = HashMap::new();
    let mut generated_minus_sampled_source = Vec::new();
    let mut generated_minus_sampled_fast = Vec::new();
    let mut requested_to_column_offset = Vec::new();
    for lat in sample_lats {
        for lon in &sample_lons {
            let source = analysis.dem.at_latlon(lat, *lon) as f64;
            let pipeline_guard = generator_pipeline.read().unwrap();
            let chunk_sample = sample_generated_surface_at(
                &pipeline_guard,
                &generator,
                &mut chunk_cache,
                &origin_gps,
                &origin_voxel,
                lat,
                *lon,
                source,
            )?;
            let sampled_fast_surface = terrain_surface_height_m(
                analysis.as_ref(),
                chunk_sample.column_lat,
                chunk_sample.column_lon,
                chunk_sample.direct_source_orthometric_m,
            );
            generated_minus_sampled_source.push(
                chunk_sample.generated_orthometric_m - chunk_sample.direct_source_orthometric_m,
            );
            generated_minus_sampled_fast
                .push(chunk_sample.generated_orthometric_m - sampled_fast_surface);
            requested_to_column_offset.push(chunk_sample.requested_to_column_offset_m);
        }
    }

    let center_source = analysis.dem.at_latlon(site.lat, site.lon) as f64;
    let pipeline_guard = generator_pipeline.read().unwrap();
    let center_sample = sample_generated_surface_at(
        &pipeline_guard,
        &generator,
        &mut chunk_cache,
        &origin_gps,
        &origin_voxel,
        site.lat,
        site.lon,
        center_source,
    )?;
    let center_sampled_fast_surface = terrain_surface_height_m(
        analysis.as_ref(),
        center_sample.column_lat,
        center_sample.column_lon,
        center_sample.direct_source_orthometric_m,
    );
    let generated_minus_sampled_source_abs: Vec<f64> = generated_minus_sampled_source
        .iter()
        .map(|value| value.abs())
        .collect();
    let generated_minus_sampled_fast_abs: Vec<f64> = generated_minus_sampled_fast
        .iter()
        .map(|value| value.abs())
        .collect();

    Ok(Some(ChunkSurfaceMetrics {
        sample_grid_size,
        sample_count: generated_minus_sampled_source.len(),
        generated_chunk_count: chunk_cache.len(),
        center_orthometric_alt_m: center_sample.generated_orthometric_m,
        center_column_lat: center_sample.column_lat,
        center_column_lon: center_sample.column_lon,
        center_sampled_source_orthometric_alt_m: center_sample.direct_source_orthometric_m,
        center_sampled_fast_surface_orthometric_alt_m: center_sampled_fast_surface,
        center_minus_sampled_source_m: center_sample.generated_orthometric_m
            - center_sample.direct_source_orthometric_m,
        center_minus_sampled_fast_surface_m: center_sample.generated_orthometric_m
            - center_sampled_fast_surface,
        center_requested_to_column_offset_m: center_sample.requested_to_column_offset_m,
        generated_minus_sampled_source_m: summarize(&generated_minus_sampled_source),
        generated_minus_sampled_source_abs_p95_m: percentile(
            &generated_minus_sampled_source_abs,
            0.95,
        ),
        generated_minus_sampled_source_abs_max_m: generated_minus_sampled_source_abs
            .iter()
            .copied()
            .fold(0.0, f64::max),
        generated_minus_sampled_fast_surface_m: summarize(&generated_minus_sampled_fast),
        generated_minus_sampled_fast_surface_abs_p95_m: percentile(
            &generated_minus_sampled_fast_abs,
            0.95,
        ),
        generated_minus_sampled_fast_surface_abs_max_m: generated_minus_sampled_fast_abs
            .iter()
            .copied()
            .fold(0.0, f64::max),
        requested_to_column_offset_m: summarize(&requested_to_column_offset),
    }))
}

fn evaluate_control(
    category: &ControlCategory,
    site: &ControlSite,
    atlas: &ControlAtlas,
    args: &Args,
    output_dir: &Path,
    pipeline: &ElevationPipeline,
    osm_cache: Option<&OsmDiskCache>,
) -> Result<ControlReport, String> {
    let window_m = site.window_m.unwrap_or(atlas.default_window_m);
    let image_size = site
        .image_size
        .or(args.image_size)
        .unwrap_or(atlas.default_image_size);
    let built = build_analysis(
        pipeline,
        osm_cache,
        args.coastline.as_deref(),
        site.lat,
        site.lon,
        window_m,
        image_size,
    );
    let BuiltAnalysis {
        analysis,
        osm_water_feature_count,
        osm_land_area_feature_count,
        osm_landuse_cell_count,
        engineered_ground_cell_count,
    } = built;
    let analysis = Arc::new(analysis);

    let mut source_values = Vec::with_capacity((analysis.dem.rows * analysis.dem.cols) as usize);
    let mut generated_values = Vec::with_capacity((analysis.dem.rows * analysis.dem.cols) as usize);
    let mut delta_values = Vec::with_capacity((analysis.dem.rows * analysis.dem.cols) as usize);
    let mut slope_values = Vec::with_capacity((analysis.dem.rows * analysis.dem.cols) as usize);
    let mut tri_values = Vec::with_capacity((analysis.dem.rows * analysis.dem.cols) as usize);
    let mut twi_values = Vec::with_capacity((analysis.dem.rows * analysis.dem.cols) as usize);

    for r in 0..analysis.dem.rows {
        let lat = analysis.dem.lat_min + r as f64 * analysis.dem.cell_size_deg;
        for c in 0..analysis.dem.cols {
            let lon = analysis.dem.lon_min + c as f64 * analysis.dem.cell_size_deg;
            let source = analysis.dem.get(r, c) as f64;
            let generated = terrain_surface_height_m(analysis.as_ref(), lat, lon, source);
            source_values.push(source);
            generated_values.push(generated);
            delta_values.push(generated - source);
            slope_values.push(analysis.slope_deg[r * analysis.dem.cols + c] as f64);
            tri_values.push(analysis.tri[r * analysis.dem.cols + c] as f64);
            twi_values.push(analysis.twi[r * analysis.dem.cols + c] as f64);
        }
    }

    let center_source = analysis.dem.at_latlon(site.lat, site.lon) as f64;
    let center_generated =
        terrain_surface_height_m(analysis.as_ref(), site.lat, site.lon, center_source);
    let engineered = analysis.engineered_ground_control_at(site.lat, site.lon);
    let reference_supports_delta = reference_supports_point_delta(site.reference_status.as_deref());
    let source_minus_reference = site
        .reference_orthometric_alt_m
        .filter(|_| reference_supports_delta)
        .map(|reference| center_source - reference);
    let generated_minus_reference = site
        .reference_orthometric_alt_m
        .filter(|_| reference_supports_delta)
        .map(|reference| center_generated - reference);

    let source_stats = summarize(&source_values);
    let generated_stats = summarize(&generated_values);
    let delta_abs: Vec<f64> = delta_values.iter().map(|v| v.abs()).collect();
    let generated_changed_cell_count = delta_abs.iter().filter(|&&value| value > 1e-6).count();
    let generated_changed_fraction = if delta_values.is_empty() {
        0.0
    } else {
        generated_changed_cell_count as f64 / delta_values.len() as f64
    };
    let delta_stats = summarize(&delta_values);
    let slope_stats = summarize(&slope_values);
    let tri_stats = summarize(&tri_values);
    let twi_stats = summarize(&twi_values);
    let surface_mode = surface_mode_for_coverage(
        osm_water_feature_count,
        osm_land_area_feature_count,
        osm_landuse_cell_count,
        engineered_ground_cell_count,
    );
    let chunk_surface = evaluate_chunk_surface_sampling(&analysis, site, atlas, args)?;

    let category_dir = output_dir.join(&category.id);
    fs::create_dir_all(&category_dir)
        .map_err(|e| format!("failed to create {}: {}", category_dir.display(), e))?;
    let site_dir = category_dir.join(sanitize_filename(&site.id));
    fs::create_dir_all(&site_dir)
        .map_err(|e| format!("failed to create {}: {}", site_dir.display(), e))?;

    let file_stem = sanitize_filename(&site.id);
    let mut artifacts = ControlArtifacts {
        source_height_png: None,
        generated_height_png: None,
        delta_png: None,
        slope_png: None,
        sheet_png: None,
        site_json: String::new(),
    };

    if !args.skip_images {
        let min_h = source_stats.min.min(generated_stats.min);
        let max_h = source_stats.max.max(generated_stats.max);
        let max_abs_delta = delta_abs.iter().copied().fold(0.0, f64::max).max(0.01);
        let source_img = render_field_image(
            analysis.dem.cols as u32,
            analysis.dem.rows as u32,
            &source_values,
            |v| hypsometric_color(v, min_h, max_h),
        );
        let generated_img = render_field_image(
            analysis.dem.cols as u32,
            analysis.dem.rows as u32,
            &generated_values,
            |v| hypsometric_color(v, min_h, max_h),
        );
        let delta_img = render_field_image(
            analysis.dem.cols as u32,
            analysis.dem.rows as u32,
            &delta_values,
            |v| delta_color(v, max_abs_delta),
        );
        let slope_img = render_field_image(
            analysis.dem.cols as u32,
            analysis.dem.rows as u32,
            &slope_values,
            slope_color,
        );
        let sheet = compose_sheet(&source_img, &generated_img, &delta_img, &slope_img);

        let source_path = site_dir.join(format!("{}-source.png", file_stem));
        let generated_path = site_dir.join(format!("{}-generated.png", file_stem));
        let delta_path = site_dir.join(format!("{}-delta.png", file_stem));
        let slope_path = site_dir.join(format!("{}-slope.png", file_stem));
        let sheet_path = site_dir.join(format!("{}-sheet.png", file_stem));
        write_image(&source_path, &source_img)?;
        write_image(&generated_path, &generated_img)?;
        write_image(&delta_path, &delta_img)?;
        write_image(&slope_path, &slope_img)?;
        write_image(&sheet_path, &sheet)?;
        artifacts.source_height_png = Some(relative_to_output(&source_path, output_dir));
        artifacts.generated_height_png = Some(relative_to_output(&generated_path, output_dir));
        artifacts.delta_png = Some(relative_to_output(&delta_path, output_dir));
        artifacts.slope_png = Some(relative_to_output(&slope_path, output_dir));
        artifacts.sheet_png = Some(relative_to_output(&sheet_path, output_dir));
    }

    let report = ControlReport {
        id: site.id.clone(),
        label: site.label.clone(),
        category_id: category.id.clone(),
        category_name: category.name.clone(),
        lat: site.lat,
        lon: site.lon,
        window_m,
        image_size,
        tags: site.tags.clone(),
        reference_source: site.reference_source.clone(),
        reference_status: site.reference_status.clone(),
        reference_notes: site.reference_notes.clone(),
        coverage: CoverageMetrics {
            surface_mode,
            osm_water_feature_count,
            osm_land_area_feature_count,
            osm_landuse_cell_count,
            engineered_ground_cell_count,
        },
        chunk_surface,
        center: CenterMetrics {
            source_orthometric_alt_m: center_source,
            generated_orthometric_alt_m: center_generated,
            generated_minus_source_m: center_generated - center_source,
            slope_deg: analysis.slope_at(site.lat, site.lon),
            tri_m: analysis.tri_at(site.lat, site.lon),
            twi: analysis.twi_at(site.lat, site.lon),
            coastal_dist_m: analysis.coastal_dist_at(site.lat, site.lon),
            osm_landuse: analysis
                .osm_landuse_at(site.lat, site.lon)
                .map(|landuse| format!("{:?}", landuse)),
            engineered_ground_target_m: engineered.map(|(target, _)| target),
            engineered_ground_strength: engineered.map(|(_, strength)| strength),
            reference_orthometric_alt_m: site.reference_orthometric_alt_m,
            source_minus_reference_m: source_minus_reference,
            generated_minus_reference_m: generated_minus_reference,
        },
        grid: GridMetrics {
            source_elevation_m: source_stats,
            generated_elevation_m: generated_stats,
            generated_minus_source_m: delta_stats,
            generated_minus_source_abs_p95_m: percentile(&delta_abs, 0.95),
            generated_minus_source_abs_max_m: delta_abs.iter().copied().fold(0.0, f64::max),
            generated_changed_cell_count,
            generated_changed_fraction,
            slope_deg: slope_stats,
            tri_m: tri_stats,
            twi: twi_stats,
        },
        artifacts,
    };

    let site_json_path = site_dir.join(format!("{}-report.json", file_stem));
    fs::write(
        &site_json_path,
        serde_json::to_string_pretty(&report).map_err(|e| format!("report json: {}", e))?,
    )
    .map_err(|e| format!("failed to write {}: {}", site_json_path.display(), e))?;

    let mut report = report;
    report.artifacts.site_json = relative_to_output(&site_json_path, output_dir);
    Ok(report)
}

fn aggregate_category(controls: &[ControlReport]) -> CategoryAggregate {
    let source_spans: Vec<f64> = controls
        .iter()
        .map(|c| c.grid.source_elevation_m.max - c.grid.source_elevation_m.min)
        .collect();
    let generated_spans: Vec<f64> = controls
        .iter()
        .map(|c| c.grid.generated_elevation_m.max - c.grid.generated_elevation_m.min)
        .collect();
    let delta_p95: Vec<f64> = controls
        .iter()
        .map(|c| c.grid.generated_minus_source_abs_p95_m)
        .collect();
    let slope_p95: Vec<f64> = controls.iter().map(|c| c.grid.slope_deg.p95).collect();
    let controls_with_osm_context = controls
        .iter()
        .filter(|c| {
            c.coverage.osm_water_feature_count > 0
                || c.coverage.osm_land_area_feature_count > 0
                || c.coverage.osm_landuse_cell_count > 0
        })
        .count();
    let controls_with_engineered_ground = controls
        .iter()
        .filter(|c| c.coverage.engineered_ground_cell_count > 0)
        .count();
    let controls_with_surface_delta = controls
        .iter()
        .filter(|c| c.grid.generated_changed_cell_count > 0)
        .count();
    let dem_only_controls = controls
        .iter()
        .filter(|c| c.coverage.surface_mode == SurfaceMode::DemOnly)
        .count();
    CategoryAggregate {
        control_count: controls.len(),
        source_span_mean_m: if source_spans.is_empty() {
            0.0
        } else {
            source_spans.iter().sum::<f64>() / source_spans.len() as f64
        },
        generated_span_mean_m: if generated_spans.is_empty() {
            0.0
        } else {
            generated_spans.iter().sum::<f64>() / generated_spans.len() as f64
        },
        delta_abs_p95_mean_m: if delta_p95.is_empty() {
            0.0
        } else {
            delta_p95.iter().sum::<f64>() / delta_p95.len() as f64
        },
        slope_p95_mean_deg: if slope_p95.is_empty() {
            0.0
        } else {
            slope_p95.iter().sum::<f64>() / slope_p95.len() as f64
        },
        controls_with_osm_context,
        controls_with_engineered_ground,
        controls_with_surface_delta,
        dem_only_controls,
    }
}

fn build_summary_csv(report: &AtlasReport) -> String {
    let mut csv = String::from(
        "category_id,category_name,control_id,label,lat,lon,window_m,surface_mode,osm_water_features,osm_land_areas,osm_landuse_cells,engineered_ground_cells,changed_cells,changed_fraction,chunk_sample_grid,chunk_sample_count,chunk_generated_chunks,chunk_center_m,chunk_center_column_lat,chunk_center_column_lon,chunk_center_offset_m,chunk_center_minus_sampled_source_m,chunk_center_minus_sampled_fast_m,chunk_delta_sampled_source_p95_abs_m,chunk_delta_sampled_source_max_abs_m,chunk_delta_sampled_fast_p95_abs_m,chunk_delta_sampled_fast_max_abs_m,reference_alt_m,source_center_m,generated_center_m,generated_minus_source_m,source_span_m,generated_span_m,delta_p95_abs_m,delta_max_abs_m,slope_p95_deg,site_json\n",
    );
    for category in &report.categories {
        for control in &category.controls {
            let chunk_sample_grid = control
                .chunk_surface
                .as_ref()
                .map(|value| value.sample_grid_size.to_string())
                .unwrap_or_default();
            let chunk_sample_count = control
                .chunk_surface
                .as_ref()
                .map(|value| value.sample_count.to_string())
                .unwrap_or_default();
            let chunk_generated_chunks = control
                .chunk_surface
                .as_ref()
                .map(|value| value.generated_chunk_count.to_string())
                .unwrap_or_default();
            let chunk_center = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.3}", value.center_orthometric_alt_m))
                .unwrap_or_default();
            let chunk_center_column_lat = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.6}", value.center_column_lat))
                .unwrap_or_default();
            let chunk_center_column_lon = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.6}", value.center_column_lon))
                .unwrap_or_default();
            let chunk_center_offset = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.3}", value.center_requested_to_column_offset_m))
                .unwrap_or_default();
            let chunk_center_minus_source = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.3}", value.center_minus_sampled_source_m))
                .unwrap_or_default();
            let chunk_center_minus_fast = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.3}", value.center_minus_sampled_fast_surface_m))
                .unwrap_or_default();
            let chunk_delta_p95 = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.3}", value.generated_minus_sampled_source_abs_p95_m))
                .unwrap_or_default();
            let chunk_delta_max = control
                .chunk_surface
                .as_ref()
                .map(|value| format!("{:.3}", value.generated_minus_sampled_source_abs_max_m))
                .unwrap_or_default();
            let chunk_fast_delta_p95 = control
                .chunk_surface
                .as_ref()
                .map(|value| {
                    format!(
                        "{:.3}",
                        value.generated_minus_sampled_fast_surface_abs_p95_m
                    )
                })
                .unwrap_or_default();
            let chunk_fast_delta_max = control
                .chunk_surface
                .as_ref()
                .map(|value| {
                    format!(
                        "{:.3}",
                        value.generated_minus_sampled_fast_surface_abs_max_m
                    )
                })
                .unwrap_or_default();
            let _ = writeln!(
                csv,
                "{},{},{},{},{:.6},{:.6},{:.1},{},{},{},{},{},{},{:.6},{},{},{},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{}",
                category.id,
                category.name,
                control.id,
                control.label.replace(',', " "),
                control.lat,
                control.lon,
                control.window_m,
                control.coverage.surface_mode.as_str(),
                control.coverage.osm_water_feature_count,
                control.coverage.osm_land_area_feature_count,
                control.coverage.osm_landuse_cell_count,
                control.coverage.engineered_ground_cell_count,
                control.grid.generated_changed_cell_count,
                control.grid.generated_changed_fraction,
                chunk_sample_grid,
                chunk_sample_count,
                chunk_generated_chunks,
                chunk_center,
                chunk_center_column_lat,
                chunk_center_column_lon,
                chunk_center_offset,
                chunk_center_minus_source,
                chunk_center_minus_fast,
                chunk_delta_p95,
                chunk_delta_max,
                chunk_fast_delta_p95,
                chunk_fast_delta_max,
                control
                    .center
                    .reference_orthometric_alt_m
                    .map(|v| format!("{:.3}", v))
                    .unwrap_or_default(),
                control.center.source_orthometric_alt_m,
                control.center.generated_orthometric_alt_m,
                control.center.generated_minus_source_m,
                control.grid.source_elevation_m.max - control.grid.source_elevation_m.min,
                control.grid.generated_elevation_m.max - control.grid.generated_elevation_m.min,
                control.grid.generated_minus_source_abs_p95_m,
                control.grid.generated_minus_source_abs_max_m,
                control.grid.slope_deg.p95,
                control.artifacts.site_json,
            );
        }
    }
    csv
}

fn main() -> Result<(), String> {
    let args = Args::parse();
    let atlas_raw = fs::read_to_string(&args.atlas)
        .map_err(|e| format!("failed to read atlas {}: {}", args.atlas.display(), e))?;
    let atlas: ControlAtlas = serde_json::from_str(&atlas_raw)
        .map_err(|e| format!("failed to parse atlas {}: {}", args.atlas.display(), e))?;

    fs::create_dir_all(&args.output)
        .map_err(|e| format!("failed to create {}: {}", args.output.display(), e))?;

    let pipeline = make_elevation_pipeline(&args.srtm_dir, args.cop30_dir.as_deref());

    let osm_cache = if args.osm_cache.exists() {
        Some(OsmDiskCache::new(&args.osm_cache))
    } else {
        None
    };

    let category_filter: Option<BTreeMap<String, ()>> = if args.categories.is_empty() {
        None
    } else {
        Some(
            args.categories
                .iter()
                .map(|value| (value.trim().to_string(), ()))
                .collect(),
        )
    };

    let mut warnings = Vec::new();
    let mut category_reports = Vec::new();

    for category in &atlas.categories {
        if let Some(filter) = &category_filter {
            if !filter.contains_key(&category.id) {
                continue;
            }
        }

        if category.controls.len() < category.expected_min_controls {
            warnings.push(format!(
                "category '{}' has {} controls; expected at least {}",
                category.id,
                category.controls.len(),
                category.expected_min_controls
            ));
        }

        let limit = args.max_per_category.unwrap_or(category.controls.len());
        let mut control_reports = Vec::new();
        for site in category.controls.iter().take(limit) {
            eprintln!(
                "[terrain_controls] {} / {} ({:.6}, {:.6})",
                category.id, site.id, site.lat, site.lon
            );
            control_reports.push(evaluate_control(
                category,
                site,
                &atlas,
                &args,
                &args.output,
                &pipeline,
                osm_cache.as_ref(),
            )?);
        }

        let aggregate = aggregate_category(&control_reports);
        if aggregate.controls_with_surface_delta == 0 && aggregate.control_count > 0 {
            warnings.push(format!(
                "category '{}' produced no generated-vs-source delta; sampled controls currently resolve to the source DEM",
                category.id
            ));
        }
        if aggregate.dem_only_controls == aggregate.control_count && aggregate.control_count > 0 {
            warnings.push(format!(
                "category '{}' had no cached OSM context in any sampled window; controls are DEM-only",
                category.id
            ));
        }

        category_reports.push(CategoryReport {
            id: category.id.clone(),
            name: category.name.clone(),
            description: category.description.clone(),
            expected_min_controls: category.expected_min_controls,
            aggregate,
            controls: control_reports,
        });
    }

    let report = AtlasReport {
        atlas_name: atlas.name.clone(),
        atlas_description: atlas.description.clone(),
        generated_at_unix_ms: unix_timestamp_millis(),
        args: std::env::args().collect(),
        warnings,
        categories: category_reports,
    };

    let report_path = args.output.join("report.json");
    fs::write(
        &report_path,
        serde_json::to_string_pretty(&report).map_err(|e| format!("report json: {}", e))?,
    )
    .map_err(|e| format!("failed to write {}: {}", report_path.display(), e))?;

    let csv_path = args.output.join("summary.csv");
    fs::write(&csv_path, build_summary_csv(&report))
        .map_err(|e| format!("failed to write {}: {}", csv_path.display(), e))?;

    eprintln!(
        "[terrain_controls] wrote {} and {}",
        report_path.display(),
        csv_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        SurfaceMode, bbox_for_site, delta_color, ecef_distance_m, gps_to_voxel_xz,
        orthometric_height_to_voxel_y, percentile, reference_supports_point_delta,
        sample_axis_positions, sanitize_filename, slope_color, snap_gps_to_voxel_xz,
        surface_cache_to_orthometric_height, surface_mode_for_coverage, voxel_column_gps,
    };
    use metaverse_core::coordinates::GPS;
    use metaverse_core::voxel::VoxelCoord;

    #[test]
    fn sanitize_keeps_safe_characters() {
        assert_eq!(sanitize_filename("bondi-beach_01"), "bondi-beach_01");
        assert_eq!(sanitize_filename("Bondi Beach!"), "Bondi-Beach-");
    }

    #[test]
    fn delta_color_distinguishes_sign() {
        let neg = delta_color(-5.0, 5.0);
        let pos = delta_color(5.0, 5.0);
        assert!(neg[2] > neg[0]);
        assert!(pos[0] > pos[2]);
    }

    #[test]
    fn slope_color_warms_with_steepness() {
        let flat = slope_color(0.0);
        let steep = slope_color(45.0);
        assert!(flat[1] > flat[0]);
        assert!(steep[0] >= steep[1]);
    }

    #[test]
    fn surface_mode_classifies_dem_only_windows() {
        assert_eq!(surface_mode_for_coverage(0, 0, 0, 0), SurfaceMode::DemOnly);
    }

    #[test]
    fn surface_mode_classifies_osm_context_without_engineered_ground() {
        assert_eq!(
            surface_mode_for_coverage(0, 2, 5, 0),
            SurfaceMode::OsmContextOnly
        );
    }

    #[test]
    fn surface_mode_classifies_engineered_ground_windows() {
        assert_eq!(
            surface_mode_for_coverage(0, 2, 5, 1),
            SurfaceMode::EngineeredGroundActive
        );
    }

    #[test]
    fn reference_supports_point_delta_rejects_context_only_statuses() {
        assert!(reference_supports_point_delta(Some(
            "public_airfield_elevation"
        )));
        assert!(reference_supports_point_delta(None));
        assert!(!reference_supports_point_delta(Some("coastal_msl_context")));
    }

    #[test]
    fn sample_axis_positions_use_cell_centers() {
        let positions = sample_axis_positions(10.0, 20.0, 4);
        assert_eq!(positions, vec![11.25, 13.75, 16.25, 18.75]);
    }

    #[test]
    fn surface_cache_height_round_trips_orthometric_conversion() {
        let origin_gps = GPS::new(-26.193298, 152.659020, 63.0);
        let origin_voxel = VoxelCoord::new(1000, 2000, 3000);
        let lat = -26.193298;
        let lon = 152.659020;
        let orthometric = 52.125;
        let voxel_y =
            orthometric_height_to_voxel_y(&origin_gps, &origin_voxel, lat, lon, orthometric);
        let round_trip = surface_cache_to_orthometric_height(
            &origin_gps,
            &origin_voxel,
            lat,
            lon,
            voxel_y as f64,
        );
        assert!((round_trip - orthometric).abs() < 1.0);
    }

    #[test]
    fn snap_gps_to_voxel_xz_never_worsens_column_distance() {
        let origin_voxel = VoxelCoord::new(0, 1, 0);
        let (vx, vz) = gps_to_voxel_xz(50.8194, -0.1360);
        let initial_gps = voxel_column_gps(&origin_voxel, vx, vz);
        let initial_dist = ecef_distance_m(GPS::new(50.8194, -0.1360, 0.0), initial_gps);
        let (_svx, _svz, _sgps, snapped_dist) =
            snap_gps_to_voxel_xz(&origin_voxel, 50.8194, -0.1360);
        assert!(snapped_dist <= initial_dist + 1e-6);
    }

    #[test]
    fn snap_gps_to_voxel_xz_handles_brighton_and_heathrow_windows() {
        for (site_lat, site_lon, expected_p95_max_m) in
            [(50.8194, -0.1360, 20.0), (51.4700, -0.4543, 15.0)]
        {
            let origin_voxel = VoxelCoord::from_ecef(&GPS::new(site_lat, site_lon, 0.0).to_ecef());
            let (lat_min, lat_max, lon_min, lon_max) = bbox_for_site(site_lat, site_lon, 240.0);
            let sample_lats = sample_axis_positions(lat_min, lat_max, 5);
            let sample_lons = sample_axis_positions(lon_min, lon_max, 5);
            let mut offsets = Vec::new();
            for lat in sample_lats {
                for lon in &sample_lons {
                    let (_, _, _, snapped_dist) = snap_gps_to_voxel_xz(&origin_voxel, lat, *lon);
                    offsets.push(snapped_dist);
                }
            }
            assert!(percentile(&offsets, 0.95) <= expected_p95_max_m);
        }
    }
}
