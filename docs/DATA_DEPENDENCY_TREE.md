# Data Dependency Tree (Reverse Engineered)

**Last Updated:** 2026-02-17  
**Purpose:** Work backwards from USE CASES to understand what data we ACTUALLY need and why

**Method:** For each thing a player does, trace backwards through ALL dependencies until we hit either:
1. ✅ Base data we can acquire
2. ❌ Data we must generate (and why we can/can't)

---

## METHODOLOGY

```
USE CASE (what player does)
  ├─ REQUIREMENT 1 (what's needed for that)
  │   ├─ SUB-REQUIREMENT 1a
  │   │   ├─ BASE DATA (✅ have it / ❌ must generate)
  │   │   └─ RULE (how to generate if missing)
  │   └─ SUB-REQUIREMENT 1b
  │       └─ ...
  └─ REQUIREMENT 2
      └─ ...
```

---

## USE CASE 1: Player walks across a field in rural England

**Location:** 51.5°N, 0.1°W (example: countryside outside London)  
**Expected experience:** Walking on grass, see trees, realistic terrain, no buildings

### Backward trace:

**Player walks on grass**
├─ **Need: Foot collision with ground**
│   ├─ Need: Ground height at exact position (x, y) → z
│   │   ├─ Need: Terrain elevation data
│   │   │   ├─ ✅ BASE: SRTM 30m elevation for that tile
│   │   │   │   └─ Question: Is 30m resolution enough for walking?
│   │   │   │       └─ Answer: NO - player feet are ~0.3m apart, 30m is 100× too coarse
│   │   │   └─ ❌ MUST GENERATE: Sub-meter detail
│   │   │       ├─ Method: Fractal subdivision
│   │   │       ├─ Input needed: 30m heights at 4 corners of area
│   │   │       ├─ Constraints needed: 
│   │   │       │   ├─ Geology type (determines roughness frequency)
│   │   │       │   │   └─ Where from? → Macrostrat API
│   │   │       │   ├─ Slope (steep = more variation)
│   │   │       │   │   └─ Where from? → Calculated from SRTM gradient
│   │   │       │   └─ Land cover (grass is smoother than rocks)
│   │   │       │       └─ Where from? → ESA WorldCover 10m
│   │   │       └─ Seed needed: Deterministic based on position
│   │   │           └─ How? → Hash of (x, y) chunk coordinates
│   │   └─ Need: Collision mesh (physics)
│   │       ├─ Can be lower detail than visual (performance)
│   │       └─ Simplified from visual mesh
│   └─ Need: Walkable surface (not too steep)
│       └─ Rule: Slope < 45° = walkable, else climbing mode
│
├─ **Need: Visual appearance of grass**
│   ├─ Need: Grass rendering
│   │   ├─ LOD Far (>100m): Texture only
│   │   │   └─ Texture: Which grass texture? → Based on climate/biome
│   │   ├─ LOD Mid (10-100m): Billboard grass patches
│   │   │   └─ Placement: Where to put grass patches?
│   │   │       └─ Noise-based distribution (not uniform)
│   │   └─ LOD Near (<10m): Individual grass blades (geometry)
│   │       ├─ Grass density: How many blades per m²?
│   │       │   └─ Based on: Season, rainfall, soil quality
│   │       └─ Grass height: How tall?
│   │           └─ Based on: Season (short in winter, tall in summer)
│   │
│   ├─ Need: What TYPE of grass grows here?
│   │   └─ Depends on: Climate zone
│   │       ├─ ✅ BASE: Climate data (temperature, rainfall) for 51.5°N, 0.1°W
│   │       │   └─ Source: WorldClim / NOAA climate database
│   │       └─ LOOKUP: Grass species for temperate oceanic climate
│   │           └─ Database needed: Species ↔ Climate mapping
│   │               └─ Example: Perennial ryegrass (Lolium perenne) common in UK
│   │
│   └─ Need: Current grass state (color, height)
│       └─ Depends on: Current date/season
│           ├─ June: Green, tall (growing season)
│           └─ January: Brown/yellow, short (dormant)
│
├─ **Need: See trees in distance**
│   ├─ Need: Tree positions (where to place)
│   │   ├─ ✅ BASE: ESA WorldCover 10m → "Tree cover" class at this location?
│   │   │   └─ Check: Does pixel at 51.5°N, 0.1°W = "tree cover"?
│   │   │       └─ If YES: Generate trees
│   │   │       └─ If NO: Open field, no trees
│   │   │
│   │   └─ If tree cover present:
│   │       ├─ ❌ MUST GENERATE: Individual tree positions
│   │       │   ├─ Method: Poisson disk sampling
│   │       │   │   ├─ Density: From WorldCover "percentage tree cover"
│   │       │   │   │   └─ Example: 60% cover = ~0.6 trees per 10m²
│   │       │   │   ├─ Min spacing: Species-dependent
│   │       │   │   │   └─ Oak trees: ~8m spacing
│   │       │   │   └─ Seed: Hash(x, y, chunk_id)
│   │       │   │
│   │       │   ├─ Constraints: Avoid roads, buildings
│   │       │   │   ├─ ✅ BASE: OSM roads for this area
│   │       │   │   └─ ✅ BASE: OSM buildings for this area
│   │       │   │
│   │       │   └─ Output: List of (x, y, z) positions
│   │       │
│   │       └─ ❌ MUST DETERMINE: Tree species
│   │           └─ Depends on: Ecological zone
│   │               ├─ Input: Latitude, elevation, climate
│   │               │   └─ 51.5°N, <100m elevation, temperate oceanic
│   │               └─ LOOKUP: Species database
│   │                   └─ Likely species: English oak (Quercus robur), 
│   │                                      Ash (Fraxinus excelsior),
│   │                                      Beech (Fagus sylvatica)
│   │                   └─ Database needed: [Climate zone] → [Species list + probabilities]
│   │
│   ├─ Need: Tree size (height, crown width)
│   │   └─ ❌ MUST GENERATE: Size distribution
│   │       ├─ Species max size: Oak = 25-35m tall
│   │       └─ Age distribution: Most mature (80%), some young (15%), rare ancient (5%)
│   │           └─ Noise-based selection per tree
│   │
│   └─ Need: Tree appearance (leaves, bark)
│       ├─ Season: June = full green leaves
│       └─ 3D model: Load oak tree model (LOD appropriate)
│
├─ **Need: Terrain looks realistic (not flat/blocky)**
│   ├─ Visual mesh detail
│   │   └─ Same fractal subdivision as collision (reuse)
│   │
│   ├─ Texture: What does ground look like?
│   │   ├─ Base: Grass texture (from land cover)
│   │   ├─ Variation: Dirt patches where worn (path usage)
│   │   └─ Detail: Normal mapping for close views
│   │
│   └─ Lighting: Shadows from trees, sun angle
│       ├─ Sun position: Calculate from date/time/lat/lon
│       └─ Shadow casting: From tree positions
│
└─ **Need: No buildings (rural field)**
    └─ ✅ BASE: OSM buildings layer for this area
        └─ Check: Are there buildings at 51.5°N, 0.1°W?
            └─ If NO buildings in OSM: Don't generate any
            └─ If buildings present: Would need to render them

---

## DATA DEPENDENCIES IDENTIFIED

From this ONE use case (walk in field), we need:

### Absolute Requirements (can't work without):
1. ✅ **SRTM 30m elevation** — Ground height baseline
2. ✅ **ESA WorldCover 10m** — Is this grass/trees/water?
3. ❌ **Fractal subdivision algorithm** — 30m → <1m detail
4. ❌ **Grass species database** — Climate → grass type
5. ❌ **Tree species database** — Climate + ecology → tree species
6. ✅ **Climate data** — Temperature/rainfall for region
7. ✅ **Date/time** — What season is it?
8. ❌ **Physics collision mesh** — Player doesn't fall through

### Optional Enhancements:
9. ✅ **OSM roads** — Avoid placing trees on roads
10. ✅ **OSM buildings** — Avoid placing trees inside buildings
11. ⚠️ **Geology (Macrostrat)** — Constrains fractal roughness
12. ⚠️ **Soil data** — Affects grass quality/density

---

## CRITICAL QUESTION: Can we generate the fractal terrain?

**Working backwards:**

To generate sub-meter terrain detail, we need:
1. **Input heights** (30m SRTM) ✅ Have it
2. **Subdivision algorithm** (fractal noise) ✅ Can implement
3. **Constraints:**
   - Geology type → roughness frequency
     - ✅ Macrostrat API available
   - Slope → variation amount
     - ✅ Calculate from SRTM gradient
   - Land cover → smoothness
     - ✅ WorldCover available
4. **Deterministic seed** ✅ Hash(x, y)
5. **Reference to validate against** ⚠️ **THIS IS THE PROBLEM**

**What validates that our fractal terrain is "correct"?**

For a random field in England:
- ❌ We have NO ground truth at <30m resolution
- ❌ We have NO photos of that exact spot
- ❌ We have NO LiDAR data for most locations
- ❓ How do we know if it's "realistic"?

**Answer:** We CAN'T validate arbitrary locations. We can only:
1. **Validate at locations with ground truth:**
   - Cities with LiDAR data
   - Famous landmarks with photos
   - Test locations we can physically visit
2. **Validate statistically:**
   - Does roughness distribution match real terrain?
   - Do slope histograms look realistic?
   - Does it "feel" right to players?

**Conclusion:** We can build the fractal system, but validation is LIMITED to special cases.

---

## USE CASE 2: Player stands at Kangaroo Point Cliffs, Brisbane

**Location:** -27.4775°S, 153.0355°E  
**Expected:** See vertical cliff face, river below, parkland  
**Reference:** We have Google Earth photo, this is our GROUND TRUTH

### Backward trace:

**Player sees cliff**
├─ **Need: Vertical cliff geometry (not stairstep blocks)**
│   ├─ PROBLEM: SRTM only shows TOP of cliff (nadir view)
│   │   └─ ✅ BASE: SRTM shows elevation change -27.4775°S (cliff top ~30m) to river (0m)
│   │       └─ Horizontal distance: ~50m → Slope = 30m/50m = 60° (steep!)
│   │
│   ├─ ❌ MUST INFER: Cliff is VERTICAL, not sloped
│   │   └─ Detection rule: Slope > 70° → likely vertical cliff
│   │       ├─ Generate vertical face from top elevation to bottom
│   │       └─ Question: How do we know bottom elevation?
│   │           └─ Trace downslope until slope < 45° (cliff base)
│   │
│   ├─ ❌ MUST GENERATE: Cliff face detail
│   │   ├─ Not just flat wall — needs rock texture, ledges, cracks
│   │   ├─ Method: Fractal detail on vertical surface
│   │   │   ├─ Constrained by: Geology type
│   │   │   │   └─ ✅ Macrostrat: Brisbane River Formation (sandstone)
│   │   │   │       └─ Sandstone: Horizontal layering, erosion patterns
│   │   │   ├─ Ledges: From sedimentary layers (every 1-3m)
│   │   │   └─ Erosion: Weather-side (east) more eroded
│   │   │       └─ ✅ Wind direction from climate data
│   │   │
│   │   └─ VALIDATION: Compare generated cliff to Google Earth photo
│   │       └─ Does it have the right:
│   │           ├─ Vertical angle? (yes/no)
│   │           ├─ Layering? (horizontal bands visible)
│   │           ├─ Overall shape? (cliff outline matches)
│   │           └─ Color? (sandstone tan/brown)
│   │
│   └─ **THIS is our test case** — We have reference photo
│
├─ **Need: River below**
│   ├─ ✅ BASE: WorldCover shows "water" class
│   ├─ ✅ BASE: SRTM shows elevation ~0m (sea level)
│   ├─ ✅ BASE: OSM has Brisbane River geometry
│   └─ Need: Water surface rendering
│       └─ Water level = 0m (tidal)
│
├─ **Need: Parkland on cliff top**
│   ├─ ✅ BASE: WorldCover shows "grassland" class on cliff top
│   ├─ ✅ BASE: OSM shows "park" tag (Kangaroo Point Cliffs Park)
│   └─ Generate: Grass + scattered trees (same as USE CASE 1)
│
└─ **Need: Correct from multiple viewpoints**
    ├─ View from river: See cliff face rising above
    ├─ View from cliff top: See river below
    └─ Both must be consistent (same geometry)

---

## DATA DEPENDENCIES FOR CLIFF TEST CASE

### Essential:
1. ✅ **SRTM 30m elevation** — Detects cliff (steep slope)
2. ✅ **Macrostrat geology** — Sandstone → layering rules
3. ❌ **Cliff detection algorithm** — Slope threshold
4. ❌ **Vertical face generation** — Extrude from top to base
5. ❌ **Rock face fractal detail** — Layering + erosion
6. ✅ **Google Earth reference photo** — VALIDATION

### This is testable because:
- ✅ We have ground truth (photo)
- ✅ Specific location (not generic)
- ✅ Known geology (Brisbane River Formation)
- ✅ Can compare generated vs real

**This should be TEST CASE #1 for terrain generation.**

---

## USE CASE 3: Player enters a building in Manhattan, NYC

**Location:** 40.7580°N, -73.9855°W (Times Square area)  
**Expected:** Skyscrapers, can enter lobby, interior space

### Backward trace:

**Player sees building**
├─ **Need: Building exists at this location**
│   ├─ ✅ BASE: OSM buildings layer
│   │   └─ Query: Buildings at 40.7580°N, -73.9855°W?
│   │       └─ Result: Likely YES (Manhattan is well-mapped in OSM)
│   │           └─ OSM completeness NYC: ~90%+
│   │
│   ├─ If OSM has building:
│   │   ├─ ✅ Have: Footprint polygon
│   │   ├─ ⚠️ Maybe have: building:levels tag
│   │   └─ ❌ Don't have: 3D geometry, facade detail
│   │
│   └─ If OSM missing building:
│       └─ ❌ MUST DETECT: Building from satellite imagery
│           ├─ ✅ BASE: Sentinel-2 10m imagery
│           ├─ Method: ML building detection
│           │   └─ Trained model needed: Image → footprint polygon
│           └─ Supplement: Microsoft/Google building footprints
│
├─ **Need: Building height (skyscraper vs low-rise)**
│   ├─ Check OSM: building:levels tag
│   │   ├─ If present: Height = levels × 3.5m (typical floor height)
│   │   └─ If missing: ❌ MUST INFER
│   │
│   └─ ❌ INFERENCE methods:
│       ├─ **Shadow analysis:**
│       │   ├─ ✅ BASE: Sentinel-2 imagery (has shadows)
│       │   ├─ Measure: Shadow length from building
│       │   ├─ Calculate: Shadow length = height × tan(sun_angle)
│       │   │   └─ Sun angle from: Date/time of image
│       │   └─ Accuracy: ±2-5m (good enough)
│       │
│       ├─ **Footprint size correlation:**
│       │   ├─ Statistical: Large footprint = likely taller
│       │   ├─ NYC pattern: >500m² footprint → 10+ floors likely
│       │   └─ Accuracy: Rough estimate
│       │
│       └─ **Regional patterns:**
│           ├─ Times Square area: Mostly 20-60 floors
│           ├─ Use nearest known heights as baseline
│           └─ Accuracy: Approximate
│       
│   └─ **VALIDATION:** Compare to known heights
│       ├─ ✅ Reference: NYC open data (building heights for many buildings)
│       └─ Test: Does inference match known data?
│
├─ **Need: Building facade (windows, entrance)**
│   ├─ ❌ MUST GENERATE: Procedural facade
│   │   ├─ Window spacing: Every 3m vertically, 3-4m horizontally
│   │   ├─ Window size: Commercial = larger (2m×2m)
│   │   ├─ Ground floor: Larger windows, glass (shops/lobby)
│   │   ├─ Entrance: Center of street-facing wall
│   │   └─ Materials: Glass + steel (modern) or brick (older)
│   │
│   └─ **Alternative:** Photo texture from Street View
│       ├─ ✅ Google Street View API (paid)
│       └─ Extract facade texture from photos
│
└─ **Player enters building**
    ├─ **Need: Door location**
    │   ├─ Ground floor, street-facing wall
    │   └─ Usually centered or at corner
    │
    ├─ **Need: Interior space (lobby)**
    │   ├─ ❌ NO DATA: Interiors don't exist in any dataset
    │   └─ ❌ MUST GENERATE: Procedural interior
    │       ├─ **Phase 1 (minimal):** Single room (lobby template)
    │       │   ├─ Size: Based on footprint (10% of ground floor)
    │       │   ├─ Generic furniture: Desk, chairs, elevator
    │       │   └─ Placeholder for owner customization
    │       │
    │       └─ **Phase 2 (later):** Full interior simulation
    │           ├─ Floor plan from footprint + building codes
    │           ├─ Elevator shafts, stairwells
    │           ├─ Office/residential layout per floor
    │           └─ Procedural furniture placement
    │
    └─ **VALIDATION:** 
        └─ ⚠️ Can't validate interior (no ground truth exists)
        └─ Only test: Does it not crash? Does it feel plausible?

---

## DATA DEPENDENCIES FOR NYC BUILDING TEST

### Essential:
1. ✅ **OSM building footprints** (NYC: ~90% complete)
2. ⚠️ **OSM building:levels tag** (partial)
3. ✅ **Sentinel-2 imagery** (shadow analysis)
4. ❌ **Height inference algorithm** (shadow length → height)
5. ❌ **Procedural facade generator**
6. ❌ **Interior template system** (minimal lobby)
7. ✅ **NYC building heights dataset** (validation)

### Testable because:
- ✅ High OSM coverage (90%+)
- ✅ Validation data exists (NYC open data)
- ✅ Can compare inferred heights to real
- ❌ Can't validate interiors (no ground truth)

**This should be TEST CASE #2 for building generation.**

---

## PATTERN RECOGNITION

From these 3 use cases, the pattern is:

```
PLAYER ACTION
  ↓
VISUAL/PHYSICAL REQUIREMENTS
  ↓
BASE DATA (what we have)
  ↓
DATA GAPS (what's missing)
  ↓
GENERATION RULES (how to fill gaps)
  ↓
VALIDATION (how to test it's correct)
```

**Critical insight:** We can only validate where we have GROUND TRUTH.

### Locations with ground truth:
1. **Famous landmarks** (photos exist)
2. **LiDAR-scanned cities** (3D data exists)
3. **Well-documented locations** (field surveys)
4. **Locations we can visit** (direct observation)

### Locations WITHOUT ground truth (most of Earth):
- ❌ Can't validate correctness
- ✅ Can validate plausibility (does it look realistic?)
- ✅ Can validate consistency (deterministic generation)
- ✅ Can validate gameplay (does it work/feel right?)

---

## RULE DEPENDENCY TREE

Now let's trace what RULES depend on what DATA:

### Rule: Fractal terrain subdivision
```
INPUTS:
  - Base elevation (SRTM 30m) ✅
  - Geology type (Macrostrat) ✅
  - Slope (calculated from SRTM) ✅
  - Land cover (WorldCover) ✅
  - Seed (hash of coordinates) ✅
  
OUTPUT:
  - Sub-meter terrain mesh
  
PARAMETERS TO DETERMINE:
  - Fractal algorithm (Diamond-square? Perlin? Simplex?)
  - Frequency scaling (how rough?)
  - Amplitude scaling (how much variation?)
  - Constraints (don't violate known heights)
  
VALIDATION:
  - ✅ Test at Kangaroo Point Cliffs (have photo reference)
  - ⚠️ Can't validate everywhere else
```

### Rule: Tree placement
```
INPUTS:
  - Land cover "tree" class (WorldCover) ✅
  - Tree density (from coverage %) ✅
  - Climate zone (from climate data) ✅
  - Constraints (avoid roads/buildings) ✅
  - Seed (hash of coordinates) ✅
  
OUTPUT:
  - List of tree positions (x, y, z)
  
PARAMETERS TO DETERMINE:
  - Poisson disk min spacing (species-dependent)
  - Clustering factor (trees aren't uniform)
  - Age distribution (young vs mature)
  
DATABASE NEEDED:
  - Climate zone → Species list + probabilities
  - Species → Size, spacing, appearance
  
VALIDATION:
  - ⚠️ Visual comparison to satellite imagery
  - ⚠️ Density matches real forests (statistical)
  - ❌ Can't validate individual tree positions
```

### Rule: Building height inference
```
INPUTS:
  - Building footprint (OSM) ✅
  - Satellite imagery (Sentinel-2) ✅
  - Sun angle at image time ✅
  - Regional building data ✅
  
OUTPUT:
  - Building height (meters)
  
ALGORITHM:
  - Measure shadow length in image
  - Calculate height = shadow_length / tan(sun_angle)
  - Cross-check with regional patterns
  - Fallback to footprint size correlation
  
VALIDATION:
  - ✅ NYC: Compare to open data building heights
  - ✅ Other cities with ground truth
  - ❌ Can't validate where no ground truth exists
```

---

## WHAT WE ACTUALLY NEED TO BUILD (IN ORDER)

### Phase 0: Foundation Data Acquisition
1. Download SRTM 30m global elevation (or just test regions)
2. Download ESA WorldCover 10m land cover (or just test regions)
3. Download OSM data for test regions (Kangaroo Point, NYC)
4. Download Sentinel-2 imagery for test regions
5. Setup Macrostrat API access (geology)
6. Setup climate data access (WorldClim)

### Phase 1: Test Case #1 - Kangaroo Point Cliffs
**Goal:** Generate cliff that looks like reference photo

Build (in order):
1. **Cliff detection algorithm**
   - Input: SRTM 30m elevation
   - Output: Slope map, identify slopes > 70°
   
2. **Vertical face generation**
   - Input: Cliff top elevation, cliff base elevation
   - Output: Vertical mesh connecting top to bottom
   
3. **Rock face detail (fractal)**
   - Input: Vertical face + geology type (sandstone)
   - Output: Detailed cliff face with layers
   - Constraints: Horizontal layering (sedimentary)
   
4. **Render and compare**
   - Generate mesh
   - Position camera at reference photo viewpoint
   - Take screenshot
   - Compare to Google Earth reference
   
5. **Iterate until match**
   - Adjust fractal parameters
   - Adjust layering rules
   - Adjust erosion patterns
   - Re-test

**Success criteria:** Generated cliff visually matches reference photo

### Phase 2: Test Case #2 - NYC Building Heights
**Goal:** Infer building heights and validate against ground truth

Build (in order):
1. **Shadow detection algorithm**
   - Input: Sentinel-2 imagery + building footprints
   - Output: Shadow polygons for each building
   
2. **Shadow length measurement**
   - Input: Shadow polygon + sun angle
   - Output: Shadow length (meters)
   
3. **Height calculation**
   - Input: Shadow length + sun angle
   - Output: Building height estimate
   
4. **Validation test**
   - Run on NYC buildings with known heights
   - Compare estimated vs actual
   - Measure error distribution
   
5. **Refine algorithm**
   - Adjust for shadow occlusion
   - Handle multiple buildings casting shadows
   - Fallback methods when shadows unclear

**Success criteria:** Height estimates within ±5m of ground truth for 80% of buildings

### Phase 3: Test Case #3 - Forest Tree Distribution
**Goal:** Generate realistic forest that matches satellite appearance

Build (in order):
1. **Species database**
   - Create: Climate zone → Species mapping
   - Sources: Ecological literature, forestry data
   
2. **Tree placement algorithm**
   - Input: Tree cover density + climate + seed
   - Output: Tree positions
   - Method: Poisson disk sampling with clustering
   
3. **Visual validation**
   - Generate forest for test location
   - Compare to satellite imagery (visual pattern match)
   - Compare density to real forest samples
   
4. **Iterate parameters**
   - Adjust spacing
   - Adjust clustering
   - Adjust size distribution

**Success criteria:** Forest looks realistic, density matches satellite observations

---

## SUMMARY: WHAT WE LEARNED

1. **We need GROUND TRUTH locations for testing**
   - Kangaroo Point Cliffs (terrain)
   - NYC (buildings)
   - Forest regions with LiDAR (trees)

2. **Most of Earth has NO ground truth**
   - Can only validate plausibility, not correctness
   - Rules must be robust from limited test cases

3. **Data dependencies are DEEP**
   - Each feature needs 5-10 data sources
   - Rules need parameters we must research/calibrate
   - Databases needed for many lookups (species, materials, etc.)

4. **We must build INCREMENTALLY**
   - One test case at a time
   - Validate each before moving on
   - Can't validate everything, accept uncertainty

5. **The real work is RULE ENGINEERING**
   - Base data exists (30m elevation, 10m land cover)
   - Algorithms are known (fractal subdivision, Poisson disk)
   - The HARD part: Calibrating parameters, building databases, integrating constraints

**We now know EXACTLY what to build first: Kangaroo Point Cliffs cliff generation.**
