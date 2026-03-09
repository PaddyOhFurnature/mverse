//! RocksDB-backed persistent world state store.
//!
//! # Layout
//! Single RocksDB database (`world.db`) with five column families:
//!
//! | CF             | Key                                       | Value                              |
//! |----------------|-------------------------------------------|------------------------------------|
//! | `voxel_ops`    | `[cx_i32][cy_i32][cz_i32][lamport_u64]`  | `[sig_64B][bincode SignedOperation]`|
//! | `parcels`      | `[min_x][min_y][min_z][max_x][max_y][max_z]` i32 BE | `[peer_id bytes]`     |
//! | `access_grants`| `[parcel_24B][peer_id_bytes]`             | `[allow_u8]`                       |
//! | `players`      | `[peer_id bytes]`                         | `[bincode PlayerState]`            |
//! | `meta`         | UTF-8 string                              | raw bytes                          |
//!
//! # Voxel op key design
//! The first 12 bytes of every `voxel_ops` key encode the chunk coordinates.
//! A RocksDB prefix extractor is set to 12 bytes, enabling efficient prefix
//! scans: `ops_for_chunk(cx, cy, cz)` reads only that chunk's entries without
//! touching any other chunk's data.
//!
//! # Write batching
//! `queue_op()` pushes to an in-memory queue. `flush_pending()` commits all
//! queued ops as a single `WriteBatch`. Call `flush_pending()` on a ~100ms timer
//! from the server tick loop — one DB write per tick regardless of op volume.
//!
//! # One-time migration
//! On first open, `meta["ops_migrated"]` is absent. The store will scan
//! `world_data/chunks/**/operations.bin`, import all `SignedOperation` vecs,
//! then set `meta["ops_migrated"] = 1`. Subsequent opens skip this.

use std::path::Path;
use std::sync::{Arc, Mutex};
use rocksdb::{
    DB, Options, ColumnFamilyDescriptor, BlockBasedOptions, Cache,
    SliceTransform, WriteBatch,
};

const CF_VOXEL_OPS:    &str = "voxel_ops";
const CF_PARCELS:      &str = "parcels";
const CF_ACCESS:       &str = "access_grants";
const CF_PLAYERS:      &str = "players";
const CF_META:         &str = "meta";

pub const WORLD_SCHEMA_VERSION: u32 = 1;

// ── Key encoding ───────────────────────────────────────────────────────────────

/// 20-byte voxel_ops key: chunk coords (12B) + Lamport timestamp (8B).
pub fn voxel_op_key(cx: i32, cy: i32, cz: i32, lamport: u64) -> [u8; 20] {
    let mut k = [0u8; 20];
    k[0..4].copy_from_slice(&cx.to_be_bytes());
    k[4..8].copy_from_slice(&cy.to_be_bytes());
    k[8..12].copy_from_slice(&cz.to_be_bytes());
    k[12..20].copy_from_slice(&lamport.to_be_bytes());
    k
}

/// 12-byte prefix covering only the chunk coordinates part of a voxel_op_key.
pub fn chunk_prefix(cx: i32, cy: i32, cz: i32) -> [u8; 12] {
    let mut k = [0u8; 12];
    k[0..4].copy_from_slice(&cx.to_be_bytes());
    k[4..8].copy_from_slice(&cy.to_be_bytes());
    k[8..12].copy_from_slice(&cz.to_be_bytes());
    k
}

/// 24-byte parcel key from (min, max) integer bounds.
pub fn parcel_key(min_x: i32, min_y: i32, min_z: i32, max_x: i32, max_y: i32, max_z: i32) -> [u8; 24] {
    let mut k = [0u8; 24];
    k[0..4].copy_from_slice(&min_x.to_be_bytes());
    k[4..8].copy_from_slice(&min_y.to_be_bytes());
    k[8..12].copy_from_slice(&min_z.to_be_bytes());
    k[12..16].copy_from_slice(&max_x.to_be_bytes());
    k[16..20].copy_from_slice(&max_y.to_be_bytes());
    k[20..24].copy_from_slice(&max_z.to_be_bytes());
    k
}

// ── Pending op queue ───────────────────────────────────────────────────────────

/// A single queued write: chunk coords + Lamport key + raw value bytes.
struct PendingOp {
    cx: i32, cy: i32, cz: i32,
    lamport: u64,
    value: Vec<u8>,
}

// ── RocksDB helpers ─────────────────────────────────────────────────────────────

fn voxel_ops_opts() -> Options {
    let mut o = Options::default();
    // Prefix extractor: first 12 bytes = chunk coordinates.
    // Enables seek_for_prev / prefix_same_as_start optimisation.
    o.set_prefix_extractor(SliceTransform::create_fixed_prefix(12));
    o.set_compression_type(rocksdb::DBCompressionType::Lz4);
    // Bloom filter on prefix (not full key) for fast "does this chunk have ops?" check
    let mut bb = BlockBasedOptions::default();
    let cache = Cache::new_lru_cache(32 * 1024 * 1024);
    bb.set_block_cache(&cache);
    bb.set_whole_key_filtering(false); // prefix-only bloom
    bb.set_bloom_filter(10.0, false);
    o.set_block_based_table_factory(&bb);
    o.set_write_buffer_size(32 * 1024 * 1024);
    o
}

fn small_cf_opts() -> Options {
    let mut o = Options::default();
    o.set_compression_type(rocksdb::DBCompressionType::Zstd);
    let mut bb = BlockBasedOptions::default();
    let cache = Cache::new_lru_cache(8 * 1024 * 1024);
    bb.set_block_cache(&cache);
    o.set_block_based_table_factory(&bb);
    o
}

fn meta_opts() -> Options {
    let mut o = Options::default();
    o.set_compression_type(rocksdb::DBCompressionType::None);
    o
}

// ── WorldStore ─────────────────────────────────────────────────────────────────

/// Shared handle to the world state database.
/// Cheap to clone — backed by `Arc`.
#[derive(Clone)]
pub struct WorldStore {
    db: Arc<DB>,
    pending: Arc<Mutex<Vec<PendingOp>>>,
}

impl WorldStore {
    /// Open (or create) the world database at `path`.
    /// Runs one-time migration from flat `operations.bin` files if needed.
    pub fn open(path: &Path, world_data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(path).map_err(|e| e.to_string())?;

        let mut db_opts = Options::default();
        db_opts.create_if_missing(true);
        db_opts.create_missing_column_families(true);

        let cf_descs = vec![
            ColumnFamilyDescriptor::new(CF_VOXEL_OPS, voxel_ops_opts()),
            ColumnFamilyDescriptor::new(CF_PARCELS,   small_cf_opts()),
            ColumnFamilyDescriptor::new(CF_ACCESS,    small_cf_opts()),
            ColumnFamilyDescriptor::new(CF_PLAYERS,   small_cf_opts()),
            ColumnFamilyDescriptor::new(CF_META,      meta_opts()),
        ];

        let db = DB::open_cf_descriptors(&db_opts, path, cf_descs)
            .map_err(|e| format!("WorldStore open failed: {e}"))?;

        let store = WorldStore {
            db: Arc::new(db),
            pending: Arc::new(Mutex::new(Vec::new())),
        };

        // Check schema version
        store.check_schema_version()?;

        // One-time migration from flat files
        store.migrate_flat_ops(world_data_dir);

        Ok(store)
    }

    fn check_schema_version(&self) -> Result<(), String> {
        let meta_cf = self.db.cf_handle(CF_META).ok_or("meta CF missing")?;
        let stored = self.db.get_cf(&meta_cf, b"world_schema_version")
            .map_err(|e| e.to_string())?
            .and_then(|b| b.try_into().ok().map(u32::from_be_bytes))
            .unwrap_or(0);

        if stored == 0 {
            // First open — write version
            self.db.put_cf(&meta_cf, b"world_schema_version", WORLD_SCHEMA_VERSION.to_be_bytes())
                .map_err(|e| e.to_string())?;
            eprintln!("WorldStore: initialised schema v{}", WORLD_SCHEMA_VERSION);
        } else if stored != WORLD_SCHEMA_VERSION {
            eprintln!("WorldStore: schema v{stored} → v{WORLD_SCHEMA_VERSION} (manual migration may be needed)");
        }
        Ok(())
    }

    /// One-time import of existing `world_data/chunks/**/operations.bin` flat files.
    fn migrate_flat_ops(&self, world_data_dir: &Path) {
        let Some(meta_cf) = self.db.cf_handle(CF_META) else { return };

        // Check migration flag
        let done = self.db.get_cf(&meta_cf, b"ops_migrated")
            .ok().flatten()
            .map(|b| b.first().copied().unwrap_or(0) == 1)
            .unwrap_or(false);

        if done { return; }

        let chunks_dir = world_data_dir.join("chunks");
        if !chunks_dir.exists() {
            let _ = self.db.put_cf(&meta_cf, b"ops_migrated", &[1u8]);
            return;
        }

        let mut imported = 0usize;
        let mut chunk_count = 0usize;

        // Walk world_data/chunks/<chunk_id>/operations.bin
        if let Ok(entries) = std::fs::read_dir(&chunks_dir) {
            for entry in entries.flatten() {
                let ops_file = entry.path().join("operations.bin");
                if !ops_file.exists() { continue; }

                let bytes = match std::fs::read(&ops_file) {
                    Ok(b) => b,
                    Err(_) => continue,
                };

                // Parse chunk coordinates from directory name: chunk_CX_CY_CZ
                let dir_name = entry.file_name();
                let dir_str = dir_name.to_string_lossy();
                let parts: Vec<&str> = dir_str.split('_').collect();
                let (cx, cy, cz) = if parts.len() >= 4 {
                    match (parts[1].parse::<i32>(), parts[2].parse::<i32>(), parts[3].parse::<i32>()) {
                        (Ok(x), Ok(y), Ok(z)) => (x, y, z),
                        _ => (0, 0, 0),
                    }
                } else { (0, 0, 0) };

                // Try deserialising as Vec<SignedOperation> (current format)
                let ops: Vec<crate::messages::SignedOperation> = 
                    bincode::deserialize(&bytes).unwrap_or_default();

                if ops.is_empty() { continue; }

                let Some(vox_cf) = self.db.cf_handle(CF_VOXEL_OPS) else { continue };
                let mut batch = WriteBatch::default();
                for op in &ops {
                    let key = voxel_op_key(cx, cy, cz, op.lamport);
                    // Value: signature (64 bytes if available) + bincode op
                    let op_bytes = bincode::serialize(op).unwrap_or_default();
                    let sig_bytes: [u8; 64] = op.signature;
                    let mut value = Vec::with_capacity(64 + op_bytes.len());
                    value.extend_from_slice(&sig_bytes);
                    value.extend_from_slice(&op_bytes);
                    batch.put_cf(&vox_cf, key, value);
                    imported += 1;
                }
                let _ = self.db.write(batch);
                chunk_count += 1;
            }
        }

        // Mark migration complete
        let _ = self.db.put_cf(&meta_cf, b"ops_migrated", &[1u8]);
        let _ = self.db.put_cf(&meta_cf, b"migrated_op_count",
            (imported as u64).to_be_bytes().as_ref());

        if imported > 0 {
            eprintln!("WorldStore: migrated {} ops from {} flat-file chunks", imported, chunk_count);
        }
    }

    // ── Voxel ops ─────────────────────────────────────────────────────────────

    /// Queue a voxel op for the next batch flush.
    /// This is non-blocking — the op is stored in memory until `flush_pending()` is called.
    pub fn queue_op(&self, cx: i32, cy: i32, cz: i32, lamport: u64,
                    sig: &[u8; 64], op_bytes: &[u8]) {
        let mut value = Vec::with_capacity(64 + op_bytes.len());
        value.extend_from_slice(sig);
        value.extend_from_slice(op_bytes);
        if let Ok(mut q) = self.pending.lock() {
            q.push(PendingOp { cx, cy, cz, lamport, value });
        }
    }

    /// Write all queued ops to RocksDB as a single WriteBatch.
    /// Call this every ~100ms from the server tick loop.
    /// Returns number of ops flushed.
    pub fn flush_pending(&self) -> usize {
        let ops = {
            let mut q = match self.pending.lock() {
                Ok(g) => g,
                Err(_) => return 0,
            };
            std::mem::take(&mut *q)
        };
        if ops.is_empty() { return 0; }

        let Some(cf) = self.db.cf_handle(CF_VOXEL_OPS) else { return 0 };
        let mut batch = WriteBatch::default();
        let count = ops.len();
        for op in ops {
            let key = voxel_op_key(op.cx, op.cy, op.cz, op.lamport);
            batch.put_cf(&cf, key, op.value);
        }
        let _ = self.db.write(batch);

        // Update op count in meta
        if let Some(meta_cf) = self.db.cf_handle(CF_META) {
            let cur = self.db.get_cf(&meta_cf, b"op_count")
                .ok().flatten()
                .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
                .unwrap_or(0);
            let _ = self.db.put_cf(&meta_cf, b"op_count",
                (cur + count as u64).to_be_bytes().as_ref());
        }
        count
    }

    /// Load all ops for a chunk in Lamport order (prefix scan).
    /// Returns raw `(lamport, value_bytes)` pairs. Caller deserialises.
    pub fn ops_for_chunk(&self, cx: i32, cy: i32, cz: i32) -> Vec<(u64, Vec<u8>)> {
        let Some(cf) = self.db.cf_handle(CF_VOXEL_OPS) else { return vec![] };
        let prefix = chunk_prefix(cx, cy, cz);
        let mut read_opts = rocksdb::ReadOptions::default();
        read_opts.set_prefix_same_as_start(true);
        let iter = self.db.iterator_cf_opt(&cf, read_opts, rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward));
        let mut result = Vec::new();
        for item in iter {
            if let Ok((key, value)) = item {
                if !key.starts_with(&prefix) { break; }
                if key.len() >= 20 {
                    let lamport = u64::from_be_bytes(key[12..20].try_into().unwrap_or([0u8; 8]));
                    result.push((lamport, value.to_vec()));
                }
            }
        }
        result
    }

    /// Count ops for a specific chunk.
    pub fn chunk_op_count(&self, cx: i32, cy: i32, cz: i32) -> usize {
        self.ops_for_chunk(cx, cy, cz).len()
    }

    // ── Parcels ───────────────────────────────────────────────────────────────

    pub fn set_parcel_owner(&self,
        min_x: i32, min_y: i32, min_z: i32,
        max_x: i32, max_y: i32, max_z: i32,
        peer_id_bytes: &[u8],
    ) {
        let Some(cf) = self.db.cf_handle(CF_PARCELS) else { return };
        let _ = self.db.put_cf(&cf, parcel_key(min_x, min_y, min_z, max_x, max_y, max_z), peer_id_bytes);
    }

    pub fn get_parcel_owner(&self,
        min_x: i32, min_y: i32, min_z: i32,
        max_x: i32, max_y: i32, max_z: i32,
    ) -> Option<Vec<u8>> {
        let cf = self.db.cf_handle(CF_PARCELS)?;
        self.db.get_cf(&cf, parcel_key(min_x, min_y, min_z, max_x, max_y, max_z)).ok()?
    }

    // ── Access grants ─────────────────────────────────────────────────────────

    pub fn set_access_grant(&self,
        min_x: i32, min_y: i32, min_z: i32,
        max_x: i32, max_y: i32, max_z: i32,
        peer_id_bytes: &[u8],
        allow: bool,
    ) {
        let Some(cf) = self.db.cf_handle(CF_ACCESS) else { return };
        let pk = parcel_key(min_x, min_y, min_z, max_x, max_y, max_z);
        let mut key = Vec::with_capacity(24 + peer_id_bytes.len());
        key.extend_from_slice(&pk);
        key.extend_from_slice(peer_id_bytes);
        let _ = self.db.put_cf(&cf, key, &[allow as u8]);
    }

    pub fn get_access_grant(&self,
        min_x: i32, min_y: i32, min_z: i32,
        max_x: i32, max_y: i32, max_z: i32,
        peer_id_bytes: &[u8],
    ) -> Option<bool> {
        let cf = self.db.cf_handle(CF_ACCESS)?;
        let pk = parcel_key(min_x, min_y, min_z, max_x, max_y, max_z);
        let mut key = Vec::with_capacity(24 + peer_id_bytes.len());
        key.extend_from_slice(&pk);
        key.extend_from_slice(peer_id_bytes);
        let val = self.db.get_cf(&cf, key).ok()??;
        Some(val.first().copied().unwrap_or(0) == 1)
    }

    // ── Players ───────────────────────────────────────────────────────────────

    /// Store player state bytes (bincode-serialised PlayerState).
    pub fn put_player(&self, peer_id_bytes: &[u8], state_bytes: &[u8]) {
        let Some(cf) = self.db.cf_handle(CF_PLAYERS) else { return };
        let _ = self.db.put_cf(&cf, peer_id_bytes, state_bytes);
    }

    pub fn get_player(&self, peer_id_bytes: &[u8]) -> Option<Vec<u8>> {
        let cf = self.db.cf_handle(CF_PLAYERS)?;
        self.db.get_cf(&cf, peer_id_bytes).ok()?
    }

    // ── Stats ─────────────────────────────────────────────────────────────────

    pub fn op_count(&self) -> u64 {
        let Some(meta_cf) = self.db.cf_handle(CF_META) else { return 0 };
        self.db.get_cf(&meta_cf, b"op_count")
            .ok().flatten()
            .and_then(|b| b.try_into().ok().map(u64::from_be_bytes))
            .unwrap_or(0)
    }

    pub fn parcel_count(&self) -> u64 {
        let Some(cf) = self.db.cf_handle(CF_PARCELS) else { return 0 };
        self.db.iterator_cf(&cf, rocksdb::IteratorMode::Start).count() as u64
    }

    pub fn player_count(&self) -> u64 {
        let Some(cf) = self.db.cf_handle(CF_PLAYERS) else { return 0 };
        self.db.iterator_cf(&cf, rocksdb::IteratorMode::Start).count() as u64
    }

    pub fn pending_count(&self) -> usize {
        self.pending.lock().map(|q| q.len()).unwrap_or(0)
    }
}
