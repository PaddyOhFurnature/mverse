use metaverse_core::svo::{SparseVoxelOctree, STONE, AIR};

fn main() {
    // Create simple test SVO with a solid block in middle
    let mut svo = SparseVoxelOctree::new(9);
    
    // Fill a 100x100x10 block at y=250-260 (center of 512³)
    for x in 206..306 {
        for y in 250..260 {
            for z in 206..306 {
                svo.set_voxel(x, y, z, STONE);
            }
        }
    }
    
    println!("Created test SVO (depth 9 = 512³)");
    println!("Filled 100x100x10 block at y=250-260\n");
    
    // Try extracting at different LODs
    for lod in 0..=3 {
        let size = (1u32 << 9) >> lod;
        let step = 1u32 << lod;
        
        println!("LOD {}: size={}, step={}", lod, size, step);
        println!("  Will sample from 0 to {} in steps of {}", size, step);
        
        // Count how many samples hit our block
        let mut hits = 0;
        let mut total_samples = 0;
        
        for x in (0..size).step_by(step as usize) {
            for y in (0..size).step_by(step as usize) {
                for z in (0..size).step_by(step as usize) {
                    total_samples += 1;
                    
                    // Check if this cube intersects our block
                    let x_max = x + step;
                    let y_max = y + step;
                    let z_max = z + step;
                    
                    if x_max > 206 && x < 306 && y_max > 250 && y < 260 && z_max > 206 && z < 306 {
                        hits += 1;
                    }
                }
            }
        }
        
        println!("  Total samples: {}, hits in block: {}", total_samples, hits);
        
        // Now check actual marching cubes
        let triangles = metaverse_core::marching_cubes::extract_mesh(&svo, lod);
        println!("  Marching cubes extracted: {} triangles\n", triangles.len());
    }
}
