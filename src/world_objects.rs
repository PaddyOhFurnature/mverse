//! Placed-object registry — the world state schema for modular placement.
//!
//! Any node type (server, relay, client) can describe and store objects that
//! exist in the world.  Placement is a write to the network; rendering is
//! a query of the network.  No recompile needed.
//!
//! # DHT key scheme
//!
//! | Key                       | Value                                      |
//! |---------------------------|--------------------------------------------|
//! | `world/object/{id}`       | Serialised [`PlacedObject`] (JSON)         |
//! | `world/chunk/{cx}/{cz}`   | Serialised [`ChunkObjectList`] (JSON)      |
//!
//! Chunk coordinates use the same grid as the voxel engine
//! ([`CHUNK_GRID_M`] = 64 m).  Each [`PlacedObject`] lives in exactly one
//! chunk bucket determined by its X/Z position.
//!
//! # Placement workflow
//!
//! 1. Admin sends `POST /api/v1/world/objects` to the server.
//! 2. Server stores in SQLite and calls `PutDhtRecord("world/chunk/{cx}/{cz}", …)`.
//! 3. All clients nearby query `GetDhtRecord("world/chunk/{cx}/{cz}")` on area load.
//! 4. Clients receive the record and render whatever object types they know about.

use ed25519_dalek::{Signature, Signer, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

// Custom serde for [u8; 64] arrays
mod serde_arrays {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.serialize(serializer)
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Vec::deserialize(deserializer)?;
        if bytes.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "Expected 64 bytes, got {}",
                bytes.len()
            )));
        }
        let mut array = [0u8; 64];
        array.copy_from_slice(&bytes);
        Ok(array)
    }
}

// ── Chunk grid ────────────────────────────────────────────────────────────────

/// World metres per chunk cell in the object spatial index.
/// Matches the voxel engine's horizontal chunk size.
pub const CHUNK_GRID_M: f32 = 64.0;

/// Convert a world X/Z position to chunk-index coordinates.
pub fn chunk_coords_for_pos(x: f32, z: f32) -> (i32, i32) {
    (
        (x / CHUNK_GRID_M).floor() as i32,
        (z / CHUNK_GRID_M).floor() as i32,
    )
}

/// DHT key for the object list in a given chunk.
pub fn chunk_dht_key(cx: i32, cz: i32) -> Vec<u8> {
    format!("world/chunk/{}/{}", cx, cz).into_bytes()
}

/// DHT key for a single placed object.
pub fn object_dht_key(id: &str) -> Vec<u8> {
    format!("world/object/{}", id).into_bytes()
}

// ── Object types ──────────────────────────────────────────────────────────────

/// What kind of placeable object this is.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectType {
    /// Wall-mounted content display — renders latest posts from `content_key` section.
    Billboard,
    /// Public interactive terminal — full-screen BBS-style browser, personalised per user.
    Terminal,
    /// Simplified kiosk — shows a specific page or content item.
    Kiosk,
    /// World transition point — teleports player to another area or server.
    Portal,
    /// Explicit player spawn / respawn marker.
    SpawnPoint,
    /// Extension point for future or third-party object types.
    Custom(String),
}

impl ObjectType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Billboard => "billboard",
            Self::Terminal => "terminal",
            Self::Kiosk => "kiosk",
            Self::Portal => "portal",
            Self::SpawnPoint => "spawn_point",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "billboard" => Self::Billboard,
            "terminal" => Self::Terminal,
            "kiosk" => Self::Kiosk,
            "portal" => Self::Portal,
            "spawn_point" => Self::SpawnPoint,
            other => Self::Custom(other.to_string()),
        }
    }
}

// ── PlacedObject ──────────────────────────────────────────────────────────────

/// A single object placed in the world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlacedObject {
    /// Unique identifier (UUID v4 or content-hash hex).
    pub id: String,
    /// What kind of object this is.
    pub object_type: ObjectType,

    // ── Spatial ──────────────────────────────────────────────────────────────
    /// World-space position in metres (X, Y, Z).
    pub position: [f32; 3],
    /// Rotation around the Y axis in radians — determines which way the object faces.
    pub rotation_y: f32,
    /// Uniform scale multiplier (1.0 = natural size).
    #[serde(default = "default_scale")]
    pub scale: f32,

    // ── Content ───────────────────────────────────────────────────────────────
    /// For billboards/kiosks: the meshsite section name (e.g. `"forums"`) or a DHT content key.
    /// For portals: the destination address.
    /// For spawns: unused (leave empty).
    #[serde(default)]
    pub content_key: String,
    /// Human-readable label shown when the player approaches.
    #[serde(default)]
    pub label: String,

    // ── Provenance ────────────────────────────────────────────────────────────
    /// Peer ID of the player/admin who placed this object.
    pub placed_by: String,
    /// Unix timestamp (milliseconds) when this object was placed.
    pub placed_at: u64,
}

fn default_scale() -> f32 {
    1.0
}

impl PlacedObject {
    /// World position as a `glam::Vec3`.
    pub fn pos_vec3(&self) -> glam::Vec3 {
        glam::Vec3::new(self.position[0], self.position[1], self.position[2])
    }

    /// Unit vector the front face of this object points toward (derived from `rotation_y`).
    pub fn facing_normal(&self) -> glam::Vec3 {
        glam::Vec3::new(self.rotation_y.sin(), 0.0, self.rotation_y.cos()).normalize()
    }

    /// Chunk coordinates for this object's position.
    pub fn chunk_coords(&self) -> (i32, i32) {
        chunk_coords_for_pos(self.position[0], self.position[2])
    }

    /// DHT key for this specific object.
    pub fn dht_key(&self) -> Vec<u8> {
        object_dht_key(&self.id)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

// ── ChunkObjectList ───────────────────────────────────────────────────────────

/// All placed objects in one chunk cell — stored as a single DHT record.
///
/// Clients fetch this with `GetDhtRecord(chunk_dht_key(cx, cz))` when entering an area.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkObjectList {
    pub cx: i32,
    pub cz: i32,
    pub objects: Vec<PlacedObject>,
}

impl ChunkObjectList {
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }
}

// ── DHT key helpers ───────────────────────────────────────────────────────────

/// DHT key for the inference-status record of one chunk.
/// Value: 8-byte little-endian Unix timestamp (ms) of when inference was last run.
pub fn inference_status_key(cx: i32, cz: i32) -> Vec<u8> {
    format!("world/inferred/{cx}/{cz}").into_bytes()
}

/// DHT key for the override list of one chunk.
pub fn chunk_override_key(cx: i32, cz: i32) -> Vec<u8> {
    format!("world/overrides/{cx}/{cz}").into_bytes()
}

// ── Object overrides ──────────────────────────────────────────────────────────

/// What to do to an inferred object.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum OverrideAction {
    /// Reposition / reorient (local metres relative to the render origin).
    Move { position: [f32; 3], rotation_y: f32 },
    /// Swap to a different model type (e.g. replace a generic streetlight with a custom lamp).
    Replace { new_type: String },
    /// Change uniform scale.
    Scale { scale: f32 },
    /// Permanently hide this inferred object from the scene.
    Remove,
}

/// A signed admin instruction that overrides one inferred [`PlacedObject`].
///
/// Stored in the DHT under [`chunk_override_key`] as part of a [`ChunkOverrideList`].
/// Any client can verify the signature without a central server.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObjectOverride {
    /// Deterministic ID of the inferred object being overridden (e.g. `"inf_a1b2c3d4..."`).
    pub target_id: String,
    /// What to do.
    pub action: OverrideAction,
    /// Author peer ID (hex string).
    pub author_peer: String,
    /// Author's Ed25519 public key (32 bytes) — used to verify the signature.
    pub public_key: [u8; 32],
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Ed25519 signature over [`signable_bytes()`].
    #[serde(with = "serde_arrays")]
    pub signature: [u8; 64],
}

impl ObjectOverride {
    /// Bytes covered by the signature.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(self.target_id.as_bytes());
        out.extend_from_slice(&bincode::serialize(&self.action).unwrap_or_default());
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    pub fn sign(&mut self, key: &impl Signer<Signature>) {
        self.signature = key.sign(&self.signable_bytes()).to_bytes();
    }

    pub fn verify(&self) -> bool {
        let Ok(vk) = VerifyingKey::from_bytes(&self.public_key) else {
            return false;
        };
        let sig = Signature::from_bytes(&self.signature);
        vk.verify(&self.signable_bytes(), &sig).is_ok()
    }
}

/// All admin overrides for one chunk cell — stored as a single DHT record.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkOverrideList {
    pub cx: i32,
    pub cz: i32,
    pub overrides: Vec<ObjectOverride>,
}

impl ChunkOverrideList {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }

    /// Merge an incoming override — replaces any existing entry for the same target.
    pub fn upsert(&mut self, ov: ObjectOverride) {
        if let Some(existing) = self
            .overrides
            .iter_mut()
            .find(|o| o.target_id == ov.target_id)
        {
            *existing = ov;
        } else {
            self.overrides.push(ov);
        }
    }
}
