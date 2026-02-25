//! Meshsite content layer — the distributed web hosted on the mesh.
//!
//! A `ContentItem` is a signed piece of content (forum post, wiki article, etc.)
//! that propagates through the gossipsub mesh exactly like a voxel operation.
//! Items are identified by the SHA-256 of their canonical fields, stored in the
//! DHT for persistence, and cached locally by every subscribed node.
//!
//! **No single server hosts the meshsite.** Every node (server, client, relay)
//! that subscribes to the content topics IS the meshsite.
//!
//! Gossipsub topic per section: `meshsite/forums`, `meshsite/wiki`, etc.
//! DHT key per item:            `meshsite/{section}/{id}`

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Gossipsub topics ──────────────────────────────────────────────────────────

pub const TOPIC_MESHSITE_FORUMS:      &str = "meshsite/forums";
pub const TOPIC_MESHSITE_WIKI:        &str = "meshsite/wiki";
pub const TOPIC_MESHSITE_MARKETPLACE: &str = "meshsite/marketplace";
pub const TOPIC_MESHSITE_POST:        &str = "meshsite/post";

/// All meshsite gossipsub topics (subscribe to all at startup).
pub const MESHSITE_TOPICS: &[&str] = &[
    TOPIC_MESHSITE_FORUMS,
    TOPIC_MESHSITE_WIKI,
    TOPIC_MESHSITE_MARKETPLACE,
    TOPIC_MESHSITE_POST,
];

/// The gossipsub topic for a given section.
pub fn topic_for_section(section: &Section) -> &'static str {
    match section {
        Section::Forums      => TOPIC_MESHSITE_FORUMS,
        Section::Wiki        => TOPIC_MESHSITE_WIKI,
        Section::Marketplace => TOPIC_MESHSITE_MARKETPLACE,
        Section::Post        => TOPIC_MESHSITE_POST,
    }
}

// ── Section ───────────────────────────────────────────────────────────────────

/// The section of the meshsite a content item belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Forums,
    Wiki,
    Marketplace,
    Post,
}

impl Section {
    pub fn as_str(&self) -> &'static str {
        match self {
            Section::Forums      => "forums",
            Section::Wiki        => "wiki",
            Section::Marketplace => "marketplace",
            Section::Post        => "post",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "forums"      => Some(Section::Forums),
            "wiki"        => Some(Section::Wiki),
            "marketplace" => Some(Section::Marketplace),
            "post"        => Some(Section::Post),
            _             => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Section::Forums      => "Forums",
            Section::Wiki        => "Wiki",
            Section::Marketplace => "Marketplace",
            Section::Post        => "Post Office",
        }
    }

    pub fn all() -> &'static [Section] {
        &[Section::Forums, Section::Wiki, Section::Marketplace, Section::Post]
    }
}

// ── ContentItem ───────────────────────────────────────────────────────────────

/// A single piece of meshsite content.
///
/// The `id` is the hex-encoded SHA-256 of `section + title + body + author + created_at`.
/// The `signature` is an ed25519 signature by `author` over the same canonical bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentItem {
    /// SHA-256 hex digest of canonical fields.
    pub id: String,
    pub section: Section,
    /// Short title or subject line (max 200 chars).
    pub title: String,
    /// Body text — plain text or minimal markdown (max 64 KiB).
    pub body: String,
    /// Peer ID of the author (base58).
    pub author: String,
    /// Ed25519 signature over `canonical_bytes()` (65 bytes).
    pub signature: Vec<u8>,
    /// Unix timestamp milliseconds when the item was created.
    pub created_at: u64,
}

impl ContentItem {
    /// Canonical bytes that are hashed and signed.
    /// Format: `section\0title\0body\0author\0created_at_decimal`.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        format!(
            "{}\0{}\0{}\0{}\0{}",
            self.section.as_str(), self.title, self.body, self.author, self.created_at
        ).into_bytes()
    }

    /// Compute the SHA-256 id from the current fields.
    pub fn compute_id(&self) -> String {
        let hash = Sha256::digest(self.canonical_bytes());
        hex::encode(hash)
    }

    /// True if `id` matches `compute_id()`.
    pub fn id_valid(&self) -> bool {
        self.id == self.compute_id()
    }

    /// DHT key for this item: `meshsite/{section}/{id}`.
    pub fn dht_key(&self) -> Vec<u8> {
        format!("meshsite/{}/{}", self.section.as_str(), self.id).into_bytes()
    }

    /// Serialise to bytes for DHT storage.
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("ContentItem serialises cleanly")
    }

    /// Deserialise from DHT bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        bincode::deserialize(data).ok()
    }
}

/// DHT key prefix for listing all content in a section: `meshsite/{section}/`
pub fn dht_section_prefix(section: &Section) -> Vec<u8> {
    format!("meshsite/{}/", section.as_str()).into_bytes()
}

// ── Submit request (from API clients) ─────────────────────────────────────────

/// JSON body for `POST /api/v1/content`.
#[derive(Debug, Deserialize)]
pub struct SubmitContent {
    pub section: String,
    pub title: String,
    pub body: String,
    pub author: String,
    /// Hex-encoded ed25519 signature over canonical_bytes of the item.
    pub signature: String,
    pub created_at: u64,
}

impl SubmitContent {
    /// Build a `ContentItem`, computing the id from the fields.
    /// Does NOT verify the signature — caller must do that.
    pub fn into_item(self) -> Option<ContentItem> {
        let section = Section::from_str(&self.section)?;
        let sig_bytes = hex::decode(&self.signature).ok()?;
        let mut item = ContentItem {
            id: String::new(),
            section,
            title: self.title,
            body: self.body,
            author: self.author,
            signature: sig_bytes,
            created_at: self.created_at,
        };
        item.id = item.compute_id();
        Some(item)
    }
}
