/// World Manager: Per-chunk SVO streaming system
///
/// Manages multiple SVO chunks, streaming them in/out based on camera position.
/// Each chunk is a small area (~1km) with high voxel resolution (~1m).

use std::collections::HashMap;
use crate::chunks::{ChunkId, gps_to_chunk_id, chunk_bounds_gps, chunk_center_ecef};
use crate::svo::SparseVoxelOctree;
use crate::terrain::generate_terrain_from_elevation;
use crate::osm_features::{carve_river, place_road, add_building};
use crate::mesh_generation::{generate_mesh, Mesh};
use crate::elevation::SrtmManager;
use crate::osm::{OsmData, OsmBuilding, OsmRoad, OsmWater};
use crate::coordinates::{EcefPos, GpsPos, gps_to_ecef};

/// A single world chunk with its SVO
pub struct Chunk {
    pub id: ChunkId,
    pub svo: SparseVoxelOctree,
    pub center: EcefPos,
    pub bounds: (GpsPos, GpsPos),
    pub voxel_size: f64,
}

/// Manages streaming of SVO chunks
pub struct WorldManager {
    chunks: HashMap<ChunkId, Chunk>,
    chunk_depth: usize,
    render_distance: f64,
    svo_depth: u8,
    last_camera_pos: Option<EcefPos>,
}

impl WorldManager {
    /// Create new world manager
    pub fn new(chunk_depth: usize, render_distance: f64, svo_depth: u8) -> Self {
        println!("[WorldManager::new] Creating with depth={}, render_distance={}, svo_depth={}", 
            chunk_depth, render_distance, svo_depth);
        let wm = Self {
            chunks: HashMap::new(),
            chunk_depth,
            render_distance,
            svo_depth,
            last_camera_pos: None,
        };
        println!("[WorldManager::new] ✓ Created successfully");
        wm
    }
    
    /// Get SVO depth (for coordinate transforms)
    pub fn svo_depth(&self) -> u8 {
        self.svo_depth
    }
    
    /// Update loaded chunks based on camera position
    pub fn update(&mut self, camera_pos: &EcefPos, srtm: &mut SrtmManager, osm_data: &OsmData) -> usize {
        // Check if camera moved significantly
        let needs_update = match self.last_camera_pos {
            None => {
                println!("[WorldManager] First update - initializing chunks");
                true
            }
            Some(ref last) => {
                let dx = camera_pos.x - last.x;
                let dy = camera_pos.y - last.y;
                let dz = camera_pos.z - last.z;
                let dist = (dx*dx + dy*dy + dz*dz).sqrt();
                if dist > 100.0 {
                    println!("[WorldManager] Camera moved {:.1}m - updating chunks", dist);
                    true
                } else {
                    false
                }
            }
        };
        
        if !needs_update {
            return self.chunks.len();
        }
        
        self.last_camera_pos = Some(*camera_pos);
        
        // Find chunks in render distance
        let target_chunks = self.find_chunks_in_range(camera_pos);
        
        // Unload far chunks
        let mut unloaded = 0;
        self.chunks.retain(|id, _| {
            let keep = target_chunks.contains(id);
            if !keep {
                unloaded += 1;
            }
            keep
        });
        
        if unloaded > 0 {
            println!("Unloaded {} chunks", unloaded);
        }
        
        // Load new chunks
        let mut loaded = 0;
        for chunk_id in target_chunks {
            if !self.chunks.contains_key(&chunk_id) {
                println!("[WorldManager] Generating chunk {:?}...", chunk_id);
                if let Some(chunk) = generate_chunk_svo(&chunk_id, self.svo_depth, srtm, osm_data) {
                    let voxel_count = 1u64 << (self.svo_depth * 3);
                    println!("[WorldManager] ✓ Chunk generated (max {} voxels)", voxel_count);
                    self.chunks.insert(chunk_id.clone(), chunk);
                    loaded += 1;
                } else {
                    println!("[WorldManager] ✗ Failed to generate chunk");
                }
            }
        }
        
        if loaded > 0 {
            println!("[WorldManager] Loaded {} new chunks (total: {})", loaded, self.chunks.len());
        }
        
        self.chunks.len()
    }
    
    /// Get chunks in render distance
    fn find_chunks_in_range(&self, camera_pos: &EcefPos) -> Vec<ChunkId> {
        // Convert camera to GPS to get chunk
        let camera_gps = crate::coordinates::ecef_to_gps(camera_pos);
        println!("[find_chunks_in_range] Camera GPS: ({:.6}, {:.6}, {:.1}m)", 
            camera_gps.lat_deg, camera_gps.lon_deg, camera_gps.elevation_m);
        
        let camera_chunk = gps_to_chunk_id(&camera_gps, self.chunk_depth as u8);
        println!("[find_chunks_in_range] Camera chunk ID: {}", camera_chunk);
        
        // For now, just return camera chunk + immediate neighbors
        // TODO: Properly search all chunks within radius
        vec![camera_chunk]
    }
    
    /// Extract meshes for all loaded chunks at appropriate LOD
    pub fn extract_meshes(&self, camera_pos: &EcefPos) -> Vec<(Vec<Mesh>, EcefPos)> {
        let mut results = Vec::new();
        
        for (_id, chunk) in &self.chunks {
            println!("[extract_meshes] Camera ECEF: ({:.1}, {:.1}, {:.1})", 
                camera_pos.x, camera_pos.y, camera_pos.z);
            println!("[extract_meshes] Chunk center ECEF: ({:.1}, {:.1}, {:.1})", 
                chunk.center.x, chunk.center.y, chunk.center.z);
            
            // Calculate distance from camera to chunk center
            let dx = camera_pos.x - chunk.center.x;
            let dy = camera_pos.y - chunk.center.y;
            let dz = camera_pos.z - chunk.center.z;
            let distance = (dx*dx + dy*dy + dz*dz).sqrt();
            
            println!("[extract_meshes] Delta: ({:.1}, {:.1}, {:.1})", dx, dy, dz);
            println!("[extract_meshes] Chunk distance: {:.1}m", distance);
            
            // Select LOD based on distance (much larger ranges for flying camera)
            let lod = if distance < 500.0 {
                0
            } else if distance < 2000.0 {
                1
            } else if distance < 5000.0 {
                2
            } else if distance < 10000.0 {
                3
            } else {
                println!("[extract_meshes] Chunk too far ({:.1}m > 10km), skipping", distance);
                continue; // Too far, don't render
            };
            
            println!("[extract_meshes] Using LOD {} for distance {:.1}m", lod, distance);
            
            // Extract mesh at selected LOD
            let meshes = generate_mesh(&chunk.svo, lod);
            println!("[extract_meshes] Extracted {} material meshes", meshes.len());
            results.push((meshes, chunk.center));
        }
        
        println!("[extract_meshes] Returning {} chunk results", results.len());
        results
    }
    
    /// Get loaded chunk count
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
}

/// Generate SVO for a single chunk
fn generate_chunk_svo(
    chunk_id: &ChunkId,
    svo_depth: u8,
    srtm: &mut SrtmManager,
    osm_data: &OsmData,
) -> Option<Chunk> {
    println!("[generate_chunk_svo] Chunk ID: {}", chunk_id);
    
    // Get chunk bounds
    let bounds = match chunk_bounds_gps(chunk_id) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to get bounds for chunk {}: {}", chunk_id, e);
            return None;
        }
    };
    
    let (sw, ne) = bounds;
    
    println!("[generate_chunk_svo] Bounds: SW({:.6}, {:.6}) NE({:.6}, {:.6})", 
        sw.lat_deg, sw.lon_deg, ne.lat_deg, ne.lon_deg);
    
    // Use proper chunk_center_ecef function instead of GPS average
    let center = chunk_center_ecef(chunk_id);
    let center_gps = GpsPos {
        lat_deg: (sw.lat_deg + ne.lat_deg) / 2.0,
        lon_deg: (sw.lon_deg + ne.lon_deg) / 2.0,
        elevation_m: 0.0,
    };
    
    println!("[generate_chunk_svo] Center GPS: ({:.6}, {:.6}, {:.1}m)", 
        center_gps.lat_deg, center_gps.lon_deg, center_gps.elevation_m);
    println!("[generate_chunk_svo] Center ECEF: ({:.1}, {:.1}, {:.1})", 
        center.x, center.y, center.z);
    
    // Calculate chunk size
    let lat_span = (ne.lat_deg - sw.lat_deg).abs() * 111_000.0; // ~111km per degree
    let lon_span = (ne.lon_deg - sw.lon_deg).abs() * 111_000.0 * sw.lat_deg.to_radians().cos();
    let area_size = lat_span.max(lon_span);
    
    // Create SVO
    let mut svo = SparseVoxelOctree::new(svo_depth);
    let svo_size = 1u32 << svo_depth;
    let voxel_size = area_size / svo_size as f64;
    
    println!("Generating chunk {}: {:.0}m area, {:.2}m voxels", chunk_id, area_size, voxel_size);
    
    // Voxelize terrain
    let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
        if lat >= sw.lat_deg && lat <= ne.lat_deg && lon >= sw.lon_deg && lon <= ne.lon_deg {
            srtm.get_elevation(lat, lon).map(|e| e as f32)
        } else {
            None
        }
    };
    
    let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
        let half = svo_size as f64 / 2.0;
        let dx = (x as f64 - half) * voxel_size;
        let dy = (y as f64 - half) * voxel_size;
        let dz = (z as f64 - half) * voxel_size;
        
        let lat_deg = center_gps.lat_deg + (dz / 111_000.0);
        let lon_deg = center_gps.lon_deg + (dx / (111_000.0 * center_gps.lat_deg.to_radians().cos()));
        let elevation_m = dy;
        
        GpsPos { lat_deg, lon_deg, elevation_m }
    };
    
    generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
    
    // Add OSM features within chunk bounds
    let chunk_center = gps_to_ecef(&center_gps);
    
    // Filter and add water
    for water in &osm_data.water {
        if feature_in_bounds(&water.polygon, &sw, &ne) {
            carve_river(&mut svo, &chunk_center, "river", &water.polygon, 30.0, voxel_size);
        }
    }
    
    // Filter and add roads (limit to avoid slowness)
    let mut roads_added = 0;
    for road in osm_data.roads.iter().take(100) {
        if feature_in_bounds(&road.nodes, &sw, &ne) {
            place_road(&mut svo, &chunk_center, "road", &road.nodes, voxel_size);
            roads_added += 1;
        }
    }
    
    // Filter and add buildings (limit to avoid slowness)
    let mut buildings_added = 0;
    for building in osm_data.buildings.iter().take(100) {
        if feature_in_bounds(&building.polygon, &sw, &ne) {
            add_building(&mut svo, &chunk_center, building, voxel_size);
            buildings_added += 1;
        }
    }
    
    println!("  {} roads, {} buildings", roads_added, buildings_added);
    
    Some(Chunk {
        id: chunk_id.clone(),
        svo,
        center,
        bounds,
        voxel_size,
    })
}

/// Check if feature overlaps chunk bounds
fn feature_in_bounds(points: &[GpsPos], sw: &GpsPos, ne: &GpsPos) -> bool {
    points.iter().any(|p| {
        p.lat_deg >= sw.lat_deg && p.lat_deg <= ne.lat_deg &&
        p.lon_deg >= sw.lon_deg && p.lon_deg <= ne.lon_deg
    })
}
