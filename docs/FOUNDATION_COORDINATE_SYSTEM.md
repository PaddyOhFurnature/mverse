# Foundation: The Bottle (Coordinate System & World Frame)

**Last Updated:** 2026-02-17  
**Purpose:** Establish WHAT EVERYTHING SITS IN before building anything

**Metaphor:** Ship in a bottle - can't build the sail until you have the mast, can't have mast without hull, can't have hull without ocean, can't have ocean without THE BOTTLE.

---

## LAYER 0: THE BOTTLE (What contains everything?)

**Question:** What is our coordinate system? Where is (0, 0, 0)?

### The Earth Exists

**Reality:**
- Earth is a sphere (technically oblate spheroid - WGS84 ellipsoid)
- Locations are: Latitude, Longitude, Altitude
- Example: Kangaroo Point Cliffs = (-27.4775°S, 153.0355°E, ~30m altitude)

**Problem:** Latitude/Longitude are ANGLES, not distances
- Can't directly render "27 degrees south"
- Need to convert to 3D Cartesian coordinates (x, y, z)

**Question:** What coordinate system do we use for 3D positions?

### Option 1: ECEF (Earth-Centered Earth-Fixed)

**What is it:**
- Origin (0, 0, 0) = Center of Earth
- X axis: Through (0°N, 0°E) - Prime Meridian at equator
- Y axis: Through (0°N, 90°E) - 90° east
- Z axis: Through North Pole
- Units: Meters from Earth center

**Conversion from GPS:**
```
Given: (lat, lon, alt) in degrees and meters
Earth radius: ~6,371,000 meters

x = (R + alt) × cos(lat) × cos(lon)
y = (R + alt) × cos(lat) × sin(lon)  
z = (R + alt) × sin(lat)

Where R = WGS84 ellipsoid radius at that latitude
```

**Example - Kangaroo Point:**
```
lat = -27.4775° = -0.479 radians
lon = 153.0355° = 2.670 radians
alt = 30 meters
R ≈ 6,371,000 meters

x ≈ 6,371,030 × cos(-0.479) × cos(2.670) ≈ -4,648,000 m
y ≈ 6,371,030 × cos(-0.479) × sin(2.670) ≈  2,560,000 m
z ≈ 6,371,030 × sin(-0.479) ≈ -2,930,000 m

Position in 3D space: (-4,648,000, 2,560,000, -2,930,000) meters
```

**THIS is where Kangaroo Point exists in 3D world space.**

### Option 2: Local Tangent Plane (relative coordinates)

**What is it:**
- Pick a reference point (e.g., Kangaroo Point Cliffs)
- Origin (0, 0, 0) = Reference point
- X axis: East
- Y axis: North  
- Z axis: Up (perpendicular to Earth surface)
- Units: Meters from reference point

**Conversion:**
```
Reference point: (-27.4775°, 153.0355°, 30m)

Other point: (-27.4780°, 153.0360°, 25m)

Convert both to ECEF, then:
x_local = ECEF_other - ECEF_reference
Rotate to local frame (East-North-Up)

Result: (x, y, z) in meters from reference
```

**Advantage:** Easier to work with (smaller numbers)
**Disadvantage:** Need to convert between chunks when far apart

---

## DECISION NEEDED: Which coordinate system?

**For this project:**
- We're rendering entire Earth (1:1 scale)
- Players can be ANYWHERE
- Need consistent frame globally

**Choice: ECEF (Earth-Centered Earth-Fixed)**
- Every location has absolute 3D coordinates
- No reference point needed
- Consistent everywhere

**Consequence:** All positions are ~6 million meters from origin
- Floating point precision issue?
- Need to handle camera positioning carefully

---

## LAYER 1: THE OCEAN (Terrain Surface)

Now we have a coordinate system. Where is the GROUND?

### Question: What is the terrain surface at Kangaroo Point?

**Input:** GPS location (-27.4775°, 153.0355°)

**Step 1: Get elevation data**
- Source: SRTM 30m resolution
- Query: Elevation at (-27.4775°, 153.0355°)
- Result: Height above sea level (meters)

**BUT WAIT:** SRTM is a 30m grid. Our exact point is BETWEEN grid cells.

**SRTM data structure:**
```
Grid cells (30m spacing):
  
  [lat_1, lon_1] → elevation_1
  [lat_1, lon_2] → elevation_2
  [lat_2, lon_1] → elevation_3
  [lat_2, lon_2] → elevation_4
  
Our point: (-27.4775°, 153.0355°)

Find surrounding 4 cells:
  NW: (-27.475°, 153.035°) → elev_nw
  NE: (-27.475°, 153.036°) → elev_ne
  SW: (-27.478°, 153.035°) → elev_sw
  SE: (-27.478°, 153.036°) → elev_se
```

**Step 2: Interpolate**
```
Our point position between cells:
  u = (lon - lon_min) / (lon_max - lon_min)
  v = (lat - lat_min) / (lat_max - lat_min)

Bilinear interpolation:
  elev_north = elev_nw × (1-u) + elev_ne × u
  elev_south = elev_sw × (1-u) + elev_se × u
  elev_point = elev_south × (1-v) + elev_north × v
```

**Result:** Elevation at our exact point (in meters above sea level)

**Example:**
```
If SRTM says:
  elev_nw = 28m
  elev_ne = 30m
  elev_sw = 26m
  elev_se = 29m
  
  u = 0.5 (halfway between west/east)
  v = 0.5 (halfway between south/north)
  
  elev_north = 28×0.5 + 30×0.5 = 29m
  elev_south = 26×0.5 + 29×0.5 = 27.5m
  elev_point = 27.5×0.5 + 29×0.5 = 28.25m
```

**So ground elevation at Kangaroo Point ≈ 28.25 meters above sea level**

---

## LAYER 2: THE HULL (Terrain Mesh)

Now we know ground is at 28.25m elevation. But we need a SURFACE to stand on.

### Question: What is a terrain mesh?

**It's a 3D surface made of triangles:**
```
Each vertex has:
  - Position (x, y, z) in ECEF coordinates
  - Normal vector (which way surface points)
  
Vertices connect into triangles:
  Triangle 1: [vertex_0, vertex_1, vertex_2]
  Triangle 2: [vertex_1, vertex_3, vertex_2]
  ...
```

### Question: How do we create terrain mesh for Kangaroo Point area?

**Step 1: Define area to generate**
```
Center: (-27.4775°, 153.0355°)
Size: 1km × 1km (500m in each direction)

Bounds:
  lat_min = -27.4775° - 0.0045° ≈ -27.482°
  lat_max = -27.4775° + 0.0045° ≈ -27.473°
  lon_min = 153.0355° - 0.0045° ≈ 153.031°
  lon_max = 153.0355° + 0.0045° ≈ 153.040°
  
(0.0045° ≈ 500m at this latitude)
```

**Step 2: Create grid of points**
```
Resolution: Every 10 meters (100 × 100 = 10,000 vertices for 1km²)

For each grid position (i, j) where i,j = 0 to 99:
  lat = lat_min + (i / 99) × (lat_max - lat_min)
  lon = lon_min + (j / 99) × (lon_max - lon_min)
  
  elevation = query_srtm_with_interpolation(lat, lon)
  
  position_ecef = convert_to_ecef(lat, lon, elevation)
  
  vertices[i][j] = position_ecef
```

**Step 3: Connect vertices into triangles**
```
For each quad in grid:
  
  v0 = vertices[i][j]
  v1 = vertices[i+1][j]
  v2 = vertices[i+1][j+1]
  v3 = vertices[i][j+1]
  
  Triangle 1: [v0, v1, v2]
  Triangle 2: [v0, v2, v3]
```

**Result: Terrain mesh covering 1km² around Kangaroo Point**

---

## LAYER 3: POSITIONING THE CLIFF

NOW we have:
- ✅ Coordinate system (ECEF)
- ✅ Ground elevation data (SRTM)
- ✅ Terrain mesh (triangles at correct positions)

**Question: WHERE is the cliff?**

### Step 1: Detect cliff location

**From terrain mesh, find steep slopes:**
```
For each vertex in mesh:
  Calculate slope = rise / run
  
  If slope > 70° (tan(70°) ≈ 2.75):
    This vertex is on a cliff
```

**Example at Kangaroo Point:**
```
Top of cliff: elevation = 30m
Bottom of cliff: elevation = 2m (near river)
Horizontal distance: ~50m

Slope = (30 - 2) / 50 = 0.56 = 56%
Angle = atan(0.56) = 29°... 

Wait, that's not vertical!
```

**PROBLEM: SRTM resolution is too coarse!**

**SRTM sees:**
```
Grid point 1: 30m elevation (cliff top)
Grid point 2: 2m elevation (river level)
Distance between points: 30m (grid spacing)

Calculated slope: (30-2)/30 = 93% = 43° angle
```

**But reality:**
- Cliff top at 30m
- Horizontal distance to cliff edge: ~5m
- Cliff drops vertically: ~28m
- River at 2m

**Real slope at cliff face: 28m / 0m = VERTICAL (90°)**

### Step 2: Detect ACTUAL cliff edge

**We need finer resolution around steep areas:**

```
1. Scan terrain mesh for slopes > 45°
2. For those areas, generate denser mesh (1m spacing instead of 10m)
3. Re-calculate slopes at higher resolution
4. Find where slope exceeds 70° → TRUE cliff edge
```

**Better approach:**
```
Look at SRTM gradient (rate of elevation change):

Point A: (-27.4774°, 153.0354°) → 30m elevation
Point B: (-27.4776°, 153.0354°) → 28m elevation  (0.0002° south ≈ 20m)
Point C: (-27.4778°, 153.0354°) → 5m elevation   (0.0002° south ≈ 20m)

Gradient A→B: (30-28)/20 = 0.1 m/m = gentle
Gradient B→C: (28-5)/20 = 1.15 m/m = STEEP

Cliff edge is between B and C
```

### Step 3: Define cliff geometry

**Now we know:**
- Cliff top edge: (-27.4776°, 153.0354°) at 28m elevation
- Cliff bottom: (-27.4778°, 153.0354°) at 2m elevation
- Cliff height: 26m
- Cliff face orientation: Facing south (towards river)

**In ECEF coordinates:**
```
Cliff top: convert_to_ecef(-27.4776°, 153.0354°, 28m)
  → (x_top, y_top, z_top)

Cliff bottom: convert_to_ecef(-27.4778°, 153.0354°, 2m)  
  → (x_bottom, y_bottom, z_bottom)
```

**Cliff face is a vertical surface connecting these two lines**

---

## LAYER 4: THE MAST (Cliff Face Structure)

NOW we can talk about the cliff face itself.

**The cliff face is attached to:**
- TOP: Terrain mesh at cliff edge (28m elevation)
- BOTTOM: Terrain mesh at cliff base (2m elevation)
- SIDES: Connects to adjacent terrain

**Cliff face mesh:**
```
Top edge vertices: Array of points along cliff top
Bottom edge vertices: Array of points along cliff bottom

For each segment:
  v_top_left
  v_top_right
  v_bottom_left  
  v_bottom_right
  
  Create quad (2 triangles):
    Triangle 1: [v_top_left, v_top_right, v_bottom_right]
    Triangle 2: [v_top_left, v_bottom_right, v_bottom_left]
```

**Positions come from:**
- TOP: Terrain mesh vertices where slope > 70°
- BOTTOM: Terrain mesh vertices at base of slope

**NOW the cliff is connected to the terrain!**

---

## LAYER 5: THE SAIL (Cliff Detail)

ONLY NOW can we add detail to the cliff face.

**We have:**
- ✅ Coordinate system (ECEF)
- ✅ Terrain mesh (from SRTM)
- ✅ Cliff top edge (from slope detection)
- ✅ Cliff bottom edge (from slope detection)
- ✅ Cliff face mesh (connecting top to bottom)

**NOW we can:**
- Subdivide cliff face into smaller patches
- Apply fractal displacement to each patch
- Add layering, erosion, detail

**The 1m² patch from before:**
- Is part of the larger cliff face mesh
- Its position is defined by where it sits on the cliff (height above base, position along cliff line)
- Its "anchor points" are vertices in the cliff face mesh
- Detail is ADDED TO the existing mesh, not floating in space

---

## SCALE REFERENCE

**Question: How do we know it's the right size?**

**Answer: Everything is in absolute real-world units (meters)**

```
1 meter in-game = 1 meter in reality

Kangaroo Point cliff:
  Real height: ~26-28 meters
  In-game height: 26-28 meters (measured from SRTM)
  
Player height: 1.7 meters
  In-game: 1.7 meters

If player stands next to cliff:
  Player head at: 1.7m
  Cliff top at: 28m
  Ratio: 28/1.7 ≈ 16× taller than player
  
This should LOOK like standing next to a 10-story building
```

**Validation:**
- Take screenshot with player at base of cliff
- Measure pixel height of player vs cliff
- Ratio should be 1:16
- Compare to reference photo (person standing at real cliff)

---

## MEASUREMENT ORIGIN

**Question: Where did we start measuring from?**

**Answer: Everything measures from:**
1. **Horizontal:** Earth center (0, 0, 0) in ECEF
2. **Vertical:** Sea level (WGS84 geoid)

**SRTM elevation = meters above mean sea level**
- Sea level at Brisbane ≈ 0m
- Cliff top ≈ 30m above sea level
- River ≈ 2m above sea level (tidal)

**ALL measurements in same reference frame:**
- Player position: ECEF coordinates
- Terrain vertices: ECEF coordinates  
- Cliff vertices: ECEF coordinates
- Camera position: ECEF coordinates

**Everything relates to everything else through absolute coordinates**

---

## THE COMPLETE STACK (Bottom to Top)

```
LAYER 0: COORDINATE SYSTEM (The Bottle)
  ↓
  ECEF (Earth-Centered Earth-Fixed)
  Origin: Center of Earth
  Units: Meters
  
LAYER 1: ELEVATION DATA (The Ocean)
  ↓
  SRTM 30m grid
  Values: Meters above sea level
  
LAYER 2: TERRAIN MESH (The Hull)
  ↓
  10m resolution grid
  Vertices at (lat, lon, elevation) → convert to ECEF
  Connected as triangles
  
LAYER 3: CLIFF DETECTION (Finding where to attach the mast)
  ↓
  Analyze terrain mesh slopes
  Find edges where slope > 70°
  Define cliff top and bottom edges
  
LAYER 4: CLIFF FACE MESH (The Mast)
  ↓
  Connect cliff top edge to cliff bottom edge
  Vertical surface
  Attached to terrain mesh
  
LAYER 5: CLIFF DETAIL (The Sail)
  ↓
  Subdivide cliff face
  Apply fractal displacement
  Add layers, erosion, texture
  ATTACHED to cliff face mesh
```

**Each layer DEPENDS on the one below it.**
**You cannot build layer 5 without layers 0-4 existing first.**

---

## WHAT WE NEED TO BUILD (In Order)

1. **ECEF coordinate converter**
   - Input: (lat, lon, alt)
   - Output: (x, y, z) in meters

2. **SRTM data loader**
   - Input: (lat, lon)
   - Output: Elevation in meters

3. **Terrain mesh generator**
   - Input: Bounding box (lat/lon), resolution
   - Output: Triangle mesh with vertices in ECEF

4. **Slope calculator**
   - Input: Terrain mesh
   - Output: Slope at each vertex

5. **Cliff edge detector**
   - Input: Slope map
   - Output: Vertices that are cliff edges

6. **Cliff face generator**
   - Input: Cliff top vertices, cliff bottom vertices
   - Output: Vertical mesh connecting them

7. **Cliff detail generator**
   - Input: Cliff face mesh
   - Output: Subdivided + displaced mesh

**ONLY THEN do we have a complete cliff attached to the terrain.**

---

## CRITICAL QUESTIONS TO ANSWER

Before writing code:

1. **Do we have SRTM data for Kangaroo Point downloaded?**
   - If not: How do we get it?
   - Format: GeoTIFF? HGT? NetCDF?

2. **What library reads SRTM files?**
   - Rust: gdal crate? Something else?

3. **What library does ECEF conversion?**
   - Need WGS84 ellipsoid calculations

4. **How do we handle floating point precision?**
   - Positions are ~6 million meters from origin
   - f32 precision: ~1 meter at that distance
   - f64 precision: ~1 millimeter
   - Need f64 for positions?

5. **What's our rendering system?**
   - wgpu pipeline expects what format?
   - Can it handle ECEF coordinates?
   - Or do we need local tangent plane for rendering?

**THESE must be answered before building anything.**

