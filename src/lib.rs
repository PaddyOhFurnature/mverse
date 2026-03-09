//! Metaverse Core - Phases 1, 2, and 1.5 (P2P Networking)
//!
//! Fresh implementation starting from foundation research.

pub mod renderer;
pub mod coordinates;
pub mod elevation;
pub mod materials;
pub mod voxel;
pub mod terrain;
pub mod worldgen;
pub mod terrain_sync;
pub mod mesh;
pub mod marching_cubes;
pub mod physics;
pub mod construct;  // The Construct — bundled lobby scene, always available offline
pub mod billboard;  // Billboard system — textured quads for Construct room wall screens

// Phase 1.5: P2P Networking (Local-First Architecture)
pub mod identity;
pub mod key_registry;   // P2P identity registry — distributed database of KeyRecords
pub mod permissions;    // Permission system — key-type and ownership enforcement
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
pub mod node_capabilities;
pub mod meshsite;       // Meshsite content types (ContentItem, Section)
pub mod world_objects;  // Placed-object registry — modular placement schema + DHT keys
pub mod web_ui;         // Shared Pi-Hole-style web dashboard — server, relay, client
pub mod autoupdate;     // Binary auto-update: check manifest, verify, exec-restart
pub mod worldnet;       // WORLDNET OS — distributed OS layer, address system, pixel renderer
pub mod osm;            // OpenStreetMap data loading from local PBF
pub mod world_inference; // Infer placed objects from OSM data + elevation
pub mod node_config;    // Unified node configuration — shared by client, server, and relay
pub mod tile_protocol;  // P2P tile request/response protocol (libp2p request-response)
pub mod tile_store;     // RocksDB-backed tile cache (OSM, SRTM, terrain)
pub mod world_store;    // RocksDB-backed world state (voxel ops, parcels, players)
