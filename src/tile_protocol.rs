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

/// Codec for the tile request-response protocol (serde_json framing).
#[derive(Clone, Default)]
pub struct TileCodec;

#[async_trait::async_trait]
impl libp2p::request_response::Codec for TileCodec {
    type Protocol = libp2p::StreamProtocol;
    type Request  = TileRequest;
    type Response = TileResponse;

    async fn read_request<T>(&mut self, _protocol: &Self::Protocol, io: &mut T)
    -> std::io::Result<Self::Request>
    where T: futures::AsyncRead + Unpin + Send {
        use futures::AsyncReadExt;
        let mut buf = Vec::new();
        io.take(1_048_576).read_to_end(&mut buf).await?;
        serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(&mut self, _protocol: &Self::Protocol, io: &mut T)
    -> std::io::Result<Self::Response>
    where T: futures::AsyncRead + Unpin + Send {
        use futures::AsyncReadExt;
        let mut buf = Vec::new();
        io.take(16_777_216).read_to_end(&mut buf).await?;
        serde_json::from_slice(&buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(&mut self, _protocol: &Self::Protocol, io: &mut T, req: Self::Request)
    -> std::io::Result<()>
    where T: futures::AsyncWrite + Unpin + Send {
        use futures::AsyncWriteExt;
        let data = serde_json::to_vec(&req)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        io.write_all(&data).await
    }

    async fn write_response<T>(&mut self, _protocol: &Self::Protocol, io: &mut T, resp: Self::Response)
    -> std::io::Result<()>
    where T: futures::AsyncWrite + Unpin + Send {
        use futures::AsyncWriteExt;
        let data = serde_json::to_vec(&resp)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        io.write_all(&data).await
    }
}
