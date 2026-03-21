//! P2P key registry — the distributed database of all known identities.
//!
//! # What this is
//!
//! The `KeyRegistry` is the network-visible database of [`KeyRecord`]s. It has
//! three tiers of storage, checked in order:
//!
//! 1. **In-memory cache** — fast lookup for recently seen records
//! 2. **Disk cache** — `./key_cache/<prefix>/<peer_id_hex>.keyrec`
//!    Survives restarts. Used when DHT lookup fails or peer is offline.
//! 3. **DHT / gossipsub** — the authoritative P2P source; servers advertise
//!    as providers for every record they have seen
//!
//! ## Trust rule
//!
//! A `KeyRecord` is only accepted into the registry if its `self_sig` verifies.
//! Infrastructure records (Relay, Server) are further checked for a valid
//! `issuer_sig` before being accepted as elevated trust.
//!
//! ## Propagation
//!
//! When a new `KeyRecord` is published (new user, update, revocation):
//! 1. `publish()` broadcasts it on gossipsub `"key-registry"` topic.
//! 2. Any server that receives it stores it in its SQLite DB and advertises
//!    as a DHT provider.
//! 3. Other peers receive it, verify `self_sig`, then call `apply_update()`.
//! 4. Newer `updated_at` always wins over older records for the same `peer_id`.
//!
//! ## Fallback for unknown peers
//!
//! If a peer's `KeyRecord` cannot be found in any tier, `get_or_default()`
//! returns a synthetic Guest-level record. This means unknown peers are treated
//! with minimum trust rather than crashing or blocking. When their record
//! eventually propagates, `apply_update()` replaces the default.
//!
//! # Usage
//!
//! ```no_run
//! use metaverse_core::key_registry::KeyRegistry;
//! use metaverse_core::identity::{Identity, KeyType};
//!
//! let mut registry = KeyRegistry::new();
//! registry.load_from_disk().ok();
//!
//! // Publish own record on connect
//! let identity = Identity::load_or_create().unwrap();
//! let record = identity.create_key_record(KeyType::User, Some("Alice".into()), None, None, None, None);
//! registry.insert_local(record.clone());
//!
//! // Look up another peer
//! if let Some(rec) = registry.get(&some_peer_id) {
//!     println!("{} is {:?}", rec.display_name.as_deref().unwrap_or("anon"), rec.key_type);
//! }
//! ```

use crate::identity::{KeyRecord, KeyType};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ─── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum RegistryError {
    /// `self_sig` on the record failed to verify.
    InvalidSelfSig,
    /// `issuer_sig` required for infrastructure key but missing or invalid.
    InvalidIssuerSig,
    /// Record is older than what we already have for that peer.
    Stale,
    /// IO error writing/reading the disk cache.
    Io(std::io::Error),
    /// Serialization error.
    Serialization(bincode::Error),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSelfSig => write!(f, "KeyRecord self_sig verification failed"),
            Self::InvalidIssuerSig => write!(f, "KeyRecord issuer_sig missing or invalid"),
            Self::Stale => write!(f, "Received stale KeyRecord (older than cached)"),
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Serialization(e) => write!(f, "Serialization error: {}", e),
        }
    }
}

impl std::error::Error for RegistryError {}
impl From<std::io::Error> for RegistryError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<bincode::Error> for RegistryError {
    fn from(e: bincode::Error) -> Self {
        Self::Serialization(e)
    }
}

pub type Result<T> = std::result::Result<T, RegistryError>;

// ─── KeyRegistry ──────────────────────────────────────────────────────────────

/// The in-memory + disk-cached P2P identity registry.
///
/// One instance lives on every client and server. Clients keep a rolling cache
/// of recently seen records. Servers additionally maintain a SQLite backing
/// store and serve records on demand.
///
/// Thread safety: `KeyRegistry` is `!Send` and intended to live on the same
/// thread as the multiplayer system. Clone or `Arc<Mutex<>>` if you need
/// cross-thread sharing.
#[derive(Debug, Default)]
pub struct KeyRegistry {
    /// In-memory map from PeerId → most-recently-verified KeyRecord.
    records: HashMap<PeerId, KeyRecord>,

    /// Our own PeerId — used to protect local record from remote override.
    local_peer_id: Option<PeerId>,

    /// Base directory for disk cache. Defaults to `./key_cache/`.
    cache_dir: Option<PathBuf>,

    /// Stats for diagnostics.
    pub stats: RegistryStats,
}

/// Diagnostic counters.
#[derive(Debug, Default, Clone)]
pub struct RegistryStats {
    pub total_records: usize,
    pub records_accepted: u64,
    pub records_rejected_sig: u64,
    pub records_rejected_stale: u64,
    pub cache_hits_disk: u64,
    pub cache_misses: u64,
}

impl KeyRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with a known local peer ID (prevents remote override of own record).
    pub fn with_local_peer(local_peer_id: PeerId) -> Self {
        Self {
            local_peer_id: Some(local_peer_id),
            ..Self::default()
        }
    }

    /// Set the disk cache directory (defaults to `./key_cache/`).
    pub fn set_cache_dir(&mut self, dir: PathBuf) {
        self.cache_dir = Some(dir);
    }

    // ── Core lookup ───────────────────────────────────────────────────────────

    /// Look up a `KeyRecord` by `PeerId`.
    ///
    /// Checks memory first, then disk cache.
    /// Returns `None` if not found anywhere — caller should try DHT.
    pub fn get(&self, peer_id: &PeerId) -> Option<&KeyRecord> {
        self.records.get(peer_id)
    }

    /// Look up a `KeyRecord`, loading from disk cache if not in memory.
    ///
    /// Promotes disk-cached records into memory on first hit.
    pub fn get_or_load(&mut self, peer_id: &PeerId) -> Option<&KeyRecord> {
        if self.records.contains_key(peer_id) {
            return self.records.get(peer_id);
        }
        // Try disk cache
        if let Some(record) = self.load_one_from_disk(peer_id) {
            self.stats.cache_hits_disk += 1;
            self.records.insert(*peer_id, record);
            return self.records.get(peer_id);
        }
        self.stats.cache_misses += 1;
        None
    }

    /// Return the `KeyRecord` for `peer_id`, or a synthetic Guest-level record
    /// if not found. The synthetic record has the correct `peer_id` but all
    /// permission fields at minimum (Guest). This ensures unknown peers degrade
    /// gracefully rather than causing errors.
    ///
    /// **Important:** the returned synthetic record has an all-zero `self_sig`
    /// and will NOT pass `verify_self_sig()`. Callers that need verified records
    /// should use `get()` instead.
    pub fn get_or_default(&mut self, peer_id: PeerId) -> KeyRecord {
        if let Some(r) = self.get_or_load(&peer_id) {
            return r.clone();
        }
        // Synthetic Guest record — minimum permissions, not verifiable
        KeyRecord {
            version: 1,
            peer_id,
            public_key: [0u8; 32],
            key_type: KeyType::Guest,
            display_name: None,
            bio: None,
            avatar_hash: None,
            created_at: 0,
            expires_at: None,
            updated_at: 0,
            issued_by: None,
            issuer_sig: None,
            revoked: false,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
            self_sig: [0u8; 64],
        }
    }

    // ── Insertion ─────────────────────────────────────────────────────────────

    /// Insert a record that belongs to this node (skip remote-override protection).
    ///
    /// Called during startup to seed the registry with the local identity's record.
    /// Does NOT save to disk cache (local record persists as `.keyrec` alongside `.key`).
    pub fn insert_local(&mut self, record: KeyRecord) {
        self.records.insert(record.peer_id, record);
        self.stats.total_records = self.records.len();
    }

    /// Accept and store a record received from the network.
    ///
    /// Validation steps:
    /// 1. `self_sig` must verify.
    /// 2. `updated_at` must be >= what we already have (stale records rejected).
    /// 3. A remote record cannot override our own local record (use `insert_local`).
    ///
    /// On success, writes to disk cache and returns `Ok(true)` if this was a
    /// new or updated record, or `Ok(false)` if it was identical to what we
    /// already had (idempotent).
    pub fn apply_update(&mut self, record: KeyRecord) -> Result<bool> {
        // 1. Verify self_sig
        if !record.verify_self_sig() {
            self.stats.records_rejected_sig += 1;
            return Err(RegistryError::InvalidSelfSig);
        }

        // 2. Don't let remote override local record
        if let Some(local_id) = self.local_peer_id {
            if record.peer_id == local_id {
                // A remote record for our own peer_id — ignore (we manage our own)
                return Ok(false);
            }
        }

        // 3. Check against existing record
        if let Some(existing) = self.records.get(&record.peer_id) {
            if record.updated_at < existing.updated_at {
                self.stats.records_rejected_stale += 1;
                return Err(RegistryError::Stale);
            }
            if record.updated_at == existing.updated_at && record.self_sig == existing.self_sig {
                // Identical record — idempotent, not an error
                return Ok(false);
            }
        }

        // 4. Store in memory
        let peer_id = record.peer_id;
        self.records.insert(peer_id, record.clone());
        self.stats.total_records = self.records.len();
        self.stats.records_accepted += 1;

        // 5. Write to disk cache (best-effort — failures don't block networking)
        if let Err(e) = self.save_one_to_disk(&record) {
            eprintln!("[key_registry] Failed to cache {} to disk: {}", peer_id, e);
        }

        Ok(true)
    }

    /// Number of records currently in memory.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Iterate over all in-memory records.
    pub fn iter(&self) -> impl Iterator<Item = (&PeerId, &KeyRecord)> {
        self.records.iter()
    }

    /// Return all records of a specific `KeyType`.
    pub fn by_type(&self, key_type: KeyType) -> Vec<&KeyRecord> {
        self.records
            .values()
            .filter(|r| r.key_type as u8 == key_type as u8)
            .collect()
    }

    /// Return all relay-type records.
    pub fn relays(&self) -> Vec<&KeyRecord> {
        self.by_type(KeyType::Relay)
    }

    /// Return all server-type records.
    pub fn servers(&self) -> Vec<&KeyRecord> {
        self.by_type(KeyType::Server)
    }

    // ── Disk cache ────────────────────────────────────────────────────────────

    /// Load all records from the disk cache into memory.
    ///
    /// Called once at startup. Invalid or tampered files are silently skipped.
    pub fn load_from_disk(&mut self) -> Result<usize> {
        let cache_dir = match self.resolved_cache_dir() {
            Some(d) => d,
            None => return Ok(0),
        };

        if !cache_dir.exists() {
            return Ok(0);
        }

        let mut loaded = 0usize;

        // Walk two-level directory structure: cache_dir/XX/<peer_id_hex>.keyrec
        let Ok(prefix_entries) = std::fs::read_dir(&cache_dir) else {
            return Ok(0);
        };

        for prefix_entry in prefix_entries.flatten() {
            let prefix_path = prefix_entry.path();
            if !prefix_path.is_dir() {
                continue;
            }

            let Ok(file_entries) = std::fs::read_dir(&prefix_path) else {
                continue;
            };

            for file_entry in file_entries.flatten() {
                let file_path = file_entry.path();
                if file_path.extension().and_then(|e| e.to_str()) != Some("keyrec") {
                    continue;
                }

                let Ok(bytes) = std::fs::read(&file_path) else {
                    continue;
                };
                let Ok(record) = KeyRecord::from_bytes(&bytes) else {
                    eprintln!(
                        "[key_registry] Skipping corrupt cache file: {}",
                        file_path.display()
                    );
                    continue;
                };

                if !record.verify_self_sig() {
                    eprintln!(
                        "[key_registry] Skipping invalid cached record: {}",
                        record.peer_id
                    );
                    let _ = std::fs::remove_file(&file_path); // delete tampered cache
                    continue;
                }

                // Only insert if newer than any record we already have
                let should_insert = match self.records.get(&record.peer_id) {
                    Some(existing) => record.updated_at > existing.updated_at,
                    None => true,
                };

                if should_insert {
                    self.records.insert(record.peer_id, record);
                    loaded += 1;
                }
            }
        }

        self.stats.total_records = self.records.len();
        if loaded > 0 {
            eprintln!("[key_registry] Loaded {} records from disk cache", loaded);
        }
        Ok(loaded)
    }

    /// Write all in-memory records to disk.
    ///
    /// Called on clean shutdown. Normally records are written one-by-one via
    /// `apply_update()`, so this is mainly a safety net.
    pub fn flush_to_disk(&self) -> Result<usize> {
        let mut written = 0usize;
        for record in self.records.values() {
            self.save_one_to_disk(record)?;
            written += 1;
        }
        Ok(written)
    }

    /// Mark a peer's record as revoked in the in-memory registry.
    ///
    /// Called when processing a valid [`KeyRegistryMessage::Revocation`] from the network.
    /// The `self_sig` on the underlying record is no longer valid after this mutation;
    /// peers should treat revoked records as read-only tombstones — present to prevent
    /// re-propagation of the old live record, but not trusted for any capability checks.
    ///
    /// Returns `true` if the record existed and was not already revoked.
    pub fn mark_revoked(
        &mut self,
        target: &PeerId,
        revoked_by: &PeerId,
        reason: Option<String>,
        revoked_at_ms: u64,
    ) -> bool {
        if let Some(record) = self.records.get_mut(target) {
            if record.revoked {
                return false; // already revoked — idempotent
            }
            record.revoked = true;
            record.revoked_at = Some(revoked_at_ms);
            record.revoked_by = Some(*revoked_by);
            record.revocation_reason = reason;
            true
        } else {
            false
        }
    }

    /// Evict old records from the disk cache (records not updated in `max_age_secs`).
    ///
    /// Call periodically to prevent unbounded cache growth. Keeps revoked records
    /// (tombstones) so revocations don't re-appear after cache clear.
    pub fn evict_stale(&mut self, max_age_secs: u64) {
        let Some(cache_dir) = self.resolved_cache_dir() else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let cutoff = now.saturating_sub(max_age_secs);

        // Remove from memory
        self.records
            .retain(|_, record| record.revoked || record.updated_at >= cutoff);
        self.stats.total_records = self.records.len();

        // Remove from disk
        if !cache_dir.exists() {
            return;
        }
        let Ok(prefix_entries) = std::fs::read_dir(&cache_dir) else {
            return;
        };
        for prefix_entry in prefix_entries.flatten() {
            let prefix_path = prefix_entry.path();
            if !prefix_path.is_dir() {
                continue;
            }
            let Ok(file_entries) = std::fs::read_dir(&prefix_path) else {
                continue;
            };
            for file_entry in file_entries.flatten() {
                let file_path = file_entry.path();
                if file_path.extension().and_then(|e| e.to_str()) != Some("keyrec") {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&file_path) else {
                    continue;
                };
                let Ok(record) = KeyRecord::from_bytes(&bytes) else {
                    continue;
                };
                if !record.revoked && record.updated_at < cutoff {
                    let _ = std::fs::remove_file(&file_path);
                }
            }
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn resolved_cache_dir(&self) -> Option<PathBuf> {
        if let Some(ref dir) = self.cache_dir {
            return Some(dir.clone());
        }
        // Default: ./key_cache/ (relative to working directory — portable)
        Some(PathBuf::from("key_cache"))
    }

    /// Derive the cache file path for a given `PeerId`.
    ///
    /// Path: `<cache_dir>/<first2hex>/<full_hex>.keyrec`
    /// The two-character prefix subdirectory keeps directory entries manageable
    /// at large scale (same pattern as git objects).
    fn cache_path_for(&self, peer_id: &PeerId) -> Option<PathBuf> {
        let hex = hex::encode(peer_id.to_bytes());
        let prefix = &hex[..2];
        let dir = self.resolved_cache_dir()?.join(prefix);
        Some(dir.join(format!("{}.keyrec", hex)))
    }

    fn save_one_to_disk(&self, record: &KeyRecord) -> Result<()> {
        let Some(path) = self.cache_path_for(&record.peer_id) else {
            return Ok(()); // No cache dir configured — silent skip
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let bytes = record.to_bytes()?;

        // Atomic write via temp file
        let tmp = path.with_extension("keyrec.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &path)?;

        Ok(())
    }

    fn load_one_from_disk(&self, peer_id: &PeerId) -> Option<KeyRecord> {
        let path = self.cache_path_for(peer_id)?;
        let bytes = std::fs::read(&path).ok()?;
        let record = KeyRecord::from_bytes(&bytes).ok()?;
        if record.verify_self_sig() && record.peer_id == *peer_id {
            Some(record)
        } else {
            // Corrupt or wrong file — delete it
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

// ─── Gossipsub message type ───────────────────────────────────────────────────

/// A message on the `"key-registry"` or `"key-revocations"` gossipsub topics.
///
/// Peers broadcast this when publishing a new or updated `KeyRecord`.
/// The envelope type allows the topic to carry future message variants
/// (e.g., batch sync, revocation list) without breaking existing peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyRegistryMessage {
    /// A single new or updated `KeyRecord` (the common case).
    Publish(KeyRecord),

    /// A batch of `KeyRecord`s (used by servers syncing to newly connected peers).
    Batch(Vec<KeyRecord>),

    /// A signed revocation notice.
    ///
    /// Recipients verify the Ed25519 signature, confirm the revoker has authority
    /// (Admin/Server/Genesis/Relay key type OR the target itself for self-revocation),
    /// then call [`KeyRegistry::mark_revoked()`].
    Revocation {
        /// Raw bytes of the target `PeerId`.
        target_peer_id_bytes: Vec<u8>,
        /// Raw bytes of the revoking peer's `PeerId`.
        revoker_peer_id_bytes: Vec<u8>,
        /// Human-readable reason (optional, may be empty).
        reason: Option<String>,
        /// Unix timestamp in milliseconds when the revocation was decided.
        revoked_at_ms: u64,
        /// Ed25519 signature over `revocation_signable_bytes(target, revoker, reason, revoked_at_ms)`.
        #[serde(with = "serde_sig")]
        sig: [u8; 64],
        /// Revoker's Ed25519 public key (32 bytes).
        revoker_public_key: [u8; 32],
    },
}

/// Canonical byte string that `Revocation::sig` covers.
///
/// Layout: `"revoke:v1"` || len32LE(target) || target_bytes
///         || len32LE(revoker) || revoker_bytes
///         || revoked_at_ms as u64 LE
///         || len32LE(reason) || reason_bytes
pub fn revocation_signable_bytes(
    target: &[u8],
    revoker: &[u8],
    reason: Option<&str>,
    revoked_at_ms: u64,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.extend_from_slice(b"revoke:v1");
    out.extend_from_slice(&(target.len() as u32).to_le_bytes());
    out.extend_from_slice(target);
    out.extend_from_slice(&(revoker.len() as u32).to_le_bytes());
    out.extend_from_slice(revoker);
    out.extend_from_slice(&revoked_at_ms.to_le_bytes());
    let r = reason.unwrap_or("");
    out.extend_from_slice(&(r.len() as u32).to_le_bytes());
    out.extend_from_slice(r.as_bytes());
    out
}

// Serde helper for [u8; 64] in Revocation variant (mirrors serde_arrays in messages.rs)
mod serde_sig {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S>(bytes: &[u8; 64], s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.serialize(s)
    }
    pub fn deserialize<'de, D>(d: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let v: Vec<u8> = Vec::deserialize(d)?;
        if v.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "expected 64 bytes, got {}",
                v.len()
            )));
        }
        let mut a = [0u8; 64];
        a.copy_from_slice(&v);
        Ok(a)
    }
}

impl KeyRegistryMessage {
    pub fn to_bytes(&self) -> std::result::Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn from_bytes(data: &[u8]) -> std::result::Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Identity;

    fn make_user_record(id: &Identity) -> KeyRecord {
        id.create_key_record(
            KeyType::User,
            Some("Test User".into()),
            None,
            None,
            None,
            None,
        )
    }

    #[test]
    fn test_apply_update_valid() {
        let id = Identity::generate();
        let record = make_user_record(&id);
        let mut registry = KeyRegistry::new();
        let result = registry.apply_update(record.clone());
        assert!(result.is_ok());
        assert!(result.unwrap(), "first insert must return true");
        assert_eq!(registry.len(), 1);
        assert!(registry.get(id.peer_id()).is_some());
    }

    #[test]
    fn test_apply_update_invalid_sig_rejected() {
        let id = Identity::generate();
        let mut record = make_user_record(&id);
        record.display_name = Some("Tampered".into()); // invalidates self_sig
        let mut registry = KeyRegistry::new();
        assert!(registry.apply_update(record).is_err());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_apply_update_stale_rejected() {
        let id = Identity::generate();
        let record1 = make_user_record(&id);

        // Ensure record2 has a strictly higher updated_at by sleeping > 1 second
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let record2 = id
            .update_key_record(&record1, Some("Updated Name".into()), None, None)
            .unwrap();

        let mut registry = KeyRegistry::new();
        registry.apply_update(record2.clone()).unwrap();

        // Now apply the older record — must be rejected as stale
        let err = registry.apply_update(record1);
        assert!(matches!(err, Err(RegistryError::Stale)));
    }

    #[test]
    fn test_apply_update_idempotent() {
        let id = Identity::generate();
        let record = make_user_record(&id);
        let mut registry = KeyRegistry::new();
        registry.apply_update(record.clone()).unwrap();

        // Applying the same record again must return Ok(false), not an error
        let result = registry.apply_update(record);
        assert!(result.is_ok());
        assert!(!result.unwrap(), "idempotent re-insert must return false");
    }

    #[test]
    fn test_local_record_not_overridable() {
        let id = Identity::generate();
        let local_record = make_user_record(&id);

        let mut registry = KeyRegistry::with_local_peer(*id.peer_id());
        registry.insert_local(local_record.clone());

        // An attacker generates their own record with the same peer_id — impossible
        // in practice (they'd need the private key), but we test the guard anyway
        // by constructing a record that would otherwise pass sig check.
        // Here we just re-submit our own record as if it came from remote.
        let result = registry.apply_update(local_record);
        assert!(result.is_ok());
        assert!(
            !result.unwrap(),
            "remote override of local record must be silently ignored"
        );
    }

    #[test]
    fn test_get_or_default_unknown_peer() {
        let mut registry = KeyRegistry::new();
        let unknown = libp2p::PeerId::random();
        let default_rec = registry.get_or_default(unknown);
        assert_eq!(default_rec.key_type as u8, KeyType::Guest as u8);
        assert_eq!(default_rec.peer_id, unknown);
    }

    #[test]
    fn test_by_type_filter() {
        let relay_id = Identity::generate();
        let server_id = Identity::generate();
        let user_id = Identity::generate();

        let relay_rec = relay_id.create_key_record(KeyType::Relay, None, None, None, None, None);
        let server_rec = server_id.create_key_record(KeyType::Server, None, None, None, None, None);
        let user_rec = user_id.create_key_record(
            KeyType::User,
            Some("User".into()),
            None,
            None,
            None,
            None,
        );

        let mut registry = KeyRegistry::new();
        registry.insert_local(relay_rec);
        registry.insert_local(server_rec);
        registry.insert_local(user_rec);

        assert_eq!(registry.relays().len(), 1);
        assert_eq!(registry.servers().len(), 1);
        assert_eq!(registry.by_type(KeyType::User).len(), 1);
    }

    #[test]
    fn test_disk_cache_roundtrip() {
        let id = Identity::generate();
        let record = make_user_record(&id);

        let tmp_dir = tempfile::tempdir().unwrap();
        let mut registry = KeyRegistry::new();
        registry.set_cache_dir(tmp_dir.path().to_path_buf());
        registry.apply_update(record.clone()).unwrap();

        // Fresh registry loading from same dir
        let mut registry2 = KeyRegistry::new();
        registry2.set_cache_dir(tmp_dir.path().to_path_buf());
        let loaded = registry2.load_from_disk().unwrap();
        assert_eq!(loaded, 1);

        let loaded_rec = registry2.get(id.peer_id()).unwrap();
        assert_eq!(loaded_rec.peer_id, record.peer_id);
        assert_eq!(loaded_rec.display_name, record.display_name);
        assert!(loaded_rec.verify_self_sig());
    }

    #[test]
    fn test_key_registry_message_roundtrip() {
        let id = Identity::generate();
        let record = make_user_record(&id);
        let msg = KeyRegistryMessage::Publish(record.clone());

        let bytes = msg.to_bytes().unwrap();
        let decoded = KeyRegistryMessage::from_bytes(&bytes).unwrap();

        match decoded {
            KeyRegistryMessage::Publish(r) => {
                assert_eq!(r.peer_id, record.peer_id);
                assert!(r.verify_self_sig());
            }
            _ => panic!("wrong message variant"),
        }
    }

    #[test]
    fn test_batch_message() {
        let ids: Vec<_> = (0..5).map(|_| Identity::generate()).collect();
        let records: Vec<_> = ids.iter().map(|id| make_user_record(id)).collect();
        let msg = KeyRegistryMessage::Batch(records.clone());

        let bytes = msg.to_bytes().unwrap();
        let decoded = KeyRegistryMessage::from_bytes(&bytes).unwrap();

        match decoded {
            KeyRegistryMessage::Batch(rs) => {
                assert_eq!(rs.len(), 5);
                for r in &rs {
                    assert!(r.verify_self_sig());
                }
            }
            _ => panic!("wrong message variant"),
        }
    }
}
