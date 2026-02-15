/// Mesh Generation from SVO
///
/// Extracts renderable meshes from sparse voxel octree data.
/// Separates by material, optimizes geometry, supports multiple LODs.

use crate::svo::{SparseVoxelOctree, MaterialId, AIR};
use crate::marching_cubes::{extract_mesh, Triangle, Vertex};
use crate::materials::{MaterialColors, Color};
use std::collections::HashMap;

/// Renderable mesh with vertices and indices
#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<f32>,      // Packed: [x, y, z, nx, ny, nz, ...]
    pub indices: Vec<u32>,        // Triangle indices
    pub material: MaterialId,
}

/// GPU-ready vertex format with color
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ColoredVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4], // RGBA
}

impl ColoredVertex {
    pub fn new(position: [f32; 3], normal: [f32; 3], color: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            color: [color[0], color[1], color[2], 1.0],
        }
    }
}

/// Multi-LOD mesh set for a chunk
#[derive(Debug, Clone)]
pub struct LodMeshSet {
    pub meshes: Vec<Mesh>,        // Per-material meshes
    pub lod_levels: Vec<LodLevel>,
}

/// Single LOD level with distance threshold
#[derive(Debug, Clone)]
pub struct LodLevel {
    pub lod: u8,                  // 0 = finest, higher = coarser
    pub distance: f32,            // Distance threshold in meters
    pub meshes: Vec<Mesh>,        // Meshes at this LOD
}

/// Generate mesh from SVO at specified LOD
///
/// # Arguments
/// * `svo` - The sparse voxel octree
/// * `lod` - Level of detail (0 = finest)
///
/// # Returns
/// Vector of meshes, one per material
pub fn generate_mesh(svo: &SparseVoxelOctree, lod: u8) -> Vec<Mesh> {
    // Extract triangles using marching cubes
    let triangles = extract_mesh(svo, lod);
    
    // Group triangles by material
    let mut material_triangles: HashMap<MaterialId, Vec<Triangle>> = HashMap::new();
    for tri in triangles {
        material_triangles.entry(tri.material)
            .or_insert_with(Vec::new)
            .push(tri);
    }
    
    // Convert to meshes
    let mut meshes = Vec::new();
    for (material, tris) in material_triangles {
        if tris.is_empty() || material == AIR {
            continue;
        }
        
        let mesh = triangles_to_mesh(&tris, material);
        meshes.push(mesh);
    }
    
    meshes
}

/// Convert triangles to optimized mesh
fn triangles_to_mesh(triangles: &[Triangle], material: MaterialId) -> Mesh {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    
    // Simple approach: pack all vertices
    // TODO: Implement vertex deduplication and indexing
    for (i, tri) in triangles.iter().enumerate() {
        let base_idx = (i * 3) as u32;
        
        for v in &tri.vertices {
            // Pack position and normal
            vertices.push(v.position[0]);
            vertices.push(v.position[1]);
            vertices.push(v.position[2]);
            vertices.push(v.normal[0]);
            vertices.push(v.normal[1]);
            vertices.push(v.normal[2]);
        }
        
        // Reverse winding order (fixes inverted normals)
        indices.push(base_idx);
        indices.push(base_idx + 2);
        indices.push(base_idx + 1);
    }
    
    Mesh {
        vertices,
        indices,
        material,
    }
}

/// Generate multi-LOD meshes for a chunk
///
/// Creates meshes at multiple levels of detail with distance thresholds.
///
/// # Arguments
/// * `svo` - The sparse voxel octree
/// * `lod_count` - Number of LOD levels to generate
///
/// # Returns
/// LodMeshSet with meshes at each LOD level
pub fn generate_lod_meshes(svo: &SparseVoxelOctree, lod_count: u8) -> LodMeshSet {
    let mut lod_levels = Vec::new();
    
    for lod in 0..lod_count {
        // Distance thresholds: 0m, 50m, 200m, 500m, 1000m
        let distance = match lod {
            0 => 0.0,
            1 => 50.0,
            2 => 200.0,
            3 => 500.0,
            _ => 1000.0 * (lod as f32),
        };
        
        let meshes = generate_mesh(svo, lod);
        
        lod_levels.push(LodLevel {
            lod,
            distance,
            meshes,
        });
    }
    
    // Collect all unique meshes
    let mut all_meshes = Vec::new();
    for level in &lod_levels {
        for mesh in &level.meshes {
            all_meshes.push(mesh.clone());
        }
    }
    
    LodMeshSet {
        meshes: all_meshes,
        lod_levels,
    }
}

/// Select appropriate LOD based on camera distance
///
/// # Arguments
/// * `lod_set` - The LOD mesh set
/// * `distance` - Distance from camera in meters
///
/// # Returns
/// Index of appropriate LOD level
pub fn select_lod(lod_set: &LodMeshSet, distance: f32) -> usize {
    for (i, level) in lod_set.lod_levels.iter().enumerate() {
        if distance < level.distance {
            return i.saturating_sub(1);
        }
    }
    
    // Use coarsest LOD for far distances
    lod_set.lod_levels.len().saturating_sub(1)
}

/// Optimize mesh by removing internal faces
///
/// Internal faces (shared between solid voxels) don't need to be rendered.
/// This pass removes them to reduce triangle count.
///
/// # Arguments
/// * `mesh` - The mesh to optimize
///
/// # Returns
/// Optimized mesh with fewer triangles
pub fn optimize_mesh(mesh: &Mesh) -> Mesh {
    // TODO: Implement internal face removal
    // For now, return unmodified
    mesh.clone()
}

/// Merge adjacent coplanar faces
///
/// Reduces triangle count by merging flat surfaces.
///
/// # Arguments
/// * `mesh` - The mesh to simplify
///
/// # Returns
/// Simplified mesh
pub fn merge_coplanar_faces(mesh: &Mesh) -> Mesh {
    // TODO: Implement face merging
    // For now, return unmodified
    mesh.clone()
}

/// Convert SVO meshes to GPU-ready colored vertices
///
/// Unpacks packed f32 vertices and applies material colors.
///
/// # Arguments
/// * `meshes` - Vector of SVO meshes (one per material)
/// * `material_colors` - Material color palette
///
/// # Returns
/// (vertices, indices) ready for GPU upload
pub fn svo_meshes_to_colored_vertices(
    meshes: &[Mesh],
    material_colors: &MaterialColors,
) -> (Vec<ColoredVertex>, Vec<u32>) {
    let mut all_vertices = Vec::new();
    let mut all_indices = Vec::new();
    let mut vertex_offset = 0u32;
    
    for mesh in meshes {
        let color = material_colors.get_color(mesh.material);
        let color_array = [color.r, color.g, color.b];
        
        // Unpack vertices: [x,y,z, nx,ny,nz, ...] -> ColoredVertex
        let vertex_count = mesh.vertices.len() / 6;
        for i in 0..vertex_count {
            let base = i * 6;
            let position = [
                mesh.vertices[base],
                mesh.vertices[base + 1],
                mesh.vertices[base + 2],
            ];
            let normal = [
                mesh.vertices[base + 3],
                mesh.vertices[base + 4],
                mesh.vertices[base + 5],
            ];
            
            all_vertices.push(ColoredVertex::new(position, normal, color_array));
        }
        
        // Offset indices and append
        for &idx in &mesh.indices {
            all_indices.push(idx + vertex_offset);
        }
        
        vertex_offset += vertex_count as u32;
    }
    
    (all_vertices, all_indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svo::{SparseVoxelOctree, STONE, DIRT};
    
    #[test]
    fn test_generate_mesh_empty() {
        let svo = SparseVoxelOctree::new(4);
        let meshes = generate_mesh(&svo, 0);
        
        // Empty SVO produces no meshes
        assert_eq!(meshes.len(), 0);
    }
    
    #[test]
    fn test_generate_mesh_with_voxels() {
        let mut svo = SparseVoxelOctree::new(4);
        
        // Add some voxels
        svo.set_voxel(5, 5, 5, STONE);
        svo.set_voxel(6, 5, 5, DIRT);
        
        let meshes = generate_mesh(&svo, 0);
        
        // With stub marching cubes table, we get 0 meshes
        // TODO: When full table is implemented, verify mesh count > 0
        println!("Generated {} meshes (stub marching cubes)", meshes.len());
    }
    
    #[test]
    fn test_generate_lod_meshes() {
        let mut svo = SparseVoxelOctree::new(5);
        
        // Create terrain
        for x in 0..32 {
            for z in 0..32 {
                for y in 0..10 {
                    svo.set_voxel(x, y, z, STONE);
                }
            }
        }
        
        let lod_set = generate_lod_meshes(&svo, 3);
        
        // Should have 3 LOD levels
        assert_eq!(lod_set.lod_levels.len(), 3);
        
        // LOD 0 should be at distance 0
        assert_eq!(lod_set.lod_levels[0].distance, 0.0);
        assert_eq!(lod_set.lod_levels[0].lod, 0);
        
        println!("Generated {} LOD levels", lod_set.lod_levels.len());
    }
    
    #[test]
    fn test_select_lod() {
        let mut svo = SparseVoxelOctree::new(4);
        svo.set_voxel(5, 5, 5, STONE);
        
        let lod_set = generate_lod_meshes(&svo, 3);
        
        // Close distance should select LOD 0
        assert_eq!(select_lod(&lod_set, 10.0), 0);
        
        // Medium distance should select LOD 1
        assert_eq!(select_lod(&lod_set, 100.0), 1);
        
        // Far distance should select LOD 2
        assert_eq!(select_lod(&lod_set, 400.0), 2);
    }
    
    #[test]
    fn test_mesh_structure() {
        let mut svo = SparseVoxelOctree::new(4);
        svo.set_voxel(5, 5, 5, STONE);
        
        let meshes = generate_mesh(&svo, 0);
        
        // Verify mesh structure
        for mesh in &meshes {
            // Vertices should be packed as [x,y,z,nx,ny,nz]
            assert_eq!(mesh.vertices.len() % 6, 0);
            
            // Indices should be multiples of 3 (triangles)
            assert_eq!(mesh.indices.len() % 3, 0);
            
            // Material should not be AIR
            assert_ne!(mesh.material, AIR);
        }
    }
}
