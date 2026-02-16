// Quick test: Does greedy meshing actually merge faces?
use metaverse_core::renderer::greedy_mesh::greedy_mesh_block;

fn main() {
    // Create 8x8x8 block filled with GRASS (material 3)
    let mut voxels = [3u8; 512]; // All GRASS
    
    // Make top layer (y=7) GRASS, everything else AIR
    for i in 0..512 {
        let y = (i / 8) % 8;
        if y != 7 {
            voxels[i] = 0; // AIR
        }
    }
    
    let origin = [0.0, 0.0, 0.0];
    let (verts, indices) = greedy_mesh_block(&voxels, origin);
    
    println!("Flat 8x8 GRASS layer:");
    println!("  Input: 64 voxels");
    println!("  Output: {} vertices, {} indices ({} triangles)", 
        verts.len(), indices.len(), indices.len()/3);
    println!("");
    println!("Expected: ~4 vertices (one merged quad), 6 indices (2 triangles)");
    println!("  Plus side faces if exposed");
    
    if verts.len() < 100 {
        println!("✓ Greedy meshing IS working (vertices merged)");
    } else {
        println!("✗ Greedy meshing FAILED (too many vertices = not merged)");
    }
}
