//! Debug mesh positions and bounds

use metaverse_core::{
    coordinates::GPS,
    elevation::{ElevationPipeline, NasFileSource, OpenTopographySource},
    marching_cubes::extract_octree_mesh,
    terrain::TerrainGenerator,
    voxel::{Octree, VoxelCoord},
};
use std::path::PathBuf;

fn main() {
    println!("=== Mesh Debug ===\n");
    
    // Generate terrain (same as viewer)
    let nas = NasFileSource::new();
    let api_key = "3e607de6969c687053f9e107a4796962".to_string();
    let cache_dir = PathBuf::from("./elevation_cache");
    let api = OpenTopographySource::new(api_key, cache_dir);
    
    let mut pipeline = ElevationPipeline::new();
    if let Some(nas_source) = nas {
        pipeline.add_source(Box::new(nas_source));
    }
    pipeline.add_source(Box::new(api));
    
    let mut generator = TerrainGenerator::new(pipeline);
    let mut octree = Octree::new();
    
    let origin = GPS::new(-27.4775, 153.0355, 0.0);
    println!("Generating terrain at {:?}...", origin);
    generator.generate_region(&mut octree, &origin, 120.0).unwrap();
    
    let origin_ecef = origin.to_ecef();
    let origin_voxel = VoxelCoord::from_ecef(&origin_ecef);
    
    println!("Origin ECEF: ({:.0}, {:.0}, {:.0})", origin_ecef.x, origin_ecef.y, origin_ecef.z);
    println!("Origin Voxel: ({}, {}, {})", origin_voxel.x, origin_voxel.y, origin_voxel.z);
    
    let mesh = extract_octree_mesh(&octree, &origin_voxel, 7);
    
    println!("\nMesh stats:");
    println!("  Vertices: {}", mesh.vertex_count());
    println!("  Triangles: {}", mesh.triangle_count());
    
    if mesh.is_empty() {
        println!("\nERROR: Mesh is empty!");
        return;
    }
    
    // Find vertex bounds
    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    let mut min_z = f32::MAX;
    let mut max_z = f32::MIN;
    
    for v in &mesh.vertices {
        let pos = v.position;
        min_x = min_x.min(pos.x);
        max_x = max_x.max(pos.x);
        min_y = min_y.min(pos.y);
        max_y = max_y.max(pos.y);
        min_z = min_z.min(pos.z);
        max_z = max_z.max(pos.z);
    }
    
    println!("\nVertex bounds:");
    println!("  X: {:.2} to {:.2} (range: {:.2})", min_x, max_x, max_x - min_x);
    println!("  Y: {:.2} to {:.2} (range: {:.2})", min_y, max_y, max_y - min_y);
    println!("  Z: {:.2} to {:.2} (range: {:.2})", min_z, max_z, max_z - min_z);
    
    let center_x = (min_x + max_x) / 2.0;
    let center_y = (min_y + max_y) / 2.0;
    let center_z = (min_z + max_z) / 2.0;
    
    println!("\nMesh center: ({:.2}, {:.2}, {:.2})", center_x, center_y, center_z);
    
    println!("\nCamera at (-80, 60, 80) should be looking at origin (0, 0, 0)");
    println!("  Distance from camera to mesh center: {:.2}", 
        ((center_x + 80.0).powi(2) + (center_y - 60.0).powi(2) + (center_z - 80.0).powi(2)).sqrt());
    
    println!("\nSample vertices (first 10):");
    for (i, v) in mesh.vertices.iter().take(10).enumerate() {
        println!("  [{}] pos=({:.2}, {:.2}, {:.2}) normal=({:.2}, {:.2}, {:.2})",
            i, v.position.x, v.position.y, v.position.z,
            v.normal.x, v.normal.y, v.normal.z);
    }
}
