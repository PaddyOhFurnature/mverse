use metaverse_core::renderer::greedy_mesh::greedy_mesh_block;
use metaverse_core::svo::MaterialId;

fn main() {
    // Simple test: 8x8x8 block of grass
    let mut voxels = vec![0u8; 8*8*8]; // AIR
    
    // Bottom layer = GRASS (material 1)
    for z in 0..8 {
        for x in 0..8 {
            voxels[x + 0*8 + z*64] = 1; // y=0 layer
        }
    }
    
    let origin = [0.0, 0.0, 0.0];
    let (verts, indices) = greedy_mesh_block(&voxels, origin);
    
    println!("Input: 64 grass voxels in bottom layer");
    println!("Output: {} vertices, {} indices ({} triangles)", 
        verts.len(), indices.len(), indices.len()/3);
    
    // Should be ~8 vertices (one merged quad), 6 indices (2 triangles)
    // Plus maybe side faces if exposed
    
    if verts.len() < 100 {
        println!("\n✓ Greedy meshing working (merged into few quads)");
    } else {
        println!("\n✗ Greedy meshing FAILED (too many vertices = not merged)");
    }
}
