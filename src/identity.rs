//! Cryptographic identity system for the P2P metaverse.
//!
//! # The Key IS the Identity
//!
//! Every non-deterministic action in the metaverse — placing a voxel, claiming a
//! parcel, trading an item, deploying a script, running a relay — is signed by an
//! Ed25519 private key. The key is your sovereignty. There is no server that can
//! recover it if lost. There is no password reset. The file IS you.
//!
//! ## Three core types in this module:
//!
//! - [`KeyType`] — the tier and role of a key (Guest through Server/Genesis)
//! - [`KeyRecord`] — the public, self-signed declaration of an identity, published
//!   to the P2P network via gossipsub and cached in the DHT
//! - [`Identity`] — the local keypair (private + public key) that never leaves the
//!   machine; used to sign operations and create [`KeyRecord`]s
//!
//! # Key Hierarchy
//!
//! ```text
//! Genesis  ── root trust anchor (cold storage, signs Server keys)
//!   └─ Server  ── world-state authority (signs Relay and Admin keys)
//!        ├─ Relay   ── routing infrastructure
//!        └─ Admin   ── region moderator
//! ─────────────────────────── (user keys, self-registered) ──────────────────
//! Business  ── organisation / brand identity
//! Personal  ── standard named user (full gameplay capabilities)
//! Anonymous ── pseudonymous, no parcel ownership
//! Guest     ── auto-generated, ephemeral, play-only
//! ```
//!
//! # KeyRecord propagation
//!
//! ```text
//! 1. Generate keypair locally (Identity::generate)
//! 2. Build and self-sign a KeyRecord (Identity::create_key_record)
//! 3. Publish to gossipsub "key-registry" topic
//! 4. Servers store in SQLite, advertise as DHT providers
//! 5. Other peers cache at ~/.metaverse/key_cache/<peer_id_hex>.keyrec
//! 6. Offline: local cache is authoritative when DHT unreachable
//! ```
//!
//! # Persistence
//!
//! - Keypair: `~/.metaverse/identity.key` (bincode, chmod 600)
//! - Key record: `~/.metaverse/identity.keyrec` (bincode, auto-generated alongside keypair)
//! - Env override: `METAVERSE_IDENTITY_FILE=~/.metaverse/mykey.key`
//!
//! **BACK UP YOUR KEY FILE. There is no recovery.**

use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use libp2p::identity::{Keypair as Libp2pKeypair, ed25519 as libp2p_ed25519};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ─── Serde helper for [u8; 64] ────────────────────────────────────────────────
// Serde only derives array impls up to [u8; 32]. We serialize 64-byte
// signatures as fixed-length byte sequences via this helper module.
mod serde_sig64 {
    use serde::{Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(bytes: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(bytes)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let v: Vec<u8> = serde::Deserialize::deserialize(d)?;
        v.try_into().map_err(|_| D::Error::custom("expected exactly 64 bytes for signature"))
    }
}

mod serde_opt_sig64 {
    use serde::{Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(opt: &Option<[u8; 64]>, s: S) -> Result<S::Ok, S::Error> {
        match opt {
            None => s.serialize_none(),
            Some(bytes) => s.serialize_some(&bytes.as_ref()),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[u8; 64]>, D::Error> {
        let opt: Option<Vec<u8>> = serde::Deserialize::deserialize(d)?;
        match opt {
            None => Ok(None),
            Some(v) => v.try_into()
                .map(Some)
                .map_err(|_| D::Error::custom("expected exactly 64 bytes for issuer_sig")),
        }
    }
}

// ─── Result / Error ───────────────────────────────────────────────────────────

/// Result type for identity operations.
pub type Result<T> = std::result::Result<T, IdentityError>;

/// Errors that can occur during identity operations.
#[derive(Debug)]
pub enum IdentityError {
    IoError(std::io::Error),
    SerializationError(bincode::Error),
    InvalidSignature,
    InvalidKeypair,
    DirectoryCreationFailed(std::io::Error),
    /// A KeyRecord failed self-signature verification on load.
    InvalidKeyRecord,
    /// Attempted an operation that requires a higher-trust key type.
    InsufficientKeyType { required: KeyType, actual: KeyType },
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::SerializationError(e) => write!(f, "Serialization error: {}", e),
            Self::InvalidSignature => write!(f, "Invalid signature"),
            Self::InvalidKeypair => write!(f, "Invalid keypair"),
            Self::DirectoryCreationFailed(e) => write!(f, "Failed to create directory: {}", e),
            Self::InvalidKeyRecord => write!(f, "KeyRecord self-signature verification failed"),
            Self::InsufficientKeyType { required, actual } =>
                write!(f, "Operation requires {:?} key; this key is {:?}", required, actual),
        }
    }
}

impl std::error::Error for IdentityError {}

impl From<std::io::Error> for IdentityError {
    fn from(e: std::io::Error) -> Self { Self::IoError(e) }
}
impl From<bincode::Error> for IdentityError {
    fn from(e: bincode::Error) -> Self { Self::SerializationError(e) }
}

// ─── KeyType ──────────────────────────────────────────────────────────────────

/// The tier and role of a cryptographic identity in the metaverse network.
///
/// Keys fall into two broad tiers:
/// - **Infrastructure keys** (Genesis, Server, Relay) require countersignature
///   from a higher-tier key and operate hardware nodes.
/// - **User keys** (Admin through Guest) are self-registered on the P2P network
///   with no central authority required.
///
/// See `docs/IDENTITY_SYSTEM.md` for the full permission table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum KeyType {
    /// Root trust anchor. Cold storage only. Signs Server keys.
    /// Never used for day-to-day operations.
    Genesis = 0,

    /// World-state authority node (`metaverse-server`).
    /// Signs Relay and Admin keys. Highest operational trust.
    Server = 1,

    /// Routing infrastructure node (`metaverse-relay`).
    /// Countersigned by a Server key. Trusted routing, no world state.
    Relay = 2,

    /// Region moderator. Granted by a Server key over a specific region.
    /// Can moderate, kick, ban in assigned areas.
    Admin = 3,

    /// Organisation or brand identity.
    /// Can own large parcels, delegate to staff via access grants.
    Business = 4,

    /// Standard named user. Full gameplay capabilities:
    /// own land, build, trade, create content, sign contracts.
    Personal = 5,

    /// Pseudonymous user. Consistent identity, no parcel ownership.
    /// Cannot sign contracts or claim permanent property.
    Anonymous = 6,

    /// Auto-generated on first run. Ephemeral (30-day expiry).
    /// Limited to free-build public zones.
    Guest = 7,
}

impl KeyType {
    /// Numeric discriminant used in canonical byte encoding.
    pub fn discriminant(self) -> u8 { self as u8 }

    /// Human-readable label shown in UI next to a player's name.
    pub fn display_label(self) -> &'static str {
        match self {
            KeyType::Genesis  => "[Genesis]",
            KeyType::Server   => "[Server]",
            KeyType::Relay    => "[Relay]",
            KeyType::Admin    => "[Admin]",
            KeyType::Business => "[Business]",
            KeyType::Personal => "",
            KeyType::Anonymous => "[Anon]",
            KeyType::Guest    => "[Guest]",
        }
    }

    /// True if this key type can own parcels permanently.
    pub fn can_own_parcels(self) -> bool {
        matches!(self, KeyType::Personal | KeyType::Business | KeyType::Server)
    }

    /// True if this key type can build in owned parcels.
    pub fn can_build_in_owned_parcels(self) -> bool {
        matches!(self, KeyType::Personal | KeyType::Business | KeyType::Admin | KeyType::Server)
    }

    /// True if this key type can sign commerce contracts.
    pub fn can_sign_contracts(self) -> bool {
        matches!(self, KeyType::Personal | KeyType::Business | KeyType::Server)
    }

    /// True if this key type requires a countersignature from a higher-tier key.
    pub fn requires_issuance(self) -> bool {
        matches!(self, KeyType::Genesis | KeyType::Server | KeyType::Relay | KeyType::Admin)
    }

    /// True if this key type is an infrastructure role (runs a hardware node).
    pub fn is_infrastructure(self) -> bool {
        matches!(self, KeyType::Genesis | KeyType::Server | KeyType::Relay)
    }

    /// Try to reconstruct from raw discriminant byte (used in canonical decoding).
    pub fn from_discriminant(b: u8) -> Option<Self> {
        match b {
            0 => Some(KeyType::Genesis),
            1 => Some(KeyType::Server),
            2 => Some(KeyType::Relay),
            3 => Some(KeyType::Admin),
            4 => Some(KeyType::Business),
            5 => Some(KeyType::Personal),
            6 => Some(KeyType::Anonymous),
            7 => Some(KeyType::Guest),
            _ => None,
        }
    }
}

impl std::fmt::Display for KeyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

// ─── KeyRecord ────────────────────────────────────────────────────────────────

/// The public, self-signed declaration of an identity on the P2P network.
///
/// A `KeyRecord` is the network-visible face of an [`Identity`]. It contains
/// no private data. Everything in it is public.
///
/// ## Trust chain
///
/// For **user keys** (Personal, Business, Anonymous, Guest):
/// - `self_sig` is the only signature required.
/// - No authority needs to issue it; users self-register.
///
/// For **infrastructure keys** (Relay, Admin, Server):
/// - `self_sig` proves key possession.
/// - `issued_by` + `issuer_sig` provides the countersignature from a
///   higher-tier key, proving authorised issuance.
/// - Peers reject infrastructure KeyRecords without valid `issuer_sig`.
///
/// ## Canonical byte encoding
///
/// `self_sig` signs [`KeyRecord::canonical_bytes_for_self_sig`].
/// `issuer_sig` (when present) signs [`KeyRecord::canonical_bytes_for_issuer_sig`],
/// which includes the completed `self_sig` so the issuer attests to the whole record.
///
/// ## Updating a record
///
/// Display name, bio, avatar hash can be changed at any time:
/// 1. Build new `KeyRecord` with updated fields and new `updated_at`.
/// 2. Re-sign with private key → new `self_sig`.
/// 3. Publish to `"key-registry"` gossipsub topic.
/// Peers keep the record with the highest `updated_at` timestamp.
///
/// `peer_id`, `public_key`, and `created_at` are permanent — they define the identity.
/// `key_type` cannot be self-upgraded (Guest → Personal requires a new keypair).
///
/// ## Revocation
///
/// Self-revoke: set `revoked = true`, update timestamp, re-sign.
/// Authority revoke: a `SignedOperation::RevokeKey` op in the world op-log
/// (does not modify this struct; permission checkers query both).
///
/// ## Expiry
///
/// Guest keys set `expires_at = now + 30 days`.
/// Peers treat expired keys as Guest-permission-level until renewed or replaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRecord {
    // ── Schema version ────────────────────────────────────────────────────────
    /// Monotonically increasing schema version. Current: 1.
    pub version: u8,

    // ── Core identity ─────────────────────────────────────────────────────────
    /// Unique identifier derived deterministically from the public key.
    pub peer_id: PeerId,

    /// Ed25519 public key (32 bytes). Stored explicitly so peers can verify
    /// signatures without needing to reconstruct from `peer_id`.
    pub public_key: [u8; 32],

    /// The type and trust tier of this key.
    pub key_type: KeyType,

    // ── Human-readable metadata ───────────────────────────────────────────────
    /// Chosen display name. `None` for Anonymous and Guest (no claimed name).
    /// Self-declared; not verified by any authority.
    pub display_name: Option<String>,

    /// Short bio or tagline (max 280 chars, enforced on create).
    pub bio: Option<String>,

    /// Content-addressed hash of avatar data stored on the DHT.
    /// `None` = use procedurally generated default avatar.
    pub avatar_hash: Option<[u8; 32]>,

    // ── Temporal ──────────────────────────────────────────────────────────────
    /// Unix timestamp (seconds) when this identity was first created.
    /// Immutable after initial publication.
    pub created_at: u64,

    /// Expiry timestamp. `None` = never expires.
    /// Guest keys expire 30 days after `created_at`.
    pub expires_at: Option<u64>,

    /// Timestamp of the most recent update to this record.
    /// Used by peers to choose between two versions of the same record.
    pub updated_at: u64,

    // ── Trust chain (infrastructure keys only) ────────────────────────────────
    /// PeerId of the key that authorised this one.
    /// Required for Server (signed by Genesis), Relay (signed by Server),
    /// Admin (signed by Server). `None` for all user keys.
    pub issued_by: Option<PeerId>,

    /// Countersignature from `issued_by` over
    /// [`KeyRecord::canonical_bytes_for_issuer_sig`].
    /// `None` for user keys.
    #[serde(with = "serde_opt_sig64")]
    pub issuer_sig: Option<[u8; 64]>,

    // ── Revocation ────────────────────────────────────────────────────────────
    /// True if this key has been revoked. Revoked keys propagate as-is so
    /// peers know they are invalid (tombstone semantics).
    pub revoked: bool,

    /// Unix timestamp when this key was revoked.
    pub revoked_at: Option<u64>,

    /// PeerId of who revoked this key.
    /// Self-revoke: same as `peer_id`. Authority revoke: Server key PeerId.
    pub revoked_by: Option<PeerId>,

    /// Human-readable reason for revocation.
    pub revocation_reason: Option<String>,

    // ── Self-signature ────────────────────────────────────────────────────────
    /// Ed25519 signature of [`KeyRecord::canonical_bytes_for_self_sig`].
    /// Proves the holder of the corresponding private key created this record.
    /// Computed last; does not cover `issuer_sig`.
    #[serde(with = "serde_sig64")]
    pub self_sig: [u8; 64],
}

// ─── Canonical byte encoding helpers ─────────────────────────────────────────

fn encode_bytes_lv(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn encode_opt_string(out: &mut Vec<u8>, s: &Option<String>) {
    match s {
        None => out.push(0),
        Some(v) => { out.push(1); encode_bytes_lv(out, v.as_bytes()); }
    }
}

fn encode_opt_u64(out: &mut Vec<u8>, v: &Option<u64>) {
    match v {
        None => out.push(0),
        Some(v) => { out.push(1); out.extend_from_slice(&v.to_le_bytes()); }
    }
}

fn encode_opt_bytes32(out: &mut Vec<u8>, v: &Option<[u8; 32]>) {
    match v {
        None => out.push(0),
        Some(v) => { out.push(1); out.extend_from_slice(v.as_ref()); }
    }
}

fn encode_opt_peer_id(out: &mut Vec<u8>, v: &Option<PeerId>) {
    match v {
        None => out.push(0),
        Some(v) => { out.push(1); encode_bytes_lv(out, &v.to_bytes()); }
    }
}

// ─── KeyRecord impl ───────────────────────────────────────────────────────────

impl KeyRecord {
    /// Build the canonical byte string that `self_sig` covers.
    ///
    /// Covers all fields **except** `self_sig` and `issuer_sig`.
    /// Field order is fixed by this implementation and must never change
    /// without a version bump.
    pub fn canonical_bytes_for_self_sig(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        // schema version
        out.push(self.version);
        // peer_id
        encode_bytes_lv(&mut out, &self.peer_id.to_bytes());
        // public_key
        out.extend_from_slice(&self.public_key);
        // key_type discriminant
        out.push(self.key_type.discriminant());
        // display_name
        encode_opt_string(&mut out, &self.display_name);
        // bio
        encode_opt_string(&mut out, &self.bio);
        // avatar_hash
        encode_opt_bytes32(&mut out, &self.avatar_hash);
        // created_at
        out.extend_from_slice(&self.created_at.to_le_bytes());
        // expires_at
        encode_opt_u64(&mut out, &self.expires_at);
        // updated_at
        out.extend_from_slice(&self.updated_at.to_le_bytes());
        // issued_by
        encode_opt_peer_id(&mut out, &self.issued_by);
        // revoked
        out.push(self.revoked as u8);
        // revoked_at
        encode_opt_u64(&mut out, &self.revoked_at);
        // revoked_by
        encode_opt_peer_id(&mut out, &self.revoked_by);
        // revocation_reason
        encode_opt_string(&mut out, &self.revocation_reason);
        out
    }

    /// Build the canonical byte string that `issuer_sig` covers.
    ///
    /// Covers everything that `self_sig` covers **plus** the completed `self_sig`
    /// itself, so the issuer attests to the entire record including the holder's
    /// proof of key possession.
    pub fn canonical_bytes_for_issuer_sig(&self) -> Vec<u8> {
        let mut out = self.canonical_bytes_for_self_sig();
        out.extend_from_slice(&self.self_sig);
        out
    }

    /// Verify `self_sig` against the embedded `public_key`.
    ///
    /// Returns `true` if the record was signed by the private key corresponding
    /// to `public_key`. This is the primary trust check; all peers run this
    /// before accepting any KeyRecord from the network.
    pub fn verify_self_sig(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.public_key) else {
            return false;
        };
        let msg = self.canonical_bytes_for_self_sig();
        let sig = Signature::from_bytes(&self.self_sig);
        vk.verify(&msg, &sig).is_ok()
    }

    /// Verify `issuer_sig` against the issuer's public key.
    ///
    /// `issuer_public_key` must be looked up from the issuer's own `KeyRecord`
    /// in the registry. Returns `false` if no `issuer_sig` is present.
    pub fn verify_issuer_sig(&self, issuer_public_key: &[u8; 32]) -> bool {
        let Some(issuer_sig_bytes) = self.issuer_sig else {
            return false;
        };
        let Ok(vk) = VerifyingKey::from_bytes(issuer_public_key) else {
            return false;
        };
        let msg = self.canonical_bytes_for_issuer_sig();
        let sig = Signature::from_bytes(&issuer_sig_bytes);
        vk.verify(&msg, &sig).is_ok()
    }

    /// True if this record is expired (has an `expires_at` in the past).
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            exp < now
        } else {
            false
        }
    }

    /// True if this record is valid for accepting operations:
    /// not revoked, not expired, self_sig verifies.
    pub fn is_operationally_valid(&self) -> bool {
        !self.revoked && !self.is_expired() && self.verify_self_sig()
    }

    /// Effective key type for permission checks.
    ///
    /// Returns `KeyType::Guest` if the record is expired or revoked, so
    /// permission checks automatically degrade without special-casing.
    pub fn effective_key_type(&self) -> KeyType {
        if self.revoked || self.is_expired() {
            KeyType::Guest
        } else {
            self.key_type
        }
    }

    /// Serialize to bytes for disk cache and gossipsub.
    pub fn to_bytes(&self) -> std::result::Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> std::result::Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }
}

impl std::fmt::Display for KeyRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.display_name.as_deref().unwrap_or("<anonymous>");
        let label = self.key_type.display_label();
        let label_part = if label.is_empty() { String::new() } else { format!(" {}", label) };
        write!(f, "{}{} ({})", name, label_part, &self.peer_id.to_string()[..12])
    }
}

// ─── Identity ─────────────────────────────────────────────────────────────────

/// Cryptographic identity for a peer in the metaverse.
///
/// Each identity consists of an Ed25519 signing key (private + public) and derived PeerId.
/// The signing key **never leaves the local machine** and is used to sign
/// all operations (voxel modifications, chat messages, etc.).
///
/// The PeerId is derived deterministically from the public key and serves
/// as the unique identifier for this peer in the P2P network.
///
/// Create a [`KeyRecord`] from an `Identity` with [`Identity::create_key_record`].
#[derive(Clone)]
pub struct Identity {
    /// Ed25519 signing key (contains both secret and public key)
    signing_key: SigningKey,
    
    /// Ed25519 verifying key (public key, derived from signing key)
    verifying_key: VerifyingKey,
    
    /// libp2p PeerId (derived from public key)
    peer_id: PeerId,
}

/// Serializable representation of an identity (for disk storage)
#[derive(Serialize, Deserialize)]
struct StoredIdentity {
    /// Secret key (32 bytes)
    secret_key: [u8; 32],
    
    /// Public key (32 bytes)
    public_key: [u8; 32],
}

impl Identity {
    /// Get the default path for storing identity.
    ///
    /// Returns `~/.metaverse/identity.key`.
    /// Override with `METAVERSE_IDENTITY_FILE=~/.metaverse/mykey.key`.
    fn default_path() -> Result<PathBuf> {
        if let Ok(custom_path) = std::env::var("METAVERSE_IDENTITY_FILE") {
            let path = if custom_path.starts_with("~/") {
                if let Some(home) = dirs::home_dir() {
                    home.join(&custom_path[2..])
                } else {
                    PathBuf::from(custom_path)
                }
            } else {
                PathBuf::from(custom_path)
            };
            return Ok(path);
        }

        let home = dirs::home_dir().ok_or_else(|| {
            IdentityError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find home directory",
            ))
        })?;

        Ok(home.join(".metaverse").join("identity.key"))
    }

    /// Derive the `.keyrec` path from a `.key` path.
    fn keyrec_path_for(key_path: &PathBuf) -> PathBuf {
        key_path.with_extension("keyrec")
    }

    /// Ensure `~/.metaverse/` exists. Returns the directory path.
    pub fn ensure_metaverse_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| {
            IdentityError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not find home directory",
            ))
        })?;

        let dir = home.join(".metaverse");
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(IdentityError::DirectoryCreationFailed)?;
            eprintln!("[identity] Created ~/.metaverse/");
        }
        Ok(dir)
    }

    /// Generate a new random `Identity`.
    ///
    /// Uses `rand::rngs::OsRng` (cryptographically secure).
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let peer_id = Self::derive_peer_id(&signing_key);
        Self { signing_key, verifying_key, peer_id }
    }

    /// Derive a libp2p `PeerId` from an Ed25519 signing key.
    fn derive_peer_id(signing_key: &SigningKey) -> PeerId {
        let secret_bytes = signing_key.to_bytes();
        let libp2p_secret = libp2p_ed25519::SecretKey::try_from_bytes(secret_bytes)
            .expect("Valid ed25519 secret key");
        let libp2p_keypair = libp2p_ed25519::Keypair::from(libp2p_secret);
        PeerId::from(Libp2pKeypair::from(libp2p_keypair).public())
    }

    /// Load identity from default path, or create and save a new one.
    ///
    /// Also loads or auto-generates the companion `.keyrec` file.
    /// On first run, prints a prominent backup warning.
    ///
    /// See `METAVERSE_IDENTITY_FILE` env var for custom path.
    pub fn load_or_create() -> Result<Self> {
        let path = Self::default_path()?;

        if path.exists() {
            eprintln!("[identity] Loading identity from {}", path.display());
            let id = Self::load_from_path(&path)?;
            // Warn if permissions are too open (Unix only)
            #[cfg(unix)]
            id.check_file_permissions(&path);
            Ok(id)
        } else {
            eprintln!("[identity] No identity found — generating new Personal identity...");
            let identity = Self::generate();
            Self::ensure_metaverse_dir()?;
            identity.save_to_path(&path)?;
            eprintln!("[identity] Saved to {}", path.display());
            eprintln!("[identity] PeerId: {}", identity.peer_id);
            eprintln!("[identity] ⚠️  CRITICAL: Back up ~/.metaverse/identity.key");
            eprintln!("[identity] ⚠️  There is NO recovery if you lose this file.");
            eprintln!("[identity] ⚠️  Losing it means losing your land, buildings, and identity.");

            // Auto-generate a default Personal KeyRecord alongside the keypair
            let keyrec_path = Self::keyrec_path_for(&path);
            let record = identity.create_key_record(
                KeyType::Personal,
                None,  // no display_name yet — user sets this in signup UI
                None,
                None,
                None,
                None,
            );
            if let Ok(bytes) = record.to_bytes() {
                let _ = fs::write(&keyrec_path, bytes);
            }

            Ok(identity)
        }
    }

    /// Load identity from a specific path (raw keypair file).
    pub fn load_from_path(path: &PathBuf) -> Result<Self> {
        let bytes = fs::read(path)?;
        let stored: StoredIdentity = bincode::deserialize(&bytes)?;
        let signing_key = SigningKey::from_bytes(&stored.secret_key);
        let verifying_key = VerifyingKey::from_bytes(&stored.public_key)
            .map_err(|_| IdentityError::InvalidKeypair)?;
        let peer_id = Self::derive_peer_id(&signing_key);
        Ok(Self { signing_key, verifying_key, peer_id })
    }

    /// Save keypair to `path` with restrictive file permissions (0o600 on Unix).
    ///
    /// The file is written atomically via a temporary file to avoid partial writes.
    pub fn save_to_path(&self, path: &PathBuf) -> Result<()> {
        let stored = StoredIdentity {
            secret_key: self.signing_key.to_bytes(),
            public_key: self.verifying_key.to_bytes(),
        };
        let bytes = bincode::serialize(&stored)?;

        // Write to temp file then rename for atomicity
        let tmp = path.with_extension("key.tmp");
        fs::write(&tmp, &bytes)?;

        // Restrict permissions before the rename so the file is never world-readable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&tmp, perms)?;
        }

        fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Warn to stderr if the key file has world- or group-readable permissions.
    #[cfg(unix)]
    fn check_file_permissions(&self, path: &PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                eprintln!(
                    "[identity] ⚠️  WARNING: {} has permissions {:04o} — should be 0600",
                    path.display(),
                    mode & 0o777
                );
                eprintln!("[identity] ⚠️  Run: chmod 600 {}", path.display());
            }
        }
    }

    /// Load the companion `.keyrec` file, or return `None` if it doesn't exist.
    pub fn load_key_record(&self) -> Option<KeyRecord> {
        let path = Self::default_path().ok()?;
        let keyrec_path = Self::keyrec_path_for(&path);
        let bytes = fs::read(&keyrec_path).ok()?;
        let record = KeyRecord::from_bytes(&bytes).ok()?;
        // Verify the record belongs to this identity
        if record.peer_id == self.peer_id && record.verify_self_sig() {
            Some(record)
        } else {
            eprintln!("[identity] ⚠️  .keyrec file failed verification — ignoring");
            None
        }
    }

    /// Save a `KeyRecord` as the companion `.keyrec` file alongside this identity.
    pub fn save_key_record(&self, record: &KeyRecord) -> Result<()> {
        let path = Self::default_path()?;
        let keyrec_path = Self::keyrec_path_for(&path);
        let bytes = record.to_bytes()?;
        fs::write(&keyrec_path, bytes)?;
        Ok(())
    }

    /// Create and sign a new `KeyRecord` for this identity.
    ///
    /// `key_type` — the type to declare (must be consistent with how you're using
    ///   this key; infrastructure keys also need `issued_by` / `issuer_sig` added
    ///   separately via [`KeyRecord`]'s issuer countersignature flow).
    ///
    /// `display_name` — human-readable name (None for Anonymous/Guest).
    /// `bio` — optional short bio (max 280 chars).
    /// `avatar_hash` — optional content-addressed avatar hash.
    /// `expires_at` — expiry Unix timestamp (None = never; use `now + 30 days` for Guest).
    /// `issued_by` — issuer's PeerId (for infrastructure keys; None for user keys).
    ///
    /// The returned record has `self_sig` computed. For infrastructure keys,
    /// the issuer must additionally call `sign_as_issuer` on the record.
    pub fn create_key_record(
        &self,
        key_type: KeyType,
        display_name: Option<String>,
        bio: Option<String>,
        avatar_hash: Option<[u8; 32]>,
        expires_at: Option<u64>,
        issued_by: Option<PeerId>,
    ) -> KeyRecord {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Truncate bio to 280 chars
        let bio = bio.map(|b| b.chars().take(280).collect::<String>());
        // Truncate display_name to 64 chars
        let display_name = display_name.map(|n| n.chars().take(64).collect::<String>());

        let mut record = KeyRecord {
            version: 1,
            peer_id: self.peer_id,
            public_key: self.verifying_key.to_bytes(),
            key_type,
            display_name,
            bio,
            avatar_hash,
            created_at: now,
            expires_at,
            updated_at: now,
            issued_by,
            issuer_sig: None,
            revoked: false,
            revoked_at: None,
            revoked_by: None,
            revocation_reason: None,
            self_sig: [0u8; 64],
        };

        // Compute and set self_sig
        let msg = record.canonical_bytes_for_self_sig();
        let sig = self.signing_key.sign(&msg);
        record.self_sig = sig.to_bytes();

        record
    }

    /// Create a Guest `KeyRecord` (auto-generated, 30-day expiry, no display name).
    pub fn create_guest_record(&self) -> KeyRecord {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let expires_at = now + 30 * 24 * 3600; // 30 days
        self.create_key_record(KeyType::Guest, None, None, None, Some(expires_at), None)
    }

    /// Update an existing `KeyRecord` with a new display name, bio, or avatar,
    /// re-signing it. `key_type`, `peer_id`, `public_key`, and `created_at` are
    /// immutable and are carried over unchanged.
    pub fn update_key_record(
        &self,
        existing: &KeyRecord,
        display_name: Option<String>,
        bio: Option<String>,
        avatar_hash: Option<[u8; 32]>,
    ) -> Result<KeyRecord> {
        // Guard: can only update a record that belongs to this identity
        if existing.peer_id != self.peer_id {
            return Err(IdentityError::InvalidKeyRecord);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let bio = bio.map(|b| b.chars().take(280).collect::<String>());
        let display_name = display_name.map(|n| n.chars().take(64).collect::<String>());

        let mut updated = existing.clone();
        updated.display_name = display_name;
        updated.bio = bio;
        updated.avatar_hash = avatar_hash;
        updated.updated_at = now;
        updated.self_sig = [0u8; 64];

        let msg = updated.canonical_bytes_for_self_sig();
        let sig = self.signing_key.sign(&msg);
        updated.self_sig = sig.to_bytes();

        Ok(updated)
    }

    /// Revoke this identity's own `KeyRecord`.
    ///
    /// Sets `revoked = true`, records the timestamp, re-signs.
    /// Publish the returned record to gossipsub so the revocation propagates.
    pub fn self_revoke(&self, existing: &KeyRecord, reason: Option<String>) -> Result<KeyRecord> {
        if existing.peer_id != self.peer_id {
            return Err(IdentityError::InvalidKeyRecord);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut revoked = existing.clone();
        revoked.revoked = true;
        revoked.revoked_at = Some(now);
        revoked.revoked_by = Some(self.peer_id);
        revoked.revocation_reason = reason;
        revoked.updated_at = now;
        revoked.self_sig = [0u8; 64];

        let msg = revoked.canonical_bytes_for_self_sig();
        let sig = self.signing_key.sign(&msg);
        revoked.self_sig = sig.to_bytes();

        Ok(revoked)
    }

    /// Add an issuer countersignature to a `KeyRecord` (for infrastructure keys).
    ///
    /// Called by the issuer (e.g., a Server key signing a Relay key).
    /// The `record` must already have a valid `self_sig` from the holder.
    pub fn sign_as_issuer(&self, record: &mut KeyRecord) {
        let msg = record.canonical_bytes_for_issuer_sig();
        let sig = self.signing_key.sign(&msg);
        record.issuer_sig = Some(sig.to_bytes());
    }

    // ── Existing API (unchanged) ──────────────────────────────────────────────

    /// Our `PeerId`.
    pub fn peer_id(&self) -> &PeerId { &self.peer_id }

    /// Ed25519 verifying key (public key).
    pub fn verifying_key(&self) -> &VerifyingKey { &self.verifying_key }

    /// Ed25519 signing key. Use carefully — contains private key material.
    pub fn signing_key(&self) -> &SigningKey { &self.signing_key }

    /// Raw signing key bytes (for deriving symmetric keys).
    ///
    /// **WARNING:** Private key material. Never log or transmit.
    pub fn signing_key_bytes(&self) -> &[u8; 32] { self.signing_key.as_bytes() }

    /// Sign `data` with this identity's private key.
    pub fn sign(&self, data: &[u8]) -> Signature { self.signing_key.sign(data) }

    /// Verify `signature` over `data` against this identity's public key.
    pub fn verify_own(&self, data: &[u8], signature: &Signature) -> bool {
        self.verifying_key.verify(data, signature).is_ok()
    }

    /// Verify `signature` over `data` against an explicit public key.
    pub fn verify_with_pubkey(vk: &VerifyingKey, data: &[u8], sig: &Signature) -> bool {
        vk.verify(data, sig).is_ok()
    }

    /// Convert to a `libp2p::identity::Keypair` (used when building the Swarm).
    pub fn to_libp2p_keypair(&self) -> Libp2pKeypair {
        let secret_bytes = self.signing_key.to_bytes();
        let libp2p_secret = libp2p_ed25519::SecretKey::try_from_bytes(secret_bytes)
            .expect("Valid ed25519 secret key");
        Libp2pKeypair::from(libp2p_ed25519::Keypair::from(libp2p_secret))
    }
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("peer_id", &self.peer_id)
            .field("verifying_key", &format!("{:02x?}", &self.verifying_key.to_bytes()[..8]))
            .field("signing_key", &"<REDACTED>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Identity (keypair) tests ──────────────────────────────────────────────

    #[test]
    fn test_generate_identity() {
        let id = Identity::generate();
        assert_ne!(id.peer_id.to_string(), "");
    }

    #[test]
    fn test_sign_and_verify() {
        let id = Identity::generate();
        let data = b"Test message for signing";
        let sig = id.sign(data);
        assert!(id.verify_own(data, &sig));
        assert!(!id.verify_own(b"Different message", &sig));
    }

    #[test]
    fn test_verify_with_pubkey() {
        let id = Identity::generate();
        let data = b"Test data";
        let sig = id.sign(data);
        assert!(Identity::verify_with_pubkey(id.verifying_key(), data, &sig));
        assert!(!Identity::verify_with_pubkey(id.verifying_key(), b"Wrong", &sig));
    }

    #[test]
    fn test_identity_persistence() {
        use tempfile::NamedTempFile;
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let id1 = Identity::generate();
        id1.save_to_path(&path).unwrap();

        let id2 = Identity::load_from_path(&path).unwrap();
        assert_eq!(id1.peer_id(), id2.peer_id());

        // Same key → same signatures
        let data = b"determinism test";
        assert_eq!(id1.sign(data).to_bytes(), id2.sign(data).to_bytes());
    }

    #[test]
    fn test_peer_id_deterministic() {
        let id = Identity::generate();
        let pid2 = Identity::derive_peer_id(&id.signing_key);
        assert_eq!(*id.peer_id(), pid2);
    }

    // ── KeyType tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_key_type_discriminants_unique() {
        use std::collections::HashSet;
        let types = [
            KeyType::Genesis, KeyType::Server, KeyType::Relay,
            KeyType::Admin, KeyType::Business, KeyType::Personal,
            KeyType::Anonymous, KeyType::Guest,
        ];
        let discs: HashSet<u8> = types.iter().map(|t| t.discriminant()).collect();
        assert_eq!(discs.len(), types.len(), "discriminants must be unique");
    }

    #[test]
    fn test_key_type_roundtrip() {
        for d in 0u8..=7 {
            let kt = KeyType::from_discriminant(d).expect("valid discriminant");
            assert_eq!(kt.discriminant(), d);
        }
        assert!(KeyType::from_discriminant(255).is_none());
    }

    #[test]
    fn test_key_type_permissions() {
        assert!(KeyType::Personal.can_own_parcels());
        assert!(KeyType::Business.can_own_parcels());
        assert!(!KeyType::Guest.can_own_parcels());
        assert!(!KeyType::Anonymous.can_own_parcels());
        assert!(!KeyType::Relay.can_own_parcels());

        assert!(KeyType::Personal.can_sign_contracts());
        assert!(!KeyType::Guest.can_sign_contracts());
        assert!(!KeyType::Anonymous.can_sign_contracts());

        assert!(KeyType::Relay.requires_issuance());
        assert!(KeyType::Server.requires_issuance());
        assert!(!KeyType::Personal.requires_issuance());
        assert!(!KeyType::Guest.requires_issuance());
    }

    // ── KeyRecord tests ───────────────────────────────────────────────────────

    #[test]
    fn test_key_record_self_sig_verifies() {
        let id = Identity::generate();
        let record = id.create_key_record(
            KeyType::Personal,
            Some("Alice".to_string()),
            Some("Test bio".to_string()),
            None,
            None,
            None,
        );
        assert!(record.verify_self_sig(), "self_sig must verify immediately after creation");
    }

    #[test]
    fn test_key_record_self_sig_invalid_after_tamper() {
        let id = Identity::generate();
        let mut record = id.create_key_record(KeyType::Personal, Some("Bob".to_string()), None, None, None, None);
        // Tamper with display name
        record.display_name = Some("EvilBob".to_string());
        assert!(!record.verify_self_sig(), "tampered record must fail verification");
    }

    #[test]
    fn test_key_record_canonical_bytes_deterministic() {
        let id = Identity::generate();
        let record = id.create_key_record(KeyType::Anonymous, None, None, None, None, None);
        // Canonical bytes must be identical on repeated calls
        assert_eq!(
            record.canonical_bytes_for_self_sig(),
            record.canonical_bytes_for_self_sig()
        );
    }

    #[test]
    fn test_key_record_different_content_different_bytes() {
        let id = Identity::generate();
        let r1 = id.create_key_record(KeyType::Personal, Some("Alice".to_string()), None, None, None, None);
        let r2 = id.create_key_record(KeyType::Personal, Some("Bob".to_string()), None, None, None, None);
        assert_ne!(r1.canonical_bytes_for_self_sig(), r2.canonical_bytes_for_self_sig());
    }

    #[test]
    fn test_key_record_serialization_roundtrip() {
        let id = Identity::generate();
        let record = id.create_key_record(
            KeyType::Business,
            Some("Acme Corp".to_string()),
            Some("We build things".to_string()),
            None,
            None,
            None,
        );
        let bytes = record.to_bytes().unwrap();
        let decoded = KeyRecord::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.peer_id, record.peer_id);
        assert_eq!(decoded.key_type as u8, record.key_type as u8);
        assert_eq!(decoded.display_name, record.display_name);
        assert!(decoded.verify_self_sig(), "decoded record must still verify");
    }

    #[test]
    fn test_guest_record_has_expiry() {
        let id = Identity::generate();
        let record = id.create_guest_record();
        assert_eq!(record.key_type as u8, KeyType::Guest as u8);
        assert!(record.expires_at.is_some(), "guest record must have expiry");
        let exp = record.expires_at.unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        // Should be 30 days ± a few seconds
        assert!((exp - now) > 29 * 24 * 3600, "expiry should be ~30 days from now");
        assert!(!record.is_expired(), "freshly created guest key must not be expired");
    }

    #[test]
    fn test_key_record_update_preserves_peer_id_and_created_at() {
        let id = Identity::generate();
        let original = id.create_key_record(KeyType::Personal, Some("Alice".to_string()), None, None, None, None);
        let updated = id.update_key_record(
            &original,
            Some("Alice Smith".to_string()),
            Some("New bio".to_string()),
            None,
        ).unwrap();

        assert_eq!(updated.peer_id, original.peer_id);
        assert_eq!(updated.created_at, original.created_at);
        assert_eq!(updated.key_type as u8, original.key_type as u8, "key_type is immutable via update");
        assert_eq!(updated.display_name, Some("Alice Smith".to_string()));
        assert!(updated.verify_self_sig(), "updated record must verify");
    }

    #[test]
    fn test_key_record_update_wrong_identity_fails() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        let record = id1.create_key_record(KeyType::Personal, Some("Alice".to_string()), None, None, None, None);
        // id2 should not be able to update id1's record
        assert!(id2.update_key_record(&record, Some("Hacked".to_string()), None, None).is_err());
    }

    #[test]
    fn test_self_revocation() {
        let id = Identity::generate();
        let record = id.create_key_record(KeyType::Personal, Some("Alice".to_string()), None, None, None, None);
        let revoked = id.self_revoke(&record, Some("Key compromised".to_string())).unwrap();

        assert!(revoked.revoked);
        assert_eq!(revoked.revoked_by, Some(*id.peer_id()));
        assert!(revoked.revocation_reason.is_some());
        assert!(revoked.verify_self_sig(), "revoked record must still self-verify (tombstone)");
        assert_eq!(revoked.effective_key_type() as u8, KeyType::Guest as u8,
            "revoked key degrades to Guest-level permissions");
    }

    #[test]
    fn test_self_revoke_wrong_identity_fails() {
        let id1 = Identity::generate();
        let id2 = Identity::generate();
        let record = id1.create_key_record(KeyType::Personal, None, None, None, None, None);
        assert!(id2.self_revoke(&record, None).is_err());
    }

    #[test]
    fn test_issuer_countersignature() {
        let server_id = Identity::generate();
        let relay_id = Identity::generate();

        // Relay creates its own record, declaring the server as issuer
        let mut relay_record = relay_id.create_key_record(
            KeyType::Relay,
            None,
            None,
            None,
            None,
            Some(*server_id.peer_id()),
        );
        assert!(relay_record.verify_self_sig());

        // Server countersigns
        server_id.sign_as_issuer(&mut relay_record);
        assert!(relay_record.issuer_sig.is_some());

        // Verify issuer sig with server's public key
        assert!(
            relay_record.verify_issuer_sig(&server_id.verifying_key().to_bytes()),
            "issuer_sig must verify against server public key"
        );

        // Tamper → issuer sig fails
        relay_record.key_type = KeyType::Server;
        assert!(!relay_record.verify_issuer_sig(&server_id.verifying_key().to_bytes()),
            "tampered record must fail issuer verification");
    }

    #[test]
    fn test_display_name_truncation() {
        let id = Identity::generate();
        let long_name = "A".repeat(200);
        let record = id.create_key_record(KeyType::Personal, Some(long_name), None, None, None, None);
        assert_eq!(record.display_name.as_ref().unwrap().len(), 64,
            "display_name must be truncated to 64 chars");
        assert!(record.verify_self_sig());
    }

    #[test]
    fn test_bio_truncation() {
        let id = Identity::generate();
        let long_bio = "B".repeat(500);
        let record = id.create_key_record(KeyType::Personal, None, Some(long_bio), None, None, None);
        assert_eq!(record.bio.as_ref().unwrap().len(), 280,
            "bio must be truncated to 280 chars");
        assert!(record.verify_self_sig());
    }

    #[test]
    fn test_effective_key_type_normal() {
        let id = Identity::generate();
        let record = id.create_key_record(KeyType::Admin, None, None, None, None, None);
        assert_eq!(record.effective_key_type() as u8, KeyType::Admin as u8,
            "valid non-revoked record returns its actual type");
    }

    #[test]
    fn test_is_operationally_valid() {
        let id = Identity::generate();
        let record = id.create_key_record(KeyType::Personal, None, None, None, None, None);
        assert!(record.is_operationally_valid());
        // Revoked record
        let revoked = id.self_revoke(&record, None).unwrap();
        assert!(!revoked.is_operationally_valid());
    }
}
