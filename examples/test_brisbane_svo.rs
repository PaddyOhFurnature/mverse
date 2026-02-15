//! Brisbane SVO with Real SRTM Data
//! Tests the full pipeline: SRTM → SVO → Marching Cubes → Mesh

use metaverse_core::svo::SparseVoxelOctree;
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::mesh_generation::generate_mesh;
use metaverse_core::coordinates::{GpsPos, gps_to_ecef};
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;

fn main() {
    println!("=== Brisbane SVO Pipeline Test ===\n");
    
    // Story Bridge area
    let center = GpsPos {
        lat_deg: -27.463697,
        lon_deg: 153.035725,
        elevation_m: 0.0,
    };
    let center_ecef = gps_to_ecef(&center);
    
    // Initialize SRTM
    let cache = DiskCache::new().expect("Failed to create cache");
    let mut srtm = SrtmManager::new(cache);
    srtm.set_network_enabled(false); // Use cached only
    
    println!("Testing SRTM data availability...");
    match srtm.get_elevation(center.lat_deg, center.lon_deg) {
        Some(elev) => println!("✓ SRTM data loaded: {:.1}m elevation at Story Bridge", elev),
        None => {
            println!("✗ No SRTM data - run: cargo run --example download_brisbane_data");
            return;
        }
    }
    
    // Create SVO (depth 6 = 64^3 for testing, ~500m coverage)
    println!("\nCreating SVO...");
    let depth = 6;
    let mut svo = SparseVoxelOctree::new(depth);
    let svo_size = 1u32 << depth;
    println!("✓ SVO: {}^3 voxels", svo_size);
    
    // Voxel size to cover ~500m area
    let area_size = 500.0; // meters
    let voxel_size = area_size / svo_size as f64;
    println!("  Voxel size: {:.2}m", voxel_size);
    println!("  Coverage: {:.0}m × {:.0}m", area_size, area_size);
    
    // Voxelize terrain
    println!("\nVoxelizing terrain from SRTM...");
    
    let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
        srtm.get_elevation(lat, lon).map(|e| e as f32)
    };
    
    let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
        // Convert voxel coords to offset from center
        let half = svo_size as f64 / 2.0;
        let dx = (x as f64 - half) * voxel_size;
        let dy = (y as f64 - half) * voxel_size; 
        let dz = (z as f64 - half) * voxel_size;
        
        // Simple linear approximation (good enough for small area)
        let lat_deg = center.lat_deg + (dz / 111_000.0);
        let lon_deg = center.lon_deg + (dx / (111_000.0 * center.lat_deg.to_radians().cos()));
        let elevation_m = dy; // y axis = elevation
        
        GpsPos { lat_deg, lon_deg, elevation_m }
    };
    
    generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
    println!("✓ Terrain voxelized");
    
    // Extract mesh
    println!("\nExtracting mesh via marching cubes...");
    let meshes = generate_mesh(&svo, 0);
    
    let total_verts: usize = meshes.iter().map(|m| m.vertices.len() / 6).sum();
    let total_tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    
    println!("✓ Extracted mesh:");
    println!("  {} material meshes", meshes.len());
    println!("  {} vertices", total_verts);
    println!("  {} triangles", total_tris);
    
    for mesh in &meshes {
        let verts = mesh.vertices.len() / 6;
        let tris = mesh.indices.len() / 3;
        if verts > 0 {
            println!("    Material {:?}: {} verts, {} tris", mesh.material, verts, tris);
        }
    }
    
    if total_verts > 0 {
        println!("\n✅ SUCCESS: Real SRTM terrain rendered via SVO pipeline");
        println!("   Pipeline: SRTM → Voxels → Marching Cubes → Triangles");
    } else {
        println!("\n⚠️  No geometry - check elevation data coverage");
    }
}
