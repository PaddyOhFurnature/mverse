//! P2P tile request/response protocol types.
//!
//! Nodes request tiles from peers using libp2p request-response.
//! DHT get_providers() tells you who has a tile; this protocol fetches it.
//! Works over any libp2p transport — TCP, QUIC, WebSocket, or packet radio.

use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::StreamProtocol;
use libp2p::request_response::Codec;
use serde::{Deserialize, Serialize};
use std::io;

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
    /// Tile found — raw bytes (bincode for OSM/terrain, raw GeoTIFF for elevation)
    Found(Vec<u8>),
    /// Peer does not have this tile
    NotFound,
}

/// Protocol identifier string.
pub const TILE_PROTOCOL: &str = "/metaverse/tiles/1.0.0";

/// Length-prefixed bincode codec for the tile protocol.
///
/// Uses a 4-byte big-endian length prefix followed by bincode bytes.
/// Binary only — no JSON (project rule: all cache/database files are bincode).
#[derive(Debug, Clone, Default)]
pub struct TileCodec;

#[async_trait]
impl Codec for TileCodec {
    type Protocol = StreamProtocol;
    type Request = TileRequest;
    type Response = TileResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len_buf = [0u8; 4];
        io.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 1 << 20 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tile request too large",
            ));
        }
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;
        bincode::deserialize(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len_buf = [0u8; 4];
        io.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 64 << 20 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "tile response too large",
            ));
        }
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;
        bincode::deserialize(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes =
            bincode::serialize(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        io.write_all(&bytes).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bytes =
            bincode::serialize(&resp).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        io.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
        io.write_all(&bytes).await?;
        io.close().await
    }
}
