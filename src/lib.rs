//! Metaverse Core - Phases 1, 2, and 1.5 (P2P Networking)
//!
//! Fresh implementation starting from foundation research.

pub mod renderer;
pub mod coordinates;
pub mod elevation;
pub mod materials;
pub mod voxel;
pub mod terrain;
pub mod terrain_sync;
pub mod mesh;
pub mod marching_cubes;
pub mod physics;

// Phase 1.5: P2P Networking (Local-First Architecture)
pub mod identity;
pub mod network;
pub mod bootstrap;  // Dynamic bootstrap node discovery
pub mod http_tunnel;  // HTTP fallback for firewall bypass
pub mod messages;
pub mod player_state;
pub mod player_persistence;
pub mod multiplayer;
pub mod remote_render;
pub mod user_content;
pub mod vector_clock;
pub mod chunk;
pub mod chunk_manager;
pub mod chunk_streaming;
pub mod chunk_loader;
pub mod chunk_placeholder;
pub mod spatial_sharding;
pub mod bandwidth;  // Bandwidth profiles and message priority (graceful degradation)
