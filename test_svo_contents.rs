use metaverse_core::svo::{SparseVoxelOctree, AIR};
use metaverse_core::terrain::generate_terrain_from_elevation;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::cache::DiskCache;
use metaverse_core::coordinates::GpsPos;

fn main() {
    let cache = DiskCache::new().unwrap();
    let mut srtm = SrtmManager::new(cache);
    srtm.set_network_enabled(true);
    
    // Create small test SVO
    let depth = 6; // 64^3 for quick test
    let mut svo = SparseVoxelOctree::new(depth);
    let size = 1u32 << depth; // 64
    
    // Brisbane center
    let center_lat = -27.4698;
    let center_lon = 153.0251;
    let area_size = 100.0; // 100m test area
    let voxel_size = area_size / size as f64; // ~1.56m voxels
    
    println!("Test SVO: {}^3 voxels, area {}m, voxel size {:.2}m", size, area_size, voxel_size);
    println!("Brisbane center: ({}, {})", center_lat, center_lon);
    
    // Voxelization
    let coords_fn = |x: u32, y: u32, z: u32| -> GpsPos {
        let half = size as f64 / 2.0;
        let dx = (x as f64 - half) * voxel_size;
        let dy = (y as f64 - half) * voxel_size;
        let dz = (z as f64 - half) * voxel_size;
        
        let lat_deg = center_lat + (dz / 111_000.0);
        let lon_deg = center_lon + (dx / (111_000.0 * center_lat.to_radians().cos()));
        let elevation_m = dy;
        
        GpsPos { lat_deg, lon_deg, elevation_m }
    };
    
    println!("\n=== Querying SRTM ===");
    let elevation_fn = |lat: f64, lon: f64| -> Option<f32> {
        srtm.get_elevation(lat, lon).map(|e| {
            println!("  SRTM({:.4}, {:.4}) = {:.1}m", lat, lon, e);
            e as f32
        })
    };
    
    generate_terrain_from_elevation(&mut svo, elevation_fn, coords_fn, voxel_size);
    
    println!("\n=== Checking SVO contents ===");
    let mut solid_count = 0;
    
    for y in 0..size {
        let mut row_solids = 0;
        for x in 0..size {
            for z in 0..size {
                if svo.get_voxel(x, y, z) != AIR {
                    row_solids += 1;
                    solid_count += 1;
                }
            }
        }
        if row_solids > 0 {
            let gps = coords_fn(size/2, y, size/2);
            println!("Y={:2} ({:6.1}m elev): {} solid voxels", y, gps.elevation_m, row_solids);
        }
    }
    
    println!("\nTotal: {} solid voxels", solid_count);
}
