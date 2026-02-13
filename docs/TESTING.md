# TESTING STRATEGY

**Purpose:** How to verify everything works, at every scale, at every phase.
**Last Updated:** 2026-02-13

---

## 1. TESTING PHILOSOPHY

Every system has three layers of verification:

1. **Unit tests** — individual functions produce correct outputs for known inputs
2. **Scale gate tests** — the system works correctly at progressively larger geographic radii
3. **Visual verification** — the rendered output looks correct to a human eye (once rendering exists)

All three layers must pass. A passing unit test does not guarantee the system works at scale. A visual check does not prove the math is correct. You need all three.

---

## 2. RUNNING TESTS

### Full test suite (MUST pass before every commit)
```bash
cd ~/metaverse/metaverse_core
cargo test --lib -- --nocapture
```

### Release-mode performance tests
```bash
cargo test --release --lib -- --nocapture
```

### Single module tests
```bash
cargo test --lib coordinate -- --nocapture
cargo test --lib chunk -- --nocapture
cargo test --lib svo -- --nocapture
cargo test --lib osm -- --nocapture
cargo test --lib elevation -- --nocapture
cargo test --lib cache -- --nocapture
cargo test --lib world -- --nocapture
cargo test --lib entity -- --nocapture
cargo test --lib physics -- --nocapture
cargo test --lib network -- --nocapture
cargo test --lib identity -- --nocapture
```

### Integration tests (require network — skipped by default)
```bash
cargo test --lib -- --nocapture --ignored
```

### Visual verification (once renderer exists)
```bash
cargo run --example viewer --release
```

---

## 3. SCALE GATE DEFINITIONS

Scale gates are mandatory checkpoints. Nothing advances to the next phase until all current-phase scale gates pass.

### Gate Table

| Gate | Radius | What It Proves |
|------|--------|----------------|
| SG-1 | Single entity | One coordinate conversion, one mesh, one render is correct |
| SG-2 | ~200m (city block) | Multiple entities positioned correctly relative to each other |
| SG-3 | 1 km | Multi-entity, multi-chunk (if applicable), 60 FPS |
| SG-4 | 10 km | Earth curvature matters, LOD transitions, multi-chunk, 30+ FPS |
| SG-5 | 100 km | Streaming/caching required, distant LOD, curvature visible |
| SG-6 | ~500 km (state/province) | Memory bounded, disk cache working, sustained traversal |
| SG-7 | ~4000 km (country) | Cross-region, P2P data sharing viable |
| SG-8 | Globe (full sphere) | All 6 quad-sphere faces, zoom from space to street, planet visible from orbit |

### Gate-to-Phase Mapping

| Phase | Required Gates | Why |
|-------|---------------|-----|
| Phase 1 (Coordinates) | Math accuracy at all scales (not visual) | Coordinate system must work globally |
| Phase 2 (Chunks) | SG-1 through SG-3 (data only) | Tile system must cover sphere correctly |
| Phase 3 (SVO) | N/A (data structure, no spatial scale) | Tested by unit tests and op log replay |
| Phase 4 (Data Pipelines) | SG-1 through SG-3 (data only) | Real data must parse and assign correctly |
| Phase 5 (Renderer) | SG-1 through SG-4 (visual) | Must see correct geometry on sphere |
| Phase 6 (Terrain+Roads+Water) | SG-1 through SG-5 (visual) | Terrain, curvature, LOD must work |
| Phase 7 (Streaming) | SG-1 through SG-6 | Memory must be bounded during traversal |
| Phase 8 (Camera+Interaction) | SG-1 through SG-6 | Teleport to different locations must work |
| Phase 9 (Build/Destroy) | SG-1 through SG-4 | Build/destroy operations must be correct and visible |
| Phase 10 (Identity+Ownership) | SG-1 through SG-4 | Ownership must work within chunks |
| Phase 11 (Networking) | SG-1 through SG-7 | P2P sync must work at city and cross-region scale |
| Phase 12 (Physics) | SG-1 through SG-4 | Physics must work at walkable/driveable scale |
| Phase 13 (Rendering) | SG-1 through SG-5 | Visual effects must work at all visible scales |
| Phase 14 (Audio+NPCs) | SG-1 through SG-4 | NPCs and audio within immediate area |
| Phase 15 (Launch) | SG-1 through SG-8 (ALL) | Everything works at every scale |

### Gate Test Procedure

For each required gate, verify ALL of the following (where applicable to current phase):

```
□ All unit tests pass (cargo test --lib)
□ FPS meets target for this gate's entity count (if rendering exists)
□ Memory usage within bounds (see memory targets below)
□ No visual artefacts: gaps, overlaps, flickering, z-fighting (if rendering exists)
□ Coordinate accuracy maintained (< required precision at this scale)
□ Chunk boundaries clean (no seams, no missing tiles)
□ Camera can navigate full area without crashes (if rendering exists)
□ Load/unload works (if streaming exists)
□ Performance benchmarks have not regressed > 10% from previous phase
```

---

## 4. MEMORY TARGETS PER GATE

| Gate | Max RAM | Max VRAM | Max Disk Cache |
|------|---------|----------|---------------|
| SG-1 | 100 MB | 50 MB | N/A |
| SG-2 | 200 MB | 100 MB | 10 MB |
| SG-3 | 500 MB | 200 MB | 50 MB |
| SG-4 | 1 GB | 500 MB | 200 MB |
| SG-5 | 1.5 GB | 750 MB | 1 GB |
| SG-6 | 2 GB | 1 GB | 5 GB |
| SG-7 | 2 GB | 1 GB | 20 GB |
| SG-8 | 2 GB | 1 GB | 50 GB |

Memory must STABILISE, not grow without bound. If walking in one direction causes RAM to increase indefinitely, chunk unloading is broken.

---

## 5. UNIT TEST STANDARDS

### Coverage Target
- Every `pub fn` has at least one test
- Every module has edge case tests (zero, negative, NaN, boundary, overflow)
- Target: 1 test per 30 lines of production code (minimum)

### Test Naming Convention
```rust
#[test]
fn test_<module>_<what_it_tests>() { ... }

// Examples:
fn test_coordinates_gps_to_ecef_equator_greenwich() { ... }
fn test_coordinates_round_trip_brisbane() { ... }
fn test_chunks_quad_sphere_depth_14_neighbours() { ... }
fn test_svo_set_and_clear_voxel() { ... }
fn test_osm_parse_missing_height_defaults_to_9m() { ... }
```

### Test Structure (Arrange-Act-Assert)
```rust
#[test]
fn test_coordinates_brisbane_to_story_bridge_distance() {
    // ARRANGE — set up known inputs
    let queen_st = GpsPos { lat_deg: -27.4698, lon_deg: 153.0251, elevation_m: 0.0 };
    let story_bridge = GpsPos { lat_deg: -27.4634, lon_deg: 153.0394, elevation_m: 0.0 };

    // ACT — call the function under test
    let distance = haversine_distance(&queen_st, &story_bridge);

    // ASSERT — verify output against known reference
    let expected = 1582.0; // metres, verified via Google Maps
    let tolerance = 10.0;  // metres
    assert!(
        (distance - expected).abs() < tolerance,
        "Distance was {distance}m, expected ~{expected}m (±{tolerance}m)"
    );
}
```

### Mandatory Test Categories Per Module

**coordinates.rs:**
- GPS → ECEF for known reference points (equator, poles, Brisbane, Everest)
- ECEF → GPS round-trip (< 1mm error)
- Haversine distances against reference values
- ENU conversions (round-trip < 1mm)
- Batch conversion (parallel matches sequential)
- Performance (throughput in release mode)
- Edge cases: poles, antimeridian, negative elevation, zero, NaN input (should return error or defined behaviour, not panic)

**chunks.rs:**
- GPS → ChunkId at multiple depths (0, 8, 14, 20)
- All 6 cube faces reachable
- Neighbour queries: same-face, cross-face, face-corner
- Parent/child consistency
- Tile bounding geometry (corners on sphere, sizes match expected)
- Containment tests
- Determinism (same input → same ChunkId always)
- Full sphere coverage (random points all resolve to valid tiles)

**svo.rs:**
- Set/get/clear individual voxels
- Fill/clear regions
- Node merging (siblings collapse)
- Op log generation and replay
- Determinism (same ops → same content hash)
- Serialisation round-trip
- Memory efficiency (scales with data, not volume)

**osm.rs:**
- Overpass query builder (valid QL output)
- Rate limiting (cooldown enforced)
- JSON parser (real fixture data)
- Missing field defaults
- Way type classification
- Malformed input handling (error, not panic)

**elevation.rs:**
- HGT filename parsing
- Binary data parsing (synthetic test data)
- Bilinear interpolation
- Void value handling
- Out-of-bounds queries

---

## 6. PERFORMANCE BENCHMARKS

### Benchmark Table

| Benchmark | When Measurable | Minimum | Target |
|-----------|----------------|---------|--------|
| GPS→ECEF batch (conversions/sec) | Phase 1 | 1M/sec | 50M/sec |
| GPS→ChunkId (single, µs) | Phase 2 | 100µs | 1µs |
| SVO set voxel (single, µs) | Phase 3 | 10µs | 1µs |
| SVO get voxel (single, µs) | Phase 3 | 10µs | 0.1µs |
| OSM parse 1 chunk (ms) | Phase 4 | 500ms | 50ms |
| SRTM parse 1 tile (ms) | Phase 4 | 1000ms | 100ms |
| Render 100 buildings (FPS) | Phase 5 | 60 | 120 |
| Render 2500 buildings (FPS) | Phase 5 | 30 | 60 |
| Render 10000 buildings (FPS) | Phase 6 | 30 | 60 |
| SVO mesh generation 1 chunk (ms) | Phase 9 | 50ms | 5ms |
| Physics step 100 bodies (ms) | Phase 12 | 5ms | 1ms |

### Recording Benchmarks

Create `benchmarks/results.csv` and append a row each time benchmarks are run:

```csv
date,commit,phase,gps_ecef_per_sec,chunk_id_us,svo_set_us,svo_get_us,osm_parse_ms,srtm_parse_ms,render_100_fps,render_2500_fps,render_10000_fps
2026-02-14,abc1234,1,12000000,,,,,,,,
```

If any benchmark regresses >10% from the previous recorded value, investigate before proceeding.

---

## 7. VISUAL VERIFICATION CHECKLISTS

### Brisbane CBD Scene (primary verification — available from Phase 5)
```
□ Buildings present at approximately correct positions
□ Brisbane River visible (as gap, or as water once Phase 6 complete)
□ Story Bridge identifiable by position relative to river
□ Queen Street Mall area has dense building cluster
□ No buildings floating in air (after terrain implemented)
□ No buildings buried in ground
□ No visible gaps between chunk tiles
□ No z-fighting (flickering surfaces where two planes overlap)
□ FPS stable and displayed
□ Camera controls responsive
```

### Globe View (available from Phase 5)
```
□ Earth appears as a sphere from far camera
□ All 6 quad-sphere faces render (no missing regions)
□ No visible seams between cube faces
□ Zoom from space to street level works smoothly
□ LOD transitions not jarring
□ Horizon has curvature at city-scale view
```

### Night Scene (available from Phase 13)
```
□ Streetlights illuminate roads
□ Building windows glow
□ Shadows correct at night (moonlight or none)
□ Sky is dark with stars
□ Sunrise/sunset transitions are smooth
```

---

## 8. REGRESSION TEST PROTOCOL

When any test fails after a code change:

```
1. STOP all other work immediately
2. Identify exactly which test(s) failed
3. Determine if YOUR change caused the failure:
   - git stash
   - cargo test --lib
   - If tests pass → your change broke something → git stash pop → fix your code
   - If tests also fail → pre-existing issue → fix it first, document it
4. Fix the CODE to make the test pass (not the test to match the code)
   - Exception: if the test itself had a bug (wrong expected value), fix the test with documentation
5. Verify ALL tests pass (not just the one you fixed)
6. Run scale gate tests at current phase level
7. Only then resume your original task
```

Never ignore a failing test. Never skip it. Never comment it out.