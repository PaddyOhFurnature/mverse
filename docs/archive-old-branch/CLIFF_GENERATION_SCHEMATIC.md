# Cliff Face Generation - Complete Data Flow Schematic

**Last Updated:** 2026-02-17  
**Purpose:** Break down cliff generation to ATOMIC operations - every input, every calculation, every parameter

**Scope:** Generate detail for a 1m² patch of vertical cliff face

---

## PROBLEM STATEMENT

**Given:**
- Location: (-27.4775°S, 153.0355°E) - Kangaroo Point Cliffs
- We know cliff EXISTS (detected from SRTM slope > 70°)
- We know cliff is ~30m tall (top elevation - bottom elevation)

**Want to generate:**
- Detailed 3D mesh for a 1m×1m patch of the cliff face
- Resolution: Sufficient for player standing 2m away (need ~5cm detail)
- Must look like real sandstone cliff

**Unknown:**
- WHAT ALGORITHM generates the detail?
- WHAT PARAMETERS control the appearance?
- WHERE do we get those parameters?
- HOW do we validate it's correct?

---

## ATOMIC UNIT: 1m² Cliff Face Patch

Location on cliff:
- Base coordinates: (-27.4775°S, 153.0355°E)
- Height on cliff: 15m above base (middle of 30m cliff)
- Orientation: Facing west (river view)
- Size: 1m wide × 1m tall

---

## DATA FLOW DIAGRAM

```
┌─────────────────────────────────────────────────────────────────────┐
│ INPUTS                                                              │
└─────────────────────────────────────────────────────────────────────┘
         │
         │  [1] Patch location (x, y, z, orientation)
         │  [2] Geology type
         │  [3] Climate/weathering data
         │  [4] Fractal parameters
         │  [5] Seed (for determinism)
         │
         ▼
┌─────────────────────────────────────────────────────────────────────┐
│ PROCESSING STEPS                                                    │
│                                                                     │
│  Step 1: Base mesh generation (flat quad)                          │
│  Step 2: Height displacement (fractal noise)                       │
│  Step 3: Layer pattern (sedimentary banding)                       │
│  Step 4: Erosion features (cracks, weathering)                     │
│  Step 5: Normal map generation (lighting detail)                   │
│  Step 6: Color/texture mapping                                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────────────────────────────────┐
│ OUTPUTS                                                             │
│                                                                     │
│  • Vertex positions (3D mesh)                                       │
│  • Triangle indices (topology)                                      │
│  • Normal vectors (lighting)                                        │
│  • UV coordinates (texture mapping)                                 │
│  • Material properties (color, roughness)                           │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## INPUT 1: PATCH LOCATION

**What we need:**
```rust
struct PatchLocation {
    lat: f64,           // -27.4775
    lon: f64,           // 153.0355
    height_above_base: f64,  // 15.0 meters
    orientation: Vector3,    // Normal vector (which way patch faces)
    size: (f64, f64),        // (1.0, 1.0) meters
}
```

**Where it comes from:**
- Parent system divides entire cliff into 1m² patches
- Iterates over: height 0m to 30m, horizontal position along cliff line

**Question:** How do we KNOW the cliff orientation (which way it faces)?
- Answer needed: Calculate from terrain
  - Method: Gradient vector at cliff top
  - Points in direction of steepest descent
  - Normal to that = cliff face direction

**Dependency:** Need to calculate cliff orientation BEFORE generating patch
- Input to that: SRTM elevation in 3×3 grid around cliff top
- Output: Normal vector (facing direction)

---

## INPUT 2: GEOLOGY TYPE

**What we need:**
```rust
struct GeologyData {
    rock_type: String,      // "Brisbane River Formation - Sandstone"
    age: u64,               // ~200 million years
    properties: RockProperties,
}

struct RockProperties {
    layered: bool,          // true for sedimentary
    layer_thickness: f32,   // 0.5 - 2.0 meters (typical sandstone)
    hardness: f32,          // Mohs scale 6-7 (quartz-rich sandstone)
    color_primary: RGB,     // Tan/brown
    color_secondary: RGB,   // Darker bands (iron oxide)
    erosion_rate: f32,      // Weathering susceptibility
}
```

**Where it comes from:**
1. **Macrostrat API query:**
   ```
   GET https://macrostrat.org/api/v2/geologic_units/intersection
   Parameters: lat=-27.4775, lng=153.0355
   Returns: JSON with rock unit data
   ```

2. **Parse response:**
   - Unit name: "Brisbane River Formation"
   - Lithology: "Sandstone"
   - Age: Triassic (~200 Ma)

3. **PROBLEM: API gives NAME, not PROPERTIES**
   - We need: layer_thickness, color, erosion_rate
   - API doesn't provide these

**Research needed:**
- Build database: Rock type → Properties
- Sources:
  - Geological literature (rock type descriptions)
  - Field observations (if we have them)
  - Default values by lithology category
  
**Example database entry:**
```json
{
  "lithology": "sandstone",
  "layer_thickness_min": 0.3,
  "layer_thickness_max": 2.0,
  "color_primary": {"r": 200, "g": 170, "b": 140},
  "color_variation": 0.15,
  "hardness_mohs": 6.5,
  "erosion_rate_mm_per_year": 0.1
}
```

**Question:** Where do we get these default values?
- Answer needed: Research geological databases, textbooks
- Fallback: Estimate from similar rocks

---

## INPUT 3: CLIMATE/WEATHERING DATA

**What we need:**
```rust
struct WeatheringData {
    annual_rainfall: f32,        // mm/year
    temperature_range: (f32, f32), // min/max °C
    freeze_thaw_cycles: u32,     // times per year
    wind_direction: Vector3,     // Prevailing wind
    wind_speed_avg: f32,         // m/s
    humidity: f32,               // %
}
```

**Where it comes from:**
1. **Climate data (WorldClim):**
   ```
   Query: (-27.4775, 153.0355)
   Returns: Temperature, precipitation monthly averages
   ```
   
2. **Calculate derived values:**
   - Freeze-thaw: Count months where temp crosses 0°C
     - Brisbane: Subtropical, never freezes
     - freeze_thaw_cycles = 0
   
3. **Wind data:**
   - Source: Meteorological bureau records
   - Brisbane: Prevailing easterly (from ocean)
   - Wind direction: East → affects which side erodes more

**Effect on cliff:**
- High rainfall + no freeze = Chemical weathering (dissolution)
- Easterly wind = East-facing surfaces erode faster
- Our patch faces WEST = More protected, less erosion

**Question:** How does this affect the mesh generation?
- Answer needed: Erosion parameters in fractal algorithm
  - More eroded = rougher surface, more detail
  - Less eroded = smoother, less variation

---

## INPUT 4: FRACTAL PARAMETERS

**This is the CRITICAL unknown - what algorithm and what parameters?**

### Research Question 1: Which fractal algorithm?

**Options:**
1. **Perlin Noise**
   - Pros: Smooth, organic looking
   - Cons: Can look too "cloudy" for rock
   
2. **Simplex Noise**
   - Pros: Faster than Perlin, fewer artifacts
   - Cons: Similar limitations
   
3. **Worley Noise (Voronoi)**
   - Pros: Cellular patterns (good for cracked rock)
   - Cons: Doesn't capture layering
   
4. **Layered noise (multiple octaves)**
   - Combine multiple frequencies
   - Pros: More realistic, controllable
   - Cons: More complex, more parameters
   
5. **Domain warping**
   - Use noise to distort coordinates of another noise
   - Pros: Very organic, realistic erosion
   - Cons: Expensive, hard to control

**DECISION NEEDED:** Which algorithm(s)?
- Research method: Generate samples of each, compare to reference photo
- Test: Does it LOOK like sandstone cliff?

### Research Question 2: What parameters?

**For layered noise approach:**
```rust
struct FractalParams {
    octaves: u32,           // How many noise layers? (3-6 typical)
    frequency: f32,         // Base frequency (scale of largest features)
    lacunarity: f32,        // Frequency multiplier per octave (2.0 typical)
    persistence: f32,       // Amplitude multiplier per octave (0.5 typical)
    amplitude: f32,         // Overall displacement amount
    seed: u64,              // For determinism
}
```

**Meaning of parameters:**
- `octaves = 4`: Combine 4 different noise frequencies
  - Octave 0: Large features (1m scale)
  - Octave 1: Medium features (0.5m scale)
  - Octave 2: Small features (0.25m scale)
  - Octave 3: Fine detail (0.125m scale)

- `frequency = 1.0`: Base frequency
  - Controls largest feature size
  - Higher = more frequent variation

- `lacunarity = 2.0`: Each octave doubles frequency
  - Standard value for natural-looking noise

- `persistence = 0.5`: Each octave halves amplitude
  - Controls how much fine detail affects result
  - Higher = more rough/detailed
  - Lower = smoother

- `amplitude = 0.1`: Total displacement (10cm)
  - How far surface moves in/out from base plane
  - Too high = unrealistic bulges
  - Too low = too smooth

**PROBLEM:** We don't know what values to use!

**Research needed:**
1. Generate test patches with varying parameters
2. Compare visually to reference photo
3. Measure roughness statistics from real cliff photos
4. Iterate until match

**Specific tests:**
```
Test 1: Vary amplitude (0.05, 0.1, 0.2, 0.5)
  - Which looks most realistic?
  
Test 2: Vary persistence (0.3, 0.5, 0.7, 0.9)
  - How rough should sandstone be?
  
Test 3: Vary octaves (3, 4, 5, 6)
  - How much detail is needed?
```

---

## INPUT 5: SEED (Determinism)

**What we need:**
```rust
fn generate_seed(patch: &PatchLocation) -> u64 {
    // Must be deterministic: same location → same seed
    // Must be unique: different locations → different seeds
    
    // Hash the location coordinates
    let hash_input = format!("{:.6}_{:.6}_{:.2}", 
        patch.lat, 
        patch.lon, 
        patch.height_above_base
    );
    
    // Use cryptographic hash for good distribution
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let result = hasher.finalize();
    
    // Convert first 8 bytes to u64
    u64::from_le_bytes(result[0..8].try_into().unwrap())
}
```

**Why critical:**
- Two players looking at same patch must see IDENTICAL geometry
- Generate once, cache, must regenerate to same result
- Any difference = visual glitches, desynced state

**Validation test:**
- Generate patch 1000 times
- All results must be byte-for-byte identical

---

## PROCESSING STEP 1: BASE MESH GENERATION

**Input:** Patch location (1m × 1m)

**Output:** Flat quad mesh

```rust
struct Mesh {
    vertices: Vec<Vector3>,
    indices: Vec<u32>,
    normals: Vec<Vector3>,
    uvs: Vec<Vector2>,
}

fn generate_base_mesh(resolution: usize) -> Mesh {
    // resolution = vertices per edge (e.g., 20 = 20×20 = 400 vertices)
    
    let vertices = Vec::new();
    for y in 0..resolution {
        for x in 0..resolution {
            let u = x as f32 / (resolution - 1) as f32;  // 0.0 to 1.0
            let v = y as f32 / (resolution - 1) as f32;  // 0.0 to 1.0
            
            vertices.push(Vector3 {
                x: u,  // 0.0 to 1.0 meters
                y: v,  // 0.0 to 1.0 meters
                z: 0.0 // Flat (will be displaced later)
            });
        }
    }
    
    // Generate triangle indices (2 triangles per quad)
    let indices = generate_grid_indices(resolution);
    
    // Initial normals all point outward (perpendicular to cliff face)
    let normals = vec![Vector3::new(0.0, 0.0, 1.0); vertices.len()];
    
    // UV coordinates match vertex positions
    let uvs: Vec<Vector2> = vertices.iter()
        .map(|v| Vector2::new(v.x, v.y))
        .collect();
    
    Mesh { vertices, indices, normals, uvs }
}
```

**Question:** What resolution is needed?
- Player 2m away, needs ~5cm detail minimum
- 1m patch / 5cm = 20 vertices per edge
- 20×20 = 400 vertices per 1m² patch
- Entire 30m cliff = 30m × 30m = 900 patches × 400 vertices = 360,000 vertices
  - Too many? Need LOD?

**Research needed:**
- Performance test: Can we render 360k vertices at 60fps?
- If no: Need LOD system (lower resolution far away)

---

## PROCESSING STEP 2: HEIGHT DISPLACEMENT

**Input:** Base mesh + fractal parameters + seed

**Output:** Displaced mesh (rough surface)

```rust
fn apply_displacement(
    mesh: &mut Mesh, 
    params: &FractalParams,
    seed: u64
) {
    // Initialize noise generator with seed
    let noise = create_noise_generator(seed);
    
    for vertex in &mut mesh.vertices {
        // Sample noise at this vertex position
        let noise_value = sample_layered_noise(
            &noise,
            vertex.x,
            vertex.y,
            params
        );
        
        // Displace vertex in Z direction (out from cliff face)
        vertex.z = noise_value * params.amplitude;
    }
}

fn sample_layered_noise(
    noise: &NoiseGenerator,
    x: f32,
    y: f32,
    params: &FractalParams
) -> f32 {
    let mut result = 0.0;
    let mut frequency = params.frequency;
    let mut amplitude = 1.0;
    
    for octave in 0..params.octaves {
        // Sample noise at this frequency
        let sample = noise.sample_2d(
            x * frequency,
            y * frequency
        );
        
        // Accumulate weighted by amplitude
        result += sample * amplitude;
        
        // Adjust for next octave
        frequency *= params.lacunarity;
        amplitude *= params.persistence;
    }
    
    // Normalize to -1.0 to 1.0 range
    result / calculate_normalization_factor(params)
}
```

**Question:** What IS `noise.sample_2d()`?
- This is a library function (e.g., `noise-rs` crate)
- Returns value in range -1.0 to 1.0
- Input coordinates → deterministic output
- Same input always gives same output (for given seed)

**Research needed:**
- Which noise library to use?
- Test different noise types (Perlin, Simplex, etc.)
- Validate determinism across platforms

---

## PROCESSING STEP 3: LAYER PATTERN

**Input:** Displaced mesh + geology properties

**Output:** Layered appearance (sedimentary banding)

**Sandstone characteristic:** Horizontal layers (bedding planes)

```rust
fn apply_layering(
    mesh: &mut Mesh,
    rock_props: &RockProperties
) {
    // Layers are HORIZONTAL (gravity deposited)
    // Add subtle variation in displacement at layer boundaries
    
    for vertex in &mut mesh.vertices {
        // Height on cliff (0.0 to 1.0 for this 1m patch)
        let height = vertex.y;
        
        // Which layer is this vertex in?
        let layer_index = (height / rock_props.layer_thickness) as u32;
        
        // Distance to layer boundary (0.0 at center, 0.5 at boundary)
        let dist_to_boundary = (height % rock_props.layer_thickness) 
            / rock_props.layer_thickness;
        
        // Layers erode at boundaries (less cohesion)
        if dist_to_boundary < 0.1 || dist_to_boundary > 0.9 {
            // Near boundary: Add slight recession (weathering)
            vertex.z -= 0.02;  // 2cm recession at layer boundary
        }
        
        // Slight color variation per layer (for texturing later)
        vertex.layer_id = layer_index;
    }
}
```

**Question:** How thick are sandstone layers at Kangaroo Point?
- Research needed: Look at reference photos, measure layer spacing
- Estimate from photo: Layers appear ~0.5m to 1.5m thick
- Use average: 1.0m

**Question:** Do layers erode more at boundaries?
- Geomorphology research: Yes, bedding planes are weakness
- Amount of erosion: Unknown, need to calibrate from photos

---

## PROCESSING STEP 4: EROSION FEATURES

**Input:** Layered mesh + weathering data

**Output:** Cracks, pitting, weathering detail

```rust
fn apply_erosion(
    mesh: &mut Mesh,
    weathering: &WeatheringData,
    seed: u64
) {
    // Erosion types:
    // 1. Cracks (from stress/weathering)
    // 2. Pitting (from dissolution)
    // 3. Scaling (surface spalling)
    
    let crack_noise = create_noise_generator(seed + 1);
    let pit_noise = create_noise_generator(seed + 2);
    
    for vertex in &mut mesh.vertices {
        // CRACKS: Vertical or along layers
        let crack_pattern = sample_crack_noise(&crack_noise, vertex);
        if crack_pattern > 0.8 {  // Threshold for crack
            vertex.z -= 0.05;  // 5cm deep crack
        }
        
        // PITTING: Random small holes (chemical weathering)
        let pit_pattern = sample_pit_noise(&pit_noise, vertex);
        if pit_pattern > 0.85 {  // Threshold for pit
            vertex.z -= 0.02;  // 2cm deep pit
        }
        
        // WEATHERING INTENSITY: Based on exposure
        let exposure = calculate_exposure(vertex, weathering);
        vertex.z -= exposure * 0.01;  // Max 1cm weathering
    }
}

fn calculate_exposure(vertex: &Vertex, weather: &WeatheringData) -> f32 {
    // Higher exposure = more weathering
    
    // Height factor: Top of cliff more exposed to rain
    let height_factor = vertex.y;  // 0.0 at bottom, 1.0 at top
    
    // Orientation factor: East face more exposed to wind
    // (Our patch faces west, so less exposed)
    let orientation_factor = 0.3;  // 30% of east-facing exposure
    
    // Combine factors
    let exposure = height_factor * 0.5 + orientation_factor * 0.5;
    
    // Scale by weathering rate from climate data
    exposure * weather.erosion_rate_mm_per_year / 1000.0
}
```

**Question:** What causes cracks in sandstone?
- Research needed: Geomorphology literature
- Factors: Freeze-thaw (not Brisbane), thermal expansion, root wedging
- Pattern: Vertical (gravity stress) or along layers (bedding plane separation)

**Question:** How much pitting occurs?
- Unknown, need to estimate from photos
- Measure: % of surface affected, depth of pits

---

## PROCESSING STEP 5: NORMAL MAP GENERATION

**Input:** Displaced mesh

**Output:** Normal vectors (for lighting)

```rust
fn calculate_normals(mesh: &mut Mesh) {
    // For each triangle, calculate face normal
    for i in (0..mesh.indices.len()).step_by(3) {
        let i0 = mesh.indices[i] as usize;
        let i1 = mesh.indices[i+1] as usize;
        let i2 = mesh.indices[i+2] as usize;
        
        let v0 = mesh.vertices[i0];
        let v1 = mesh.vertices[i1];
        let v2 = mesh.vertices[i2];
        
        // Cross product of triangle edges
        let edge1 = v1 - v0;
        let edge2 = v2 - v0;
        let face_normal = edge1.cross(edge2).normalize();
        
        // Accumulate to vertex normals (for smooth shading)
        mesh.normals[i0] += face_normal;
        mesh.normals[i1] += face_normal;
        mesh.normals[i2] += face_normal;
    }
    
    // Normalize all vertex normals
    for normal in &mut mesh.normals {
        *normal = normal.normalize();
    }
}
```

**This is standard 3D graphics, well-understood.**

---

## PROCESSING STEP 6: COLOR/TEXTURE

**Input:** Mesh + rock properties

**Output:** Color per vertex or texture UV mapping

```rust
fn assign_colors(
    mesh: &mut Mesh,
    rock_props: &RockProperties,
    seed: u64
) {
    let color_noise = create_noise_generator(seed + 3);
    
    for (vertex, layer_id) in mesh.vertices.iter().zip(&vertex.layer_ids) {
        // Base color from rock properties
        let mut color = rock_props.color_primary;
        
        // Layer variation (some layers darker/lighter)
        if layer_id % 3 == 0 {
            color = rock_props.color_secondary;  // Iron oxide band
        }
        
        // Subtle noise variation (not uniform color)
        let noise_val = color_noise.sample_2d(vertex.x * 10.0, vertex.y * 10.0);
        color = color.vary_by(noise_val * 0.1);  // ±10% variation
        
        vertex.color = color;
    }
}
```

**Question:** What color is Brisbane River Formation sandstone?
- Research needed: Geological descriptions, field photos
- From reference photo: Tan/brown with some darker bands
- RGB estimate: (200, 170, 140) ± variation

---

## OUTPUT: COMPLETE 1M² PATCH

**Final result:**
```rust
struct CliffPatch {
    mesh: Mesh,              // ~400 vertices, ~760 triangles
    material: Material,      // Color, roughness, etc.
    collision: CollisionMesh, // Simplified for physics
}
```

**Size:** ~20KB per patch (vertices + normals + colors + indices)

**For entire 30m cliff:**
- 900 patches × 20KB = 18MB
- Reasonable to cache in memory

---

## VALIDATION PROCESS

**How do we know if this is correct?**

### Test 1: Visual Comparison
1. Generate 1m² patch with various parameters
2. Render from reference photo viewpoint
3. Place side-by-side with actual photo
4. Ask: Does it look like the same rock?

**Measurable criteria:**
- Color match (RGB difference)
- Roughness (statistical measure of displacement)
- Layer spacing (visual inspection)
- Overall "feel" (subjective but important)

### Test 2: Determinism Check
1. Generate same patch 100 times
2. Compare all vertex positions
3. Must be EXACTLY identical (byte-for-byte)

### Test 3: Performance
1. Generate 900 patches (full cliff)
2. Render at 60fps
3. Measure: Generation time, memory usage, frame time

### Test 4: Parameter Sensitivity
1. Vary each parameter ±10%
2. Observe visual changes
3. Identify: Which parameters matter most?

---

## UNKNOWNS REQUIRING RESEARCH

1. **Fractal algorithm choice**
   - Need: Test multiple algorithms, compare to reference
   - Method: Generate samples, visual assessment
   
2. **Fractal parameters**
   - Need: Optimal values for sandstone appearance
   - Method: Iterative testing with reference photos
   
3. **Rock properties database**
   - Need: layer_thickness, color, erosion rates for all rock types
   - Method: Literature review, build database
   
4. **Weathering models**
   - Need: How climate affects erosion patterns
   - Method: Geomorphology research
   
5. **Performance optimization**
   - Need: Can we render entire cliff at 60fps?
   - Method: Performance profiling, LOD if needed

---

## NEXT ACTIONS (ATOMIC TASKS)

1. **Setup test environment**
   - Load SRTM data for Kangaroo Point
   - Load reference photo
   - Setup camera at photo viewpoint
   
2. **Implement base mesh generation**
   - 20×20 vertex grid
   - Test: Renders as flat quad
   
3. **Research fractal algorithms**
   - Read: How Perlin vs Simplex vs others work
   - Decide: Which to use for rock
   
4. **Implement one fractal algorithm**
   - Add noise-rs crate
   - Test: Generates varied surface
   
5. **Test parameter space**
   - Generate 20 samples with different parameters
   - Visual comparison to reference
   - Record: Which looks best?
   
6. **Research sandstone properties**
   - Find: Layer thickness at Kangaroo Point
   - Find: Color values
   - Find: Weathering patterns
   
7. **Implement layering**
   - Add horizontal banding
   - Test: Looks stratified?
   
8. **Iterate until match**
   - Refine parameters
   - Compare to reference
   - Repeat until satisfied

**ONLY AFTER we can generate ONE convincing 1m² patch should we think about entire cliff.**

