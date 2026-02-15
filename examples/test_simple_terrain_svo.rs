use metaverse_core::svo::SparseVoxelOctree;
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;
use metaverse_core::coordinates::GpsPos;
use metaverse_core::mesh_generation::generate_mesh;

fn main() {
    println!("=== Testing Simple Terrain SVO ===\n");
    
    // Brisbane center
    let center = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    
    // Initialize SRTM
    let cache = DiskCache::new().expect("Failed to create cache");
    let mut srtm = SrtmManager::new(cache);
    
    // Create SVO depth 7 (128³) for 677m area
    let depth = 7;
    let mut svo = SparseVoxelOctree::new(depth);
    let svo_size = 1u32 << depth;
    let area_size = 677.0;
    let voxel_size = area_size / svo_size as f64;
    
    println!("SVO: {}³ voxels", svo_size);
    println!("Area: {:.0}m", area_size);
    println!("Voxel size: {:.2}m\n", voxel_size);
    
    // Generate terrain
    let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
        srtm.get_elevation(lat, lon).map(|e| e as f32)
    };
    
    let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
        let half = svo_size as f64 / 2.0;
        let dx = (x as f64 - half) * voxel_size;
        let dy = (y as f64 - half) * voxel_size;
        let dz = (z as f64 - half) * voxel_size;
        
        let lat_deg = center.lat_deg + (dz / 111_000.0);
        let lon_deg = center.lon_deg + (dx / (111_000.0 * center.lat_deg.to_radians().cos()));
        let elevation_m = dy;
        
        GpsPos { lat_deg, lon_deg, elevation_m }
    };
    
    println!("Generating terrain...");
    generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
    println!("✓ Terrain generated\n");
    
    // Extract mesh at LOD 0
    println!("Extracting mesh at LOD 0...");
    let meshes = generate_mesh(&svo, 0);
    
    println!("Extracted {} material meshes", meshes.len());
    for (i, mesh) in meshes.iter().enumerate() {
        println!("  Mesh {}: {} vertices", i, mesh.vertices.len());
    }
    
    if meshes.is_empty() {
        println!("\n✗ NO MESHES - marching cubes found no surfaces");
    } else {
        println!("\n✓ SUCCESS");
    }
}
