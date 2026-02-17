# Terrain Rendering: Voxels+Smooth vs SDF

**Last Updated:** 2026-02-17  
**Purpose:** Choose rendering approach for organic volumetric Earth

---

## THE REQUIREMENTS

From reference images and discussion:
- ✅ Volumetric (caves, tunnels, overhangs, interiors)
- ✅ Organic appearance (NOT blocky, irregular natural surfaces)
- ✅ Detailed at all scales (aerial view shows fields, close view shows cave detail)
- ✅ Modifiable (players can dig, build)
- ✅ Real-world data integration (SRTM elevation, OSM buildings)
- ✅ Earth-scale (6,371km radius, detail down to centimeters)
- ✅ Network synchronizable (P2P, deterministic)

---

## OPTION 1: Voxels + Smooth Mesh Extraction

### What It Is
- **Storage:** Discrete grid of material IDs (air, stone, water, etc.)
- **Rendering:** Algorithm converts voxels → smooth triangle mesh
- **Common algorithm:** Marching Cubes (samples 8 corners, interpolates smooth surface)

### Example Flow
```
1. Store voxels: [AIR, AIR, STONE, STONE, STONE, AIR...]
2. Marching cubes: Sample corners → find air/solid boundary
3. Generate smooth triangles at boundary
4. Render triangular mesh (standard GPU pipeline)
```

---

### PROS: Voxels + Smooth

#### ✅ Simple Discrete Storage
- Each cubic meter = one material ID (1 byte)
- Easy to understand: "this location IS stone"
- Binary decisions (solid or not)

#### ✅ Proven at Scale
- Minecraft (blocky rendering, but voxel storage works)
- Astroneer (smooth rendering via marching cubes)
- 7 Days to Die (destructible voxel terrain)
- No Man's Sky (procedural voxel planets)

#### ✅ Easy Modification
- Set voxel = change one grid cell
- Digging: clear voxels
- Building: set voxels to material
- Local operation (doesn't affect distant voxels)

#### ✅ Network Synchronization
- Send voxel changes: "Set (x,y,z) to STONE"
- Deterministic (everyone has same voxel grid)
- Small messages (coordinate + material ID)
- Easy conflict resolution

#### ✅ Real-World Data Integration
- SRTM elevation → fill voxels below surface with STONE
- OSM buildings → set voxels to CONCRETE in building footprint
- Direct mapping: real-world location → voxel coordinate

#### ✅ Sparse Storage Available
- Octree compression (uniform regions = one node)
- Deep underground = one node saying "all STONE"
- Only store detail where needed
- Empty air = cheap

#### ✅ Standard GPU Rendering
- Mesh extraction → triangles
- Triangles render on any GPU (standard pipeline)
- No special shaders needed
- Well-understood performance

---

### CONS: Voxels + Smooth

#### ❌ Grid Resolution Limits Detail
- 1m voxels = 1m maximum detail
- Smaller features need smaller voxels
- 0.1m voxels = 1000× more storage
- Can't have infinite detail

#### ❌ Mesh Extraction Cost
- Must run algorithm to convert voxels → mesh
- Not free (CPU time)
- Need to update mesh when voxels change
- Can be slow for large volumes

#### ❌ Marching Cubes Artifacts
- Can produce thin triangles (bad for rendering)
- May create holes in some configurations
- "Ambiguous cases" in lookup table
- Not perfectly smooth (interpolated from grid)

#### ❌ Memory for Dense Detail
- 1m³ Earth at 1m voxels = ~10²¹ voxels
- Even with octree, detailed areas use RAM
- 1km² at 0.1m resolution = 100 million voxels
- Need streaming/caching system

#### ❌ Stair-Stepping on Diagonals
- Grid-aligned (X/Y/Z axes favored)
- Diagonal surfaces show grid bias
- Smooth extraction helps but doesn't eliminate
- 45° cliff may show slight stepping

---

## OPTION 2: Signed Distance Fields (SDF)

### What It Is
- **Storage:** Each point stores distance to nearest surface
- **Sign:** Positive = outside, negative = inside, zero = on surface
- **Rendering:** Ray marching or sphere tracing to find surface intersections

### Example Flow
```
1. Store SDF: point (x,y,z) → distance value (e.g., -5.3m = 5.3m underground)
2. Ray march: Cast ray, step forward by distance value, repeat until near zero
3. Found surface: Compute normal from SDF gradient
4. Shade pixel (or generate mesh for rasterization)
```

---

### PROS: SDF

#### ✅ Infinitely Smooth Surfaces
- Not limited by grid resolution
- Analytical smoothness (gradient = exact normal)
- No stair-stepping or grid artifacts
- Perfect curves, spheres, organic shapes

#### ✅ Compact Representation
- Can represent complex shapes with simple functions
- Sphere = "distance to center - radius"
- Combining shapes = min/max operations
- May need less storage than dense voxels

#### ✅ Easy CSG Operations
- Union: min(sdf1, sdf2)
- Subtraction: max(sdf1, -sdf2)
- Smooth blending: smooth_min(sdf1, sdf2)
- Natural for caves (subtract sphere from terrain)

#### ✅ No Mesh Extraction Needed (Ray Marching)
- Can render directly (ray march in shader)
- No intermediate mesh step
- Dynamic changes = instant (no mesh rebuild)

#### ✅ Accurate Collision Detection
- Distance to surface is STORED
- Fast proximity queries
- Sphere casting trivial
- Physics can use SDF directly

#### ✅ Adaptive Detail
- Detail comes from evaluation, not storage
- Can zoom infinitely (if SDF is analytical)
- Or sample at different resolutions

---

### CONS: SDF

#### ❌ Complex Implementation
- Less common in games (fewer examples)
- Rust libraries limited (most are C++/GLSL)
- Need to write custom rendering pipeline
- More moving parts to debug

#### ❌ Ray Marching Performance
- GPU must step through volume (many iterations)
- Slower than triangle rasterization for close objects
- Depends on step size vs distance to surface
- Large empty spaces = many steps

#### ❌ Real-World Data Integration Harder
- SRTM = heightmap, not distance field
- Need to convert: elevation → SDF (non-trivial)
- OSM buildings = polygons, need conversion
- Extra processing step for all input data

#### ❌ Modification More Complex
- Can't just "set voxel"
- Need to recompute distance field
- CSG operations work but need careful blending
- Local edit may affect large area (distance propagates)

#### ❌ Network Synchronization Harder
- Can't send "set voxel" messages
- Need to send operations or field updates
- Floating-point distance values (not discrete)
- Determinism harder (floating point consistency)

#### ❌ Rendering Pipeline Non-Standard
- Ray marching = different from triangle rasterization
- May not integrate well with traditional game rendering
- Buildings/entities still use triangles (hybrid pipeline)
- More complex shader code

#### ❌ Storage Still Needed
- Analytical SDF only works for simple shapes
- Earth-scale organic terrain needs sampled SDF (3D texture)
- Sampled SDF = memory like voxels BUT floating point (4× larger)
- Or procedural (noisy, hard to make deterministic)

#### ❌ Fewer Tools/Libraries
- Voxel tools: many (MagicaVoxel, Goxel, etc.)
- SDF tools: fewer (mostly research/demo)
- Less community knowledge
- Harder to find help when stuck

---

## HEAD-TO-HEAD COMPARISON

| Criterion | Voxels + Smooth | SDF |
|-----------|----------------|-----|
| **Smoothness** | Good (interpolated) | Excellent (analytical) |
| **Volumetric** | ✅ Yes | ✅ Yes |
| **Caves/overhangs** | ✅ Yes | ✅ Yes |
| **Player modification** | ✅ Easy | ⚠️ Complex |
| **SRTM integration** | ✅ Direct | ⚠️ Conversion needed |
| **OSM integration** | ✅ Direct | ⚠️ Conversion needed |
| **Network sync** | ✅ Simple | ❌ Complex |
| **Rendering speed** | ✅ Fast (triangles) | ⚠️ Slower (ray march) |
| **Implementation** | ✅ Well-known | ❌ Complex |
| **Memory** | ⚠️ High (sparse octree helps) | ⚠️ High (4× for floats) |
| **Detail limit** | ❌ Grid resolution | ✅ Infinite (analytical) |
| **Grid artifacts** | ⚠️ Slight | ✅ None |
| **Rust ecosystem** | ✅ Many libs | ❌ Few libs |
| **Deterministic** | ✅ Yes | ⚠️ Harder |

---

## FOR YOUR SPECIFIC REQUIREMENTS

### ✅ Volumetric (caves, tunnels, overhangs, interiors)
- **Voxels:** ✅ Yes
- **SDF:** ✅ Yes
- **Winner:** Tie

### ✅ Organic appearance (NOT blocky, irregular natural)
- **Voxels:** ⚠️ Good with smooth extraction (not perfect)
- **SDF:** ✅ Excellent (truly smooth)
- **Winner:** SDF (but voxels "good enough"?)

### ✅ Detailed at all scales (aerial → cave detail)
- **Voxels:** ⚠️ Limited by grid resolution, need LOD
- **SDF:** ✅ Analytical detail, or sampled at needed resolution
- **Winner:** SDF (but voxels with LOD can work)

### ✅ Modifiable (players can dig, build)
- **Voxels:** ✅ Trivial (set voxel = material)
- **SDF:** ❌ Complex (recompute distance field)
- **Winner:** Voxels (clear win)

### ✅ Real-world data integration (SRTM, OSM)
- **Voxels:** ✅ Direct mapping
- **SDF:** ❌ Conversion required
- **Winner:** Voxels (clear win)

### ✅ Earth-scale (6,371km radius)
- **Voxels:** ⚠️ Octree compression required
- **SDF:** ⚠️ Sampled or procedural (also needs compression)
- **Winner:** Tie (both challenging)

### ✅ Network synchronizable (P2P, deterministic)
- **Voxels:** ✅ Easy (discrete ops)
- **SDF:** ❌ Hard (floating point determinism)
- **Winner:** Voxels (clear win)

---

## RECOMMENDATION

### For This Project: **Voxels + Smooth Mesh**

**Why:**
1. **Real-world data integration is critical** (SRTM, OSM)
   - Voxels = direct, SDF = conversion complexity
2. **Player modification is core feature** (dig, build)
   - Voxels = simple, SDF = complex
3. **Network sync must be deterministic** (P2P)
   - Voxels = discrete operations, SDF = float consistency issues
4. **Implementation risk** (solo dev, new project)
   - Voxels = proven, many examples
   - SDF = research-level complexity

**Voxels are "good enough" for organic:**
- Marching cubes produces smooth surfaces
- Grid artifacts minimal at 0.5-1m resolution
- Reference images (Astroneer, 7 Days to Die) prove it works

**SDF would be better IF:**
- You had a team with SDF expertise
- Perfect smoothness was critical (it's not - caves are jagged)
- You didn't need real-world data integration
- You didn't need player modification
- You had 6+ months for custom renderer

---

## HYBRID APPROACH (Future Optimization)

**Possible later:**
- Use voxels as canonical storage
- Generate SDF from voxels for rendering
- Best of both: discrete storage, smooth rendering
- But adds complexity (defer until proven needed)

---

## NEXT STEPS (If Voxels Chosen)

1. ✅ Foundation research (coordinate system, SRTM) - DONE
2. ⏳ Define voxel data structure
3. ⏳ SRTM → voxel generation (fill terrain)
4. ⏳ Smooth mesh extraction algorithm (marching cubes or dual contouring)
5. ⏳ Test at Kangaroo Point Cliffs
6. ⏳ Validate organic appearance vs reference images

---

## DECISION NEEDED

**Do you agree with Voxels + Smooth Mesh?**

Or do you want to take the SDF path despite complexity?

