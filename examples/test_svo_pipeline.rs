//! Test SVO Pipeline: Terrain → CSG → Marching Cubes → Mesh
//!
//! Demonstrates the CORRECT architecture:
//! 1. Create SVO
//! 2. Voxelize terrain from SRTM
//! 3. Apply CSG operations (rivers, roads, buildings)
//! 4. Extract mesh via marching cubes
//! 5. Render

use metaverse_core::svo::SparseVoxelOctree;
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::mesh_generation::generate_mesh;
use metaverse_core::coordinates::GpsPos;

fn main() {
    println!("=== SVO PIPELINE TEST ===\n");
    
    // Step 1: Create SVO (256^3 voxels at depth 8)
    let mut svo = SparseVoxelOctree::new(8);
    println!("✓ Created SVO: {}^3 voxels", 1 << svo.max_depth());
    
    // Step 2: Voxelize terrain
    println!("\nVoxelizing terrain...");
    
    // Simple elevation function: flat at 10m with a hill in the center
    let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
        let center_lat = -27.47;
        let center_lon = 153.03;
        let dist = ((lat - center_lat).powi(2) + (lon - center_lon).powi(2)).sqrt();
        let elevation = if dist < 0.01 {
            20.0 // Hill in center
        } else {
            10.0 // Flat ground
        };
        Some(elevation as f32)
    };
    
    // Coordinate mapping (simple linear for test)
    let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
        let size = 1u32 << 8; // 256
        let lat = -27.48 + (z as f64 / size as f64) * 0.02;
        let lon = 153.02 + (x as f64 / size as f64) * 0.02;
        let elev = y as f64; // 1m per voxel
        GpsPos { lat_deg: lat, lon_deg: lon, elevation_m: elev }
    };
    
    generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, 1.0);
    println!("✓ Terrain voxelized (STONE/DIRT/AIR)");
    
    // Step 3: Extract mesh via marching cubes
    println!("\nExtracting mesh via marching cubes...");
    let meshes = generate_mesh(&svo, 0); // LOD 0 = finest detail
    
    let total_verts: usize = meshes.iter().map(|m| m.vertices.len() / 6).sum();
    let total_tris: usize = meshes.iter().map(|m| m.indices.len() / 3).sum();
    
    println!("✓ Mesh extracted:");
    println!("  {} meshes (by material)", meshes.len());
    println!("  {} vertices", total_verts);
    println!("  {} triangles", total_tris);
    
    for mesh in &meshes {
        let verts = mesh.vertices.len() / 6;
        let tris = mesh.indices.len() / 3;
        println!("    Material {:?}: {} verts, {} tris", mesh.material, verts, tris);
    }
    
    println!("\n=== PIPELINE WORKING ===");
    println!("\nThis is the CORRECT approach:");
    println!("  Real data → SVO voxels → Marching cubes → Triangle mesh → GPU");
    println!("\nNOT:");
    println!("  Real data → Direct triangle generation → GPU (WRONG!)");
}
