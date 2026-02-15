use metaverse_core::{coordinates::*, chunks::*, world_manager::*, svo::*, elevation::*};

fn main() {
    let chunk_id = gps_to_chunk_id(&GpsPos {
        lat_deg: -27.4698,
        lon_deg: 153.0251,
        elevation_m: 0.0,
    }, 14);
    
    println!("Testing chunk: {}", chunk_id);
    
    let svo_depth = 7;  // 128³
    let size = 1u32 << svo_depth;
    println!("SVO depth {}: {}³ = {} voxels per side", svo_depth, size, size);
    
    // Test voxelization (simplified)
    let bounds = chunk_bounds_gps(&chunk_id).unwrap();
    println!("Bounds: SW({:.6}, {:.6}) NE({:.6}, {:.6})",
        bounds.0.lat_deg, bounds.0.lon_deg, bounds.1.lat_deg, bounds.1.lon_deg);
}
