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

### 4.3 Voxel System Design

**DECIDED:** See `VOXEL_STRUCTURE_DESIGN.md` for complete specification

**Voxel Size:** 1 meter base resolution
- Human-scale features (doors, rooms, trees)
- Variable LOD: 1m (close) → 2m → 4m → 8m → 16m (distant)
- Octree naturally supports multi-resolution

**Material Representation:** u8 enum (256 materials max)
```rust
#[repr(u8)]
pub enum Material {
    AIR = 0,           // Most common (optimize for this)
    STONE = 2,
    DIRT = 7,
    WATER = 50,
    CONCRETE = 80,
    GLASS = 83,
    // ... up to 256 total
}
```

**Material Properties:** Separate lookup table (not stored per-voxel)
```rust
struct MaterialProperties {
    solid: bool,              // Blocks movement?
    transparent: bool,        // See through?
    opacity: f32,            // 0.0-1.0
    density: f32,            // kg/m³ (physics)
    color: [u8; 3],          // Base RGB
    texture_id: u16,         // Atlas index
    // ... see MATERIAL_PROPERTIES_CLARIFICATION.md
}
```

**Sparse Octree:** 3 node types, depth 23
```rust
pub enum OctreeNode {
    Empty,                           // 1 byte - atmosphere
    Solid(Material),                 // 2 bytes - uniform regions
    Branch {                         // 64 bytes - detailed areas
        children: Box<[OctreeNode; 8]>,
        bounds: AABB,
        cached_mesh: Option<Mesh>,
    },
}
```
- Compression: 500 million × for uniform regions
- Depth 23: 1.5m leaf voxels (close to 1m target)
- Most of Earth at depth 3-5 (coarse, uniform)
- Surface ±200m at depth 20-23 (detailed)

**Coordinate Mapping:** ECEF f64 → Voxel i64
```rust
const WORLD_MIN: Vec3<f64> = Vec3::new(-6_400_000.0, -6_400_000.0, -6_400_000.0);
const VOXEL_SIZE: f64 = 1.0;

fn ecef_to_voxel(ecef: Vec3<f64>) -> Vec3<i64> {
    let relative = ecef - WORLD_MIN;
    Vec3::new(
        (relative.x / VOXEL_SIZE).floor() as i64,
        (relative.y / VOXEL_SIZE).floor() as i64,
        (relative.z / VOXEL_SIZE).floor() as i64,
    )
}
```

**Chunking:** 1km × 1km × 2km vertical
- Chunk size: ~4 MB compressed (octree)
- Load radius: 5km (79 chunks = 320 MB RAM)
- Unload distance: >10km from player
- Cache to disk: `./chunk_cache/{chunk_id}.bin`

**Performance Targets:**
- 1M voxel queries/second (get_voxel)
- 10K voxel modifications/second (set_voxel)
- 1 second to generate 1km² terrain

### 4.4 Mesh Extraction Algorithm

**DECIDED:** Marching Cubes - See `MESH_EXTRACTION_ALGORITHM.md`

**Algorithm:** Marching Cubes (Lorensen & Cline, 1987)

**Rationale:**
- ✅ Simple implementation (~300 lines, lookup tables)
- ✅ Fast execution (1M cubes/second)
- ✅ Well-documented (37 years, thousands of examples)
- ✅ Good enough quality (No Man's Sky level target)
- ✅ Quick to implement (4-6 hours)

**How it works:**
1. Process each voxel cube (8 corners)
2. Calculate cube index from solid/air pattern (256 cases)
3. Lookup edge intersections (EDGE_TABLE)
4. Generate triangles (TRIANGLE_TABLE)
5. Interpolate vertex positions (linear, midpoint)
6. Compute normals (face normal or smooth)

**Lookup Tables:**
```rust
const EDGE_TABLE: [u16; 256] = [ /* which edges intersected */ ];
const TRIANGLE_TABLE: [[i8; 15]; 256] = [ /* how to connect */ ];
```
- Copy from Paul Bourke reference (public domain)
- Pre-computed, no runtime calculation

**Performance:**
- 1km² terrain (~1M cubes): ~1 second
- Optimizations: vertex sharing, normal smoothing, parallel

**Future Upgrades (if needed):**
- Dual Contouring for sharp building corners
- Hybrid: Marching Cubes (terrain) + Dual Contouring (architecture)
- LOD mesh simplification for distant chunks

**Chunk Boundaries:**
- Generate overlapping vertices at boundaries
- Shared vertices ensure seamless stitching
- No cracks or T-junctions

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

### Phase 0: Research & Setup (COMPLETE ✅)

**All Foundation Questions Answered:**
- ✅ Q1: Coordinate library (geoconv chosen)
- ✅ Q2: SRTM data sources (NAS + OpenTopography + Earthdata)
- ✅ Q3: Rendering coordinates (floating origin technique)
- ✅ Q4: Validation strategy (15 tests defined, error margins set)
- ✅ Q5: GeoTIFF pipeline (multi-source redundancy designed)
- ✅ Q6: Voxel structure (1m octree, 1km chunks, 256 materials)
- ✅ Q7: Mesh extraction (Marching Cubes chosen)

**Documentation Created:** 18 research/design documents (~150 pages)

**Blocked:**
- ⏳ SRTM file download (for Phase 2 GeoTIFF testing)
  - Can proceed with Phase 1 without this

**Ready to Start:**
- **Phase 1: Coordinate System Validation** (TDD implementation)

---

## 7. DECIDED BUT NOT IMPLEMENTED

**Foundation architecture complete, implementation pending:**

1. ✅ **Chunk system** - 1km × 1km × 2km, ECEF-based addressing, variable LOD
2. ✅ **Voxel structure** - u8 materials, octree depth 23, sparse compression
3. ✅ **Mesh generation** - Marching Cubes, vertex sharing, normal smoothing
4. ⏳ **SRTM pipeline** - Multi-source, local cache, redundancy (designed, awaiting file)
5. ⏳ **Coordinate system** - ECEF f64 + floating origin (designed, Phase 1 implements)

**Still to be designed (later phases):**

6. **OSM integration** - Building generation, road placement, feature extraction
7. **Network protocol** - P2P discovery, state sync, CRDTs
8. **Physics system** - Collision (use voxel solid flags), deterministic simulation
9. **Rendering pipeline** - Shader design, PBR materials, lighting
10. **Player systems** - Movement, interaction, inventory
11. **Procedural generation** - Trees, rocks, caves, details beyond SRTM/OSM

**These will be designed as Phases 1-6 complete.**

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

