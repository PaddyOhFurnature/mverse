/// Material Rendering
///
/// Defines visual properties (colors, textures) for each MaterialId.
/// Used by renderer to assign colors to mesh vertices.

use crate::svo::MaterialId;

/// RGB color (0.0 - 1.0)
#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Color {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }
    
    pub fn to_array(&self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

/// Material color palette
pub struct MaterialColors {
    colors: Vec<Color>,
}

impl MaterialColors {
    /// Create default material color palette
    pub fn default_palette() -> Self {
        let mut colors = vec![Color::new(0.0, 0.0, 0.0); 256];
        
        // AIR (0) - transparent/invisible (handled separately)
        colors[0] = Color::new(0.0, 0.0, 0.0);
        
        // STONE (1) - gray
        colors[1] = Color::new(0.5, 0.5, 0.5);
        
        // DIRT (2) - brown
        colors[2] = Color::new(0.55, 0.4, 0.25);
        
        // CONCRETE (3) - light gray
        colors[3] = Color::new(0.7, 0.7, 0.7);
        
        // WOOD (4) - brown wood
        colors[4] = Color::new(0.6, 0.4, 0.2);
        
        // METAL (5) - metallic gray
        colors[5] = Color::new(0.6, 0.6, 0.65);
        
        // GLASS (6) - light blue transparent
        colors[6] = Color::new(0.8, 0.9, 1.0);
        
        // WATER (7) - blue
        colors[7] = Color::new(0.2, 0.5, 0.8);
        
        // ASPHALT (8) - dark gray
        colors[8] = Color::new(0.3, 0.3, 0.3);
        
        // BRICK (9) - red-brown
        colors[9] = Color::new(0.7, 0.4, 0.3);
        
        // SAND (10) - sandy yellow
        colors[10] = Color::new(0.9, 0.85, 0.6);
        
        // GRASS (11) - green
        colors[11] = Color::new(0.3, 0.6, 0.3);
        
        // ICE (12) - light blue
        colors[12] = Color::new(0.8, 0.9, 1.0);
        
        // SNOW (13) - white
        colors[13] = Color::new(0.95, 0.95, 0.95);
        
        // MUD (14) - dark brown
        colors[14] = Color::new(0.4, 0.3, 0.2);
        
        // CLAY (15) - orange-brown
        colors[15] = Color::new(0.75, 0.5, 0.35);
        
        Self { colors }
    }
    
    /// Get color for material
    pub fn get_color(&self, material: MaterialId) -> Color {
        let idx = material.0 as usize;
        if idx < self.colors.len() {
            self.colors[idx]
        } else {
            Color::new(1.0, 0.0, 1.0) // Magenta for unknown materials
        }
    }
    
    /// Set color for material
    pub fn set_color(&mut self, material: MaterialId, color: Color) {
        let idx = material.0 as usize;
        if idx < self.colors.len() {
            self.colors[idx] = color;
        }
    }
}

/// Convert mesh with normals to colored mesh with vertex colors
///
/// # Arguments
/// * `vertices` - Packed vertices [x,y,z,nx,ny,nz,...]
/// * `indices` - Triangle indices
/// * `material` - Material ID
/// * `palette` - Material color palette
///
/// # Returns
/// Colored vertices [x,y,z,r,g,b,...] and indices
pub fn apply_material_colors(
    vertices: &[f32],
    indices: &[u32],
    material: MaterialId,
    palette: &MaterialColors,
) -> (Vec<f32>, Vec<u32>) {
    let color = palette.get_color(material);
    let mut colored_vertices = Vec::with_capacity((vertices.len() / 6) * 6);
    
    // Convert [x,y,z,nx,ny,nz] to [x,y,z,r,g,b]
    for chunk in vertices.chunks(6) {
        if chunk.len() == 6 {
            // Position
            colored_vertices.push(chunk[0]);
            colored_vertices.push(chunk[1]);
            colored_vertices.push(chunk[2]);
            
            // Color (replace normal with color for now)
            // TODO: Keep normal for lighting, add color separately
            colored_vertices.push(color.r);
            colored_vertices.push(color.g);
            colored_vertices.push(color.b);
        }
    }
    
    (colored_vertices, indices.to_vec())
}

/// Apply simple directional lighting to colors based on normals
///
/// # Arguments
/// * `vertices` - Packed vertices [x,y,z,nx,ny,nz,...]
/// * `base_color` - Base material color
/// * `light_dir` - Light direction (normalized)
///
/// # Returns
/// Lit vertices [x,y,z,r,g,b,...]
pub fn apply_lighting(
    vertices: &[f32],
    base_color: Color,
    light_dir: [f32; 3],
) -> Vec<f32> {
    let mut lit_vertices = Vec::with_capacity((vertices.len() / 6) * 6);
    
    for chunk in vertices.chunks(6) {
        if chunk.len() == 6 {
            let normal = [chunk[3], chunk[4], chunk[5]];
            
            // Lambertian diffuse lighting
            let dot = (normal[0] * light_dir[0] 
                     + normal[1] * light_dir[1] 
                     + normal[2] * light_dir[2])
                .max(0.0);
            
            // Ambient + diffuse
            let ambient = 0.3;
            let intensity = ambient + (1.0 - ambient) * dot;
            
            // Position
            lit_vertices.push(chunk[0]);
            lit_vertices.push(chunk[1]);
            lit_vertices.push(chunk[2]);
            
            // Lit color
            lit_vertices.push(base_color.r * intensity);
            lit_vertices.push(base_color.g * intensity);
            lit_vertices.push(base_color.b * intensity);
        }
    }
    
    lit_vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svo::{STONE, DIRT, WATER, ASPHALT, CONCRETE};
    
    #[test]
    fn test_material_colors() {
        let palette = MaterialColors::default_palette();
        
        // STONE should be gray
        let stone_color = palette.get_color(STONE);
        assert_eq!(stone_color.r, 0.5);
        assert_eq!(stone_color.g, 0.5);
        assert_eq!(stone_color.b, 0.5);
        
        // DIRT should be brown
        let dirt_color = palette.get_color(DIRT);
        assert!(dirt_color.r > dirt_color.g);
        assert!(dirt_color.g > dirt_color.b);
        
        // WATER should be blue
        let water_color = palette.get_color(WATER);
        assert!(water_color.b > water_color.g);
        assert!(water_color.g > water_color.r);
        
        // ASPHALT should be dark
        let asphalt_color = palette.get_color(ASPHALT);
        assert!(asphalt_color.r < 0.5);
        
        // CONCRETE should be light gray
        let concrete_color = palette.get_color(CONCRETE);
        assert!(concrete_color.r > 0.6);
        assert_eq!(concrete_color.r, concrete_color.g);
        assert_eq!(concrete_color.g, concrete_color.b);
    }
    
    #[test]
    fn test_apply_material_colors() {
        let palette = MaterialColors::default_palette();
        
        // Create test vertices [x,y,z,nx,ny,nz]
        let vertices = vec![
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,  // Vertex 1
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0,  // Vertex 2
            0.0, 1.0, 0.0, 0.0, 1.0, 0.0,  // Vertex 3
        ];
        let indices = vec![0, 1, 2];
        
        let (colored, idx) = apply_material_colors(&vertices, &indices, STONE, &palette);
        
        // Should have same number of floats
        assert_eq!(colored.len(), vertices.len());
        
        // Should have same indices
        assert_eq!(idx, indices);
        
        // Check first vertex has position and color
        assert_eq!(colored[0], 0.0); // x
        assert_eq!(colored[1], 0.0); // y
        assert_eq!(colored[2], 0.0); // z
        assert_eq!(colored[3], 0.5); // r (stone gray)
        assert_eq!(colored[4], 0.5); // g
        assert_eq!(colored[5], 0.5); // b
    }
    
    #[test]
    fn test_lighting() {
        let base_color = Color::new(0.5, 0.5, 0.5);
        let light_dir = [0.0, 1.0, 0.0]; // Light from above
        
        // Vertices with normal pointing up
        let vertices = vec![
            0.0, 0.0, 0.0, 0.0, 1.0, 0.0,  // Normal points toward light
            1.0, 0.0, 0.0, 0.0, -1.0, 0.0, // Normal points away from light
        ];
        
        let lit = apply_lighting(&vertices, base_color, light_dir);
        
        // First vertex should be bright (dot product = 1.0)
        let brightness1 = lit[3]; // r component
        assert!(brightness1 > 0.4);
        
        // Second vertex should be darker (dot product = 0.0)
        let brightness2 = lit[9]; // r component of second vertex
        assert!(brightness2 < brightness1);
    }
}
