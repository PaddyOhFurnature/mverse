use metaverse_core::coordinates::*;
use metaverse_core::world_manager::WorldManager;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;
use metaverse_core::osm::OsmData;

fn main() {
    // Simple stdout logging
    println!("=== WorldManager Debug Test ===\n");
    
    // Brisbane test position
    let brisbane_gps = GpsPos {
        lat_deg: -27.4705,
        lon_deg: 153.0260,
        elevation_m: 0.0,
    };
    println!("Target: Brisbane");
    println!("  GPS: ({:.6}°, {:.6}°)", brisbane_gps.lat_deg, brisbane_gps.lon_deg);
    
    let brisbane_ecef = gps_to_ecef(&brisbane_gps);
    println!("  ECEF: ({:.1}, {:.1}, {:.1})", brisbane_ecef.x, brisbane_ecef.y, brisbane_ecef.z);
    
    // Camera 500m above Brisbane
    let distance = (brisbane_ecef.x*brisbane_ecef.x + brisbane_ecef.y*brisbane_ecef.y + brisbane_ecef.z*brisbane_ecef.z).sqrt();
    let up_x = brisbane_ecef.x / distance;
    let up_y = brisbane_ecef.y / distance;
    let up_z = brisbane_ecef.z / distance;
    
    let camera_ecef = EcefPos {
        x: brisbane_ecef.x + up_x * 500.0,
        y: brisbane_ecef.y + up_y * 500.0,
        z: brisbane_ecef.z + up_z * 500.0,
    };
    println!("  Camera (+500m): ({:.1}, {:.1}, {:.1})\n", camera_ecef.x, camera_ecef.y, camera_ecef.z);
    
    // Create SRTM manager
    println!("Creating SRTM manager...");
    let cache = DiskCache::new().expect("Failed to create cache");
    let mut srtm = SrtmManager::new(cache);
    srtm.set_network_enabled(false); // Use cache only
    println!("  ✓ Done\n");
    
    // Use empty OSM data for now
    println!("Using empty OSM data");
    let osm_data = OsmData::default();
    println!();
    
    // Create WorldManager
    println!("Creating WorldManager...");
    let mut world_manager = WorldManager::new(
        14,     // chunk depth (400m chunks)
        2000.0, // render distance (2km)
        9,      // SVO depth (512^3 = ~0.78m voxels)
    );
    println!("  ✓ Done\n");
    
    // Update chunks
    println!("=== First Update (Camera at +500m) ===");
    let num_chunks = world_manager.update(&camera_ecef, &mut srtm, &osm_data);
    println!("Total chunks loaded: {}\n", num_chunks);
    
    // Extract meshes
    println!("=== Extracting Meshes ===");
    let chunk_meshes = world_manager.extract_meshes(&camera_ecef);
    println!("Chunks with meshes: {}", chunk_meshes.len());
    
    let mut total_vertices = 0;
    for (i, (meshes, center)) in chunk_meshes.iter().enumerate() {
        println!("\nChunk {}:", i);
        println!("  Center ECEF: ({:.1}, {:.1}, {:.1})", center.x, center.y, center.z);
        println!("  Material meshes: {}", meshes.len());
        for (mat_idx, mesh) in meshes.iter().enumerate() {
            let verts = mesh.vertices.len();
            total_vertices += verts;
            println!("    Material {}: {} vertices", mat_idx, verts);
        }
    }
    
    println!("\n=== Summary ===");
    println!("Total vertices: {}", total_vertices);
    
    if total_vertices == 0 {
        println!("\n⚠ WARNING: NO VERTICES GENERATED!");
        println!("This means marching cubes found no surfaces in the SVO.");
        println!("Possible reasons:");
        println!("  1. SRTM cache is empty (no elevation data)");
        println!("  2. Terrain generation failed silently");
        println!("  3. Marching cubes bug");
    } else {
        println!("\n✓ Geometry generated successfully");
    }
}
