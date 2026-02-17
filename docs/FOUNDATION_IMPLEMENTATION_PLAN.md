# Foundation Implementation Plan

**Last Updated:** 2026-02-17  
**Purpose:** Methodical plan to answer foundation questions and validate coordinate system

---

## THE FOUR CRITICAL QUESTIONS

Before building any terrain features, we must answer:

### Question 1: What Rust library does WGS84 ↔ ECEF conversion?

**Research needed:**
- Search crates.io for: "WGS84", "ECEF", "geodetic", "coordinate conversion"
- Evaluate candidates:
  - Maintenance status (last updated?)
  - Accuracy claims
  - Test coverage
  - Dependencies
  - Performance

**Acceptance criteria:**
- Library found and evaluated
- Decision documented with rationale
- Added to Cargo.toml

**Time estimate:** 1-2 hours research

---

### Question 2: How do we access SRTM elevation data?

**Research needed:**
- SRTM file format (GeoTIFF, HGT, NetCDF?)
- Data source (NASA Earthdata, OpenTopography, other?)
- Which tiles cover test locations:
  - Kangaroo Point Cliffs: (-27.4775°S, 153.0355°E)
  - Download URL or API
- Rust libraries that read the format:
  - GDAL bindings?
  - Pure Rust alternatives?

**Acceptance criteria:**
- Data format identified
- Download source identified
- At least ONE test tile downloaded
- Rust library found to read it
- Can query elevation at a single (lat, lon) point

**Time estimate:** 2-3 hours research + download

---

### Question 3: What rendering coordinate system (precision)?

**Current understanding:**
- TECH_SPEC.md already defines this!
- ECEF f64 for absolute positions
- Chunk-local f32 for rendering
- Floating origin technique

**Validation needed:**
- Verify f64 → f32 conversion doesn't lose precision
- Test at scale gates (1m, 1km, 100km, global)

**Acceptance criteria:**
- Understand existing design in TECH_SPEC.md
- Write tests for precision at each scale gate
- Verify conversions work correctly

**Time estimate:** 1 hour review + testing

---

### Question 4: How do we validate coordinate conversions?

**Test strategy:**
- Known test points (GPS → ECEF → GPS should round-trip)
- Scale gate testing (RULES.md requirement)
- Reference data sources

**Test points needed:**
1. **Origin point:** (0°, 0°, 0m) → Known ECEF
2. **Equator:** (0°, 90°E, 0m) → Should be on Y axis
3. **North pole:** (90°N, any, 0m) → Should be on Z axis
4. **Known location:** Kangaroo Point with verified elevation
5. **Antipodal points:** Maximum distance apart
6. **Random points:** Statistical validation

**Acceptance criteria:**
- Test suite with known reference points
- Round-trip GPS → ECEF → GPS within tolerance (<1mm)
- Cross-validation with authoritative source
- All scale gates pass

**Time estimate:** 2-3 hours test writing

---

## IMPLEMENTATION PHASES

### Phase 0: Research & Setup (CURRENT)

**Tasks:**
1. ✅ Understand the foundation problem
2. ✅ Document the questions
3. ⏳ Answer Question 1 (coordinate library)
4. ⏳ Answer Question 2 (SRTM data access)
5. ⏳ Review Question 3 (existing TECH_SPEC design)
6. ⏳ Plan Question 4 (validation strategy)

**Output:** Decisions documented, libraries chosen, test data acquired

---

### Phase 1: Coordinate System Validation

**Goal:** Prove GPS ↔ ECEF conversion works correctly at all scales

**Tasks:**
1. Add chosen coordinate library to Cargo.toml
2. Write test: GPS (0°, 0°, 0m) → ECEF → GPS round-trip
3. Write test: Equator point
4. Write test: North pole
5. Write test: South pole
6. Write test: Kangaroo Point Cliffs
7. Write test: Antipodal points (maximum distance)
8. Write test: Random points (statistical validation)
9. Write test: Precision at each scale gate (1m, 1km, 100km, global)
10. All tests pass

**Acceptance criteria (from RULES.md):**
- All tests pass
- Scale gate testing proves precision sufficient
- Round-trip error < 1mm
- Code compiles with no warnings
- Commit with clean test suite

**Time estimate:** 4-6 hours

---

### Phase 2: SRTM Data Pipeline

**Goal:** Query elevation at any (lat, lon) point with validated accuracy

**Tasks:**
1. Download SRTM test tile (Kangaroo Point region)
2. Add SRTM reader library to Cargo.toml
3. Write test: Load tile successfully
4. Write test: Query elevation at tile center
5. Write test: Query elevation at tile edges
6. Write test: Query elevation between grid points (interpolation)
7. Write test: Validate against known elevations
8. Implement caching (memory → disk → network)
9. All tests pass

**Test data sources:**
- Kangaroo Point Cliffs: Known landmarks with verified elevations
- Cross-validation: Multiple SRTM sources (SRTM vs ALOS vs Copernicus)

**Acceptance criteria:**
- Can load SRTM tile
- Can query elevation at any point
- Bilinear interpolation between grid points
- Matches known elevations within SRTM accuracy (±16m absolute, ±10m relative)
- Caching works (subsequent queries fast)
- All tests pass

**Time estimate:** 6-8 hours

---

### Phase 3: Basic Terrain Mesh Generation

**Goal:** Generate ONE terrain tile with correct positions and scale

**Tasks:**
1. Define test area: 1km × 1km around Kangaroo Point
2. Create grid: 100×100 vertices (10m spacing)
3. For each vertex:
   a. Calculate GPS position (lat, lon)
   b. Query SRTM elevation
   c. Convert to ECEF (x, y, z)
4. Connect vertices into triangles
5. Write test: Vertex positions correct in ECEF
6. Write test: Triangle connectivity valid
7. Write test: Scale correct (1m in-game = 1m real)
8. Write test: Mesh has no gaps or holes
9. Render mesh (visual validation)
10. All tests pass

**Scale validation:**
- Measure distance between two vertices
- Compare to real-world distance (GPS great circle distance)
- Error < 0.1% (within f64 precision)

**Visual validation:**
- Screenshot from known viewpoint
- Compare to Google Earth view
- Terrain shape matches (qualitative)

**Acceptance criteria:**
- Mesh generates successfully
- Positions mathematically correct (tests pass)
- Visual appearance plausible
- Scale correct (measured distances match reality)
- Performance acceptable (generation time logged)
- All tests pass

**Time estimate:** 8-10 hours

---

### Phase 4: Slope Calculation & Cliff Detection

**Goal:** Identify where cliffs exist in terrain

**Tasks:**
1. Write test: Calculate slope at flat terrain (should be ~0°)
2. Write test: Calculate slope at 45° terrain
3. Write test: Calculate slope at vertical cliff (should be ~90°)
4. Implement slope calculation from terrain mesh
5. Write test: Detect cliff edges at Kangaroo Point
6. Write test: No false cliffs in flat areas
7. Generate slope map (for debugging/visualization)
8. All tests pass

**Cliff detection criteria:**
- Slope > 70° = cliff candidate
- Minimum height > 5m (avoid small bumps)
- Continuous cliff edge (not isolated vertices)

**Validation:**
- Known cliff location (Kangaroo Point): Should detect
- Known flat area (Brisbane River): Should NOT detect
- Visual inspection: Slope map makes sense

**Acceptance criteria:**
- Slope calculation accurate
- Cliff detection finds known cliffs
- No false positives in flat areas
- All tests pass

**Time estimate:** 4-6 hours

---

### Phase 5: Cliff Face Mesh Generation

**Goal:** Create vertical mesh connecting cliff top to cliff bottom

**Tasks:**
1. Write test: Cliff face has top edge vertices
2. Write test: Cliff face has bottom edge vertices
3. Write test: Cliff face connects top to bottom
4. Write test: Cliff face is vertical (or near-vertical)
5. Write test: Cliff face attaches to terrain mesh (no gaps)
6. Implement cliff face generation
7. Render cliff + terrain together
8. Visual validation
9. All tests pass

**Validation:**
- Cliff top elevation matches terrain
- Cliff bottom elevation matches base
- No floating geometry
- No gaps between cliff and terrain
- Looks vertical (not stair-stepped)

**Acceptance criteria:**
- Cliff mesh generates
- Attaches correctly to terrain
- Looks vertical from appropriate viewpoint
- All tests pass
- Visual comparison to reference photo

**Time estimate:** 6-8 hours

---

## AFTER Phase 5: THEN We Can Talk About Detail

**Only after Phases 0-5 are complete and validated:**
- Fractal subdivision (cliff detail)
- Layering (sedimentary banding)
- Erosion patterns
- Texturing
- Etc.

**But NOT before the foundation is solid.**

---

## CURRENT STATUS

**Completed:**
- ✅ Conceptual understanding
- ✅ Documentation of problem
- ✅ Checkpoint created
- ✅ Git commit

**Next immediate action:**
Research Question 1: What coordinate library?

---

## METHODOLOGY

**For each phase:**
1. Write tests FIRST (TDD - RULES.md requirement)
2. Implement minimal code to pass tests
3. Run full test suite: `cargo test --lib -- --nocapture`
4. ALL tests must pass before proceeding
5. Commit with descriptive message
6. Move to next phase

**Never skip ahead.**
**Never proceed with failing tests.**
**Measure twice, cut once.**

