# Question 3: Rendering Coordinates - Floating Origin Technique

**Last Updated:** 2026-02-17  
**Purpose:** Solve f32 GPU precision problem for Earth-scale rendering

---

## THE PRECISION PROBLEM

### ECEF f64 Storage (Validated ✓)

From `COORDINATE_SCALE_EVALUATION.md`:
- Earth radius: ~6,371,000 meters
- f64 precision at this scale: ~1.4 nanometers
- Our requirement: 1cm (collision accuracy)
- **Safety margin: 10 million times better than needed** ✅

**Conclusion:** f64 ECEF is perfect for storage.

---

### GPU Rendering Problem (f32 precision)

**GPUs use f32 for vertex positions** (not f64):
- Most shaders written for f32 (smaller, faster)
- f64 support limited (older GPUs don't have it)
- Even with f64 support, f32 is 2× faster

**f32 precision at Earth scale:**
```
f32 significand: 24 bits
Precision = value / 2^24

At Earth radius (6.4M meters):
Precision = 6,400,000 / 16,777,216
         ≈ 0.38 meters
```

**Actually worse due to rounding:**
- Nearest representable values ~0.76m apart
- **Can't distinguish positions within ~1 meter** ❌

**This breaks:**
- Player collision (need 1cm accuracy)
- Object placement (need millimeter accuracy)
- Smooth animation (positions "snap" to grid)

**Standard game engine workaround: DON'T RENDER AT EARTH SCALE**
- Most games: world is ±10km max
- Beyond that: skybox, or separate "world map"
- We can't do this (need continuous Earth)

---

## THE SOLUTION: FLOATING ORIGIN

### Core Concept

**Camera is ALWAYS at position (0, 0, 0) in render space.**
**The world translates relative to the camera.**

```
Traditional rendering:
  Camera position: (6,371,000, 0, 0) meters  ← Big number, poor f32 precision
  Tree position:   (6,371,050, 0, 0) meters  ← Big number, poor f32 precision
  Difference: 50 meters                      ← What GPU actually needs

Floating origin:
  Camera position: (0, 0, 0)                 ← Always zero in render space
  Tree position:   (50, 0, 0)                ← Small number, excellent f32 precision
  Difference: 50 meters                      ← Same result, better precision
```

**Key insight:** GPU only needs RELATIVE positions (object to camera), not absolute positions.

---

### How It Works

**1. Store absolute positions in f64 ECEF**
```rust
// Canonical storage (never changes)
let camera_ecef: Vec3<f64> = Vec3::new(6_371_000.0, 0.0, 0.0);
let tree_ecef: Vec3<f64> = Vec3::new(6_371_050.0, 10.0, 5.0);
let rock_ecef: Vec3<f64> = Vec3::new(6_370_980.0, -15.0, 3.0);
```

**2. When rendering: translate to camera-relative f32**
```rust
// GPU vertex shader input
fn to_render_space(entity_ecef: Vec3<f64>, camera_ecef: Vec3<f64>) -> Vec3<f32> {
    let relative_f64 = entity_ecef - camera_ecef;  // f64 subtraction (nanometer precision)
    let relative_f32 = relative_f64.as_f32();      // Convert to f32
    relative_f32
}

// Examples:
to_render_space(tree_ecef, camera_ecef)
  = (6_371_050 - 6_371_000, 10 - 0, 5 - 0)
  = (50.0, 10.0, 5.0)  // Small numbers, excellent f32 precision

to_render_space(rock_ecef, camera_ecef)
  = (6_370_980 - 6_371_000, -15 - 0, 3 - 0)
  = (-20.0, -15.0, 3.0)  // Small numbers, excellent f32 precision
```

**3. Camera view matrix is identity-like**
```rust
// Camera is at origin, looking in some direction
let view_matrix = look_at(
    Vec3::ZERO,           // Camera position (always zero in render space)
    camera_forward,       // Look direction (from camera orientation)
    camera_up             // Up vector
);
```

**4. When camera moves: update all entities**
```rust
// Camera moves 100m east in ECEF
camera_ecef += Vec3::new(100.0, 0.0, 0.0);

// Next frame: all entities recalculated relative to new camera position
// This happens every frame anyway (view matrix changes)
```

---

## PRECISION VALIDATION

### f32 Precision at Different Distances

```
Distance from camera | f32 precision | Acceptable?
---------------------|---------------|------------
±1 meter            | ~0.00006 mm   | ✅ Way better than needed
±10 meters          | ~0.0006 mm    | ✅ Sub-micrometer
±100 meters         | ~0.006 mm     | ✅ 6 micrometers
±1 kilometer        | ~0.06 mm      | ✅ 60 micrometers
±10 kilometers      | ~0.6 mm       | ✅ Sub-millimeter
±100 kilometers     | ~6 mm         | ⚠️ Getting poor
```

**Render distance limit: ~10km**
- Within 10km: sub-millimeter precision ✅
- Beyond 10km: use LOD (lower detail, precision less critical)
- Distant mountains: can tolerate centimeter-level precision

**Our requirement:**
- Player interaction radius: ~100m (excellent precision)
- Visible detail: ~1km (excellent precision)
- Distant terrain: 10km+ (adequate precision with LOD)

**Conclusion: Floating origin solves the problem** ✅

---

## IMPLEMENTATION DETAILS

### When Camera Moves

**Every frame:**
```rust
struct Renderer {
    camera_ecef: Vec3<f64>,        // Absolute position
    camera_orientation: Quat<f64>, // Rotation
}

fn render_frame(&self, entities: &[Entity]) {
    // Convert all entity positions to camera-relative
    let vertex_data: Vec<Vertex> = entities.iter().map(|entity| {
        let pos_f32 = to_render_space(entity.ecef_pos, self.camera_ecef);
        Vertex { position: pos_f32, ... }
    }).collect();
    
    // Upload to GPU
    gpu.update_vertex_buffer(vertex_data);
    
    // Camera view matrix (camera at origin)
    let view = look_at(Vec3::ZERO, self.camera_orientation.forward(), Vec3::Y);
    gpu.set_uniform("view", view);
    
    // Render
    gpu.draw();
}
```

**Optimization: Only update when camera moves**
```rust
if camera_moved_more_than(1.0) {  // 1 meter threshold
    // Recalculate all vertex positions
} else {
    // Reuse cached vertex buffer
}
```

---

### Chunk Boundaries

**Alternative approach: Chunk-local coordinates**

Instead of camera-relative, could use chunk-relative:
```rust
struct Chunk {
    origin_ecef: Vec3<f64>,        // Chunk center in ECEF
    local_entities: Vec<Entity>,   // Entity positions in chunk-local f32
}

// Entity position in chunk
entity.local_pos = (entity.ecef - chunk.origin_ecef).as_f32();

// Render: chunk origin to camera offset
let chunk_offset = (chunk.origin_ecef - camera_ecef).as_f32();
vertex_world_pos = entity.local_pos + chunk_offset;
```

**Pros:**
- Entities don't need recalculation when camera moves
- Each chunk is independent (easier multithreading)

**Cons:**
- Need to handle chunk boundaries (stitching)
- More complex shader (extra offset per chunk)

**Decision:** Start with camera-relative (simpler), optimize to chunk-local if needed.

---

## COORDINATE SPACES SUMMARY

**Three coordinate spaces:**

### 1. ECEF f64 (Storage)
- **Purpose:** Canonical absolute positions
- **Used for:** All entity storage, network sync, physics
- **Precision:** Nanometer at Earth scale
- **Never changes:** Absolute reference frame

### 2. Camera-Relative f32 (Rendering)
- **Purpose:** GPU vertex positions
- **Computed:** `(entity_ecef - camera_ecef).as_f32()`
- **Precision:** Sub-millimeter within 10km
- **Updates:** Every frame (when camera moves)

### 3. GPS Lat/Lon (Human Interface)
- **Purpose:** User input, display, data ingestion
- **Converted to:** ECEF for all internal use
- **Never used for:** Rendering or physics

---

## VALIDATION TESTS (To Write in Phase 1)

### Test 1: Precision at Origin
```rust
#[test]
fn test_render_precision_at_origin() {
    let camera = Vec3::new(6_371_000.0, 0.0, 0.0);
    let entity = Vec3::new(6_371_001.0, 0.0, 0.0);  // 1m away
    
    let render_pos = to_render_space(entity, camera);
    assert_eq!(render_pos, Vec3::new(1.0, 0.0, 0.0));
    
    // Check f32 can represent this exactly
    assert_eq!(render_pos.x as f64, 1.0);  // No precision loss
}
```

### Test 2: Precision at 10km
```rust
#[test]
fn test_render_precision_at_10km() {
    let camera = Vec3::new(6_371_000.0, 0.0, 0.0);
    let entity = Vec3::new(6_381_000.0, 0.0, 0.0);  // 10km away
    
    let render_pos = to_render_space(entity, camera);
    assert_eq!(render_pos.x, 10_000.0);
    
    // Check precision: f32 should distinguish 1mm at 10km
    let entity_plus_1mm = Vec3::new(6_381_000.001, 0.0, 0.0);
    let render_plus_1mm = to_render_space(entity_plus_1mm, camera);
    
    let difference = render_plus_1mm.x - render_pos.x;
    assert!(difference > 0.0);  // Can distinguish 1mm
    assert!(difference < 0.01); // And it's approximately 1mm
}
```

### Test 3: Camera Movement
```rust
#[test]
fn test_camera_movement() {
    let mut camera = Vec3::new(6_371_000.0, 0.0, 0.0);
    let entity = Vec3::new(6_371_050.0, 10.0, 5.0);
    
    let render_pos_1 = to_render_space(entity, camera);
    assert_eq!(render_pos_1, Vec3::new(50.0, 10.0, 5.0));
    
    // Camera moves 30m east
    camera.x += 30.0;
    
    let render_pos_2 = to_render_space(entity, camera);
    assert_eq!(render_pos_2, Vec3::new(20.0, 10.0, 5.0));  // Entity appears 30m closer
}
```

### Test 4: Round-Trip Accuracy
```rust
#[test]
fn test_round_trip_accuracy() {
    let camera = Vec3::new(6_371_000.0, 0.0, 0.0);
    let entity_original = Vec3::new(6_371_050.5, 10.25, 5.125);
    
    // To render space and back
    let render_pos = to_render_space(entity_original, camera);
    let entity_reconstructed = camera + render_pos.as_f64();
    
    let error = (entity_reconstructed - entity_original).length();
    assert!(error < 0.001);  // Less than 1mm error
}
```

---

## SHADER IMPLICATIONS

**Vertex shader receives camera-relative positions:**
```wgsl
struct Vertex {
    @location(0) position: vec3<f32>,  // Already camera-relative
    @location(1) normal: vec3<f32>,
}

struct Uniforms {
    view: mat4x4<f32>,        // Camera rotation (position is always zero)
    projection: mat4x4<f32>,  // Perspective
}

@vertex
fn vs_main(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    
    // Position is already relative to camera (which is at origin)
    let view_pos = uniforms.view * vec4(in.position, 1.0);
    out.clip_position = uniforms.projection * view_pos;
    
    return out;
}
```

**No special handling needed in shader** - positions are already correct.

---

## PROS AND CONS

### ✅ PROS

- **Solves f32 precision problem** completely
- **Standard technique** (used by Space Engineers, Kerbal Space Program, etc.)
- **No GPU changes needed** (standard f32 shaders work)
- **Simple math** (just subtract camera position)
- **Works at any scale** (not limited to Earth)

### ⚠️ CONS

- **CPU cost every frame** (recalculate vertex positions when camera moves)
  - Mitigation: Only update if camera moved >1m
  - Or use chunk-local coords (entities don't move with camera)
- **All entities must be updated** when camera moves
  - Mitigation: Spatial partitioning (only update visible entities)
- **Slightly more complex** than naive rendering
  - But necessary for Earth-scale

---

## DECISION

**Use Floating Origin technique:**
- Camera always at (0,0,0) in render space
- All entity positions converted to camera-relative f32
- Provides sub-millimeter precision within 10km ✅
- Standard, proven approach ✅

**Implementation approach:**
1. Start with simple camera-relative (Phase 6)
2. Test precision and performance
3. Optimize to chunk-local if needed (future)

---

## NEXT STEPS

- [x] Understand floating origin technique
- [x] Document the math
- [x] Validate precision at different distances
- [x] Plan validation tests
- [ ] Implement in Phase 1 (coordinate system tests)
- [ ] Actually test in Phase 6 (first render)

**Question 3: ANSWERED ✅**

