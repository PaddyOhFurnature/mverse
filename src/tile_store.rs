//! RocksDB-backed tile cache for OSM, SRTM, and terrain chunks.
//!
//! # Layout
//! Single RocksDB database (`tiles.db`) with four column families:
//!
//! | CF       | Key                                      | Value                            |
//! |----------|------------------------------------------|----------------------------------|
//! | `osm`    | `[s_i32][w_i32][n_i32][e_i32]` ×10000 BE | `[blake3_32B][bincode OsmData]`  |
//! | `srtm`   | `[lat_i16][lon_i16]` BE                  | `[blake3_32B][raw bytes]`        |
//! | `terrain`| `[cx_i32][cy_i32][cz_i32]` BE            | `[blake3_32B][bincode ChunkData]`|
//! | `meta`   | UTF-8 string                             | raw bytes                        |
//!
//! Every value is prefixed with a 32-byte Blake3 checksum of the data bytes.
//! On read, the checksum is verified; a mismatch auto-evicts the entry and
//! returns `None` (triggering re-download upstream).
//!
//! # Version management
//! Each CF stores its current schema version in `meta` (keys `"osm_version"`,
//! `"srtm_version"`, `"terrain_version"`). If the stored version differs from
//! the compiled constant the CF is dropped and recreated — no manual purge needed.

use std::path::Path;
use std::sync::Arc;
use rocksdb::{
    DB, Options, ColumnFamilyDescriptor, BlockBasedOptions, Cache,
    SliceTransform, WriteBatch, ReadOptions,
};

// ── Process-level TileStore registry ──────────────────────────────────────────
// RocksDB allows only one open per DB path per process. This registry returns
// the existing Arc when the same path is opened a second time,
// preventing "lock hold by current process" LOCK conflicts.
// Uses Weak so the DB is closed when all real holders drop their Arcs.
use std::sync::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;

static TILE_STORE_REGISTRY: std::sync::OnceLock<Mutex<HashMap<PathBuf, std::sync::Weak<TileStoreInner>>>> =
    std::sync::OnceLock::new();

fn registry() -> &'static Mutex<HashMap<PathBuf, std::sync::Weak<TileStoreInner>>> {
    TILE_STORE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}


// ── Schema versions ────────────────────────────────────────────────────────────
pub const OSM_TILE_VERSION:     u32 = 8;
pub const SRTM_TILE_VERSION:    u32 = 1;
pub const TERRAIN_TILE_VERSION: u32 = 1;

const CHECKSUM_LEN: usize = 32;

// ── Column family names ─────────────────────────────────────────────────────────
const CF_OSM:     &str = "osm";
const CF_SRTM:    &str = "srtm";
const CF_TERRAIN: &str = "terrain";
const CF_META:    &str = "meta";

// ── Key encoding ───────────────────────────────────────────────────────────────

/// Encode OSM tile bounds as a 16-byte key.
/// Coords are multiplied by 10000, rounded to i32, stored big-endian.
/// Big-endian ensures lexicographic order matches numeric order.
pub fn osm_key(s: f64, w: f64, n: f64, e: f64) -> [u8; 16] {
    let mut k = [0u8; 16];
    k[0..4].copy_from_slice(&((s * 10_000.0).round() as i32).to_be_bytes());
    k[4..8].copy_from_slice(&((w * 10_000.0).round() as i32).to_be_bytes());
    k[8..12].copy_from_slice(&((n * 10_000.0).round() as i32).to_be_bytes());
    k[12..16].copy_from_slice(&((e * 10_000.0).round() as i32).to_be_bytes());
    k
}

/// Encode SRTM tile origin as a 4-byte key.
pub fn srtm_key(lat: i32, lon: i32) -> [u8; 4] {
    let mut k = [0u8; 4];
    k[0..2].copy_from_slice(&(lat as i16).to_be_bytes());
    k[2..4].copy_from_slice(&(lon as i16).to_be_bytes());
    k
}

/// Encode terrain chunk coordinates as a 12-byte key.
pub fn terrain_key(cx: i32, cy: i32, cz: i32) -> [u8; 12] {
    let mut k = [0u8; 12];
    k[0..4].copy_from_slice(&cx.to_be_bytes());
    k[4..8].copy_from_slice(&cy.to_be_bytes());
    k[8..12].copy_from_slice(&cz.to_be_bytes());
    k
}

// ── Checksum helpers ───────────────────────────────────────────────────────────

fn make_value(data: &[u8]) -> Vec<u8> {
    let hash = blake3::hash(data);
    let mut v = Vec::with_capacity(CHECKSUM_LEN + data.len());
    v.extend_from_slice(hash.as_bytes());
    v.extend_from_slice(data);
    v
}

fn verify_and_extract(raw: &[u8]) -> Option<&[u8]> {
    if raw.len() < CHECKSUM_LEN {
        return None;
    }
    let (stored_hash, data) = raw.split_at(CHECKSUM_LEN);
    let computed = blake3::hash(data);
    if computed.as_bytes() == stored_hash {
        Some(data)
    } else {
        None
    }
}

// ── RocksDB helpers ─────────────────────────────────────────────────────────────

fn make_cf_opts(compressed: bool) -> Options {
    let mut o = Options::default();
    if compressed {
        o.set_compression_type(rocksdb::DBCompressionType::Zstd);
        o.set_zstd_max_train_bytes(0);
    }
    // Bloom filter: ~1% false-positive rate, good for "does this tile exist?" check
    let mut bb = BlockBasedOptions::default();
    let cache = Cache::new_lru_cache(64 * 1024 * 1024); // 64MB block cache
    bb.set_block_cache(&cache);
    bb.set_bloom_filter(10.0, false);
    o.set_block_based_table_factory(&bb);
    o.set_write_buffer_size(64 * 1024 * 1024); // 64MB memtable
    o
}

fn meta_opts() -> Options {
    let mut o = Options::default();
    o.set_compression_type(rocksdb::DBCompressionType::None);
    o
}

// ── TileStore ──────────────────────────────────────────────────────────────────

/// Inner DB holder — one per unique path, reference-counted via Arc.
struct TileStoreInner {
    db: DB,
}

/// Shared handle to the tile cache database.
/// Cheap to clone — backed by `Arc<TileStoreInner>`.
/// **Process-singleton per path**: calling `open()` with the same canonical path
/// returns the same underlying `Arc` instead of attempting a second RocksDB open.
#[derive(Clone)]
pub struct TileStore {
    db: Arc<TileStoreInner>,
}

impl TileStore {
    /// Open (or create) the tile database at `path`.
    ///
    /// If the same canonical path is already open in this process, returns
    /// a clone of the existing handle (no second RocksDB open, no LOCK conflict).
    ///
    /// Automatically checks schema versions and wipes stale column families.
    /// Safe to call from multiple threads after open — `Arc` is `Send + Sync`.
    pub fn open(path: &Path) -> Result<Self, String> {
        let canonical = path.canonicalize()
            .unwrap_or_else(|_| {
                // Path doesn't exist yet — create dir first, then canonicalize
                let _ = std::fs::create_dir_all(path);
                path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
            });

        // Check registry first — return existing Arc if already open
        {
            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(weak) = reg.get(&canonical) {
                if let Some(strong) = weak.upgrade() {
                    return Ok(TileStore { db: strong });
                }
                // Weak expired — remove stale entry and open fresh
                reg.remove(&canonical);
            }
        }

        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let cf_descs = vec![
            ColumnFamilyDescriptor::new(CF_OSM,     make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_SRTM,    make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_TERRAIN, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_META,    meta_opts()),
        ];

        let db = DB::open_cf_descriptors(&db_opts, path, cf_descs)
            .map_err(|e| format!("TileStore open failed: {e}"))?;

        let inner = Arc::new(TileStoreInner { db });
        {
            let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
            reg.insert(canonical, Arc::downgrade(&inner));
        }

        let store = TileStore { db: inner };
        store.check_versions()?;
        Ok(store)
    }

    fn check_versions(&self) -> Result<(), String> {
        self.check_cf_version("osm_version",     OSM_TILE_VERSION,     CF_OSM)?;
        self.check_cf_version("srtm_version",    SRTM_TILE_VERSION,    CF_SRTM)?;
        self.check_cf_version("terrain_version", TERRAIN_TILE_VERSION, CF_TERRAIN)?;
        Ok(())
    }

    fn check_cf_version(&self, meta_key: &str, expected: u32, cf_name: &str) -> Result<(), String> {
        let meta_cf = self.db.db.cf_handle(CF_META)
            .ok_or("meta CF missing")?;
        let stored = self.db.db.get_cf(&meta_cf, meta_key)
            .map_err(|e| e.to_string())?
            .and_then(|b| b.try_into().ok().map(u32::from_be_bytes))
            .unwrap_or(0);

        if stored != expected {
            // Stale version — wipe the CF by deleting all keys in range
            let cf = self.db.db.cf_handle(cf_name).ok_or(format!("{cf_name} CF missing"))?;
            let iter = self.db.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
            let mut batch = WriteBatch::default();
            for item in iter {
                if let Ok((key, _)) = item {
                    batch.delete_cf(&cf, &key);
                }
            }
            // Reset the tile count for this CF
            let count_key = format!("{cf_name}_count");
            batch.put_cf(&meta_cf, count_key.as_bytes(), 0u64.to_be_bytes());
            // Store new version
            batch.put_cf(&meta_cf, meta_key.as_bytes(), expected.to_be_bytes());
            self.db.db.write(batch).map_err(|e| e.to_string())?;
            eprintln!("TileStore: {cf_name} version {stored}→{expected}, stale entries cleared");
        } else {
            // Write version if it was absent (first open)
            let mut batch = WriteBatch::default();
            batch.put_cf(&meta_cf, meta_key.as_bytes(), expected.to_be_bytes());
            self.db.db.write(batch).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    // ── OSM ───────────────────────────────────────────────────────────────────

    pub fn get_osm(&self, s: f64, w: f64, n: f64, e: f64) -> Option<Vec<u8>> {
        let cf = self.db.db.cf_handle(CF_OSM)?;
        let raw = self.db.db.get_cf(&cf, osm_key(s, w, n, e)).ok()??;
        match verify_and_extract(&raw) {
            Some(data) => Some(data.to_vec()),
            None => {
                // Corrupt entry — evict and return None (triggers re-download)
                let _ = self.db.db.delete_cf(&cf, osm_key(s, w, n, e));
                self.dec_count(CF_OSM);
                eprintln!("TileStore: evicted corrupt OSM tile s={s:.4} w={w:.4}");
                None
            }
        }
    }

    /// Store a pre-validated OSM tile (bincode bytes that already deserialised OK).
    pub fn put_osm(&self, s: f64, w: f64, n: f64, e: f64, data: &[u8]) {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else { return };
        let existed = self.db.db.get_cf(&cf, osm_key(s, w, n, e))
            .ok().flatten().is_some();
        let _ = self.db.db.put_cf(&cf, osm_key(s, w, n, e), make_value(data));
        if !existed { self.inc_count(CF_OSM); }
    }

    pub fn has_osm(&self, s: f64, w: f64, n: f64, e: f64) -> bool {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else { return false };
        self.db.db.get_cf(&cf, osm_key(s, w, n, e))
            .ok().flatten().is_some()
    }

    pub fn delete_osm(&self, s: f64, w: f64, n: f64, e: f64) {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else { return };
        if self.has_osm(s, w, n, e) {
            let _ = self.db.db.delete_cf(&cf, osm_key(s, w, n, e));
            self.dec_count(CF_OSM);
        }
    }

    // ── SRTM ──────────────────────────────────────────────────────────────────

    pub fn get_srtm(&self, lat: i32, lon: i32) -> Option<Vec<u8>> {
        let cf = self.db.db.cf_handle(CF_SRTM)?;
        let raw = self.db.db.get_cf(&cf, srtm_key(lat, lon)).ok()??;
        match verify_and_extract(&raw) {
            Some(data) => Some(data.to_vec()),
            None => {
                let _ = self.db.db.delete_cf(&cf, srtm_key(lat, lon));
                self.dec_count(CF_SRTM);
                eprintln!("TileStore: evicted corrupt SRTM tile lat={lat} lon={lon}");
                None
            }
        }
    }

    pub fn put_srtm(&self, lat: i32, lon: i32, data: &[u8]) {
        let Some(cf) = self.db.db.cf_handle(CF_SRTM) else { return };
        let existed = self.db.db.get_cf(&cf, srtm_key(lat, lon))
            .ok().flatten().is_some();
        let _ = self.db.db.put_cf(&cf, srtm_key(lat, lon), make_value(data));
        if !existed { self.inc_count(CF_SRTM); }
    }

    pub fn has_srtm(&self, lat: i32, lon: i32) -> bool {
        let Some(cf) = self.db.db.cf_handle(CF_SRTM) else { return false };
        self.db.db.get_cf(&cf, srtm_key(lat, lon)).ok().flatten().is_some()
    }

    pub fn delete_srtm(&self, lat: i32, lon: i32) {
        let Some(cf) = self.db.db.cf_handle(CF_SRTM) else { return };
        if self.has_srtm(lat, lon) {
            let _ = self.db.db.delete_cf(&cf, srtm_key(lat, lon));
            self.dec_count(CF_SRTM);
        }
    }

    // ── Terrain ───────────────────────────────────────────────────────────────

    pub fn get_terrain(&self, cx: i32, cy: i32, cz: i32) -> Option<Vec<u8>> {
        let cf = self.db.db.cf_handle(CF_TERRAIN)?;
        let raw = self.db.db.get_cf(&cf, terrain_key(cx, cy, cz)).ok()??;
        match verify_and_extract(&raw) {
            Some(data) => Some(data.to_vec()),
            None => {
                let _ = self.db.db.delete_cf(&cf, terrain_key(cx, cy, cz));
                self.dec_count(CF_TERRAIN);
                eprintln!("TileStore: evicted corrupt terrain chunk {cx},{cy},{cz}");
                None
            }
        }
    }

    pub fn put_terrain(&self, cx: i32, cy: i32, cz: i32, data: &[u8]) {
        let Some(cf) = self.db.db.cf_handle(CF_TERRAIN) else { return };
        let existed = self.db.db.get_cf(&cf, terrain_key(cx, cy, cz))
            .ok().flatten().is_some();
        let _ = self.db.db.put_cf(&cf, terrain_key(cx, cy, cz), make_value(data));
        if !existed { self.inc_count(CF_TERRAIN); }
    }

    pub fn has_terrain(&self, cx: i32, cy: i32, cz: i32) -> bool {
        let Some(cf) = self.db.db.cf_handle(CF_TERRAIN) else { return false };
        self.db.db.get_cf(&cf, terrain_key(cx, cy, cz)).ok().flatten().is_some()
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    /// Instant tile count read from a meta counter (no full scan).
    pub fn tile_count(&self, cf_name: &str) -> u64 {
        let Some(meta_cf) = self.db.db.cf_handle(CF_META) else { return 0 };
        let key = format!("{cf_name}_count");
        self.db.db.get_cf(&meta_cf, key.as_bytes())
            .ok().flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(0)
    }

    pub fn osm_count(&self)     -> u64 { self.tile_count(CF_OSM) }
    pub fn srtm_count(&self)    -> u64 { self.tile_count(CF_SRTM) }
    pub fn terrain_count(&self) -> u64 { self.tile_count(CF_TERRAIN) }

    /// Iterate all stored OSM tile coordinates.
    /// Decodes each 16-byte key back to (s, w, n, e) float pairs.
    /// Used for DHT announce-all at startup.
    pub fn iter_osm_coords(&self) -> Vec<(f64, f64, f64, f64)> {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else { return vec![] };
        let mut result = Vec::new();
        for item in self.db.db.iterator_cf(&cf, rocksdb::IteratorMode::Start) {
            if let Ok((key, _)) = item {
                if key.len() == 16 {
                    let s = i32::from_be_bytes(key[0..4].try_into().unwrap()) as f64 / 10000.0;
                    let w = i32::from_be_bytes(key[4..8].try_into().unwrap()) as f64 / 10000.0;
                    let n = i32::from_be_bytes(key[8..12].try_into().unwrap()) as f64 / 10000.0;
                    let e = i32::from_be_bytes(key[12..16].try_into().unwrap()) as f64 / 10000.0;
                    result.push((s, w, n, e));
                }
            }
        }
        result
    }

    /// Scan all tiles in a CF and verify checksums.
    /// Returns `(total, corrupt)` — corrupt entries are deleted automatically.
    pub fn verify_cf(&self, cf_name: &str) -> (u64, u64) {
        let Some(cf) = self.db.db.cf_handle(cf_name) else { return (0, 0) };
        let iter = self.db.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
        let (mut total, mut corrupt) = (0u64, 0u64);
        let mut to_delete: Vec<Vec<u8>> = Vec::new();
        for item in iter {
            if let Ok((key, value)) = item {
                total += 1;
                if verify_and_extract(&value).is_none() {
                    corrupt += 1;
                    to_delete.push(key.to_vec());
                }
            }
        }
        for key in &to_delete {
            let _ = self.db.db.delete_cf(&cf, key);
        }
        if corrupt > 0 {
            // Recount — easier than trying to decrement by exact corrupt count
            let Some(meta_cf) = self.db.db.cf_handle(CF_META) else { return (total, corrupt) };
            let new_count = total - corrupt;
            let count_key = format!("{cf_name}_count");
            let _ = self.db.db.put_cf(&meta_cf, count_key.as_bytes(), new_count.to_be_bytes());
        }
        (total, corrupt)
    }

    /// Wipe an entire column family (nuclear option — all tiles deleted).
    pub fn purge_cf(&self, cf_name: &str) -> Result<usize, String> {
        let cf = self.db.db.cf_handle(cf_name)
            .ok_or(format!("unknown CF: {cf_name}"))?;
        let iter = self.db.db.iterator_cf(&cf, rocksdb::IteratorMode::Start);
        let mut batch = WriteBatch::default();
        let mut count = 0usize;
        for item in iter {
            if let Ok((key, _)) = item {
                batch.delete_cf(&cf, &key);
                count += 1;
            }
        }
        // Reset counter
        if let Some(meta_cf) = self.db.db.cf_handle(CF_META) {
            let count_key = format!("{cf_name}_count");
            batch.put_cf(&meta_cf, count_key.as_bytes(), 0u64.to_be_bytes());
        }
        self.db.db.write(batch).map_err(|e| e.to_string())?;
        Ok(count)
    }

    // ── Internal counter helpers ───────────────────────────────────────────────

    fn inc_count(&self, cf_name: &str) {
        let Some(meta_cf) = self.db.db.cf_handle(CF_META) else { return };
        let key = format!("{cf_name}_count");
        let cur = self.db.db.get_cf(&meta_cf, key.as_bytes())
            .ok().flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(0);
        let _ = self.db.db.put_cf(&meta_cf, key.as_bytes(), (cur + 1).to_be_bytes());
    }

    fn dec_count(&self, cf_name: &str) {
        let Some(meta_cf) = self.db.db.cf_handle(CF_META) else { return };
        let key = format!("{cf_name}_count");
        let cur = self.db.db.get_cf(&meta_cf, key.as_bytes())
            .ok().flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(1);
        let _ = self.db.db.put_cf(&meta_cf, key.as_bytes(), cur.saturating_sub(1).to_be_bytes());
    }
}

// ── Old flat-file cleanup ──────────────────────────────────────────────────────

/// Detect old flat-file tile directories and delete them in the background.
///
/// Workflow:
/// 1. If `dir` contains any `.bin` files → rename to `<dir>_deleting_<timestamp>`
/// 2. Create fresh empty `dir`
/// 3. Spawn OS thread: `find <renamed> -type f -delete` at idle I/O priority, then remove dir
///
/// Returns immediately. Deletion runs entirely in background.
pub fn cleanup_old_tile_dir(dir: &Path) {
    // Check if the dir exists and has .bin files (sample first 3 entries only)
    let has_bins = std::fs::read_dir(dir)
        .ok()
        .and_then(|mut rd| {
            // Only peek — don't enumerate all 2M files
            for _ in 0..3 {
                if let Some(Ok(e)) = rd.next() {
                    if e.path().extension().and_then(|x| x.to_str()) == Some("bin") {
                        return Some(true);
                    }
                }
            }
            None
        })
        .unwrap_or(false);

    if !has_bins {
        return;
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Build the rename target path
    let mut old_path = dir.to_path_buf();
    let dir_name = dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tiles");
    let parent = dir.parent().unwrap_or(dir);
    let old_path = parent.join(format!("{}_deleting_{}", dir_name, ts));

    if let Err(e) = std::fs::rename(dir, &old_path) {
        eprintln!("cleanup_old_tile_dir: rename failed: {e}");
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("cleanup_old_tile_dir: mkdir failed: {e}");
        return;
    }

    let old_path_owned = old_path.clone();
    eprintln!("🗑  Old tile dir detected → renamed to {:?}, deleting in background…", old_path_owned);

    std::thread::spawn(move || {
        // ionice -c3 = idle I/O class: only uses disk when nothing else needs it
        let status = std::process::Command::new("ionice")
            .args(["-c3", "find"])
            .arg(&old_path_owned)
            .args(["-type", "f", "-delete"])
            .status();

        match status {
            Ok(s) if s.success() => {
                let _ = std::fs::remove_dir_all(&old_path_owned);
                eprintln!("🗑  Old tile dir deleted: {:?}", old_path_owned);
            }
            _ => {
                // ionice not available — fall back to plain find
                let _ = std::process::Command::new("find")
                    .arg(&old_path_owned)
                    .args(["-type", "f", "-delete"])
                    .status();
                let _ = std::fs::remove_dir_all(&old_path_owned);
                eprintln!("🗑  Old tile dir deleted (no ionice): {:?}", old_path_owned);
            }
        }
    });
}

/// Background deletion of old flat `.hgt`/`.tif` elevation files after TileStore migration.
///
/// Same strategy as `cleanup_old_tile_dir`: rename the dir atomically, recreate an empty
/// one, then delete the renamed tree from a background thread at idle I/O priority.
pub fn cleanup_old_srtm_dir(dir: &Path) {
    let has_elevation_files = std::fs::read_dir(dir)
        .ok()
        .and_then(|mut rd| {
            for _ in 0..5 {
                if let Some(Ok(e)) = rd.next() {
                    let ext = e.path().extension()
                        .and_then(|x| x.to_str())
                        .unwrap_or("")
                        .to_lowercase();
                    if ext == "hgt" || ext == "tif" || ext == "tiff" {
                        return Some(true);
                    }
                }
            }
            None
        })
        .unwrap_or(false);

    if !has_elevation_files {
        return;
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let dir_name = dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("elevation_cache");
    let parent = dir.parent().unwrap_or(dir);
    let old_path = parent.join(format!("{}_deleting_{}", dir_name, ts));

    if let Err(e) = std::fs::rename(dir, &old_path) {
        eprintln!("cleanup_old_srtm_dir: rename failed: {e}");
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("cleanup_old_srtm_dir: mkdir failed: {e}");
        return;
    }

    let old_path_owned = old_path.clone();
    eprintln!("🗑  Old SRTM dir detected → renamed to {:?}, deleting in background…", old_path_owned);

    std::thread::spawn(move || {
        let status = std::process::Command::new("ionice")
            .args(["-c3", "find"])
            .arg(&old_path_owned)
            .args(["-type", "f", "-delete"])
            .status();

        match status {
            Ok(s) if s.success() => {
                let _ = std::fs::remove_dir_all(&old_path_owned);
                eprintln!("🗑  Old SRTM dir deleted: {:?}", old_path_owned);
            }
            _ => {
                let _ = std::process::Command::new("find")
                    .arg(&old_path_owned)
                    .args(["-type", "f", "-delete"])
                    .status();
                let _ = std::fs::remove_dir_all(&old_path_owned);
                eprintln!("🗑  Old SRTM dir deleted (no ionice): {:?}", old_path_owned);
            }
        }
    });
}
