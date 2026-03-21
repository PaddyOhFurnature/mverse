use crate::tile_store::{PassId, TileStore};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct MergeControlPackOptions {
    pub pack_root: PathBuf,
    pub dest_db: PathBuf,
    pub site_ids: Vec<String>,
    pub limit_sites: Option<usize>,
    pub skip_osm: bool,
    pub dry_run: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct MergeStats {
    scanned: u64,
    added: u64,
    updated: u64,
    identical: u64,
    unreadable: u64,
}

impl MergeStats {
    fn add_assign(&mut self, other: MergeStats) {
        self.scanned += other.scanned;
        self.added += other.added;
        self.updated += other.updated;
        self.identical += other.identical;
        self.unreadable += other.unreadable;
    }

    fn has_changes(&self) -> bool {
        self.added > 0 || self.updated > 0
    }
}

fn pass_list() -> [PassId; 6] {
    [
        PassId::Terrain,
        PassId::Substrate,
        PassId::Hydro,
        PassId::Roads,
        PassId::Buildings,
        PassId::FeatureRules,
    ]
}

fn pass_label(pass: PassId) -> &'static str {
    match pass {
        PassId::Terrain => "terrain",
        PassId::Substrate => "substrate",
        PassId::Hydro => "hydro",
        PassId::Roads => "roads",
        PassId::Buildings => "buildings",
        PassId::FeatureRules => "feature_rules",
    }
}

fn discover_site_dirs(
    pack_root: &Path,
    site_filter: &BTreeSet<String>,
    limit_sites: Option<usize>,
) -> Result<Vec<PathBuf>, String> {
    let mut dirs = Vec::new();
    for entry in fs::read_dir(pack_root).map_err(|e| format!("read_dir {}: {}", pack_root.display(), e))? {
        let entry = entry.map_err(|e| format!("read_dir entry {}: {}", pack_root.display(), e))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join("tiles.db").exists() || !path.join("manifest.json").exists() {
            continue;
        }
        let site_id = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("non-utf8 site directory: {}", path.display()))?
            .to_string();
        if !site_filter.is_empty() && !site_filter.contains(&site_id) {
            continue;
        }
        dirs.push(path);
    }
    dirs.sort();

    if let Some(limit) = limit_sites {
        dirs.truncate(limit);
    }

    if !site_filter.is_empty() {
        let discovered: BTreeSet<String> = dirs
            .iter()
            .filter_map(|dir| dir.file_name().and_then(|name| name.to_str()).map(|s| s.to_string()))
            .collect();
        let missing: Vec<String> = site_filter.difference(&discovered).cloned().collect();
        if !missing.is_empty() {
            return Err(format!(
                "requested site ids not found under {}: {}",
                pack_root.display(),
                missing.join(", ")
            ));
        }
    }

    if dirs.is_empty() {
        return Err(format!("no site worlds found under {}", pack_root.display()));
    }

    Ok(dirs)
}

fn merge_osm(src: &TileStore, dest: &TileStore, dry_run: bool) -> MergeStats {
    let mut stats = MergeStats::default();
    for (s, w, n, e) in src.iter_osm_coords() {
        stats.scanned += 1;
        let Some(data) = src.get_osm(s, w, n, e) else {
            stats.unreadable += 1;
            continue;
        };
        match dest.get_osm(s, w, n, e) {
            Some(existing) if existing == data => stats.identical += 1,
            Some(_) => {
                stats.updated += 1;
                if !dry_run {
                    dest.put_osm(s, w, n, e, &data);
                }
            }
            None => {
                stats.added += 1;
                if !dry_run {
                    dest.put_osm(s, w, n, e, &data);
                }
            }
        }
    }
    stats
}

fn merge_pass(src: &TileStore, dest: &TileStore, pass: PassId, dry_run: bool) -> MergeStats {
    let mut stats = MergeStats::default();
    for (cx, cy, cz) in src.iter_chunk_pass_coords(pass) {
        stats.scanned += 1;
        let Some(data) = src.get_chunk_pass(cx, cy, cz, pass) else {
            stats.unreadable += 1;
            continue;
        };
        match dest.get_chunk_pass(cx, cy, cz, pass) {
            Some(existing) if existing == data => stats.identical += 1,
            Some(_) => {
                stats.updated += 1;
                if !dry_run {
                    dest.put_chunk_pass(cx, cy, cz, pass, &data);
                }
            }
            None => {
                stats.added += 1;
                if !dry_run {
                    dest.put_chunk_pass(cx, cy, cz, pass, &data);
                }
            }
        }
    }
    stats
}

fn print_stats(label: &str, stats: MergeStats) {
    println!(
        "  {:<14} scanned={:<7} add={:<7} update={:<7} same={:<7} unreadable={:<7}",
        label, stats.scanned, stats.added, stats.updated, stats.identical, stats.unreadable
    );
}

pub fn run_merge_control_pack(args: &MergeControlPackOptions) -> Result<(), String> {
    let site_filter: BTreeSet<String> = args
        .site_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .map(|id| id.to_string())
        .collect();

    let site_dirs = discover_site_dirs(&args.pack_root, &site_filter, args.limit_sites)?;
    let dest = TileStore::open(&args.dest_db)?;

    println!(
        "[merge-control-pack] mode={} pack_root={} dest_db={} sites={}",
        if args.dry_run { "dry-run" } else { "apply" },
        args.pack_root.display(),
        args.dest_db.display(),
        site_dirs.len()
    );

    let mut total_osm = MergeStats::default();
    let mut total_passes = [MergeStats::default(); 6];

    for site_dir in &site_dirs {
        let site_id = site_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unknown>");
        let src_db = site_dir.join("tiles.db");
        let src = TileStore::open(&src_db)?;

        println!("[merge-control-pack] site={site_id} src_db={}", src_db.display());

        if !args.skip_osm {
            let osm_stats = merge_osm(&src, &dest, args.dry_run);
            total_osm.add_assign(osm_stats);
            print_stats("osm", osm_stats);
        }

        for (index, pass) in pass_list().into_iter().enumerate() {
            let stats = merge_pass(&src, &dest, pass, args.dry_run);
            total_passes[index].add_assign(stats);
            if stats.scanned > 0 || stats.has_changes() || stats.unreadable > 0 {
                print_stats(pass_label(pass), stats);
            }
        }
    }

    println!("[merge-control-pack] totals");
    if !args.skip_osm {
        print_stats("osm", total_osm);
    }
    for (index, pass) in pass_list().into_iter().enumerate() {
        let stats = total_passes[index];
        if stats.scanned > 0 || stats.has_changes() || stats.unreadable > 0 {
            print_stats(pass_label(pass), stats);
        }
    }

    if args.dry_run {
        println!("[merge-control-pack] dry-run complete — no destination changes written");
    } else {
        println!("[merge-control-pack] merge complete");
    }

    Ok(())
}