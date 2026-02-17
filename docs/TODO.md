# TODO - Foundation Phase

**Last Updated:** 2026-02-17  
**Status:** Pre-development - Research and design phase

---

## PHASE 0: FOUNDATION RESEARCH (IN PROGRESS)

### ✅ COMPLETED

- [x] Understand coordinate system foundations
- [x] Trace WGS84 origins to defined standards  
- [x] Validate f64 precision at Earth scale
- [x] Choose coordinate library (geoconv)
- [x] Identify SRTM data sources (NAS + OpenTopography + Earthdata)
- [x] Decide terrain representation (voxels + smooth mesh, No Man's Sky level)
- [x] Organize documentation (archive old branch)
- [x] Create fresh TECH_SPEC.md

### ⏳ IN PROGRESS

- [ ] **SRTM file download** (awaiting completion)
  - Stanford global GeoTIFF downloading to NAS
  - Path: `/mnt/nas/srtm-v3-1s.tif`

### 📋 RESEARCH TODO (Can do while waiting)

#### Question 3: Rendering Coordinates (30-60 min)
- [ ] Document floating origin technique
- [ ] Define camera-relative f32 conversion math
- [ ] Validate precision at ±10km from camera
- [ ] Plan rendering coordinate tests

#### Question 4: Validation Strategy (1 hour)
- [ ] Find reference coordinate conversion sources
- [ ] Get known ECEF values for test points:
  - [ ] Origin (0°, 0°, 0m)
  - [ ] Equator points (0°, 90°E, etc.)
  - [ ] North Pole (90°N)
  - [ ] South Pole (90°S)
  - [ ] Kangaroo Point Cliffs (-27.4775°S, 153.0355°E)
  - [ ] Antipodal pairs
- [ ] Define error margins (<1mm for round-trip)
- [ ] Write validation test plan

#### Question 5: GeoTIFF Library Research (1-2 hours)
- [ ] Research `gdal` crate API
- [ ] Research `geotiff` crate API
- [ ] Understand GeoTIFF coordinate transforms (lat/lon → pixel)
- [ ] Plan bilinear interpolation (query between pixels)
- [ ] Write pseudo-code for "get elevation at (lat, lon)"
- [ ] **Test both libraries** (when file downloads)

#### Question 6: Voxel Data Structure Design (2-3 hours) - CRITICAL
- [ ] Define voxel size (0.5m? 1m? variable LOD?)
- [ ] Choose material encoding:
  - [ ] How many materials? (16? 256?)
  - [ ] Enum design (AIR, STONE, DIRT, WATER, CONCRETE, etc.)
  - [ ] Material properties (solid, transparent, etc.)
- [ ] Design sparse octree:
  - [ ] Node types (Empty, Solid, Branch)
  - [ ] Octree depth (how deep to subdivide?)
  - [ ] Branching factor (8 children per node)
- [ ] Define voxel coordinate system:
  - [ ] Global voxel grid? Or chunk-local?
  - [ ] ECEF → voxel coordinate mapping
- [ ] Calculate memory requirements:
  - [ ] 1km² at 1m voxels
  - [ ] 1km² at 0.5m voxels
  - [ ] With octree compression estimates
- [ ] Plan LOD strategy (coarser voxels at distance)

#### Question 7: Mesh Extraction Algorithm (2-3 hours)
- [ ] Choose algorithm:
  - [ ] Research Marching Cubes
  - [ ] Research Dual Contouring
  - [ ] Research Naive Surface Nets
  - [ ] Make decision
- [ ] Understand lookup tables (edge table, triangle table)
- [ ] Plan implementation approach:
  - [ ] Sample voxel corners
  - [ ] Interpolate vertex positions
  - [ ] Generate triangles
  - [ ] Compute normals
- [ ] Define vertex format (position, normal, material, UV?)
- [ ] Plan chunk boundary handling (seamless stitching)

---

## PHASE 1: COORDINATE SYSTEM VALIDATION (NOT STARTED)

**Goal:** Prove GPS ↔ ECEF conversion works at all scales

### Setup
- [ ] Add `geoconv` to Cargo.toml
- [ ] Create `src/coordinates.rs`
- [ ] Create `src/tests/coordinate_tests.rs`

### Test Suite (TDD - Write tests FIRST)
- [ ] Test: Origin point (0°, 0°, 0m)
- [ ] Test: Equator point (0°, 90°E, 0m)
- [ ] Test: North Pole (90°N, 0°, 0m)
- [ ] Test: South Pole (90°S, 0°, 0m)
- [ ] Test: Kangaroo Point Cliffs (-27.4775°S, 153.0355°E, 20m)
- [ ] Test: Antipodal points (maximum distance)
- [ ] Test: Random points (100 random GPS → ECEF → GPS)
- [ ] Test: Scale gate 1m (precision validation)
- [ ] Test: Scale gate 1km
- [ ] Test: Scale gate 100km
- [ ] Test: Scale gate global (Earth diameter)
- [ ] Test: Round-trip error < 1mm

### Implementation
- [ ] Implement GPS → ECEF using geoconv
- [ ] Implement ECEF → GPS using geoconv
- [ ] Implement floating origin transform (ECEF f64 → camera-relative f32)
- [ ] All tests pass ✅
- [ ] No compiler warnings ✅
- [ ] Git commit with clean test suite

---

## PHASE 2: SRTM DATA PIPELINE (NOT STARTED)

**Goal:** Query elevation at any (lat, lon) with validated accuracy

### Library Choice
- [ ] Test `gdal` with actual SRTM file
- [ ] Test `geotiff` with actual SRTM file
- [ ] Benchmark query performance
- [ ] Make decision (document rationale)

### Test Suite (TDD)
- [ ] Test: Load SRTM file successfully
- [ ] Test: Query elevation at file center
- [ ] Test: Query elevation at file edges
- [ ] Test: Query elevation at known location (validate against reference)
- [ ] Test: Bilinear interpolation between pixels
- [ ] Test: Out-of-bounds handling (return error or sea level?)
- [ ] Test: Performance (1000 queries/second minimum)

### Implementation
- [ ] Add chosen library to Cargo.toml
- [ ] Create `src/elevation.rs`
- [ ] Implement SRTM file loading
- [ ] Implement lat/lon → pixel coordinate conversion
- [ ] Implement elevation query (bilinear interpolation)
- [ ] Implement caching (memory LRU cache)
- [ ] All tests pass ✅
- [ ] Git commit

---

## PHASE 3: VOXEL STRUCTURE (NOT STARTED)

**Goal:** Define and test sparse voxel octree storage

### Design (from Question 6 research)
- [ ] Document voxel size decision
- [ ] Document material enum design
- [ ] Document octree structure
- [ ] Document voxel coordinate mapping

### Test Suite (TDD)
- [ ] Test: Create empty octree
- [ ] Test: Set single voxel
- [ ] Test: Get single voxel
- [ ] Test: Clear single voxel
- [ ] Test: Fill region (bulk operation)
- [ ] Test: Clear region (bulk operation)
- [ ] Test: Octree compression (uniform regions → single node)
- [ ] Test: Memory usage (1km³ uniform vs detailed)
- [ ] Test: Query performance (1M voxel queries/second)

### Implementation
- [ ] Create `src/voxel.rs`
- [ ] Define `Material` enum
- [ ] Define `OctreeNode` enum (Empty, Solid, Branch)
- [ ] Implement `set_voxel(x, y, z, material)`
- [ ] Implement `get_voxel(x, y, z) -> Material`
- [ ] Implement `fill_region(min, max, material)`
- [ ] Implement octree compression (merge uniform children)
- [ ] All tests pass ✅
- [ ] Git commit

---

## PHASE 4: TERRAIN GENERATION (NOT STARTED)

**Goal:** Generate voxels from SRTM elevation data

### Test Suite (TDD)
- [ ] Test: Generate 1km² flat terrain (uniform elevation)
- [ ] Test: Generate 1km² with elevation gradient
- [ ] Test: Generate terrain at Kangaroo Point Cliffs
- [ ] Test: Voxels below surface = STONE
- [ ] Test: Voxels at surface = DIRT/GRASS
- [ ] Test: Voxels above surface = AIR
- [ ] Test: Coastal terrain (land + water)

### Implementation
- [ ] Create `src/terrain_generator.rs`
- [ ] Implement `generate_terrain_voxels(bounds, srtm_data)`
- [ ] For each voxel position:
  - [ ] Convert to GPS (lat, lon)
  - [ ] Query SRTM elevation
  - [ ] Set material based on height relative to surface
- [ ] Handle water (sea level = 0m)
- [ ] All tests pass ✅
- [ ] Git commit

---

## PHASE 5: MESH EXTRACTION (NOT STARTED)

**Goal:** Convert voxels to smooth triangle mesh

### Algorithm Implementation (from Question 7 research)
- [ ] Document algorithm choice
- [ ] Implement lookup tables (edge table, triangle table)

### Test Suite (TDD)
- [ ] Test: Extract mesh from single solid voxel (should be cube)
- [ ] Test: Extract mesh from sphere of voxels (should be smooth sphere)
- [ ] Test: Extract mesh from flat terrain (should be flat plane)
- [ ] Test: Extract mesh from cliff (should be vertical surface)
- [ ] Test: Vertex normals computed correctly
- [ ] Test: No duplicate vertices
- [ ] Test: No holes in mesh

### Implementation
- [ ] Create `src/mesh_extraction.rs`
- [ ] Implement chosen algorithm (Marching Cubes)
- [ ] Sample voxel grid
- [ ] Generate vertices (interpolated positions)
- [ ] Generate triangles (from lookup table)
- [ ] Compute normals (from gradient)
- [ ] All tests pass ✅
- [ ] Git commit

---

## PHASE 6: FIRST RENDER (NOT STARTED)

**Goal:** Render 1km² terrain mesh at Kangaroo Point Cliffs

### Prerequisites
- [ ] All Phase 1-5 tests passing
- [ ] wgpu rendering setup (basic)

### Tasks
- [ ] Generate terrain voxels (1km² around Kangaroo Point)
- [ ] Extract mesh from voxels
- [ ] Set up camera at known viewpoint
- [ ] Render mesh (basic shading)
- [ ] Screenshot
- [ ] Visual comparison to reference photo

### Validation
- [ ] Terrain shape matches reality (qualitative)
- [ ] Scale is correct (measure distance in screenshot)
- [ ] No visual artifacts (holes, gaps, flickering)
- [ ] Frame rate acceptable (>30fps)

---

## BEYOND PHASE 6

**After basic terrain rendering works:**

- [ ] OSM building integration
- [ ] Procedural detail (trees, rocks, etc.)
- [ ] Player movement and collision
- [ ] Chunk LOD system
- [ ] Network synchronization
- [ ] Cave generation
- [ ] Building interiors
- [ ] etc.

**But first: Get foundation solid and ONE test case working.**

---

## NOTES

**Current focus:** Phase 0 research (Questions 3-7)  
**Blocker:** SRTM file download (for GeoTIFF library testing)  
**Estimated research time:** ~7 hours of design work available  
**Then:** Ready for rapid implementation (Phase 1-6)

**Methodology:** TDD (tests before code), all tests must pass before proceeding

