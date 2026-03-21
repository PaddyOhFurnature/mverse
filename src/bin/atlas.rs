use clap::{Parser, Subcommand};
use metaverse_core::control_pack_merge::{MergeControlPackOptions, run_merge_control_pack};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn resolve_repo_relative(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root().join(path)
    }
}

fn parse_csv(value: &Option<String>) -> Vec<String> {
    value
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn slugify(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        fallback.to_string()
    } else {
        slug
    }
}

fn round_to(value: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    (value * scale).round() / scale
}

#[derive(Debug, Deserialize)]
struct ControlAtlas {
    #[serde(default = "default_window_m")]
    default_window_m: f64,
    #[serde(default)]
    categories: Vec<ControlCategory>,
}

fn default_window_m() -> f64 {
    240.0
}

#[derive(Debug, Deserialize)]
struct ControlCategory {
    #[serde(default)]
    id: String,
    #[serde(default)]
    controls: Vec<ControlSite>,
}

#[derive(Debug, Deserialize)]
struct ControlSite {
    id: String,
    #[serde(default)]
    label: String,
    lat: f64,
    lon: f64,
}

fn load_atlas(path: &Path) -> Result<ControlAtlas, String> {
    let atlas_text = fs::read_to_string(path)
        .map_err(|e| format!("failed to read atlas {}: {}", path.display(), e))?;
    serde_json::from_str(&atlas_text)
        .map_err(|e| format!("failed to parse atlas {}: {}", path.display(), e))
}

fn iter_selected_controls<'a>(
    atlas: &'a ControlAtlas,
    control_ids: &BTreeSet<String>,
    category_ids: &BTreeSet<String>,
) -> Vec<(&'a str, &'a ControlSite)> {
    let mut selected = Vec::new();
    for category in &atlas.categories {
        if !category_ids.is_empty() && !category_ids.contains(&category.id) {
            continue;
        }
        for control in &category.controls {
            if !control_ids.is_empty() && !control_ids.contains(&control.id) {
                continue;
            }
            selected.push((category.id.as_str(), control));
        }
    }
    selected
}

#[derive(Parser, Debug)]
#[command(name = "atlas")]
#[command(about = "Rust-first atlas workflow helpers")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate numbered teleport points JSON from the control atlas.
    TeleportPoints(TeleportPointsArgs),
    /// Generate capture-route JSON for control-atlas flyby/orbit inspection.
    FlybyRoute(FlybyRouteArgs),
    /// Merge per-site control pack TileStores into a single global tiles.db.
    #[command(alias = "merge")]
    MergePack(MergePackArgs),
}

#[derive(clap::Args, Debug)]
struct TeleportPointsArgs {
    #[arg(long, default_value = "scripts/control_atlas/global_terrain_controls.json")]
    atlas: PathBuf,
    #[arg(long, default_value = "scripts/teleport_points/control-teleports.json")]
    output: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    controls: Option<String>,
    #[arg(long)]
    categories: Option<String>,
    #[arg(long, default_value_t = 0)]
    limit: usize,
    #[arg(long, default_value_t = 10.0)]
    ground_offset_m: f64,
    #[arg(long, default_value_t = 80.0)]
    staging_altitude_m: f64,
    #[arg(long, default_value_t = 12)]
    min_loaded_chunks: usize,
    #[arg(long, default_value_t = 30)]
    loading_timeout_secs: usize,
    #[arg(long, default_value_t = 180.0)]
    yaw_deg: f64,
    #[arg(long, default_value_t = -14.0)]
    pitch_deg: f64,
    #[arg(long, default_value_t = false)]
    prefer_water: bool,
}

#[derive(clap::Args, Debug)]
struct FlybyRouteArgs {
    #[arg(long, default_value = "scripts/control_atlas/global_terrain_controls.json")]
    atlas: PathBuf,
    #[arg(long, default_value = "scripts/capture_routes/control_flyby.json")]
    output: PathBuf,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    controls: Option<String>,
    #[arg(long)]
    categories: Option<String>,
    #[arg(long, default_value = "beauty")]
    views: String,
    #[arg(long)]
    orbit_radius_m: Option<f64>,
    #[arg(long, default_value_t = 10.0)]
    ground_offset_m: f64,
    #[arg(long)]
    topdown_ground_offset_m: Option<f64>,
    #[arg(long, default_value_t = -18.0)]
    oblique_pitch_deg: f64,
    #[arg(long, default_value_t = -70.0)]
    topdown_pitch_deg: f64,
    #[arg(long, default_value_t = 80.0)]
    staging_altitude_m: f64,
    #[arg(long, default_value_t = 15)]
    settle_frames: usize,
    #[arg(long, default_value_t = 12)]
    min_loaded_chunks: usize,
    #[arg(long, default_value_t = 90)]
    loading_timeout_secs: usize,
    #[arg(long, default_value_t = false)]
    skip_topdown: bool,
}

#[derive(clap::Args, Debug)]
struct MergePackArgs {
    /// Root containing one directory per site world (each with tiles.db + manifest.json)
    #[arg(long, default_value = "world_data/terrain-proof-worlds/control-atlas-pack")]
    pack_root: PathBuf,
    /// Destination global TileStore path
    #[arg(long, default_value = "world_data/tiles.db")]
    dest_db: PathBuf,
    /// Optional comma-separated subset of site ids to merge
    #[arg(long, value_delimiter = ',')]
    site_ids: Vec<String>,
    /// Optional limit on how many discovered sites to merge
    #[arg(long)]
    limit_sites: Option<usize>,
    /// Skip copying OSM tiles from each site store
    #[arg(long)]
    skip_osm: bool,
    /// Only report the pending merge; do not modify the destination DB
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct TeleportPointOut<'a> {
    id: &'a str,
    label: &'a str,
    lat: f64,
    lon: f64,
    yaw_deg: f64,
    pitch_deg: f64,
    prefer_water: bool,
    category_id: &'a str,
}

#[derive(Debug, Serialize)]
struct TeleportPayload<'a> {
    name: String,
    ground_offset_m: f64,
    staging_altitude_m: f64,
    min_loaded_chunks: usize,
    loading_timeout_secs: usize,
    points: Vec<TeleportPointOut<'a>>,
}

#[derive(Debug, Serialize)]
struct FlybyPointOut {
    name: String,
    lat: f64,
    lon: f64,
    yaw_deg: f64,
    pitch_deg: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    ground_offset_m: Option<f64>,
}

#[derive(Debug, Serialize)]
struct FlybyPayload {
    name: String,
    ground_offset_m: f64,
    settle_frames: usize,
    staging_altitude_m: f64,
    min_loaded_chunks: usize,
    loading_timeout_secs: usize,
    views: Vec<String>,
    points: Vec<FlybyPointOut>,
}

const VALID_VIEWS: &[&str] = &[
    "beauty",
    "height",
    "slope",
    "ground",
    "hydro",
    "roads",
    "buildings",
];

fn meters_to_lat_deg(meters: f64) -> f64 {
    meters / 111_320.0
}

fn meters_to_lon_deg(meters: f64, lat_deg: f64) -> f64 {
    let lon_scale = lat_deg.to_radians().cos().abs().max(0.1);
    meters / (111_320.0 * lon_scale)
}

fn offset_lat_lon(lat: f64, lon: f64, north_m: f64, east_m: f64) -> (f64, f64) {
    (
        lat + meters_to_lat_deg(north_m),
        lon + meters_to_lon_deg(east_m, lat),
    )
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("failed to serialize {}: {}", path.display(), e))?;
    fs::write(path, format!("{}\n", json))
        .map_err(|e| format!("failed to write {}: {}", path.display(), e))
}

fn run_teleport_points(args: TeleportPointsArgs) -> Result<(), String> {
    let atlas_path = resolve_repo_relative(&args.atlas);
    let output_path = resolve_repo_relative(&args.output);
    let atlas = load_atlas(&atlas_path)?;

    let control_filter: BTreeSet<String> = parse_csv(&args.controls).into_iter().collect();
    let category_filter: BTreeSet<String> = parse_csv(&args.categories).into_iter().collect();
    let mut selected = iter_selected_controls(&atlas, &control_filter, &category_filter);
    if args.limit > 0 {
        selected.truncate(args.limit);
    }
    if selected.is_empty() {
        return Err("selection matched no controls".to_string());
    }

    let selected_ids: BTreeSet<String> = selected.iter().map(|(_, control)| control.id.clone()).collect();
    let missing_controls: Vec<String> = control_filter
        .difference(&selected_ids)
        .cloned()
        .collect();
    if !missing_controls.is_empty() {
        return Err(format!("unknown control ids: {}", missing_controls.join(", ")));
    }

    let route_name = args.name.unwrap_or_else(|| {
        if control_filter.len() == 1 && category_filter.is_empty() {
            format!("{}-teleports", control_filter.iter().next().unwrap())
        } else if category_filter.len() == 1 && control_filter.is_empty() {
            format!("{}-teleports", category_filter.iter().next().unwrap())
        } else if !category_filter.is_empty() {
            format!("{}-teleports", category_filter.iter().cloned().collect::<Vec<_>>().join("-"))
        } else if !control_filter.is_empty() {
            format!("{}-teleports", control_filter.iter().cloned().collect::<Vec<_>>().join("-"))
        } else {
            "all-controls-teleports".to_string()
        }
    });

    let points = selected
        .into_iter()
        .map(|(category_id, control)| TeleportPointOut {
            id: &control.id,
            label: if control.label.is_empty() {
                &control.id
            } else {
                &control.label
            },
            lat: control.lat,
            lon: control.lon,
            yaw_deg: args.yaw_deg,
            pitch_deg: args.pitch_deg,
            prefer_water: args.prefer_water,
            category_id,
        })
        .collect::<Vec<_>>();

    let payload = TeleportPayload {
        name: slugify(&route_name, "teleport-points"),
        ground_offset_m: args.ground_offset_m,
        staging_altitude_m: args.staging_altitude_m,
        min_loaded_chunks: args.min_loaded_chunks,
        loading_timeout_secs: args.loading_timeout_secs,
        points,
    };

    write_json(&output_path, &payload)?;

    println!("wrote {}", output_path.display());
    println!("teleport_set={}", payload.name);
    println!("points={}", payload.points.len());
    for (idx, point) in payload.points.iter().take(10).enumerate() {
        println!("{:>3} {} | {}", idx + 1, point.id, point.label);
    }
    if payload.points.len() > 10 {
        println!("... {} more", payload.points.len() - 10);
    }
    Ok(())
}

fn run_flyby_route(args: FlybyRouteArgs) -> Result<(), String> {
    let atlas_path = resolve_repo_relative(&args.atlas);
    let output_path = resolve_repo_relative(&args.output);
    let atlas = load_atlas(&atlas_path)?;

    let views: Vec<String> = args
        .views
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect();
    if views.is_empty() {
        return Err("at least one --views entry is required".to_string());
    }
    let unknown_views: Vec<String> = views
        .iter()
        .filter(|view| !VALID_VIEWS.contains(&view.as_str()))
        .cloned()
        .collect();
    if !unknown_views.is_empty() {
        return Err(format!("unknown capture views: {}", unknown_views.join(", ")));
    }

    let control_filter: BTreeSet<String> = parse_csv(&args.controls).into_iter().collect();
    let category_filter: BTreeSet<String> = parse_csv(&args.categories).into_iter().collect();
    let selected = iter_selected_controls(&atlas, &control_filter, &category_filter);
    if selected.is_empty() {
        return Err("selection matched no controls".to_string());
    }

    let selected_ids: BTreeSet<String> = selected.iter().map(|(_, control)| control.id.clone()).collect();
    let missing_controls: Vec<String> = control_filter
        .difference(&selected_ids)
        .cloned()
        .collect();
    if !missing_controls.is_empty() {
        return Err(format!("unknown control ids: {}", missing_controls.join(", ")));
    }

    let orbit_radius_m = args.orbit_radius_m.unwrap_or(atlas.default_window_m);
    if orbit_radius_m <= 0.0 {
        return Err("--orbit-radius-m must be positive".to_string());
    }
    let topdown_ground_offset_m = args
        .topdown_ground_offset_m
        .unwrap_or((args.ground_offset_m * 3.0).max(20.0));

    let route_name = args.name.unwrap_or_else(|| {
        if control_filter.len() == 1 && category_filter.is_empty() {
            format!("{}-flyby", control_filter.iter().next().unwrap())
        } else if category_filter.len() == 1 && control_filter.is_empty() {
            format!("{}-flyby", category_filter.iter().next().unwrap())
        } else if !category_filter.is_empty() {
            format!("{}-flyby", category_filter.iter().cloned().collect::<Vec<_>>().join("-"))
        } else if !control_filter.is_empty() {
            format!("{}-flyby", control_filter.iter().cloned().collect::<Vec<_>>().join("-"))
        } else {
            "all-controls-flyby".to_string()
        }
    });

    let orbit_layout = [
        ("north", orbit_radius_m, 0.0, 180.0),
        ("east", 0.0, orbit_radius_m, 270.0),
        ("south", -orbit_radius_m, 0.0, 0.0),
        ("west", 0.0, -orbit_radius_m, 90.0),
    ];
    let mut route_points = Vec::new();
    for (_, control) in selected {
        for (suffix, north_m, east_m, yaw_deg) in orbit_layout {
            let (point_lat, point_lon) = offset_lat_lon(control.lat, control.lon, north_m, east_m);
            route_points.push(FlybyPointOut {
                name: format!("{}-{}", control.id, suffix),
                lat: round_to(point_lat, 7),
                lon: round_to(point_lon, 7),
                yaw_deg,
                pitch_deg: args.oblique_pitch_deg,
                ground_offset_m: None,
            });
        }
        if !args.skip_topdown {
            route_points.push(FlybyPointOut {
                name: format!("{}-topdown", control.id),
                lat: round_to(control.lat, 7),
                lon: round_to(control.lon, 7),
                yaw_deg: 180.0,
                pitch_deg: args.topdown_pitch_deg,
                ground_offset_m: Some(topdown_ground_offset_m),
            });
        }
    }

    let payload = FlybyPayload {
        name: slugify(&route_name, "control-flyby"),
        ground_offset_m: args.ground_offset_m,
        settle_frames: args.settle_frames,
        staging_altitude_m: args.staging_altitude_m,
        min_loaded_chunks: args.min_loaded_chunks,
        loading_timeout_secs: args.loading_timeout_secs,
        views,
        points: route_points,
    };

    write_json(&output_path, &payload)?;

    let control_count = payload
        .points
        .iter()
        .filter(|point| point.name.ends_with("-north"))
        .count();
    println!("wrote {}", output_path.display());
    println!("route_name={}", payload.name);
    println!("controls={}", control_count);
    println!("points={}", payload.points.len());
    println!("views={}", payload.views.join(","));
    Ok(())
}

fn run_merge_pack(args: MergePackArgs) -> Result<(), String> {
    let options = MergeControlPackOptions {
        pack_root: resolve_repo_relative(&args.pack_root),
        dest_db: resolve_repo_relative(&args.dest_db),
        site_ids: args.site_ids,
        limit_sites: args.limit_sites,
        skip_osm: args.skip_osm,
        dry_run: args.dry_run,
    };
    run_merge_control_pack(&options)
}

fn main() -> Result<(), String> {
    match Cli::parse().command {
        Command::TeleportPoints(args) => run_teleport_points(args),
        Command::FlybyRoute(args) => run_flyby_route(args),
        Command::MergePack(args) => run_merge_pack(args),
    }
}
