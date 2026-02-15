use metaverse_core::world_manager::WorldManager;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::osm::OsmData;
use metaverse_core::cache::DiskCache;
use metaverse_core::coordinates::{GpsPos, gps_to_ecef};
use metaverse_core::mesh_generation::generate_mesh;

fn main() {
    println!("=== Testing WorldManager SVO Generation ===\n");
    
    // Initialize SRTM
    let mut srtm = SrtmManager::new();
    
    // Load OSM data
    let cache = DiskCache::new().expect("Failed to create cache");
    let osm_data = cache.get::<OsmData>("brisbane_cbd")
        .expect("Failed to load OSM data");
    
    println!("Loaded {} buildings, {} roads", 
        osm_data.buildings.len(), osm_data.roads.len());
    
    // Create WorldManager
    let mut wm = WorldManager::new(14, 2000.0, 7);
    println!("Created WorldManager: depth=14, svo_depth=7\n");
    
    // Camera at Brisbane
    let camera_gps = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 100.0 };
    let camera_ecef = gps_to_ecef(&camera_gps);
    
    // Update chunks
    println!("Updating chunks for camera position...");
    wm.update(&camera_ecef, &mut srtm, &osm_data);
    
    // Extract meshes
    println!("\nExtracting meshes at LOD 0...");
    let chunk_meshes = wm.extract_meshes(&camera_ecef);
    
    println!("Extracted {} chunks", chunk_meshes.len());
    for (i, (meshes, _center)) in chunk_meshes.iter().enumerate() {
        println!("  Chunk {}: {} material meshes", i, meshes.len());
        for (j, mesh) in meshes.iter().enumerate() {
            println!("    Mesh {}: {} vertices, {} triangles", 
                j, mesh.vertices.len(), mesh.vertices.len() / 3);
        }
    }
    
    if chunk_meshes.is_empty() || chunk_meshes[0].0.is_empty() {
        println!("\n✗ NO MESHES GENERATED - SVO is empty or marching cubes failed");
    } else {
        println!("\n✓ Meshes generated successfully");
    }
}
