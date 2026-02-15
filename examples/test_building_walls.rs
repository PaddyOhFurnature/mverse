// Test building wall generation
use metaverse_core::renderer::mesh::generate_building;
use glam::Vec3;

fn main() {
    // Simple square building (4 walls)
    let footprint = vec![
        (-27.470, 153.025),  // Corner 1
        (-27.470, 153.026),  // Corner 2  
        (-27.469, 153.026),  // Corner 3
        (-27.469, 153.025),  // Corner 4
    ];
    
    let (vertices, indices) = generate_building(
        &footprint,
        0.0,    // ground level
        20.0,   // 20m tall
        Vec3::new(0.7, 0.7, 0.8),
    );
    
    println!("\n=== Building Geometry Test ===");
    println!("Square building: 4 corners, 20m tall\n");
    println!("Generated:");
    println!("  {} vertices", vertices.len());
    println!("  {} indices", indices.len());
    println!("  {} triangles\n", indices.len() / 3);
    
    // Expected for a complete box with 4 walls:
    // - Base: 4 vertices
    // - Top: 4 vertices  
    // - Walls: 4 walls × 2 triangles = 8 triangles = 24 indices
    // - Base cap: 2 triangles = 6 indices
    // - Top cap: 2 triangles = 6 indices
    // Total: 36 indices minimum for complete closed box
    
    println!("Expected for COMPLETE box:");
    println!("  8 vertices (4 base + 4 top)");
    println!("  36+ indices (4 walls + base + top)");
    println!("    - 4 walls: 24 indices");
    println!("    - Base cap: 6 indices");
    println!("    - Top cap: 6 indices\n");
    
    if vertices.len() < 8 {
        println!("❌ PROBLEM: Not enough vertices! Missing {} vertices", 8 - vertices.len());
    } else {
        println!("✓ Vertices OK");
    }
    
    if indices.len() < 36 {
        println!("❌ PROBLEM: Not enough indices for complete box! Missing {} indices", 36 - indices.len());
        println!("   This means walls or caps are missing!");
    } else {
        println!("✓ Indices OK - all walls and caps present");
    }
    
    // Test a more complex building (Pentagon - 5 walls)
    let pentagon = vec![
        (-27.470, 153.025),
        (-27.470, 153.026),
        (-27.4695, 153.0265),
        (-27.469, 153.026),
        (-27.469, 153.025),
    ];
    
    let (verts2, indices2) = generate_building(
        &pentagon,
        0.0,
        15.0,
        Vec3::new(0.7, 0.7, 0.8),
    );
    
    println!("\n=== Pentagon Building (5 walls) ===");
    println!("Generated: {} vertices, {} indices ({} triangles)",
        verts2.len(), indices2.len(), indices2.len() / 3);
    println!("Expected: 10 vertices (5 base + 5 top), 45+ indices (5 walls + caps)");
}
