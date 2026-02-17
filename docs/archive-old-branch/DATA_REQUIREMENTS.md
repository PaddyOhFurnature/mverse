# Data Requirements & Generation Rules

**Last Updated:** 2026-02-17  
**Purpose:** Work backwards from metaverse requirements to define what data we NEED and how to create it

This document defines requirements-driven data needs, not availability-driven cataloging.

---

## APPROACH

```
FEATURE → DATA NEEDED → SOURCES + RULES → VALIDATION
```

For each capability, define:
1. **What it needs to work** (minimum viable data)
2. **Where we get base data** (real sources)
3. **What rules generate the rest** (inference/simulation)
4. **How we test it's good enough** (validation)

---

## 1. PLAYER MOVEMENT & COLLISION

**What players need to do:** Walk, run, drive, fly anywhere on Earth

### 1.1 Walking/Running on Ground

**NEEDS:**
- Solid ground surface to stand on
- Collision mesh for terrain
- Walkable vs non-walkable areas
- Slope limits (can't walk up cliffs)

**BASE DATA:**
- ✅ SRTM 30m elevation → ground height
- ✅ WorldCover 10m → surface type (grass, rock, water, etc.)

**GENERATION RULES:**
- **Detail mesh:** Fractal subdivide 30m→1m based on:
  - Slope (steep = more detail needed)
  - Surface type (rock = rougher than grass)
  - Noise frequency varies by geology
- **Collision simplification:** Physics mesh can be lower detail than visual
- **Walkability:** 
  - Slope > 45° = requires climbing
  - Water depth > 0.5m = swimming
  - Buildings = interior if door exists, else blocked

**VALIDATION:**
- Player doesn't fall through terrain
- Movement feels natural on different surfaces
- No obvious grid artifacts from 30m source

---

### 1.2 Swimming/Water Bodies

**NEEDS:**
- Water surface level
- Underwater terrain
- Current/flow direction (rivers)

**BASE DATA:**
- ✅ WorldCover → water body locations
- ✅ SRTM → riverbed elevation
- ✅ OSM → river flow direction tags
- ✅ GEBCO → ocean bathymetry

**GENERATION RULES:**
- **Lake/reservoir level:** Find lowest outlet elevation, fill to that height
- **River flow:** Gradient descent from source to mouth
- **Ocean depth:** GEBCO 500m data, interpolate to shore
- **Underwater detail:** Same fractal rules as land terrain

**VALIDATION:**
- Water flows downhill
- Lakes don't overflow basins
- Ocean shore transitions smoothly

---

### 1.3 Climbing/Vertical Movement

**NEEDS:**
- Climbable surface detection
- Handhold/foothold locations
- Overhang geometry

**BASE DATA:**
- ✅ SRTM elevation (but only top-down view!)
- ⚠️ LiDAR where available (has overhang data)

**GENERATION RULES:**
- **Cliff detection:** Slope > 70° from DEM
- **Cliff face geometry:** 
  - Vertical extrusion from top elevation to base
  - Fractal detail for rock face texture
  - Ledges from geological layer simulation
- **Handholds:** Procedural based on rock type
  - Granite: cracks, crystals
  - Sandstone: layered ledges
  - Limestone: solution pockets

**VALIDATION:**
- Cliffs are vertical (not stairstep blocks)
- Climbing routes exist (not impossible smooth walls)
- Visual matches reference photos of real location

---

## 2. BUILDING INTERACTION

**What players need to do:** Enter buildings, interact with structures, modify/build

### 2.1 Building Exteriors

**NEEDS:**
- Building footprint (where it is)
- Building height (how tall)
- Roof geometry
- Facade detail (windows, doors, materials)
- Entrance locations

**BASE DATA:**
- ✅ OSM building footprints (21% global average)
- ⚠️ Microsoft/Google footprints (supplements OSM)
- ✅ Sentinel-2 10m imagery
- ⚠️ Google Street View (where available)

**GENERATION RULES:**

**A. Missing Building Detection**
- Input: Sentinel-2 imagery + existing OSM buildings
- Method: ML building detection (trained model)
- Output: Additional building footprints
- Validation: Visual inspection, cross-check with Google Earth

**B. Height Estimation**
- Input: Footprint + satellite imagery + region data
- Method: Priority order:
  1. OSM `building:levels` tag if exists
  2. Shadow length analysis (sun angle + shadow in imagery)
  3. Surrounding building heights (statistical average)
  4. Regional typical by building type (residential=2-3 floors, commercial=variable)
  5. Footprint size correlation (bigger = taller usually)
- Output: Height in meters
- Validation: Compare to known heights in test cities

**C. Roof Geometry**
- Input: Building footprint + region + climate
- Method: Regional style rules:
  - Cold climate: Steep pitched (snow shedding)
  - Hot climate: Flat (rooftop usage)
  - Historical districts: Local architectural style
  - Modern commercial: Flat
- Inference from imagery: Detect roof type from satellite view
- Output: Roof mesh
- Validation: Visual match to imagery

**D. Facade Detail**
- Input: Building height + width + type + region
- Method: Procedural texture:
  - Window spacing: Standard modules (every 3-4m)
  - Window size: Residential vs commercial patterns
  - Entrance: Ground floor, typically centered or corner
  - Materials: Climate + region (brick, concrete, wood, etc.)
- Photo textures: Extract from Street View where available
- Output: Facade texture + geometry
- Validation: Looks plausible, not obviously procedural

**E. Entrance Detection**
- Input: Footprint + street adjacency
- Method: 
  - Primary entrance on street-facing wall
  - Probability map from building type
  - Street View analysis (if available)
- Output: Door location(s)
- Validation: Can player reach entrance from street

---

### 2.2 Building Interiors (Deferred/Minimal)

**NEEDS (MINIMAL):**
- Entry portal (door works)
- Basic interior space (not empty void)
- Owner can customize

**BASE DATA:**
- ❌ None (almost no interior data exists)

**GENERATION RULES:**
- **Phase 1 (NOW):** Door = teleport to "interior template" (like Skyrim)
  - Small footprint = single room
  - Large footprint = multi-room procedural layout
  - Generic furniture placement
- **Phase 2 (LATER):** Architectural simulation
  - Floor plan from footprint + building code rules
  - Room function inference (bedroom, kitchen, etc.)
  - Furniture procedural placement
- **Phase 3 (EVENTUAL):** Player/owner customization
  - Override procedural with custom interior
  - Persistence per building
  - Networked to all players

**VALIDATION:**
- Can enter building (doesn't crash)
- Interior volume matches exterior footprint
- Looks "good enough" until owner customizes

---

## 3. VEGETATION & NATURAL FEATURES

**What players need to see:** Forests, trees, grass, rocks - organic world detail

### 3.1 Individual Trees

**NEEDS:**
- Tree locations (where to place)
- Tree species (what kind)
- Tree size/age
- Seasonal state (leaves, etc.)

**BASE DATA:**
- ✅ WorldCover 10m tree cover class
- ✅ Climate data (temperature, precipitation)
- ✅ Elevation data
- ✅ Soil type (from geology)

**GENERATION RULES:**

**A. Tree Density Map**
- Input: WorldCover "tree cover" pixels
- Method: Each 10m pixel has density value 0-100%
- Output: Trees per square meter probability

**B. Tree Placement**
- Input: Density map + terrain + constraints
- Method: Poisson disk sampling:
  - Min spacing = species typical (2-10m depending on tree type)
  - Avoid placement on: roads, buildings, rock outcrops
  - Cluster slightly (trees aren't perfectly random)
  - Seed from GPS coordinate (deterministic)
- Output: Tree position (x,y,z)

**C. Species Selection**
- Input: Location (lat/lon/elevation) + climate
- Method: Ecological zone lookup:
  - Tropical rainforest: Palm, mahogany, kapok, etc.
  - Temperate forest: Oak, maple, pine (by region)
  - Boreal: Spruce, fir, birch
  - Altitude rules: Different species by elevation band
- Database: Species climate preference table
- Output: Species ID

**D. Tree Size/Age**
- Input: Species + location
- Method: Statistical distribution:
  - Most trees = mature (80% of max size)
  - Some young (10-20% of max)
  - Some ancient (>100% max, rare)
  - Near roads/clearings: younger regrowth
- Output: Height, trunk diameter, crown size

**E. Seasonal State**
- Input: Species + date + hemisphere
- Method: Phenology simulation:
  - Deciduous: Bare (winter) → buds (spring) → full (summer) → colors (fall)
  - Evergreen: Always green
  - Tropical: May flower/fruit seasonally
- Output: Leaf state, color

**VALIDATION:**
- Forest density looks right (not too sparse/dense)
- Species match the region (no palm trees in Alaska)
- Size distribution realistic (not all same size)
- Seasonal changes happen
- Deterministic (same result for same location/date)

---

### 3.2 Grass & Undergrowth

**NEEDS:**
- Ground cover (not bare dirt everywhere)
- Grass detail (when close)
- Variety (not uniform texture)

**BASE DATA:**
- ✅ WorldCover → grassland class
- ✅ Climate → grass growth potential

**GENERATION RULES:**
- **Far view (>100m):** Texture only (grass texture on terrain)
- **Mid view (10-100m):** Grass patches (billboards/cards)
- **Close view (<10m):** Individual grass blades (geometry)
- **Variety:** Noise-based mixing of 3-5 grass types per biome
- **Height:** Climate + season (tall in summer, short in winter)
- **Wind:** Procedural animation (wave pattern)

**VALIDATION:**
- Looks like grass, not carpet
- Performance acceptable (LOD working)

---

### 3.3 Rocks & Boulders

**NEEDS:**
- Boulder placement (where)
- Size distribution
- Rock type appearance

**BASE DATA:**
- ✅ Terrain roughness (from DEM slope variance)
- ✅ Geology type (Macrostrat)

**GENERATION RULES:**
- **Rocky areas:** High slope, alpine zones, recent glaciation, volcanic
- **Density:** Proportional to terrain roughness + geology
- **Placement:** Noise-based with clustering
- **Size:** Power law distribution (many small, few huge)
- **Type:** From geology (granite, basalt, sandstone, etc.)
- **Orientation:** Random rotation, partially buried

**VALIDATION:**
- Boulders on mountains, not in flat plains
- Clustering looks natural
- Rock type matches terrain

---

## 4. INFRASTRUCTURE & URBAN FEATURES

### 4.1 Roads & Streets

**NEEDS:**
- Road network (connectivity)
- Road width
- Surface type (paved, dirt, etc.)
- Lane markings (eventually)

**BASE DATA:**
- ✅ OSM road network (~80% complete)
- ✅ GRIP roads (fills gaps)

**GENERATION RULES:**

**A. Missing Road Inference**
- Input: Known roads + buildings + terrain
- Method: Network completion:
  - Buildings far from roads likely have access
  - Generate road from building to nearest known road
  - Route follows terrain (minimize slope/distance cost)
- Output: Additional road segments
- Validation: Network is connected, routes are plausible

**B. Road Width**
- Input: OSM highway tag
- Method: Standard widths:
  - Motorway: 3.5m × lanes (typically 4-8 lanes)
  - Primary: 7-10m
  - Secondary: 6-8m
  - Residential: 5-6m
  - Track/path: 2-3m
- Output: Road polygon width

**C. Surface Type**
- Input: Road class + region development
- Method: Rules:
  - Motorway/primary: Always paved
  - Secondary: Paved in developed areas, may be gravel rural
  - Residential: Paved in cities, dirt in remote areas
  - Track: Dirt/gravel
- Output: Surface material

**D. Road Detail (Close View)**
- Lane markings: Procedural based on width/class
- Road signs: At intersections, speed limit zones
- Surface wear: Age + traffic simulation (cracks, potholes)

**VALIDATION:**
- Roads connect sensibly
- Widths look right
- Can drive on them (collision works)

---

### 4.2 Street Furniture

**NEEDS:**
- Street lamps
- Traffic lights
- Signs
- Benches, trash cans, etc.

**BASE DATA:**
- ❌ None (rarely in OSM)

**GENERATION RULES:**

**A. Street Lamps**
- Input: Road network + area density
- Method: Placement rules:
  - Spacing: Every 25-50m on urban roads
  - Sides: Both sides for wide roads, alternating for narrow
  - Intersections: Always lit
  - Rural roads: No lights or sparse
- Lamp type: By region/era (modern LED vs vintage)
- Output: Lamp position + type

**B. Traffic Lights**
- Input: Road intersections
- Method: Install if:
  - Intersection complexity (3+ roads)
  - Road class (primary/secondary intersections)
  - Urban density (city centers)
- Output: Traffic light position + configuration

**C. Signs**
- Input: Road network + region
- Method: Procedural placement:
  - Speed limit: Every km, at zone changes
  - Street name: At intersections
  - Directional: At major junctions
  - Warning: Based on terrain (curves, steep, etc.)
- Output: Sign position + content

**D. Other Furniture**
- Benches: Parks, bus stops, plazas
- Trash cans: Near benches, high-traffic areas
- Bollards: Pedestrian zones
- Bike racks: Commercial areas
- Density proportional to urban development

**VALIDATION:**
- Cities feel populated with detail
- Spacing looks realistic
- Not too cluttered, not too sparse

---

## 5. UNDERGROUND & SUBSURFACE

### 5.1 Cave Systems

**NEEDS:**
- Cave locations (where caves exist)
- Cave geometry (tunnels, chambers)
- Accessibility (can player enter?)

**BASE DATA:**
- ⚠️ World Cave Database (only known/explored caves)
- ✅ Geology (Macrostrat) → karst regions
- ✅ Elevation + hydrology → cave-forming conditions

**GENERATION RULES:**

**A. Cave Probability Map**
- Input: Geology type + elevation + water table
- Method: High probability where:
  - Limestone/karst geology
  - Water flow (dissolution)
  - Elevation band (water table interaction)
- Output: Cave probability per chunk

**B. Cave Network Generation**
- Input: Probability map + seed
- Method: Procedural generation:
  - Entry points at cliff faces, sinkholes
  - Tunnel paths follow water flow gradient
  - Chamber sizes from geological stability
  - Stalactite/stalagmite based on age + drip rate
- Constraints:
  - Don't conflict with known caves (use real data first)
  - Don't intersect underground infrastructure in cities
- Output: Cave mesh + collision

**C. Known Cave Integration**
- Input: World Cave Database records
- Method: Use real cave maps where available
- Override procedural in those locations
- Output: Accurate cave geometry for explored caves

**VALIDATION:**
- Caves only in appropriate geology
- Entrances are accessible
- Interior navigable (not impossible squeezes)
- Known caves match real layouts

---

### 5.2 Underground Infrastructure (Urban)

**NEEDS:**
- Subway tunnels (where they exist)
- Utility corridors (water, sewer, power, data)
- Service tunnels
- Basements (buildings)

**BASE DATA:**
- ✅ OSM subway lines + stations
- ⚠️ City GIS data (where available)
- ❌ Utilities (proprietary, unavailable)

**GENERATION RULES:**

**A. Subway Tunnels**
- Input: OSM subway lines (stations + routes)
- Method: Tunnel path between stations:
  - Depth: Typically 10-30m below surface
  - Avoid steep grades (max 4-6%)
  - Curve radius limits (trains can't turn sharply)
  - Tunnel boring machine constraints (straight when possible)
- Output: Tunnel mesh + tracks

**B. Utility Network Inference**
- Input: Roads + buildings + terrain
- Method: Network flow simulation:
  - Water: Follows roads, downhill to buildings
  - Sewer: Follows roads, gravity flow to treatment plant
  - Power/data: Follows roads, tree topology from substations
- Depth: Standard burial (1-3m)
- Output: Utility corridor volumes (hollow for player access)

**C. Building Basements**
- Input: Building footprint + depth
- Method: Rules:
  - Commercial/large buildings: Likely have basement
  - Residential: Depends on region (common in cold climates)
  - Depth: 1-2 floors typically
- Output: Basement volume connected to building

**VALIDATION:**
- Subways match known routes
- Utilities don't conflict (pipes don't intersect)
- Can navigate underground (it's a traversable space)

---

### 5.3 Volumetric Terrain Substrate

**NEEDS:**
- Soil layer (diggable)
- Bedrock (harder to dig)
- Layering (sedimentary strata)
- Voids (caves, pockets)

**BASE DATA:**
- ✅ Surface elevation (SRTM)
- ✅ Geology (Macrostrat) → rock types
- ⚠️ Geological cross-sections (sparse)

**GENERATION RULES:**

**A. Layer Simulation**
- Input: Surface elevation + geology type + age
- Method: Geological deposition model:
  - Sedimentary: Horizontal layers, thickness by age
  - Igneous: Intrusions, dikes (from volcanic activity)
  - Metamorphic: Folded, tilted layers
- Each layer has:
  - Material type (sandstone, shale, granite, etc.)
  - Hardness (mining difficulty)
  - Permeability (water flow)
- Output: 3D voxel material IDs

**B. Soil Depth**
- Input: Surface geology + climate + slope
- Method: Soil formation rules:
  - Flat areas: Thicker soil (accumulation)
  - Steep slopes: Thin soil (erosion)
  - Vegetation: Builds soil (organic matter)
  - Climate: Warm+wet = faster soil formation
- Output: Soil thickness (0.5-5m typically)

**C. Water Table**
- Input: Elevation + precipitation + permeability
- Method: Hydrology simulation:
  - Water fills permeable layers
  - Level varies by season/rainfall
  - Springs at outcrops
- Output: Water table depth

**D. Resource Distribution**
- Input: Geology + depth
- Method: Ore deposit rules:
  - Coal: Sedimentary basins
  - Metal ores: Igneous intrusions
  - Gems: Metamorphic zones
- Rarity: Exponential scarcity
- Output: Resource voxel probability

**VALIDATION:**
- Can dig through soil, hits rock below
- Layers make geological sense
- Resource distribution feels realistic (rare but findable)

---

## 6. VISUAL FIDELITY & ATMOSPHERE

### 6.1 Lighting

**NEEDS:**
- Sun position (time of day)
- Moon position
- Stars
- Atmospheric scattering (sky color)
- Shadows

**BASE DATA:**
- ✅ GPS coordinates
- ✅ Date/time
- ✅ Astronomical calculations (ephemeris)

**GENERATION RULES:**
- **Sun/Moon:** Calculated from coordinates + time (deterministic)
- **Sky color:** Rayleigh scattering simulation (blue sky, red sunset)
- **Stars:** Star catalog (magnitude > 6.0 visible)
- **Weather:** Cloud layer from weather data/simulation
- **Shadows:** Real-time (close) or baked (distant)

**VALIDATION:**
- Sun rises in east, sets in west
- Day/night cycle correct for location
- Lighting looks natural

---

### 6.2 Weather

**NEEDS:**
- Current weather (rain, snow, fog, etc.)
- Weather transitions
- Regional variation

**BASE DATA:**
- ✅ Climate data (historical patterns)
- ⚠️ Weather API (real-time, optional)

**GENERATION RULES:**
- **Option A (Simplified):** Climate-based probability
  - Season + latitude → weather type chances
  - Markov chain state transitions
  - Deterministic from date + seed (all players see same weather)
  
- **Option B (Real-time):** Weather API integration
  - Fetch current weather for region
  - Interpolate between weather stations
  - Network cost, requires internet

**VALIDATION:**
- Weather appropriate for season/location
- Transitions smooth (not instant)
- All players see same weather (sync)

---

## 7. MULTIPLAYER SYNCHRONIZATION

### 7.1 Deterministic World State

**NEEDS:**
- Same world for all players
- Procedural generation must be identical
- Modifications must propagate

**RULES:**
- **Seed-based generation:** GPS coordinate + chunk ID = seed
  - Same seed → same trees, rocks, etc.
  - Every player generates identically
  
- **Generation order matters:**
  - Must generate in consistent order
  - No HashMap iteration for logic (non-deterministic)
  - Sort before iterating
  
- **Floating point determinism:**
  - Use fixed timestep for physics
  - Careful with cross-platform float differences
  
- **Base data versioning:**
  - OSM data changes over time
  - Lock to specific dataset version
  - Update globally (all players update together)

**VALIDATION:**
- Two clients, same location → identical world
- Procedural objects in same positions
- Physics simulation stays in sync

---

### 7.2 Player Modifications (Voxel Changes)

**NEEDS:**
- Player digs hole → all players see hole
- Player builds structure → persists
- Network efficient (can't send entire world)

**RULES:**
- **Base world = procedural** (not networked)
- **Only modifications networked** (CRDT operation log)
- **Modification types:**
  - Voxel set/clear (digging, placing)
  - Entity placement (built structures)
  - State changes (door open/close)
  
- **Operation format:**
  ```
  {
    id: UUID,
    timestamp: u64,
    player: PublicKey,
    type: VoxelSet,
    position: (x, y, z),
    material: Stone,
    signature: Ed25519
  }
  ```
  
- **Conflict resolution:** Last-write-wins (timestamp)
- **Persistence:** Operations stored, replayed on load
- **Sync:** Gossipsub broadcasts to nearby players

**VALIDATION:**
- Dig hole, friend sees it
- Modifications persist across logout
- No duplication/missing operations

---

## 8. PERFORMANCE & LEVEL-OF-DETAIL

### 8.1 Multi-Resolution Rendering

**NEEDS:**
- Distant terrain: Low detail (performance)
- Close terrain: High detail (quality)
- Smooth transitions (no pop-in)

**RULES:**

**Distance-based LOD:**
```
Distance     | Resolution | Detail Level
-------------|------------|-------------
0-100m       | 1m         | Full detail (individual grass, rocks)
100-1000m    | 10m        | Medium (tree groups, terrain texture)
1-10km       | 100m       | Low (terrain shape, forests as patches)
10-100km     | 1km        | Minimal (elevation silhouette)
>100km       | 10km       | Horizon (atmospheric fade)
```

**Hysteresis:**
- Load higher LOD at distance D
- Unload at distance D × 1.2 (prevent thrashing)

**Mesh Generation:**
- Generate async (don't block frame)
- Cache generated meshes (disk + memory)
- Priority queue (closest first)

**VALIDATION:**
- Smooth framerate (>60fps)
- No visible pop-in
- Terrain visible to horizon

---

### 8.2 Data Streaming

**NEEDS:**
- Load world as player moves
- Unload distant areas
- Network/disk efficient

**RULES:**

**Chunk-based loading:**
- World divided into chunks (e.g., 256m × 256m)
- Load/unload by distance
- Neighbor chunks kept in memory (seamless borders)

**Data sources (priority):**
```
1. Memory cache (instant)
2. Disk cache (~/.metaverse/cache/)
3. Network (P2P, nearby players)
4. Generate procedurally
5. API fetch (OSM, elevation, last resort)
```

**Cache strategy:**
- LRU eviction (least recently used)
- Size limit (configurable, e.g., 10GB)
- Invalidation on base data update

**VALIDATION:**
- World loads ahead of player
- No stutter when moving
- Cache hit rate >90%

---

## SUMMARY: REQUIREMENTS → DATA → RULES

| Feature | Needs | Base Data | Generation Rules | Validation |
|---------|-------|-----------|------------------|------------|
| **Walking** | Collision mesh | SRTM 30m elevation | Fractal detail 30m→1m | No falling through terrain |
| **Cliffs** | Vertical geometry | SRTM slope detection | Extrude cliff faces + fractal texture | Looks like real cliff |
| **Buildings** | Footprint + height | OSM (21% coverage) + imagery | ML detection + height inference + procedural facade | Plausible structures |
| **Trees** | Position + species | WorldCover tree class + climate | Poisson disk placement + ecological zones | Realistic forests |
| **Grass** | Ground cover | WorldCover grassland | LOD billboards → geometry | Looks organic |
| **Roads** | Network + surface | OSM roads (80%) | Gap filling + width/material rules | Connected network |
| **Street lamps** | Placement | None | Road-based spacing rules | Realistic urban lighting |
| **Caves** | Tunnel geometry | Cave DB + geology | Karst probability + tunnel generation | Geologically plausible |
| **Subsurface** | Layered materials | Macrostrat + elevation | Deposition simulation + resource distribution | Can dig, layers make sense |
| **Lighting** | Sun/moon/sky | GPS + time | Astronomical calc + scattering | Correct for location/time |
| **Weather** | Conditions | Climate data | Seasonal probability or API | Appropriate for region |
| **Multiplayer** | Same world state | Deterministic seed | Consistent generation order | Identical results |
| **LOD** | Multi-resolution | Single source | Distance-based mesh generation | Smooth performance |

---

## NEXT ACTIONS

1. **Validate one category end-to-end:**
   - Pick: Terrain detail (30m → 1m fractal)
   - Test location: Kangaroo Point Cliffs
   - Success criteria: Looks like a cliff, not blocks

2. **Build rule system architecture:**
   - Procedural generation framework
   - Deterministic seeding
   - LOD integration

3. **Data pipeline:**
   - Download Phase 0 datasets
   - Build import tools
   - Cache structure

4. **Test synchronization:**
   - Two clients, same seed
   - Verify identical generation
   - Measure performance

**Bottom line: We have enough base data. Now we need the RULES to transform it into a living world.**
