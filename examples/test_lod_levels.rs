use metaverse_core::coordinates::*;
use metaverse_core::svo::SparseVoxelOctree;
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::mesh_generation::generate_mesh;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;

fn main() {
    println!("Testing LOD levels with SVO depth 9 (512³)...\n");
    
    let mut srtm = SrtmManager::new(DiskCache::new().unwrap());
    srtm.set_network_enabled(false);
    
    let center_gps = GpsPos {
        lat_deg: -27.4705,
        lon_deg: 153.0260,
        elevation_m: 0.0,
    };
    
    let area_size = 400.0; // 400m chunk
    let svo_depth = 9;
    let svo_size = 1u32 << svo_depth; // 512
    let voxel_size = area_size / svo_size as f64;
    
    println!("Chunk: {}m area", area_size);
    println!("SVO: {}³ voxels", svo_size);
    println!("Voxel size: {:.2}m\n", voxel_size);
    
    let mut svo = SparseVoxelOctree::new(svo_depth as u8);
    
    let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
        srtm.get_elevation(lat, lon).map(|e| e as f32)
    };
    
    let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
        let half = svo_size as f64 / 2.0;
        let dx = (x as f64 - half) * voxel_size;
        let dy = (y as f64 - half) * voxel_size;
        let dz = (z as f64 - half) * voxel_size;
        
        let lat_deg = center_gps.lat_deg + (dz / 111_000.0);
        let lon_deg = center_gps.lon_deg + (dx / (111_000.0 * center_gps.lat_deg.to_radians().cos()));
        let elevation_m = dy;
        
        GpsPos { lat_deg, lon_deg, elevation_m }
    };
    
    println!("Generating terrain...");
    generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
    println!("✓ Terrain generated\n");
    
    for lod in 0..=4 {
        print!("LOD {}: ", lod);
        let meshes = generate_mesh(&svo, lod);
        let total_verts: usize = meshes.iter().map(|m| m.vertices.len()).sum();
        
        if total_verts > 0 {
            println!("{} vertices (skip every {} voxels)", total_verts, 1 << lod);
        } else {
            println!("NO VERTICES (too coarse!)");
        }
    }
}
