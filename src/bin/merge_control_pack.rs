use clap::Parser;
use metaverse_core::control_pack_merge::{MergeControlPackOptions, run_merge_control_pack};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "merge-control-pack",
    about = "Merge per-site control pack TileStores into a single global tiles.db"
)]
struct Args {
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

fn main() -> Result<(), String> {
    let args = Args::parse();
    let options = MergeControlPackOptions {
        pack_root: args.pack_root,
        dest_db: args.dest_db,
        site_ids: args.site_ids,
        limit_sites: args.limit_sites,
        skip_osm: args.skip_osm,
        dry_run: args.dry_run,
    };
    run_merge_control_pack(&options)
}