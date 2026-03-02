//! P2P tile request/response protocol types.
//!
//! Nodes request tiles from peers using libp2p request-response.
//! DHT get_providers() tells you who has a tile; this protocol fetches it.
//! Works over any libp2p transport — TCP, QUIC, WebSocket, or packet radio.

use serde::{Deserialize, Serialize};

/// A request for tile data from a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TileRequest {
    /// OSM tile for bounding box (snapped to 0.01° grid)
    OsmTile { s: f64, w: f64, n: f64, e: f64 },
    /// SRTM elevation tile (1° × 1°)
    ElevationTile { lat: i32, lon: i32 },
    /// Pre-generated terrain chunk octree
    TerrainChunk { cx: i64, cz: i64 },
}

/// Response to a tile request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TileResponse {
    /// Tile found — raw bincode/GeoTIFF bytes
    Found(Vec<u8>),
    /// Peer does not have this tile
    NotFound,
}

/// Protocol identifier string.
pub const TILE_PROTOCOL: &str = "/metaverse/tiles/1.0.0";

/// Codec marker for the tile request-response protocol.
/// The actual `request_response::Codec` impl lives in the server binary
/// to keep async code out of the library (avoiding complex codegen).
#[derive(Clone, Default)]
pub struct TileCodec;
