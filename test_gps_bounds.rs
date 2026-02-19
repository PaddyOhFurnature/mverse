use metaverse_core::chunk::ChunkId;
use metaverse_core::voxel::VoxelCoord;

fn main() {
    let origin_voxel = VoxelCoord::new(1353000, 8968690, 3475406);
    let spawn_chunk = ChunkId::from_voxel(&origin_voxel);
    
    println!("Spawn chunk: {}", spawn_chunk);
    println!("Spawn voxel bounds: {:?} to {:?}", spawn_chunk.min_voxel(), spawn_chunk.max_voxel());
    
    let (lat_min, lat_max, lon_min, lon_max) = spawn_chunk.gps_bounds();
    println!("Spawn GPS bounds: lat [{:.6}, {:.6}], lon [{:.6}, {:.6}]", lat_min, lat_max, lon_min, lon_max);
    
    // Check neighbor chunks
    let neighbor = ChunkId::new(spawn_chunk.x + 1, spawn_chunk.y, spawn_chunk.z);
    println!("\nNeighbor chunk: {}", neighbor);
    println!("Neighbor voxel bounds: {:?} to {:?}", neighbor.min_voxel(), neighbor.max_voxel());
    
    let (lat_min2, lat_max2, lon_min2, lon_max2) = neighbor.gps_bounds();
    println!("Neighbor GPS bounds: lat [{:.6}, {:.6}], lon [{:.6}, {:.6}]", lat_min2, lat_max2, lon_min2, lon_max2);
}
