# TODO - Foundation Phase

**Last Updated:** 2026-02-17  
**Status:** Pre-development - Research and design phase

---

## PHASE 0: FOUNDATION RESEARCH (COMPLETE ✅)

### ✅ ALL RESEARCH COMPLETED

**Foundation Questions Answered:**

#### Question 1: Coordinate Library ✅
- **Decision:** `geoconv` crate
- **Rationale:** Simple API, type-safe, f64 precision, well-documented, active maintenance
- **Documentation:** `research-coordinates.md`

#### Question 2: SRTM Data Sources ✅
- **Primary:** NAS file (`/mnt/nas/srtm-v3-1s.tif`) - global GeoTIFF, downloading
- **Fallback:** OpenTopography API (key: `3e607de6969c687053f9e107a4796962`)
- **Reference:** NASA Earthdata (user has account)
- **Documentation:** `SRTM_DATA_ACCESS.md`, `SRTM_REDUNDANT_PIPELINE.md`

#### Question 3: Rendering Coordinates ✅
- **Decision:** Floating Origin technique
- **Method:** Camera at (0,0,0), world translated relative to camera
- **Conversion:** `vertex_f32 = (entity_ecef_f64 - camera_ecef_f64).as_f32()`
- **Precision:** Sub-millimeter within 10km radius
- **Documentation:** `RENDERING_COORDINATES.md`

#### Question 4: Validation Strategy ✅
- **Test Points:** 15 validation tests defined
  - Known points (origin, poles, Kangaroo Point, antipodal pairs)
  - Round-trip tests (GPS → ECEF → GPS < 1mm error)
  - Scale gates (1m, 1km, 100km, global)
  - Floating origin precision (1m, 10km)
- **Reference:** 5 online calculators identified (RF Wireless World primary)
- **Error Margins:** <1mm within 1m, <1cm within 1km, <10m within 100km
- **Documentation:** `VALIDATION_STRATEGY.md`

#### Question 5: GeoTIFF Library & Pipeline ✅
- **Architecture:** Multi-source redundant pipeline
- **Strategy:** Waterfall (cache → NAS → API), Parallel (race), Validation (compare all)
- **Cache:** Local `./elevation_cache/` (1° tiles, LRU eviction, backup-friendly)
- **Libraries:** `gdal` vs `geotiff` (test both when file downloads)
- **Documentation:** `SRTM_REDUNDANT_PIPELINE.md`

#### Question 6: Voxel Structure ✅
- **Voxel Size:** 1 meter base resolution (variable LOD: 1m → 16m)
- **Material:** u8 enum (256 materials: AIR, STONE, DIRT, WATER, CONCRETE, GLASS, etc.)
- **Properties:** Separate lookup table (solid, transparent, density, color, texture)
- **Octree:** 3 node types (Empty, Solid, Branch), depth 23, sparse compression
- **Chunking:** 1km × 1km × 2km vertical (~4MB per chunk, load 79 chunks = 320MB)
- **Coordinates:** ECEF f64 → Voxel i64 (floor division, ±6.4M world bounds)
- **Performance:** 1M queries/sec, 10K modifications/sec targets
- **Documentation:** `VOXEL_STRUCTURE_DESIGN.md`, `MATERIAL_PROPERTIES_CLARIFICATION.md`

#### Question 7: Mesh Extraction ✅
- **Algorithm:** Marching Cubes (1987)
- **Rationale:** Simple (~300 lines), fast (lookup tables), well-documented, good enough quality
- **Implementation:** Edge table (256 entries), Triangle table (256×15), linear interpolation
- **Performance:** ~1 second for 1km² terrain (1M cubes)
- **Future:** Can upgrade to Dual Contouring if sharp features needed
- **Documentation:** `MESH_EXTRACTION_ALGORITHM.md`

### ⏳ AWAITING EXTERNAL DEPENDENCY

- [ ] **SRTM file download completion** (blocking Phase 2 GeoTIFF library testing)
  - Stanford global GeoTIFF: `srtm-v3-1s.tif`
  - Destination: `/mnt/nas/srtm-v3-1s.tif`
  - Can proceed with Phase 1 (coordinates) without this

### 📄 DOCUMENTATION CREATED

**Foundation Research (8 documents):**
- `ABSOLUTE_FOUNDATION.md` - Coordinate origin tracing
- `COORDINATE_SCALE_EVALUATION.md` - Precision analysis
- `FOUNDATION_COORDINATE_SYSTEM.md` - Layer-by-layer foundation
- `SRTM_DATA_ACCESS.md` - Elevation data sources
- `WORLD_DEPTH_BOUNDARIES.md` - Simulation boundaries
- `TERRAIN_RENDERING_COMPARISON.md` - Voxels vs SDF analysis
- `FOUNDATION_WORK_PLAN.md` - Research task breakdown

**Detailed Design (6 documents):**
- `RENDERING_COORDINATES.md` - Floating origin technique
- `VALIDATION_STRATEGY.md` - Test points and error margins
- `SRTM_REDUNDANT_PIPELINE.md` - Multi-source elevation pipeline
- `VOXEL_STRUCTURE_DESIGN.md` - Octree and chunking architecture
- `MATERIAL_PROPERTIES_CLARIFICATION.md` - Glass, water, transparency
- `MESH_EXTRACTION_ALGORITHM.md` - Marching Cubes implementation

**Project Structure:**
- `TECH_SPEC.md` - Fresh technical specification
- `TODO.md` - This file
- `DOC_ORGANIZATION.md` - What's current vs archived

**Total:** 18 new/updated documents, ~150 pages of design

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

