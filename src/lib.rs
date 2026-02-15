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
pub mod srtm_downloader;  // New async multi-source downloader
pub mod renderer;

#[cfg(test)]
mod tests;
pub mod terrain;
pub mod osm_features;
pub mod marching_cubes;
pub mod mesh_generation;
pub mod materials;
pub mod svo_integration;
