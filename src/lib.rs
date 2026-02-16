pub mod coordinates;
pub mod chunks;
pub mod chunk_manager;
pub mod lod;
pub mod svo;
pub mod cache;
pub mod osm;
pub mod elevation;
pub mod elevation_sources;
pub mod elevation_downloader;
pub mod srtm_downloader;
// pub mod terrain_mesh;  // REMOVED - bypassed SVO pipeline
pub mod renderer;

// Continuous query system (Phase 1)
pub mod spatial_index;
pub mod adaptive_cache;
pub mod continuous_world;
pub mod procedural_generator;

#[cfg(test)]
mod tests;
pub mod terrain;
pub mod osm_features;
pub mod marching_cubes;
pub mod mesh_generation;
pub mod materials;
pub mod world_manager;
// pub mod svo_integration;  // REMOVED - bypassed SVO pipeline
