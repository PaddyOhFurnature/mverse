# TECHNICAL SPECIFICATION - FRESH START

**Last Updated:** 2026-02-17  
**Purpose:** Architectural decisions for 1:1 Earth-scale metaverse  
**Status:** Foundation research phase - NO CODE YET

---

## PROJECT VISION

**What:** 1:1 scale spherical Earth metaverse  
**Scale:** Real Earth (6,371km radius) with detail down to centimeters  
**Style:** GTA V meets Minecraft on real-world sphere  
**Graphics Target:** No Man's Sky level detail (organic, not blocky, "pretty good")  
**Network:** P2P, fully interactive (build/destroy everything)  
**Data:** Real-world (OSM buildings, SRTM terrain) + procedural generation

---

## 1. COORDINATE SYSTEM

### 1.1 The Foundation Problem

**Question:** Where is the "center of Earth"?  
**Answer:** It's a DEFINED point in WGS84 mathematical model, not a physical measurement.

**Key insight:** All coordinate systems trace back to arbitrary reference points and agreed standards.
- WGS84 center = calculated center of mass from satellite observations
- Greenwich meridian = chosen in 1800s as longitude zero
- Sea level (geoid) = mean ocean surface, varies ±100m from ellipsoid

**See:** `ABSOLUTE_FOUNDATION.md` for full tracing of measurement origins

---

### 1.2 Coordinate Spaces

We use THREE coordinate systems:

#### **ECEF (Earth-Centered, Earth-Fixed)** - Absolute Reference

**What:**
- Origin: WGS84 ellipsoid center (defined point)
- X axis: through equator at 0° longitude (Greenwich)
- Y axis: through equator at 90° East
- Z axis: through North Pole
- Units: meters (f64)

**Precision:**
- At Earth radius (6.4M meters): ~1.4 nanometer precision
- Our minimum need (collision): 1cm
- Safety margin: 10 million times better than required

**Used for:**
- Canonical storage of all positions
- Cross-chunk calculations
- Global positioning
- Network synchronization (deterministic)

**Conversion from GPS:**
```
(lat, lon, elevation) → ECEF (x, y, z)
Using WGS84 ellipsoid parameters:
- Semi-major axis: 6,378,137.0 m
- Flattening: 1/298.257223563
```

**Library:** `geoconv` (Rust crate, type-safe, well-documented)

**See:** `research-coordinates.md` for library evaluation

---

#### **Camera-Relative f32** - GPU Rendering

**The Problem:**
- GPU shaders use f32 (not f64)
- f32 at 6.4M meters = only ~0.76m precision
- Not good enough for 1cm collision accuracy

**The Solution: Floating Origin**
- Camera position stored in ECEF f64 (absolute)
- When rendering: `vertex_f32 = (entity_ecef_f64 - camera_ecef_f64) as f32`
- Camera always at (0,0,0) in render space
- World translates relative to camera
- f32 now represents small offsets (±10km max)
- Precision at ±10km: sub-millimeter

**Proven technique:** Used by Space Engineers, Kerbal Space Program, Universe Sandbox

**Used for:**
- GPU vertex positions
- Shader calculations
- Rendering pipeline only (not storage)

---

#### **GPS (Geodetic)** - Human Interface

**What:**
- Latitude, Longitude, Elevation (WGS84)
- Human-readable coordinates
- Standard for maps, navigation

**Used for:**
- User input (teleport to location)
- OSM data ingestion
- Display to user
- SRTM elevation data

**Never used for:**
- Internal calculations (convert to ECEF first)
- Rendering (convert to ECEF, then camera-relative)
- Storage (always store ECEF)

---

### 1.3 Scale Requirements VALIDATED

**Analysis:** See `COORDINATE_SCALE_EVALUATION.md`

| Scale | Requirement | ECEF f64 Precision | Status |
|-------|-------------|-------------------|--------|
| Collision | 1cm | ~1.4nm | ✅ 10M× safety margin |
| Player position | 1mm | ~1.4nm | ✅ |
| Global distance | 1m | ~1.4nm | ✅ |
| Rendering | 1mm within 10km | f32 camera-relative <0.1mm | ✅ |

**Conclusion:** f64 ECEF + f32 floating origin handles ALL scale requirements.

---

## 2. WORLD DEPTH BOUNDARIES

### 2.1 What We Simulate vs Define

**Key insight:** The coordinate system works from core to space, but we only STORE detail near the surface.

**See:** `WORLD_DEPTH_BOUNDARIES.md` for full explanation

### 2.2 Depth Layers

```
Deep space:      35,786km+        Moon, celestial (skybox)
Space:           100km - 35,786   Satellites, ISS (sparse)
Atmosphere:      500m - 100km     Weather, clouds (procedural)
Above ground:    0m - 500m        Buildings, bridges (OSM + voxels)
Surface:         ~0m              Terrain mesh (SRTM)
Near underground: -200m to 0m     Tunnels, subways (OSM + voxels)
Deep underground: -6,371km to -200m  Solid rock (UNIFORM voxels)
Core:            -6,371km         Coordinate origin (NO STORAGE)
```

**Storage strategy:**
- Surface ±200m: DETAILED voxels (1m³ resolution)
- Deep underground: UNIFORM voxels (one octree node = 1km³ of basalt)
- Core: NOT STORED (just math origin)
- Atmosphere: PROCEDURAL (not voxels)

**Generation trigger:**
- Pre-generated: ±200m from surface
- On-demand: If player digs deeper, generate procedurally
- Always consistent (same seed = same result)

---

## 3. ELEVATION DATA (SRTM)

### 3.1 Data Sources

**See:** `SRTM_DATA_ACCESS.md` for full details

**PRIMARY: Local NAS**
- Stanford global GeoTIFF: `srtm-v3-1s.tif`
- Path: `/mnt/nas/srtm-v3-1s.tif` (download in progress)
- Resolution: 1 arc-second (~30m at equator)
- Coverage: 60°N to 56°S
- No API rate limits

**FALLBACK: OpenTopography API**
- API Key: `3e607de6969c687053f9e107a4796962`
- On-demand queries
- Fill gaps outside SRTM coverage

**REFERENCE: NASA Earthdata**
- Authoritative source
- Validation and metadata

### 3.2 File Format

**GeoTIFF:**
- TIFF image with geographic metadata
- Pixel values = elevation in meters
- Georeferenced (lat/lon → pixel coordinate)
- Self-describing (contains WGS84 coordinate system)

**Rust libraries (need to test with actual file):**
- `gdal` - GDAL bindings (industry standard, needs system library)
- `geotiff` - Pure Rust (simpler deployment)

**Decision deferred:** Test both when file download completes

---

## 4. TERRAIN REPRESENTATION

### 4.1 Voxels + Smooth Mesh Extraction

**Decision:** Use voxels for structure, smooth mesh for rendering  
**Target:** No Man's Sky level detail (organic, not blocky)

**See:** `TERRAIN_RENDERING_COMPARISON.md` for full Voxels vs SDF analysis

### 4.2 Why Voxels + Smooth?

**PROS:**
- ✅ Real-world data integration (SRTM → fill voxels, direct mapping)
- ✅ Player modification (dig = clear voxels, build = set voxels)
- ✅ Network sync (discrete operations, deterministic)
- ✅ Volumetric (caves, tunnels, overhangs all work)
- ✅ Proven at scale (Astroneer, No Man's Sky, 7 Days to Die)

**CONS:**
- ⚠️ Grid resolution limits detail (1m voxels = 1m max features)
- ⚠️ Mesh extraction cost (must convert voxels → triangles)

**Why not SDF:**
- ❌ SRTM heightmap → distance field conversion complex
- ❌ Player modification harder (recompute distance field)
- ❌ Network sync difficult (float determinism issues)
- ❌ Less common (fewer examples, harder to implement)

### 4.3 Voxel System Design (TBD)

**To be designed:**
- Voxel size (0.5m? 1m? variable LOD?)
- Material representation (byte enum? 16 materials?)
- Sparse storage (octree depth, branching factor)
- ECEF → voxel coordinate mapping
- Chunk boundaries (how big? overlap?)

**See:** `FOUNDATION_WORK_PLAN.md` Question 6

### 4.4 Mesh Extraction Algorithm (TBD)

**Options:**
- Marching Cubes (standard, well-documented, lookup tables)
- Dual Contouring (better sharp features, more complex)
- Naive Surface Nets (simpler, may be sufficient)

**To be researched:**
- Algorithm choice
- Implementation details
- Chunk boundary handling (seamless stitching)
- Performance at scale

**See:** `FOUNDATION_WORK_PLAN.md` Question 7

---

## 5. VALIDATION STRATEGY

### 5.1 Scale Gate Testing

**From RULES.md:** Must validate at each scale before proceeding

**Scale gates:**
1. **1m:** Player collision, small objects
2. **1km:** City blocks, terrain tiles
3. **100km:** Cities, large regions
4. **Global:** Earth-scale, antipodal points

### 5.2 Test Points (TBD)

**Need to define:**
- Origin (0°, 0°, 0m) → known ECEF
- Equator points
- North/South poles
- Kangaroo Point Cliffs (-27.4775°S, 153.0355°E)
- Antipodal pairs (maximum distance)
- Random statistical validation

**See:** `FOUNDATION_WORK_PLAN.md` Question 4

### 5.3 Acceptance Criteria

**Coordinate conversion:**
- Round-trip GPS → ECEF → GPS: error < 1mm
- Cross-validation with authoritative sources
- All scale gates pass

**Terrain generation:**
- Visual comparison to reference photos
- Scale accuracy (measured distance = real distance)
- No gaps, holes, or artifacts

---

## 6. CURRENT STATUS

### Phase 0: Research & Setup (IN PROGRESS)

**Completed:**
- ✅ Coordinate system foundation understood
- ✅ WGS84 origin traced to defined standards
- ✅ Scale requirements validated (f64 sufficient)
- ✅ Coordinate library chosen (geoconv)
- ✅ SRTM data sources identified
- ✅ Terrain representation decided (voxels + smooth)
- ✅ Documentation organized (old archived)

**In Progress:**
- ⏳ SRTM file downloading to NAS
- ⏳ GeoTIFF library testing (awaiting file)

**Next:**
- Review rendering coordinates (Question 3)
- Plan validation tests (Question 4)
- Design voxel structure (Question 6)
- Research mesh extraction (Question 7)

---

## 7. NOT YET DEFINED

**Major architectural decisions still needed:**

1. **Chunk system** - Size, addressing, LOD strategy
2. **Voxel structure** - Materials, octree depth, storage
3. **Mesh generation** - Algorithm, vertex format, boundaries
4. **OSM integration** - Building generation, road placement
5. **Network protocol** - P2P discovery, state sync, CRDTs
6. **Physics system** - Collision, deterministic simulation
7. **Rendering pipeline** - Shader design, materials, lighting

**These will be designed as foundation work progresses.**

---

## 8. DEVELOPMENT METHODOLOGY

**From RULES.md:**

1. **Tests before code** (TDD mandatory)
2. **All tests must pass** before any commit
3. **Scale gate testing** required at each phase
4. **No unsafe** without documented proof of necessity
5. **Measure before optimizing** (no premature optimization)
6. **Priority:** Correctness → Performance → Readability → Simplicity

**Current phase:** Research and design (no implementation yet)

---

## REFERENCES

**Foundation research:**
- `ABSOLUTE_FOUNDATION.md` - Coordinate origin tracing
- `COORDINATE_SCALE_EVALUATION.md` - Precision analysis
- `FOUNDATION_COORDINATE_SYSTEM.md` - Layer-by-layer foundation
- `SRTM_DATA_ACCESS.md` - Elevation data sources
- `WORLD_DEPTH_BOUNDARIES.md` - Simulation boundaries
- `TERRAIN_RENDERING_COMPARISON.md` - Voxels vs SDF analysis
- `FOUNDATION_WORK_PLAN.md` - Next steps plan

**Core project docs:**
- `RULES.md` - Development constraints
- `TESTING.md` - Validation strategy
- `GLOSSARY.md` - Terminology

**Archived old branch:**
- `docs/archive-old-branch/` - Previous implementation (reference only)

