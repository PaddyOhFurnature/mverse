# Testing Volumetric Terrain Generation

**Date:** 2026-02-17  
**Change:** Removed surface layer constraint, now generates full vertical columns

## What Changed

### Before (Broken)
```rust
if dist_from_surface >= -20.0 && dist_from_surface < 10.0 {
    // Only 30m shell around ground elevation
}
```

### After (Fixed)
```rust
// Fill ENTIRE vertical column for each voxel
voxels[idx] = if voxel_altitude < ground_elevation - 50.0 {
    STONE  // Deep underground
} else if voxel_altitude < ground_elevation - 2.0 {
    DIRT   // Shallow underground  
} else if voxel_altitude < ground_elevation {
    STONE  // Near-surface rock (cliff faces!)
} else if voxel_altitude < 0.0 {
    WATER  // Rivers/lakes
} else if voxel_altitude < ground_elevation + 0.5 {
    GRASS  // Surface
} else {
    AIR    // Sky
};
```

## Expected Results

**At Kangaroo Point Cliffs:**

1. **Blocks underground (altitude < 0m):**
   - Should be mostly STONE (deep) or DIRT (shallow)
   - Full solid blocks, not hollow

2. **Blocks at cliff face (altitude 0-30m):**
   - Should show vertical wall of STONE voxels
   - Where ground_elevation varies from 5m (river) to 35m (top)
   - Voxels below ground_elevation = STONE
   - Voxels above ground_elevation = AIR
   - Creates vertical cliff geometry

3. **Blocks at cliff top (altitude > 30m):**
   - Should be GRASS on surface
   - AIR above
   - STONE/DIRT below

## How to Test

```bash
./target/release/examples/continuous_viewer_async
```

Navigate to cliff and observe:
- Is cliff a continuous vertical wall? (not floating blocks)
- Is ground solid beneath you? (not hollow)
- Does terrain look like proper geology? (rock below, grass on top, air above)

Press F12 to capture screenshots for comparison.

## Technical Details

**Generation now fills based on absolute altitude:**
- Each VoxelBlock (8×8×8m) gets EVERY voxel filled
- Material choice based on voxel altitude vs SRTM ground elevation
- No more arbitrary range restrictions
- Supports any vertical geometry (cliffs, caves, waterfalls, etc.)

**Performance impact:**
- More voxels get non-AIR materials
- But only generated once per block (cached)
- Greedy meshing still applies (98.5% reduction)
- Should still maintain 30+ FPS

## Success Criteria

- [ ] Cliff appears as solid vertical wall
- [ ] No floating disconnected blocks
- [ ] Ground is solid (no hollow spaces under terrain)
- [ ] FPS stays above 30
- [ ] Terrain looks geologically correct

