//! Feature records store the OSM-derived rules that generated chunk geometry.
//! These travel with the chunk and allow clients to:
//! - Verify/reproduce the geometry deterministically
//! - Apply dynamic modifications (flood events, road closures)
//! - Understand what features are present without parsing OSM

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FeatureRecord {
    Waterway(WaterwayFeature),
    Road(RoadFeature),
    Building(BuildingFeature),
    Dam(DamFeature),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaterwayFeature {
    pub osm_id: i64,
    pub waterway_type: String,     // "river", "canal", "stream", "drain"
    pub water_surface_elev_m: f32, // ASL elevation at this chunk
    pub width_m: f32,
    pub max_depth_m: f32,
    pub substrate: String, // "mud", "gravel", "sand", "stone"
    pub is_tidal: bool,
    pub tidal_range_m: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoadFeature {
    pub osm_id: i64,
    pub road_type: String, // "motorway", "residential", etc.
    pub lanes: u8,
    pub max_speed_kph: u16,
    pub is_bridge: bool,
    pub is_tunnel: bool,
    pub layer: i8,
    pub surface: String, // "asphalt", "concrete", "gravel"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingFeature {
    pub osm_id: i64,
    pub height_m: f32,
    pub levels: u8,
    pub building_type: String, // "residential", "commercial", "industrial"
    pub roof_type: String,     // "flat", "pitched", "dome"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DamFeature {
    pub osm_id: i64,
    pub wall_height_m: f32,
    pub reservoir_level_m: f32, // current operational water level ASL
    pub wall_material: String,  // "concrete", "earthen", "rock"
}

/// Serialize a list of feature records to bytes for TileStore storage.
pub fn serialize_features(records: &[FeatureRecord]) -> Vec<u8> {
    bincode::serialize(records).unwrap_or_default()
}

/// Deserialize feature records from TileStore bytes.
pub fn deserialize_features(bytes: &[u8]) -> Vec<FeatureRecord> {
    bincode::deserialize(bytes).unwrap_or_default()
}
