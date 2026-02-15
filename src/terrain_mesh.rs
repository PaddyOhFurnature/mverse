/// Generate terrain mesh from SRTM elevation data
use crate::coordinates::{gps_to_ecef, GpsPos};
use crate::elevation::SrtmManager;
use crate::svo_integration::ColoredVertex;

pub fn generate_terrain_mesh(
    center: &GpsPos,
    radius_m: f64,
    grid_spacing_m: f64,
    srtm: &mut SrtmManager,
) -> (Vec<ColoredVertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    let lat_deg_per_m = 1.0 / 111_000.0;
    let lon_deg_per_m = 1.0 / (111_000.0 * center.lat_deg.to_radians().cos());
    
    let lat_range = radius_m * lat_deg_per_m;
    let lon_range = radius_m * lon_deg_per_m;
    
    let num_samples = ((radius_m * 2.0 / grid_spacing_m) as usize).min(200); // Cap at 200x200
    
    eprintln!("[Terrain] Grid: {}x{} samples, spacing {:.0}m", num_samples, num_samples, grid_spacing_m);
    
    let mut vertex_grid = Vec::new();
    for i in 0..=num_samples {
        let mut row = Vec::new();
        let lat_frac = i as f64 / num_samples as f64;
        let lat_deg = center.lat_deg - lat_range + (lat_frac * lat_range * 2.0);
        
        for j in 0..=num_samples {
            let lon_frac = j as f64 / num_samples as f64;
            let lon_deg = center.lon_deg - lon_range + (lon_frac * lon_range * 2.0);
            
            let elevation = srtm.get_elevation(lat_deg, lon_deg).unwrap_or(0.0);
            
            let pos = GpsPos { lat_deg, lon_deg, elevation_m: elevation };
            let ecef = gps_to_ecef(&pos);
            
            let normal_len = (ecef.x * ecef.x + ecef.y * ecef.y + ecef.z * ecef.z).sqrt();
            let normal = [
                (ecef.x / normal_len) as f32,
                (ecef.y / normal_len) as f32,
                (ecef.z / normal_len) as f32,
            ];
            
            let color = if elevation < 5.0 {
                [0.76, 0.70, 0.50, 1.0] // Sandy
            } else if elevation < 20.0 {
                [0.34, 0.55, 0.34, 1.0] // Grass
            } else {
                [0.25, 0.42, 0.25, 1.0] // Dark green
            };
            
            row.push(vertices.len() as u32);
            vertices.push(ColoredVertex {
                position: [ecef.x as f32, ecef.y as f32, ecef.z as f32],
                normal,
                color,
            });
        }
        vertex_grid.push(row);
    }
    
    for i in 0..num_samples {
        for j in 0..num_samples {
            let v0 = vertex_grid[i][j];
            let v1 = vertex_grid[i][j + 1];
            let v2 = vertex_grid[i + 1][j];
            let v3 = vertex_grid[i + 1][j + 1];
            
            indices.extend_from_slice(&[v0, v1, v2, v1, v3, v2]);
        }
    }
    
    eprintln!("[Terrain] {} vertices, {} triangles", vertices.len(), indices.len() / 3);
    (vertices, indices)
}
