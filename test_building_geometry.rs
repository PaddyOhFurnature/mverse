// Test: Does building generation actually create all 4 walls?
use metaverse_core::renderer::mesh::generate_building;
use glam::Vec3;

fn main() {
    // Simple square building
    let footprint = vec![
        (-27.470, 153.025),  // SW corner
        (-27.470, 153.026),  // SE corner  
        (-27.469, 153.026),  // NE corner
        (-27.469, 153.025),  // NW corner
    ];
    
    let (vertices, indices) = generate_building(
        &footprint,
        0.0,    // ground level
        20.0,   // 20m tall
        Vec3::new(0.7, 0.7, 0.8),
    );
    
    println!("Square building (4 corners, 20m tall):");
    println!("  Vertices: {}", vertices.len());
    println!("  Indices: {}", indices.len());
    println!("  Triangles: {}", indices.len() / 3);
    
    // Expected for a box:
    // - 8 vertices (4 base + 4 top)
    // - 4 walls × 2 triangles = 8 triangles = 24 indices
    // - 2 caps (base + top) × N triangles
    
    println!("\nExpected for complete box:");
    println!("  4 walls = 8 triangles = 24 indices minimum");
    println!("  + base/top caps");
    
    if indices.len() < 24 {
        println!("\n⚠️  WARNING: Not enough indices for 4 walls!");
    }
}
