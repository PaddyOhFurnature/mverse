# Material Properties System - Transparency and Physics

**Last Updated:** 2026-02-17  
**Question:** How to handle glass (solid but transparent) and water (fluid but visible)?

---

## THE PROBLEM

**Different properties don't align:**
- **Glass:** Solid (blocks movement) + Transparent (can see through)
- **Water:** Fluid (can move through) + Visible (renders surface)
- **Air:** Non-solid (no collision) + Transparent (invisible)
- **Stone:** Solid (blocks movement) + Opaque (can't see through)

**Can't just use "solid vs air" for rendering or physics**

---

## THE SOLUTION: Property-Based System

**Voxel stores ONLY material ID** (u8)

**Material properties define behavior:**

```rust
struct MaterialProperties {
    // Physics
    solid: bool,              // Blocks player movement?
    density: f32,             // kg/m³ (for water physics, buoyancy)
    friction: f32,            // 0.0-1.0 (ice = low, rubber = high)
    
    // Rendering
    transparent: bool,        // Can see through?
    opacity: f32,             // 0.0-1.0 (glass = 0.2, water = 0.8, stone = 1.0)
    refractive_index: f32,    // For refraction (glass = 1.5, water = 1.33)
    emissive: bool,           // Glows? (lava, lights)
    
    // Visual
    color: [u8; 3],           // Base color (if no texture)
    texture_id: u16,          // Index into texture atlas
    
    // Gameplay
    destructible: bool,       // Can player mine/dig?
    hardness: f32,            // How long to break (stone > dirt > air)
}
```

---

## EXAMPLES

### Air
```rust
MaterialProperties {
    // Physics
    solid: false,             // Walk through
    density: 1.2,             // kg/m³ (atmosphere)
    friction: 0.0,
    
    // Rendering
    transparent: true,        // See through
    opacity: 0.0,             // Completely transparent
    refractive_index: 1.0,    // No refraction
    emissive: false,
    
    // Visual
    color: [0, 0, 0],         // No color
    texture_id: 0,            // No texture
    
    // Gameplay
    destructible: false,      // Can't "break" air
    hardness: 0.0,
}
```

### Glass
```rust
MaterialProperties {
    // Physics
    solid: true,              // BLOCKS MOVEMENT ✓
    density: 2500.0,
    friction: 0.1,            // Smooth
    
    // Rendering
    transparent: true,        // CAN SEE THROUGH ✓
    opacity: 0.1,             // Mostly transparent
    refractive_index: 1.5,    // Glass refraction
    emissive: false,
    
    // Visual
    color: [220, 230, 255],   // Slight blue tint
    texture_id: 5,
    
    // Gameplay
    destructible: true,       // Can break glass
    hardness: 3.0,            // Medium hardness
}
```

### Water
```rust
MaterialProperties {
    // Physics
    solid: false,             // CAN MOVE THROUGH (slowly) ✓
    density: 1000.0,          // kg/m³ (buoyancy force)
    friction: 0.8,            // Drag in water
    
    // Rendering
    transparent: true,        // CAN SEE THROUGH ✓
    opacity: 0.6,             // Semi-transparent
    refractive_index: 1.33,   // Water refraction
    emissive: false,
    
    // Visual
    color: [30, 60, 180],     // Blue
    texture_id: 50,           // Animated water texture
    
    // Gameplay
    destructible: false,      // Can't "break" water
    hardness: 0.0,
}
```

### Stone
```rust
MaterialProperties {
    // Physics
    solid: true,              // BLOCKS MOVEMENT ✓
    density: 2700.0,
    friction: 0.6,
    
    // Rendering
    transparent: false,       // OPAQUE ✓
    opacity: 1.0,
    refractive_index: 1.0,
    emissive: false,
    
    // Visual
    color: [128, 128, 128],
    texture_id: 2,
    
    // Gameplay
    destructible: true,
    hardness: 8.0,            // Hard to break
}
```

---

## HOW EACH SYSTEM USES PROPERTIES

### Physics / Collision

**Question:** Can player move into this voxel?

```rust
fn can_move_into(material: Material) -> bool {
    !MATERIAL_PROPERTIES[material as usize].solid
}

// Examples:
can_move_into(Material::AIR)    // true  - walk freely
can_move_into(Material::GLASS)  // false - blocked by glass
can_move_into(Material::WATER)  // true  - can wade/swim
can_move_into(Material::STONE)  // false - blocked
```

**Movement speed modified by density:**
```rust
fn movement_speed(material: Material) -> f32 {
    let props = &MATERIAL_PROPERTIES[material as usize];
    if props.solid {
        0.0  // Can't move
    } else {
        // Slower in water (high density) than air (low density)
        1.0 / (1.0 + props.density / 1000.0)
    }
}

// Examples:
movement_speed(Material::AIR)    // ~1.0   (normal speed)
movement_speed(Material::WATER)  // ~0.5   (half speed in water)
movement_speed(Material::GLASS)  // 0.0    (blocked)
```

---

### Mesh Extraction (Marching Cubes)

**Question:** Where to generate surface mesh?

**Option A: Generate at ALL material boundaries**
```rust
fn should_generate_surface(material_a: Material, material_b: Material) -> bool {
    material_a != material_b  // Any transition
}

// This generates:
// - Air/Stone boundary ✓
// - Air/Glass boundary ✓
// - Air/Water boundary ✓
// - Glass/Stone boundary ✓
// - Water/Stone boundary ✓
```

**Problem:** Generates WAY too many surfaces (every voxel boundary)

---

**Option B: Generate at OPAQUE boundaries only**
```rust
fn should_generate_surface(material_a: Material, material_b: Material) -> bool {
    let props_a = &MATERIAL_PROPERTIES[material_a as usize];
    let props_b = &MATERIAL_PROPERTIES[material_b as usize];
    
    // Generate surface if opacity changes significantly
    (props_a.opacity - props_b.opacity).abs() > 0.1
}

// This generates:
// - Air/Stone boundary ✓ (opacity 0.0 vs 1.0)
// - Air/Water boundary ✓ (opacity 0.0 vs 0.6)
// - Stone/Water boundary ✓ (opacity 1.0 vs 0.6)
// - Air/Glass boundary ? (opacity 0.0 vs 0.1 - just barely)
```

**Problem:** Misses subtle transparent/opaque transitions

---

**Option C: Generate based on "visual change"**
```rust
fn should_generate_surface(material_a: Material, material_b: Material) -> bool {
    if material_a == material_b {
        return false;  // Same material, no surface
    }
    
    let props_a = &MATERIAL_PROPERTIES[material_a as usize];
    let props_b = &MATERIAL_PROPERTIES[material_b as usize];
    
    // Generate if EITHER material is opaque (visible surface)
    // OR if both transparent but different (glass/water boundary)
    props_a.opacity > 0.5 || props_b.opacity > 0.5 || 
    (props_a.transparent && props_b.transparent && props_a.opacity != props_b.opacity)
}
```

**This handles:**
- Air/Stone: Yes (stone opaque) ✓
- Air/Glass: Yes (both transparent but different opacity) ✓
- Air/Water: Yes (water semi-opaque) ✓
- Glass/Water: Yes (different transparent materials) ✓
- Stone/Glass: Yes (stone opaque) ✓

---

### Rendering / Shaders

**After mesh extraction, shader handles transparency:**

```wgsl
struct Material {
    opacity: f32,
    refractive_index: f32,
    color: vec3<f32>,
    // ...
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let material = materials[in.material_id];
    
    // Base color from texture or material color
    var color = texture_sample(in.uv, material.texture_id);
    
    // Apply transparency
    if material.opacity < 1.0 {
        // Transparent material
        color.a = material.opacity;
        
        // Optional: refraction (expensive)
        if material.refractive_index != 1.0 {
            // Bend light based on refractive index
            let refracted_dir = refract(view_dir, in.normal, material.refractive_index);
            // Sample environment/background along refracted ray
            // ...
        }
    }
    
    return color;
}
```

**Rendering order:**
1. Opaque objects first (stone, dirt, concrete)
2. Transparent objects back-to-front (water, glass)
   - Depth sorting required for correct blending

---

### Water-Specific Handling

**Water is special (fluid physics):**

```rust
// Physics system
fn apply_water_physics(entity: &mut Entity, voxel_material: Material) {
    let props = &MATERIAL_PROPERTIES[voxel_material as usize];
    
    if voxel_material == Material::WATER {
        // Buoyancy force
        let buoyant_force = props.density * GRAVITY * entity.volume;
        entity.apply_force(Vec3::new(0.0, buoyant_force, 0.0));
        
        // Drag
        entity.velocity *= 1.0 - props.friction;
        
        // Swimming animation
        entity.set_animation(Animation::Swim);
    }
}
```

**Water surface rendering (special case):**
```rust
// Generate animated water surface mesh
if material == Material::WATER && above_material == Material::AIR {
    // This is water surface (special rendering)
    // - Animated waves (vertex shader displacement)
    // - Reflections (screen-space or cubemap)
    // - Foam at edges
}
```

---

## VOXEL REPRESENTATION UNCHANGED

**Important:** Voxel still just stores material ID

```rust
// This is ALL that's stored in the octree
enum OctreeNode {
    Empty,                    // Implicitly Material::AIR
    Solid(Material),          // Just the u8 material ID
    Branch { children: ... },
}

// Properties looked up at runtime
let material = octree.get_voxel(pos);
let props = &MATERIAL_PROPERTIES[material as usize];

if props.solid {
    // Can't walk here
}

if props.transparent {
    // Need to render with alpha blending
}
```

**No extra storage per voxel** - properties are in a lookup table

---

## FUTURE EXTENSIONS

### Per-Voxel State (Beyond Material)

**For complex behaviors:**
```rust
struct VoxelState {
    material: Material,           // Base material (u8)
    
    // Optional state (only stored if needed)
    water_level: Option<u8>,      // 0-255 for partial fill
    damage: Option<u8>,           // Block health
    orientation: Option<u8>,      // For directional blocks (stairs, logs)
    metadata: Option<u16>,        // Custom data
}
```

**But for Phase 1-6: Just material ID is sufficient**

---

### Partial Voxels (Sub-Voxel Resolution)

**For thin features:**
```rust
// A glass window might be only 5cm thick, not full 1m voxel
struct ThinVoxel {
    material: Material,
    thickness: f32,  // 0.0-1.0 (fraction of voxel)
    orientation: Vec3<f32>,  // Normal direction
}
```

**But this complicates everything - defer until needed**

---

## ANSWER TO YOUR QUESTION

**"Glass is basically a solid air block"**

**No - it's a SOLID TRANSPARENT block:**
- Physics: Uses `solid: true` (blocks movement like stone)
- Rendering: Uses `transparent: true` + `opacity: 0.1` (see through like air)
- Mesh: Generates surface at air/glass boundary (visible)
- Shader: Applies alpha blending + refraction

**"Water is an air block but denser"**

**No - it's a FLUID SEMI-TRANSPARENT block:**
- Physics: Uses `solid: false` (can move through) + `density: 1000` (buoyancy, drag)
- Rendering: Uses `transparent: true` + `opacity: 0.6` (semi-see-through)
- Mesh: Generates surface at air/water boundary (visible water surface)
- Shader: Applies alpha blending + reflections + animated waves

**The key: Material ID is just a lookup key. Properties define ALL behavior.**

**No special cases in voxel storage - all complexity in property tables and rendering.**

---

## IMPLEMENTATION

**Phase 3 (Voxel Structure):**
```rust
// Define material enum
pub enum Material { AIR, STONE, GLASS, WATER, ... }

// Define properties table
const MATERIAL_PROPERTIES: [MaterialProperties; 256] = [ /* ... */ ];
```

**Phase 5 (Mesh Extraction):**
```rust
// Marching cubes checks material transitions
fn extract_mesh(octree: &Octree) -> Mesh {
    for each cube corner {
        let material = octree.get_voxel(corner);
        let props = &MATERIAL_PROPERTIES[material as usize];
        
        // Use opacity to decide if surface needed
        if should_generate_surface(material_a, material_b) {
            // Generate triangle
        }
    }
}
```

**Phase 6 (Rendering):**
```rust
// Shader receives material ID, looks up properties
@fragment
fn fs_main(material_id: u32) -> vec4<f32> {
    let props = material_properties[material_id];
    
    // Apply transparency, refraction, etc.
    // ...
}
```

---

## DOES THIS ANSWER YOUR CONCERN?

**Glass and water are NOT special cases in voxel storage.**

**They're just materials with specific property combinations:**
- Glass = solid + transparent
- Water = fluid + semi-transparent + high density
- Air = non-solid + fully transparent + low density

**All handled by property lookup, not voxel representation.**

