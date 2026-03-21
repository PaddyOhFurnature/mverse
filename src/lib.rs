//! Metaverse Core - Phases 1, 2, and 1.5 (P2P Networking)
//!
//! Fresh implementation starting from foundation research.

pub mod billboard;
pub mod client_app;
pub mod construct; // The Construct — bundled lobby scene, always available offline
pub mod coordinates;
pub mod elevation;
pub mod marching_cubes;
pub mod materials;
pub mod mesh;
pub mod physics;
pub mod renderer;
pub mod terrain;
pub mod terrain_sync;
pub mod voxel;
pub mod worldgen; // Billboard system — textured quads for Construct room wall screens

// Phase 1.5: P2P Networking (Local-First Architecture)
pub mod autoupdate; // Binary auto-update: check manifest, verify, exec-restart
pub mod bandwidth; // Bandwidth profiles and message priority (graceful degradation)
pub mod biome; // Tier 3 biome and substrate classification
pub mod bootstrap; // Dynamic bootstrap node discovery
pub mod chunk;
pub mod chunk_loader;
pub mod chunk_manager;
pub mod chunk_placeholder;
pub mod chunk_streaming;
pub mod control_pack_merge;
pub mod feature_record; // OSM-derived feature records stored alongside chunk passes
pub mod http_tunnel; // HTTP fallback for firewall bypass
pub mod identity;
pub mod key_registry; // P2P identity registry — distributed database of KeyRecords
pub mod meshsite; // Meshsite content types (ContentItem, Section)
pub mod messages;
pub mod multiplayer;
pub mod network;
pub mod node_capabilities;
pub mod node_config; // Unified node configuration — shared by client, server, and relay
pub mod osm; // OpenStreetMap data loading from local PBF
pub mod permissions; // Permission system — key-type and ownership enforcement
pub mod player_persistence;
pub mod player_state;
pub mod remote_render;
pub mod spatial_sharding;
pub mod terrain_analysis; // Tier 2 SRTM analysis — slope, TWI, flow, TRI, aspect
pub mod tile_protocol; // P2P tile request/response protocol (libp2p request-response)
pub mod tile_store; // RocksDB-backed tile cache (OSM, SRTM, terrain)
pub mod user_content;
pub mod vector_clock;
pub mod vegetation; // Tier 4c procedural vegetation placement (trees, shrubs)
pub mod web_ui; // Shared Pi-Hole-style web dashboard — server, relay, client
pub mod world_inference; // Deterministic OSM→PlacedObject inference (benches, lamps, etc.)
pub mod world_objects; // Placed-object registry — modular placement schema + DHT keys
pub mod world_store;
pub mod worldgen_osm; // Offline OSM feature baking for worldgen (waterways, etc.)
pub mod worldgen_river; // Tier 4a river profile computation (flow direction, width, depth)
pub mod worldgen_roads; // Tier 4b road geometry carving (carriageway, footpath, bridge, tunnel)
pub mod worldnet; // WORLDNET OS — distributed OS layer, address system, pixel renderer // RocksDB-backed world state (voxel ops, parcels, players)
