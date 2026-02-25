# World Depth Boundaries - Physical vs Simulated

**Last Updated:** 2026-02-17  
**Purpose:** Clarify what we simulate vs what we mathematically define

---

## YOUR CLAY ANALOGY

**Physical Earth (basketball-sized clay model):**
- Golf ball = Inner core (solid iron)
- White clay to baseball = Outer core (liquid)
- Cheap clay = Mantle (solid but plastic)
- Thin high-quality clay = Crust (rigid, brittle)
- Surface detail = Mountains, oceans, trees

**Question:** Do we code all those layers, or does the math start somewhere?

---

## THE ANSWER: We Define Boundaries, Not Full Simulation

### What TECH_SPEC.md Already Says (Line 202-210):

Each chunk is a **vertical column** from core to space:

```
Deep space:      35,786km+     (Moon, celestial - skybox mostly)
Space:           100km-35,786  (satellites, ISS - very sparse)
Atmosphere:      500m-100km    (weather, clouds - procedural)
Above ground:    0m-500m       (buildings, bridges - OSM + SVO)
Surface:         ~0m           (terrain mesh from SRTM)
Near underground: -200m to 0m  (tunnels, subways, basements - OSM + SVO)
Deep underground: -6,371km to -200m (solid rock - UNIFORM SVO)
```

**Key insight from line 210:**
> "Most chunks will only have detailed SVO data near the surface. Deep rock and atmosphere are represented as uniform SVO nodes (extremely cheap)."

---

## WHAT THIS MEANS IN PRACTICE

### 1. The Core is NOT Simulated

**ECEF coordinate origin (0,0,0):**
- Mathematical point at "center of Earth"
- Used for calculations
- **NO VOXELS STORED THERE**

**Why we don't simulate the core:**
- Players can't go there
- Doesn't affect gameplay
- Gravity = constant (9.81 m/s²) not calculated from mass
- Magnetic field = procedural, not from actual iron core

### 2. The "Generation Floor" is Dynamic

**Near surface (±200m):**
- DETAILED simulation
- Every cubic meter can be different
- SVO stores actual voxel data
- Materials: dirt, rock, concrete, water, air

**Deep underground (-200m to -6,371km):**
- UNIFORM representation
- One SVO node says "this entire 1km³ volume is solid basalt"
- Not stored per-voxel (would be ~10²¹ voxels!)
- Can drill through it, but it's procedurally generated (not pre-stored)

**Atmosphere (500m to 100km):**
- PROCEDURAL (clouds, weather)
- Not voxels at all (too sparse)
- Particle systems, volume textures

### 3. The "Surface" Reference Point

**SRTM elevation data:**
- Measured from **geoid** (mean sea level)
- Sea level = 0m elevation
- But geoid ≠ WGS84 ellipsoid (varies ±100m globally)

**Our zero point:**
- WGS84 ellipsoid surface (defined mathematically)
- Add SRTM elevation to get actual terrain height
- Example: Mount Everest is ~8,849m above ellipsoid

**Below "sea level":**
- Death Valley: -86m (below sea level, but still has terrain)
- Mariana Trench: -10,994m (ocean floor)
- These are still "surface" - the interface between solid and fluid/air

---

## THE DIRT-TO-TREE EXAMPLE

**"Tree on dirt, dirt on rock, water table, bedrock to core"**

### What We Actually Store:

```
+500m: Air (voxel = AIR material)
   |
+2m:  Tree trunk (voxel = WOOD material, generated from OSM tree node)
   |
+0m:  Dirt surface (voxel = SOIL material, top of SRTM terrain)
   |
-1m:  Dirt (voxel = SOIL, procedurally generated with variation)
   |
-5m:  Clay layer (voxel = CLAY, procedural based on geology data)
   |
-10m: Water table (voxel = WATER in porous rock)
   |
-50m: Bedrock (voxel = GRANITE, procedural)
   |
-200m: === DETAIL BOUNDARY ===
   |
-1km: Deep rock (ONE SVO node = "1km³ of basalt")
   |
-10km: Moho discontinuity (still ONE SVO node per large volume)
   |
-2900km: Core-mantle boundary (still uniform representation)
   |
-6371km: Center of Earth (NO VOXELS - just coordinate origin)
```

### Generation Logic:

**Does the math start somewhere and go up?**
YES and NO:

**For rendering/gameplay (surface ±200m):**
```rust
fn generate_surface_voxels(x, y, z) {
    if z > surface_elevation {
        return Air;  // Above ground
    }
    
    let depth_below_surface = surface_elevation - z;
    
    if depth_below_surface < 0.5 {
        return Soil;  // Top layer
    } else if depth_below_surface < 10.0 {
        return procedural_subsoil(x, y, z);  // Varied layers
    } else if depth_below_surface < 50.0 {
        return procedural_rock(x, y, z);  // Bedrock
    } else {
        return UniformRock;  // Beyond detail threshold
    }
}
```

**For deep volumes:**
```rust
fn get_deep_voxel(x, y, z) {
    // Don't store individual voxels
    // Just return material type for entire region
    let distance_from_center = sqrt(x² + y² + z²);
    
    if distance_from_center > 6371e3 - 200 {
        return detailed_near_surface(x, y, z);  // Actually simulate
    } else if distance_from_center > 3480e3 {
        return Mantle;  // Uniform - no detail needed
    } else if distance_from_center > 1220e3 {
        return OuterCore;  // Uniform
    } else {
        return InnerCore;  // Uniform
    }
}
```

---

## WHY THIS WORKS

### 1. Sparse Voxel Octree (SVO) is Sparse

**Empty/uniform = not stored:**
- 1km³ of solid basalt = ONE octree node
- 1m³ of varied cave rock = 1,000,000,000 leaf voxels

**Storage:**
- Deep uniform mantle: ~1 KB per 1000km³
- Detailed surface cave: ~10 MB per 1km³

### 2. Players Only Interact with Surface

**Deepest human-made structure:**
- Kola Superdeep Borehole: -12.3km
- Mariana Trench: -10.9km
- Most gameplay: ±100m from surface

**So we only NEED detail near surface.**

### 3. Procedural Generation Below Detail Threshold

**If a player digs deeper than -200m:**
- Generate rock procedurally (Perlin noise, geology rules)
- Create it on-demand (not pre-stored)
- Still consistent (same seed = same result)
- Still interactive (can mine it)

**But we don't PRE-GENERATE or STORE it.**

---

## THE BASKETBALL ANALOGY ANSWER

**In clay model:** You physically build all layers from core to surface.

**In digital model:** 
- You DEFINE all layers mathematically
- You STORE only what players can interact with
- You GENERATE detail on-demand when needed
- You IMPLY the rest (uniform SVO nodes)

**The math doesn't "start" at surface and go up.**
**The math spans core to space, but DETAIL STORAGE is ±200m from surface.**

---

## PRACTICAL IMPLICATIONS

### For Coordinate System (Current Work):
- ECEF origin at Earth center is MATH, not storage
- Coordinates work from core to space
- But chunks only have detailed voxels near surface

### For SRTM Data:
- SRTM gives us SURFACE elevation
- That's where detail STARTS
- Below surface: procedural based on geology
- Above surface: buildings from OSM, air otherwise

### For SVO Implementation:
- Root node spans core to space
- Subdivides only where detail needed
- Near surface: subdivide to 1m³ voxels
- Deep rock: stay at 1km³ or larger nodes
- Empty air/uniform rock: stay at huge nodes

### For Testing:
- Test coordinate conversions at all scales (core to space)
- Test voxel generation only near surface (±200m)
- Test that deep uniform regions work (don't crash, minimal storage)

---

## ANALOGY UPDATE

**Better than clay:**

It's like a **hologram on a sphere**.

- The sphere EXISTS mathematically (coordinate system)
- The hologram (detail) EXISTS only where you look (surface ±200m)
- The interior is IMPLIED but not RENDERED (uniform SVO)
- If you drill down, the hologram extends deeper (procedural generation)

**Or like a video game skybox:**
- Mountains in distance LOOK 3D
- But they're just a texture on a dome
- If you walk toward them, they become REAL geometry
- Same here: deep Earth is "skybox" until you dig there

---

## QUESTION ANSWERED

**"Is everything below the crust coded, or not even coded?"**

**Answer:** 
It's DEFINED (coordinate system knows it exists)
It's NOT STORED (SVO doesn't have voxels there)
It's GENERATED on-demand (if you dig, it creates detail)

**"Does the math at some point say 'generation starts here and goes up'?"**

**Answer:**
The math works from core to space (coordinate system is complete)
The DETAIL generation starts at surface ±200m
The STORAGE is sparse (only store what has variation)

**So YES:** Generation has a "detail threshold" around the surface.
**But NO:** The coordinate system and physics work everywhere.

It's both! Foundation is complete, detail is local.

