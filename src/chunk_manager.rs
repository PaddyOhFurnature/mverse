//! Chunk Manager: Spatial loading/unloading of world geometry
//!
//! Divides the world into chunks and only loads geometry within render distance.
//! This allows rendering the full dataset without GPU buffer overflow.

use std::collections::{HashMap, HashSet};
use glam::DVec3;

use crate::chunks::{ChunkId, ecef_to_cube_face, gps_to_chunk_id};
use crate::coordinates::{EcefPos, gps_to_ecef, GpsPos, ecef_to_gps};
use crate::osm::{OsmData, OsmBuilding, OsmRoad, OsmWater};

/// Manages loading and unloading of world chunks based on camera position
pub struct ChunkManager {
    /// Currently loaded chunks (ChunkId -> OsmData)
    loaded_chunks: HashMap<ChunkId, OsmData>,
    
    /// Chunk depth/LOD level to use
    chunk_depth: usize,
    
    /// Maximum render distance in meters
    render_distance_m: f64,
    
    /// Last camera position (for detecting movement)
    last_camera_ecef: Option<EcefPos>,
}

impl ChunkManager {
    /// Creates a new chunk manager
    ///
    /// # Arguments
    /// * `chunk_depth` - Quadtree depth for chunks (higher = smaller chunks)
    ///   - Depth 5: ~1000km chunks (whole continents)
    ///   - Depth 7: ~250km chunks (large cities)
    ///   - Depth 9: ~60km chunks (city districts) **RECOMMENDED**
    ///   - Depth 11: ~15km chunks (neighborhoods)
    /// * `render_distance_m` - Only load chunks within this distance
    pub fn new(chunk_depth: usize, render_distance_m: f64) -> Self {
        Self {
            loaded_chunks: HashMap::new(),
            chunk_depth,
            render_distance_m,
            last_camera_ecef: None,
        }
    }
    
    /// Updates loaded chunks based on camera position
    ///
    /// Loads new chunks that entered render distance, unloads chunks that left.
    ///
    /// # Arguments
    /// * `camera_ecef` - Current camera position in ECEF coordinates
    /// * `osm_data` - Full OSM dataset to partition into chunks
    ///
    /// # Returns
    /// Number of chunks currently loaded
    pub fn update(&mut self, camera_ecef: &EcefPos, osm_data: &OsmData) -> usize {
        // Check if camera moved significantly (more than 500m)
        let needs_update = match self.last_camera_ecef {
            None => true,
            Some(ref last_pos) => {
                let camera_vec = DVec3::new(camera_ecef.x, camera_ecef.y, camera_ecef.z);
                let last_vec = DVec3::new(last_pos.x, last_pos.y, last_pos.z);
                (camera_vec - last_vec).length() > 500.0
            }
        };
        
        if !needs_update {
            return self.loaded_chunks.len();
        }
        
        self.last_camera_ecef = Some(*camera_ecef);
        
        // Determine which chunks should be loaded
        let target_chunks = self.find_chunks_in_range(camera_ecef);
        
        // Unload chunks that are now out of range
        self.loaded_chunks.retain(|chunk_id, _| target_chunks.contains(chunk_id));
        
        // Load new chunks that entered range
        for chunk_id in target_chunks {
            if !self.loaded_chunks.contains_key(&chunk_id) {
                let chunk_data = self.partition_osm_data_for_chunk(&chunk_id, osm_data);
                self.loaded_chunks.insert(chunk_id, chunk_data);
            }
        }
        
        self.loaded_chunks.len()
    }
    
    /// Finds all chunk IDs within render distance of camera
    fn find_chunks_in_range(&self, camera_ecef: &EcefPos) -> HashSet<ChunkId> {
        let mut chunks = HashSet::new();
        
        // Get camera's chunk
        let camera_gps = ecef_to_gps(camera_ecef);
        let camera_chunk = gps_to_chunk_id(&camera_gps, self.chunk_depth as u8);
        chunks.insert(camera_chunk.clone());
        
        // TODO: Add neighboring chunks based on render distance
        // For now, just load the camera's chunk and its 8 neighbors at same depth
        // This is a simplification - proper implementation would:
        // 1. Calculate actual distance to each chunk center
        // 2. Load all chunks whose bounding sphere intersects render distance
        
        // Add immediate neighbors (8 surrounding chunks)
        for neighbor in self.get_neighbor_chunks(&camera_chunk) {
            chunks.insert(neighbor);
        }
        
        chunks
    }
    
    /// Gets neighboring chunks at the same depth
    fn get_neighbor_chunks(&self, chunk_id: &ChunkId) -> Vec<ChunkId> {
        // TODO: Implement proper neighbor finding
        // For now, return empty - we'll just load the camera chunk
        Vec::new()
    }
    
    /// Partitions OSM data into a specific chunk
    fn partition_osm_data_for_chunk(&self, chunk_id: &ChunkId, full_data: &OsmData) -> OsmData {
        // Filter buildings/roads/water that fall within this chunk
        // TODO: Implement proper spatial filtering using chunk bounds
        
        // For now, return a subset based on simple position checks
        let buildings: Vec<OsmBuilding> = full_data.buildings
            .iter()
            .filter(|b| self.is_in_chunk(chunk_id, &b.polygon[0]))
            .cloned()
            .collect();
            
        let roads: Vec<OsmRoad> = full_data.roads
            .iter()
            .filter(|r| r.nodes.iter().any(|n| {
                let gps = GpsPos {
                    lat_deg: n.lat_deg,
                    lon_deg: n.lon_deg,
                    elevation_m: n.elevation_m,
                };
                self.is_in_chunk(chunk_id, &gps)
            }))
            .cloned()
            .collect();
            
        let water: Vec<OsmWater> = full_data.water
            .iter()
            .filter(|w| self.is_in_chunk(chunk_id, &w.polygon[0]))
            .cloned()
            .collect();
        
        OsmData {
            buildings,
            roads,
            water,
            parks: Vec::new(), // TODO: Add parks filtering
        }
    }
    
    /// Checks if a GPS position falls within a chunk
    fn is_in_chunk(&self, chunk_id: &ChunkId, gps: &GpsPos) -> bool {
        let point_chunk = gps_to_chunk_id(gps, self.chunk_depth as u8);
        *chunk_id == point_chunk
    }
    
    /// Gets all loaded chunks and their data
    pub fn get_loaded_chunks(&self) -> &HashMap<ChunkId, OsmData> {
        &self.loaded_chunks
    }
    
    /// Gets render distance in meters
    pub fn render_distance(&self) -> f64 {
        self.render_distance_m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chunk_manager_creation() {
        let manager = ChunkManager::new(9, 10000.0);
        assert_eq!(manager.chunk_depth, 9);
        assert_eq!(manager.render_distance_m, 10000.0);
        assert_eq!(manager.loaded_chunks.len(), 0);
    }
    
    #[test]
    fn test_update_loads_chunks() {
        let mut manager = ChunkManager::new(9, 10000.0);
        let camera = EcefPos {
            x: 6378137.0, // Equator at prime meridian
            y: 0.0,
            z: 0.0,
        };
        
        let osm_data = OsmData {
            buildings: Vec::new(),
            roads: Vec::new(),
            water: Vec::new(),
            parks: Vec::new(),
        };
        
        let loaded_count = manager.update(&camera, &osm_data);
        assert!(loaded_count > 0, "Should load at least camera's chunk");
    }
}
