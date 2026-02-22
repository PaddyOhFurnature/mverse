//! Material type system for voxels
//! 
//! Each voxel stores a single u8 MaterialId.
//! Material properties (color, density, etc.) are looked up from a global table.

/// Material identifier (256 possible materials)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MaterialId {
    /// Empty space (most common - optimize for this)
    AIR = 0,
    
    /// Natural terrain
    STONE = 1,
    DIRT = 2,
    GRASS = 3,
    SAND = 4,
    GRAVEL = 5,
    SNOW = 6,
    ICE = 7,
    
    /// Liquids
    WATER = 10,
    LAVA = 11,
    
    /// Vegetation
    WOOD = 20,
    LEAVES = 21,
    
    /// Manufactured
    CONCRETE = 30,
    BRICK = 31,
    ASPHALT = 32,
    GLASS = 33,
    STEEL = 34,
    
    /// Deep underground
    BEDROCK = 100,
}

/// Physical properties of a material
#[derive(Debug, Clone, Copy)]
pub struct MaterialProperties {
    /// Does this material block movement?
    pub solid: bool,
    
    /// Can you see through this material?
    pub transparent: bool,
    
    /// Mass per cubic meter (kg/m³)
    pub density: f32,
    
    /// Base color (RGB)
    pub color: [u8; 3],
}

impl MaterialId {
    /// Get the properties for this material
    pub fn properties(self) -> MaterialProperties {
        MATERIAL_TABLE[self as usize]
    }
}

/// Global material properties lookup table
const MATERIAL_TABLE: [MaterialProperties; 256] = {
    let mut table = [MaterialProperties {
        solid: false,
        transparent: true,
        density: 0.0,
        color: [0, 0, 0],
    }; 256];
    
    // AIR (0)
    table[0] = MaterialProperties {
        solid: false,
        transparent: true,
        density: 1.2,  // Air at sea level
        color: [135, 206, 235],  // Sky blue
    };
    
    // STONE (1)
    table[1] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 2500.0,
        color: [128, 128, 128],  // Gray
    };
    
    // DIRT (2)
    table[2] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 1600.0,
        color: [139, 69, 19],  // Brown
    };
    
    // GRASS (3)
    table[3] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 1200.0,
        color: [34, 139, 34],  // Green
    };
    
    // SAND (4)
    table[4] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 1600.0,
        color: [238, 214, 175],  // Tan
    };
    
    // GRAVEL (5)
    table[5] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 1800.0,
        color: [169, 169, 169],  // Dark gray
    };
    
    // SNOW (6)
    table[6] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 400.0,
        color: [255, 250, 250],  // White
    };
    
    // ICE (7)
    table[7] = MaterialProperties {
        solid: true,
        transparent: true,
        density: 917.0,
        color: [175, 238, 238],  // Pale cyan
    };
    
    // WATER (10)
    table[10] = MaterialProperties {
        solid: false,
        transparent: true,
        density: 1000.0,
        color: [65, 105, 225],  // Blue
    };
    
    // LAVA (11)
    table[11] = MaterialProperties {
        solid: false,
        transparent: false,
        density: 3100.0,
        color: [255, 69, 0],  // Red-orange
    };
    
    // WOOD (20)
    table[20] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 700.0,
        color: [139, 90, 43],  // Brown
    };
    
    // LEAVES (21)
    table[21] = MaterialProperties {
        solid: true,
        transparent: true,
        density: 400.0,
        color: [46, 125, 50],  // Dark green
    };
    
    // CONCRETE (30)
    table[30] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 2400.0,
        color: [192, 192, 192],  // Light gray
    };
    
    // BRICK (31)
    table[31] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 2000.0,
        color: [178, 34, 34],  // Brick red
    };
    
    // ASPHALT (32)
    table[32] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 2400.0,
        color: [64, 64, 64],  // Dark gray
    };
    
    // GLASS (33)
    table[33] = MaterialProperties {
        solid: true,
        transparent: true,
        density: 2500.0,
        color: [230, 255, 255],  // Very pale cyan
    };
    
    // STEEL (34)
    table[34] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 7850.0,
        color: [176, 196, 222],  // Light steel blue
    };
    
    // BEDROCK (100)
    table[100] = MaterialProperties {
        solid: true,
        transparent: false,
        density: 3000.0,
        color: [32, 32, 32],  // Very dark gray
    };
    
    table
};

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_material_air() {
        let props = MaterialId::AIR.properties();
        assert_eq!(props.solid, false);
        assert_eq!(props.transparent, true);
        assert_eq!(props.density, 1.2);
    }
    
    #[test]
    fn test_material_stone() {
        let props = MaterialId::STONE.properties();
        assert_eq!(props.solid, true);
        assert_eq!(props.transparent, false);
        assert_eq!(props.color, [128, 128, 128]);
    }
    
    #[test]
    fn test_material_water() {
        let props = MaterialId::WATER.properties();
        assert_eq!(props.solid, false);
        assert_eq!(props.transparent, true);
        assert_eq!(props.density, 1000.0);
    }
    
    #[test]
    fn test_material_glass() {
        let props = MaterialId::GLASS.properties();
        assert_eq!(props.solid, true);
        assert_eq!(props.transparent, true);
    }
}
