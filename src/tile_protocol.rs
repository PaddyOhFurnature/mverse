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

impl libp2p::request_response::Codec for TileCodec {
    type Protocol = libp2p::StreamProtocol;
    type Request  = TileRequest;
    type Response = TileResponse;

    fn read_request<'a, 'b, T>(
        &'a mut self,
        _protocol: &'b Self::Protocol,
        io: &'a mut T,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<Self::Request>> + Send + 'a>>
    where T: futures::AsyncRead + Unpin + Send + 'a {
        Box::pin(async move {
            use futures::AsyncReadExt;
            let mut buf = Vec::new();
            io.take(1_048_576).read_to_end(&mut buf).await?;
            serde_json::from_slice(&buf)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })
    }

    fn read_response<'a, 'b, T>(
        &'a mut self,
        _protocol: &'b Self::Protocol,
        io: &'a mut T,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<Self::Response>> + Send + 'a>>
    where T: futures::AsyncRead + Unpin + Send + 'a {
        Box::pin(async move {
            use futures::AsyncReadExt;
            let mut buf = Vec::new();
            io.take(16_777_216).read_to_end(&mut buf).await?;
            serde_json::from_slice(&buf)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })
    }

    fn write_request<'a, 'b, T>(
        &'a mut self,
        _protocol: &'b Self::Protocol,
        io: &'a mut T,
        req: Self::Request,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>>
    where T: futures::AsyncWrite + Unpin + Send + 'a {
        Box::pin(async move {
            use futures::AsyncWriteExt;
            let data = serde_json::to_vec(&req)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            io.write_all(&data).await
        })
    }

    fn write_response<'a, 'b, T>(
        &'a mut self,
        _protocol: &'b Self::Protocol,
        io: &'a mut T,
        resp: Self::Response,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'a>>
    where T: futures::AsyncWrite + Unpin + Send + 'a {
        Box::pin(async move {
            use futures::AsyncWriteExt;
            let data = serde_json::to_vec(&resp)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            io.write_all(&data).await
        })
    }
}
