# Terrain Quality Analysis - Kangaroo Point Cliffs

**Date:** 2026-02-17  
**Status:** Critical issues identified, fixes planned

## Problem Statement

User compared our terrain rendering against Google Earth reference photo of Kangaroo Point Cliffs:
- **Expected:** Near-vertical cliff face, 20-30m tall, smooth rock surface
- **Actual:** Blocky "asteroids game" appearance, floating blocks, gentle slope instead of cliff

## Root Cause Analysis

### 1. Surface Layer Too Thin (CRITICAL)
```rust
// Current: Only 6 meters of vertical range
if dist_from_surface >= -4.0 && dist_from_surface < 2.0 {
    // Generate voxel
}
```

**Problem:** Cliff is 20-30m tall, we only render 6m of surface!
- At any given ground elevation, we only generate voxels from -4m to +2m
- A 30m cliff has elevation change from ~5m (river) to ~35m (top)
- With 6m surface thickness, most of the cliff is AIR (not rendered)
- Results in floating disconnected chunks instead of continuous cliff face

**Fix:** Increase to -20m to +10m (30m range) minimum

### 2. SRTM Resolution Too Coarse
- **Data source:** NASA SRTM 1 arc-second (~90m at Brisbane latitude)
- **Cliff dimensions:** ~5-10m wide cliff face, 20-30m tall
- **Sampling rate:** Approximately 1-2 elevation points across entire cliff

**Problem:** Can't capture vertical drop detail
- 90m between samples means cliff edge might fall between sample points
- Bilinear interpolation creates gentle slope instead of sharp drop
- Vertical cliff face geometry completely lost

**Potential fixes:**
1. Use higher-resolution DEM (LiDAR if available)
2. Add cliff detection heuristic (sharp elevation change → vertical fill)
3. Accept limitation for prototype (document as known issue)

### 3. Greedy Meshing Artifacts
**Current algorithm:** Merges adjacent same-material faces into large quads
- Works great for flat surfaces (98.5% triangle reduction)
- Creates floating blocks on irregular surfaces with sparse voxel data

**Why:** 
- With thin surface layer + coarse SRTM, voxels are disconnected
- Greedy meshing creates large quads that emphasize blockiness
- No smoothing or interpolation to connect surfaces

### 4. No Sub-Voxel Surface Refinement
- Hard binary decision: voxel center above/below surface → AIR or SOLID
- No interpolation, no normal smoothing
- 1m voxels acceptable IF surface layer thick enough to be continuous

## Visual Evidence

**Reference:** `screenshot/Screenshot from 2026-02-17 10-07-58.png`
- Google Earth photogrammetry showing actual cliff
- Near-vertical rock face, columnar basalt texture
- Clear edge where flat parkland meets cliff
- Continuous smooth surface

**Our renders:** `screenshot/async_1771286715_*.png`
- Chaotic green/gray/blue blocks floating in space
- No clear vertical face
- Looks like random noise, not terrain
- "asteroids game" appearance (user description)

## Impact Assessment

**Severity:** CRITICAL - Core requirement not met
- Terrain must look realistic for metaverse
- Current quality unacceptable for any use case
- Affects all terrain rendering, not just cliffs

**User frustration:** HIGH
- Multiple explicit demands to fix
- "GET YOUR SHIT SORTED"
- Lost confidence due to repeated quality issues

## Proposed Solution (Ordered by Priority)

### Phase 1: Fix Surface Layer Thickness (IMMEDIATE)
**Time:** 10 minutes  
**Impact:** HIGH - Should fix most floating blocks

```rust
// Change from:
if dist_from_surface >= -4.0 && dist_from_surface < 2.0 {

// To:
if dist_from_surface >= -20.0 && dist_from_surface < 10.0 {
```

**Benefits:**
- 6m → 30m range captures entire cliff height
- Continuous surface instead of floating chunks
- Still only 30 voxel layers per column (acceptable memory cost)

**Risks:**
- Slightly higher memory usage (5x more voxels per column)
- Might expose SRTM resolution issues more clearly

### Phase 2: Test with Smaller Voxels (OPTIONAL)
**Time:** 15 minutes  
**Impact:** MEDIUM - Better vertical detail

```rust
// Change from:
pub const VOXEL_SIZE_M: f64 = 1.0;

// To:
pub const VOXEL_SIZE_M: f64 = 0.5;  // 50cm voxels
```

**Benefits:**
- Finer detail on vertical surfaces
- Smoother appearance after greedy meshing
- Better cliff representation

**Risks:**
- 8x memory increase (0.5m = 2x per dimension = 2³ = 8x total)
- 8x more voxels to process
- May impact performance significantly

**Decision:** Test AFTER Phase 1, only if needed

### Phase 3: Cliff Detection Heuristic (RESEARCH)
**Time:** 2-3 hours  
**Impact:** HIGH - Proper cliff geometry

**Algorithm:**
1. Detect sharp elevation changes (>10m over <50m horizontal distance)
2. Fill vertical column of voxels instead of 6m surface layer
3. Use STONE material for cliff faces
4. Requires gradient calculation from SRTM data

**Benefits:**
- Vertical cliff faces even with coarse SRTM data
- Handles cliffs, canyons, steep slopes properly
- More realistic terrain overall

**Complexity:** Medium - need robust gradient calculation

### Phase 4: Higher Resolution Elevation Data (LONG-TERM)
**Options:**
1. NASADEM (improved SRTM processing)
2. ALOS World 3D (30m resolution)
3. LiDAR point clouds (sub-meter resolution, limited coverage)
4. OpenTopography API (research data)

**Status:** Research needed - most high-res data not freely available globally

## Testing Plan

### Test 1: Thick Surface Layer
1. Change -4/+2 to -20/+10
2. Rebuild and test
3. Take screenshot at same cliff location
4. Compare: Are floating blocks gone? Does cliff appear continuous?

### Test 2: Visual Validation
Required screenshots:
- Same angle as Google Earth reference
- Ground level looking up at cliff
- Top of cliff looking down
- Side view showing full cliff face

### Test 3: Performance Impact
- Measure voxel count before/after
- Measure FPS before/after
- Measure memory usage before/after
- Ensure still playable (>30 FPS)

### Test 4: Other Terrain Types
- Test flat areas (shouldn't break)
- Test gentle slopes (should still work)
- Test river/water (verify water rendering)

## Success Criteria

**Minimum acceptable:**
- Cliff appears as continuous surface (not floating blocks)
- Vertical drop visible in render
- No catastrophic performance regression (<20 FPS)

**Ideal:**
- Cliff recognizable as cliff face
- Comparable to Google Earth quality (allowing for voxel aesthetic)
- 30+ FPS maintained
- User satisfied with visual quality

## Next Actions

1. ✅ Document findings (this file)
2. ⏳ Implement Phase 1 fix (thick surface layer)
3. ⏳ Test and capture comparison screenshots
4. ⏳ Evaluate results, proceed to Phase 2/3 if needed
5. ⏳ Update docs/TODO.md with terrain quality tasks

## References

- Google Earth screenshot: `screenshot/Screenshot from 2026-02-17 10-07-58.png`
- Current renders: `screenshot/async_1771286715_*.png`
- Generator code: `src/procedural_generator.rs:119-135`
- SRTM data: NASA SRTM 1 arc-second (~90m resolution)
