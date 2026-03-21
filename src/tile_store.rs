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

use rocksdb::{BlockBasedOptions, Cache, ColumnFamilyDescriptor, DB, Options, WriteBatch};
use std::path::Path;
use std::sync::Arc;

// ── Process-level TileStore registry ──────────────────────────────────────────
// RocksDB allows only one open per DB path per process. This registry returns
// the existing Arc when the same path is opened a second time,
// preventing "lock hold by current process" LOCK conflicts.
// Uses Weak so the DB is closed when all real holders drop their Arcs.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

static TILE_STORE_REGISTRY: std::sync::OnceLock<
    Mutex<HashMap<PathBuf, std::sync::Weak<TileStoreInner>>>,
> = std::sync::OnceLock::new();

fn registry() -> &'static Mutex<HashMap<PathBuf, std::sync::Weak<TileStoreInner>>> {
    TILE_STORE_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

// ── Schema versions ────────────────────────────────────────────────────────────
pub const OSM_TILE_VERSION: u32 = 9;
pub const SRTM_TILE_VERSION: u32 = 1;
/// Must match `TERRAIN_CACHE_VERSION` in chunk_loader.rs.  Bump both together
/// whenever the terrain binary format changes — the DB wipes all stale chunks on open.
pub const TERRAIN_TILE_VERSION: u32 = 14;

/// Pass versions — bump ONLY the pass whose algorithm changes.
/// The TileStore wipes only that pass's data on version mismatch.
pub const PASS_TERRAIN_VERSION: u16 = 9; // SRTM shape (Octree) + engineered-ground fringe halo for recreation/sports complexes
pub const PASS_SUBSTRATE_VERSION: u16 = 1; // Biome/substrate material layer
pub const PASS_HYDRO_VERSION: u16 = 26; // Rivers, water bodies
pub const PASS_ROADS_VERSION: u16 = 32; // Roads snapshot stored after hydro; bump when upstream composition changes
pub const PASS_BUILDINGS_VERSION: u16 = 0; // Not yet generated

const CHECKSUM_LEN: usize = 32;

// ── Column family names ─────────────────────────────────────────────────────────
const CF_OSM: &str = "osm";
const CF_SRTM: &str = "srtm";
const CF_TERRAIN: &str = "terrain";
const CF_META: &str = "meta";

const CF_PASS_TERRAIN: &str = "pass_terrain";
const CF_PASS_SUBSTRATE: &str = "pass_substrate";
const CF_PASS_HYDRO: &str = "pass_hydro";
const CF_PASS_ROADS: &str = "pass_roads";
const CF_PASS_BUILDINGS: &str = "pass_buildings";
const CF_FEATURE_RULES: &str = "feature_rules";

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

/// Encode terrain chunk coordinates as a 12-byte big-endian key.
pub fn terrain_key(cx: i32, cy: i32, cz: i32) -> [u8; 12] {
    let mut k = [0u8; 12];
    k[0..4].copy_from_slice(&cx.to_be_bytes());
    k[4..8].copy_from_slice(&cy.to_be_bytes());
    k[8..12].copy_from_slice(&cz.to_be_bytes());
    k
}

/// Encode chunk coordinates as a 12-byte little-endian key for pass column families.
fn chunk_key(cx: i32, cy: i32, cz: i32) -> [u8; 12] {
    let mut k = [0u8; 12];
    k[0..4].copy_from_slice(&cx.to_le_bytes());
    k[4..8].copy_from_slice(&cy.to_le_bytes());
    k[8..12].copy_from_slice(&cz.to_le_bytes());
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
        let canonical = path.canonicalize().unwrap_or_else(|_| {
            // Path doesn't exist yet — create dir first, then canonicalize
            let _ = std::fs::create_dir_all(path);
            path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
        });

        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;

        // IMPORTANT: hold the registry lock across the first open for a path.
        // Without this, two threads can both miss the registry, race into
        // RocksDB::open on the same LOCK file, and one panics with
        // "lock hold by current process" before the winning thread inserts the Arc.
        let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(weak) = reg.get(&canonical) {
            if let Some(strong) = weak.upgrade() {
                return Ok(TileStore { db: strong });
            }
            // Weak expired — remove stale entry and open fresh.
            reg.remove(&canonical);
        }

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let cf_descs = vec![
            ColumnFamilyDescriptor::new(CF_OSM, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_SRTM, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_TERRAIN, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_META, meta_opts()),
            ColumnFamilyDescriptor::new(CF_PASS_TERRAIN, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_PASS_SUBSTRATE, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_PASS_HYDRO, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_PASS_ROADS, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_PASS_BUILDINGS, make_cf_opts(true)),
            ColumnFamilyDescriptor::new(CF_FEATURE_RULES, make_cf_opts(true)),
        ];

        let db = DB::open_cf_descriptors(&db_opts, path, cf_descs)
            .map_err(|e| format!("TileStore open failed: {e}"))?;

        let inner = Arc::new(TileStoreInner { db });
        let store = TileStore {
            db: Arc::clone(&inner),
        };
        store.check_versions()?;
        reg.insert(canonical, Arc::downgrade(&inner));
        Ok(store)
    }

    fn check_versions(&self) -> Result<(), String> {
        self.check_cf_version("osm_version", OSM_TILE_VERSION, CF_OSM)?;
        self.check_cf_version("srtm_version", SRTM_TILE_VERSION, CF_SRTM)?;
        self.check_cf_version("terrain_version", TERRAIN_TILE_VERSION, CF_TERRAIN)?;
        self.check_cf_version(
            "pass_terrain_version",
            PASS_TERRAIN_VERSION as u32,
            CF_PASS_TERRAIN,
        )?;
        self.check_cf_version(
            "pass_substrate_version",
            PASS_SUBSTRATE_VERSION as u32,
            CF_PASS_SUBSTRATE,
        )?;
        self.check_cf_version(
            "pass_hydro_version",
            PASS_HYDRO_VERSION as u32,
            CF_PASS_HYDRO,
        )?;
        self.check_cf_version(
            "pass_roads_version",
            PASS_ROADS_VERSION as u32,
            CF_PASS_ROADS,
        )?;
        self.check_cf_version(
            "pass_buildings_version",
            PASS_BUILDINGS_VERSION as u32,
            CF_PASS_BUILDINGS,
        )?;
        Ok(())
    }

    fn check_cf_version(&self, meta_key: &str, expected: u32, cf_name: &str) -> Result<(), String> {
        let meta_cf = self.db.db.cf_handle(CF_META).ok_or("meta CF missing")?;
        let stored = self
            .db
            .db
            .get_cf(&meta_cf, meta_key)
            .map_err(|e| e.to_string())?
            .and_then(|b| b.try_into().ok().map(u32::from_be_bytes))
            .unwrap_or(0);

        if stored != expected {
            // Stale version — wipe the CF by deleting all keys in range
            let cf = self
                .db
                .db
                .cf_handle(cf_name)
                .ok_or(format!("{cf_name} CF missing"))?;
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
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else {
            return;
        };
        let existed = self
            .db
            .db
            .get_cf(&cf, osm_key(s, w, n, e))
            .ok()
            .flatten()
            .is_some();
        let _ = self
            .db
            .db
            .put_cf(&cf, osm_key(s, w, n, e), make_value(data));
        if !existed {
            self.inc_count(CF_OSM);
        }
    }

    pub fn has_osm(&self, s: f64, w: f64, n: f64, e: f64) -> bool {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else {
            return false;
        };
        self.db
            .db
            .get_cf(&cf, osm_key(s, w, n, e))
            .ok()
            .flatten()
            .is_some()
    }

    pub fn delete_osm(&self, s: f64, w: f64, n: f64, e: f64) {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else {
            return;
        };
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
        let Some(cf) = self.db.db.cf_handle(CF_SRTM) else {
            return;
        };
        let existed = self
            .db
            .db
            .get_cf(&cf, srtm_key(lat, lon))
            .ok()
            .flatten()
            .is_some();
        let _ = self.db.db.put_cf(&cf, srtm_key(lat, lon), make_value(data));
        if !existed {
            self.inc_count(CF_SRTM);
        }
    }

    pub fn has_srtm(&self, lat: i32, lon: i32) -> bool {
        let Some(cf) = self.db.db.cf_handle(CF_SRTM) else {
            return false;
        };
        self.db
            .db
            .get_cf(&cf, srtm_key(lat, lon))
            .ok()
            .flatten()
            .is_some()
    }

    pub fn delete_srtm(&self, lat: i32, lon: i32) {
        let Some(cf) = self.db.db.cf_handle(CF_SRTM) else {
            return;
        };
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
        let Some(cf) = self.db.db.cf_handle(CF_TERRAIN) else {
            return;
        };
        let existed = self
            .db
            .db
            .get_cf(&cf, terrain_key(cx, cy, cz))
            .ok()
            .flatten()
            .is_some();
        let _ = self
            .db
            .db
            .put_cf(&cf, terrain_key(cx, cy, cz), make_value(data));
        if !existed {
            self.inc_count(CF_TERRAIN);
        }
    }

    pub fn has_terrain(&self, cx: i32, cy: i32, cz: i32) -> bool {
        let Some(cf) = self.db.db.cf_handle(CF_TERRAIN) else {
            return false;
        };
        self.db
            .db
            .get_cf(&cf, terrain_key(cx, cy, cz))
            .ok()
            .flatten()
            .is_some()
    }

    // ── Pass-based chunk storage ───────────────────────────────────────────────

    /// Get a chunk pass. Returns `None` on miss or version mismatch.
    /// Data format on disk: `[u16 version LE][payload bytes]`; the version prefix
    /// is stripped — callers receive only the payload.
    pub fn get_chunk_pass(&self, cx: i32, cy: i32, cz: i32, pass: PassId) -> Option<Vec<u8>> {
        let cf = self.db.db.cf_handle(pass.cf_name())?;
        let key = chunk_key(cx, cy, cz);
        let raw = self.db.db.get_cf(&cf, &key).ok()??;
        if raw.len() < 2 {
            return None;
        }
        let stored_version = u16::from_le_bytes([raw[0], raw[1]]);
        if stored_version != pass.current_version() {
            return None;
        }
        Some(raw[2..].to_vec())
    }

    /// Store a chunk pass. Prepends `[u16 version LE]` to the payload before writing.
    pub fn put_chunk_pass(&self, cx: i32, cy: i32, cz: i32, pass: PassId, data: &[u8]) {
        let Some(cf) = self.db.db.cf_handle(pass.cf_name()) else {
            return;
        };
        let key = chunk_key(cx, cy, cz);
        let mut buf = Vec::with_capacity(2 + data.len());
        buf.extend_from_slice(&pass.current_version().to_le_bytes());
        buf.extend_from_slice(data);
        let _ = self.db.db.put_cf(&cf, &key, &buf);
    }

    /// Check if a chunk pass exists and is current version.
    pub fn has_chunk_pass(&self, cx: i32, cy: i32, cz: i32, pass: PassId) -> bool {
        let Some(cf) = self.db.db.cf_handle(pass.cf_name()) else {
            return false;
        };
        let key = chunk_key(cx, cy, cz);
        let Ok(Some(raw)) = self.db.db.get_cf(&cf, &key) else {
            return false;
        };
        if raw.len() < 2 {
            return false;
        }
        let stored_version = u16::from_le_bytes([raw[0], raw[1]]);
        stored_version == pass.current_version()
    }

    /// Delete a specific pass for a chunk (e.g. to force re-generation of just that pass).
    pub fn delete_chunk_pass(&self, cx: i32, cy: i32, cz: i32, pass: PassId) {
        let Some(cf) = self.db.db.cf_handle(pass.cf_name()) else {
            return;
        };
        let key = chunk_key(cx, cy, cz);
        let _ = self.db.db.delete_cf(&cf, &key);
    }

    /// Returns which passes are currently stored and current-version for this chunk.
    pub fn chunk_pass_status(&self, cx: i32, cy: i32, cz: i32) -> Vec<(PassId, bool)> {
        let passes = [
            PassId::Terrain,
            PassId::Substrate,
            PassId::Hydro,
            PassId::Roads,
            PassId::Buildings,
            PassId::FeatureRules,
        ];
        passes
            .iter()
            .map(|&p| (p, self.has_chunk_pass(cx, cy, cz, p)))
            .collect()
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    /// Instant tile count read from a meta counter (no full scan).
    pub fn tile_count(&self, cf_name: &str) -> u64 {
        let Some(meta_cf) = self.db.db.cf_handle(CF_META) else {
            return 0;
        };
        let key = format!("{cf_name}_count");
        self.db
            .db
            .get_cf(&meta_cf, key.as_bytes())
            .ok()
            .flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(0)
    }

    pub fn osm_count(&self) -> u64 {
        self.tile_count(CF_OSM)
    }
    pub fn srtm_count(&self) -> u64 {
        self.tile_count(CF_SRTM)
    }
    pub fn terrain_count(&self) -> u64 {
        self.tile_count(CF_TERRAIN)
    }

    /// Iterate all stored OSM tile coordinates.
    /// Decodes each 16-byte key back to (s, w, n, e) float pairs.
    /// Used for DHT announce-all at startup.
    pub fn iter_osm_coords(&self) -> Vec<(f64, f64, f64, f64)> {
        let Some(cf) = self.db.db.cf_handle(CF_OSM) else {
            return vec![];
        };
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

    /// Iterate all current-version chunk coordinates stored for a specific pass.
    pub fn iter_chunk_pass_coords(&self, pass: PassId) -> Vec<(i32, i32, i32)> {
        let Some(cf) = self.db.db.cf_handle(pass.cf_name()) else {
            return vec![];
        };
        let mut result = Vec::new();
        for item in self.db.db.iterator_cf(&cf, rocksdb::IteratorMode::Start) {
            if let Ok((key, value)) = item {
                if key.len() != 12 || value.len() < 2 {
                    continue;
                }
                let stored_version = u16::from_le_bytes([value[0], value[1]]);
                if stored_version != pass.current_version() {
                    continue;
                }
                let cx = i32::from_le_bytes(key[0..4].try_into().unwrap());
                let cy = i32::from_le_bytes(key[4..8].try_into().unwrap());
                let cz = i32::from_le_bytes(key[8..12].try_into().unwrap());
                result.push((cx, cy, cz));
            }
        }
        result
    }

    /// Scan all tiles in a CF and verify checksums.
    /// Returns `(total, corrupt)` — corrupt entries are deleted automatically.
    pub fn verify_cf(&self, cf_name: &str) -> (u64, u64) {
        let Some(cf) = self.db.db.cf_handle(cf_name) else {
            return (0, 0);
        };
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
            let Some(meta_cf) = self.db.db.cf_handle(CF_META) else {
                return (total, corrupt);
            };
            let new_count = total - corrupt;
            let count_key = format!("{cf_name}_count");
            let _ = self
                .db
                .db
                .put_cf(&meta_cf, count_key.as_bytes(), new_count.to_be_bytes());
        }
        (total, corrupt)
    }

    /// Wipe an entire column family (nuclear option — all tiles deleted).
    pub fn purge_cf(&self, cf_name: &str) -> Result<usize, String> {
        let cf = self
            .db
            .db
            .cf_handle(cf_name)
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
        let Some(meta_cf) = self.db.db.cf_handle(CF_META) else {
            return;
        };
        let key = format!("{cf_name}_count");
        let cur = self
            .db
            .db
            .get_cf(&meta_cf, key.as_bytes())
            .ok()
            .flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(0);
        let _ = self
            .db
            .db
            .put_cf(&meta_cf, key.as_bytes(), (cur + 1).to_be_bytes());
    }

    fn dec_count(&self, cf_name: &str) {
        let Some(meta_cf) = self.db.db.cf_handle(CF_META) else {
            return;
        };
        let key = format!("{cf_name}_count");
        let cur = self
            .db
            .db
            .get_cf(&meta_cf, key.as_bytes())
            .ok()
            .flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(1);
        let _ = self.db.db.put_cf(
            &meta_cf,
            key.as_bytes(),
            cur.saturating_sub(1).to_be_bytes(),
        );
    }
}

// ── PassId ─────────────────────────────────────────────────────────────────────

/// Identifies which generation pass a chunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassId {
    Terrain,
    Substrate,
    Hydro,
    Roads,
    Buildings,
    FeatureRules,
}

impl PassId {
    fn cf_name(&self) -> &'static str {
        match self {
            PassId::Terrain => CF_PASS_TERRAIN,
            PassId::Substrate => CF_PASS_SUBSTRATE,
            PassId::Hydro => CF_PASS_HYDRO,
            PassId::Roads => CF_PASS_ROADS,
            PassId::Buildings => CF_PASS_BUILDINGS,
            PassId::FeatureRules => CF_FEATURE_RULES,
        }
    }

    pub fn current_version(&self) -> u16 {
        match self {
            PassId::Terrain => PASS_TERRAIN_VERSION,
            PassId::Substrate => PASS_SUBSTRATE_VERSION,
            PassId::Hydro => PASS_HYDRO_VERSION,
            PassId::Roads => PASS_ROADS_VERSION,
            PassId::Buildings => PASS_BUILDINGS_VERSION,
            PassId::FeatureRules => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TileStore;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn concurrent_open_same_path_does_not_hit_rocksdb_lock_race() {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let rel_path = tempdir.path().join("tiles.db");
        let abs_path = tempdir
            .path()
            .canonicalize()
            .expect("canonical tempdir")
            .join("tiles.db");

        for _round in 0..4 {
            let start = Arc::new(Barrier::new(12));
            let hold = Arc::new(Barrier::new(12));
            let mut handles = Vec::new();

            for idx in 0..12 {
                let start = Arc::clone(&start);
                let hold = Arc::clone(&hold);
                let path = if idx % 2 == 0 {
                    rel_path.clone()
                } else {
                    abs_path.clone()
                };

                handles.push(thread::spawn(move || {
                    start.wait();
                    let store = TileStore::open(&path)
                        .unwrap_or_else(|e| panic!("concurrent TileStore::open failed: {e}"));
                    hold.wait();
                    store
                }));
            }

            for handle in handles {
                let _store = handle.join().expect("join concurrent open thread");
            }
        }
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
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("tiles");
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
    eprintln!(
        "🗑  Old tile dir detected → renamed to {:?}, deleting in background…",
        old_path_owned
    );

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
                    let ext = e
                        .path()
                        .extension()
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

    let dir_name = dir
        .file_name()
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
    eprintln!(
        "🗑  Old SRTM dir detected → renamed to {:?}, deleting in background…",
        old_path_owned
    );

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
