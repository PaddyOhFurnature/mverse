# Foundation Work Plan - While Waiting for SRTM Download

**Last Updated:** 2026-02-17  
**Status:** Phase 0 research continuing

---

## COMPLETED SO FAR (Today)

### ✅ Question 1: Coordinate Library
- **Answer:** geoconv (Rust geodetic conversions)
- **Rationale:** Simple API, type-safe, f64 precision, well-documented
- **Status:** Decision made, documented

### ✅ Question 2: SRTM Data Access  
- **Answer:** Three sources available
  - Primary: Your NAS (Stanford global GeoTIFF) - downloading
  - Fallback: OpenTopography API (key: 3e607de6969c687053f9e107a4796962)
  - Reference: NASA Earthdata (your account)
- **Format:** GeoTIFF
- **Status:** Data source identified, awaiting download

### ✅ Conceptual Understanding
- Coordinate system origins (WGS84 is defined standard, not physical measurement)
- World depth boundaries (foundation complete, detail local to ±200m from surface)
- Scale evaluation (f64 has nanometer precision at Earth scale)
- Terrain rendering decision (Voxels + Smooth mesh, No Man's Sky level acceptable)

---

## REMAINING FOUNDATION WORK (Can Do Now)

### Question 3: Review Rendering Coordinate System

**What to check:**
- TECH_SPEC.md says use ECEF f64 + chunk-local f32 + floating origin
- Does this actually solve the precision problem?
- What's the chunk size? (affects local coordinate range)
- How do we convert ECEF → chunk-local?

**Tasks:**
1. Review TECH_SPEC.md Section 1.2-1.5 (coordinate spaces)
2. Understand floating origin technique
3. Define chunk size (affects f32 range)
4. Document conversion math: ECEF f64 → chunk-local f32
5. Plan tests to validate precision

**Time:** 30 minutes review + 30 minutes documentation

**Blocker:** None - can do now

---

### Question 4: Validation Strategy

**What we need:**
- Test points with KNOWN correct values
- How do we validate our coordinate conversions are correct?
- What reference data do we compare against?

**Tasks:**
1. Find reference coordinate conversions (online calculators, authoritative sources)
2. Define test points:
   - Origin (0°, 0°, 0m)
   - Equator points
   - Poles
   - Kangaroo Point Cliffs
   - Antipodal pairs
3. Get EXPECTED ECEF values for each test point
4. Define acceptable error margins (<1mm for round-trip)

**Time:** 1 hour research + documentation

**Blocker:** None - can do now

---

### Question 5: GeoTIFF Library Choice

**Current state:**
- Found `gdal` (GDAL bindings, needs system library)
- Found `geotiff` (pure Rust)
- Can't decide without testing actual file

**What we CAN do now:**
1. Research both libraries' APIs
2. Write pseudo-code for "query elevation at (lat, lon)"
3. Understand GeoTIFF coordinate transforms (lat/lon → pixel x/y)
4. Plan bilinear interpolation (query between pixels)
5. Understand file structure (bands, blocks, compression)

**Time:** 1-2 hours research

**Blocker:** Can't actually test until file downloads

---

### Question 6: Voxel Data Structure Design

**What we need to define:**
- Voxel size (0.5m? 1m? variable?)
- Material representation (byte ID? enum?)
- Sparse storage strategy (octree? hashmap? chunks?)
- How to map ECEF coordinates → voxel coordinates
- Memory budget per chunk

**Tasks:**
1. Define voxel coordinate system (chunk-local or global?)
2. Choose material encoding (enum with ID mapping)
3. Design sparse octree structure (nodes, depth, branching)
4. Calculate memory requirements (1km² at different resolutions)
5. Plan LOD strategy (coarser voxels at distance)

**Time:** 2-3 hours analysis + design

**Blocker:** None - pure design work

---

### Question 7: Mesh Extraction Algorithm

**Choices:**
- Marching Cubes (standard, well-documented)
- Dual Contouring (better sharp features)
- Naive Surface Nets (simpler, good enough?)

**What we need:**
1. Choose algorithm (probably Marching Cubes)
2. Understand lookup tables (edge table, triangle table)
3. Plan implementation (sample voxels, interpolate vertices, generate triangles)
4. Define vertex format (position, normal, material?)
5. How to handle chunk boundaries (seamless stitching)

**Time:** 2-3 hours research + planning

**Blocker:** None - pure algorithm research

---

## RECOMMENDED WORK ORDER (While Waiting)

### PRIORITY 1: Question 3 (30-60 min) - CRITICAL FOUNDATION
**Review rendering coordinates from TECH_SPEC**
- Understand the existing design decisions
- Validate they solve our precision needs
- Document any gaps or concerns

### PRIORITY 2: Question 4 (1 hour) - VALIDATION PREPARATION
**Plan coordinate validation tests**
- Get reference test points with known ECEF values
- Define test strategy
- Ready to write tests immediately after adding library

### PRIORITY 3: Question 6 (2-3 hours) - DESIGN WORK
**Design voxel data structure**
- Critical architectural decision
- Affects everything downstream
- No code yet, pure design

### PRIORITY 4: Question 5 (1-2 hours) - GEOTIFF RESEARCH
**Research GeoTIFF libraries**
- Understand APIs
- Plan usage patterns
- Can't test until file downloads, but can prepare

### PRIORITY 5: Question 7 (2-3 hours) - MESH ALGORITHM
**Research mesh extraction**
- Choose algorithm
- Understand implementation
- Prepare for implementation phase

---

## TOTAL FOUNDATION WORK AVAILABLE

**If SRTM download takes 8 hours:**
- We have ~7 hours of design/research work ready
- All can be done without the file
- Prepares us for rapid implementation once file arrives

**Work is front-loaded:**
- Understand existing design (TECH_SPEC review)
- Plan validation (test strategy)
- Design data structures (voxels, chunks)
- Research algorithms (mesh extraction)

**Then when file arrives:**
- Add libraries to Cargo.toml
- Write coordinate conversion tests (Question 4 ready)
- Test GeoTIFF loading (Question 5 ready)
- Implement voxel generation (Question 6 ready)
- Implement mesh extraction (Question 7 ready)

---

## MY RECOMMENDATION

**Start with Question 3 now:**

Review TECH_SPEC.md rendering coordinates, understand the floating origin design, validate it solves our precision problem, document any concerns.

**This is 30-60 minutes and unblocks understanding of the whole system.**

**Want me to start there?**

