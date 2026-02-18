//! Mesh data structures for rendering
//!
//! Simple triangle mesh representation extracted from voxels

use glam::Vec3;

/// 3D vertex with position and normal
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
}

impl Vertex {
    pub fn new(position: Vec3, normal: Vec3) -> Self {
        Self { position, normal }
    }
}

/// Triangle defined by 3 vertex indices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    pub indices: [usize; 3],
}

impl Triangle {
    pub fn new(i0: usize, i1: usize, i2: usize) -> Self {
        Self {
            indices: [i0, i1, i2],
        }
    }
}

/// Triangle mesh
#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<Triangle>,
}

impl Mesh {
    /// Create empty mesh
    pub fn new() -> Self {
        Self {
            vertices: Vec::new(),
            triangles: Vec::new(),
        }
    }
    
    /// Create mesh with capacity
    pub fn with_capacity(vertex_count: usize, triangle_count: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertex_count),
            triangles: Vec::with_capacity(triangle_count),
        }
    }
    
    /// Add vertex and return its index
    pub fn add_vertex(&mut self, vertex: Vertex) -> usize {
        let index = self.vertices.len();
        self.vertices.push(vertex);
        index
    }
    
    /// Add triangle
    pub fn add_triangle(&mut self, triangle: Triangle) {
        self.triangles.push(triangle);
    }
    
    /// Add a line segment rendered as a thick quad (billboard)
    /// 
    /// Creates a quad perpendicular to the camera to make lines visible.
    /// For proper wireframes, creates 6 vertices (2 triangles) to form a thick line.
    /// DOUBLE-SIDED: Creates triangles on both sides so visible from any angle.
    pub fn add_line(&mut self, v0: usize, v1: usize) {
        // Get the two endpoint vertices
        let p0 = self.vertices[v0].position;
        let p1 = self.vertices[v1].position;
        let color = self.vertices[v0].normal; // Using normal for color
        
        // Calculate line direction and perpendicular offset
        let dir = (p1 - p0).normalize();
        let thickness = 0.02; // 2cm thick lines
        
        // Create perpendicular vector (try Y-axis first, then X if parallel to Y)
        let up = if dir.y.abs() < 0.9 {
            Vec3::Y
        } else {
            Vec3::X
        };
        let perp = dir.cross(up).normalize() * thickness;
        
        // Create quad vertices around the line
        let v0a = self.add_vertex(Vertex::new(p0 - perp, color));
        let v0b = self.add_vertex(Vertex::new(p0 + perp, color));
        let v1a = self.add_vertex(Vertex::new(p1 - perp, color));
        let v1b = self.add_vertex(Vertex::new(p1 + perp, color));
        
        // Create two triangles to form the quad (front-facing)
        self.triangles.push(Triangle::new(v0a, v1a, v0b));
        self.triangles.push(Triangle::new(v0b, v1a, v1b));
        
        // Create two more triangles for the back (reverse winding)
        self.triangles.push(Triangle::new(v0b, v1a, v0a));
        self.triangles.push(Triangle::new(v1b, v1a, v0b));
    }
    
    /// Get vertex count
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    
    /// Get triangle count
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
    
    /// Check if mesh is empty
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }
    
    /// Clear mesh data
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.triangles.clear();
    }
    
    /// Merge another mesh into this one
    pub fn merge(&mut self, other: &Mesh) {
        let vertex_offset = self.vertices.len();
        
        // Add vertices
        self.vertices.extend_from_slice(&other.vertices);
        
        // Add triangles with adjusted indices
        for tri in &other.triangles {
            self.triangles.push(Triangle::new(
                tri.indices[0] + vertex_offset,
                tri.indices[1] + vertex_offset,
                tri.indices[2] + vertex_offset,
            ));
        }
    }
}

impl Default for Mesh {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_empty_mesh() {
        let mesh = Mesh::new();
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);
        assert!(mesh.is_empty());
    }
    
    #[test]
    fn test_add_vertex() {
        let mut mesh = Mesh::new();
        let v = Vertex::new(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        
        let idx = mesh.add_vertex(v);
        assert_eq!(idx, 0);
        assert_eq!(mesh.vertex_count(), 1);
        assert_eq!(mesh.vertices[0].position, Vec3::new(1.0, 2.0, 3.0));
    }
    
    #[test]
    fn test_add_triangle() {
        let mut mesh = Mesh::new();
        
        // Add 3 vertices
        mesh.add_vertex(Vertex::new(Vec3::ZERO, Vec3::Y));
        mesh.add_vertex(Vertex::new(Vec3::X, Vec3::Y));
        mesh.add_vertex(Vertex::new(Vec3::Z, Vec3::Y));
        
        // Add triangle
        mesh.add_triangle(Triangle::new(0, 1, 2));
        
        assert_eq!(mesh.triangle_count(), 1);
        assert_eq!(mesh.triangles[0].indices, [0, 1, 2]);
        assert!(!mesh.is_empty());
    }
    
    #[test]
    fn test_merge_meshes() {
        let mut mesh1 = Mesh::new();
        mesh1.add_vertex(Vertex::new(Vec3::new(1.0, 0.0, 0.0), Vec3::Y));
        mesh1.add_vertex(Vertex::new(Vec3::new(2.0, 0.0, 0.0), Vec3::Y));
        mesh1.add_triangle(Triangle::new(0, 1, 0));
        
        let mut mesh2 = Mesh::new();
        mesh2.add_vertex(Vertex::new(Vec3::new(3.0, 0.0, 0.0), Vec3::Y));
        mesh2.add_vertex(Vertex::new(Vec3::new(4.0, 0.0, 0.0), Vec3::Y));
        mesh2.add_triangle(Triangle::new(0, 1, 0));
        
        mesh1.merge(&mesh2);
        
        assert_eq!(mesh1.vertex_count(), 4);
        assert_eq!(mesh1.triangle_count(), 2);
        
        // Second triangle indices should be offset by 2
        assert_eq!(mesh1.triangles[1].indices, [2, 3, 2]);
    }
    
    #[test]
    fn test_clear() {
        let mut mesh = Mesh::new();
        mesh.add_vertex(Vertex::new(Vec3::ZERO, Vec3::Y));
        mesh.add_triangle(Triangle::new(0, 0, 0));
        
        mesh.clear();
        
        assert_eq!(mesh.vertex_count(), 0);
        assert_eq!(mesh.triangle_count(), 0);
        assert!(mesh.is_empty());
    }
}
