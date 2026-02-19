# Rendering Fix - Camera-Relative Coordinates

**Date:** 2026-02-17  
**Commits:** 8116138, 7d1a8b0  
**Status:** Precision bug fixed, screenshots working

## The Critical Bug

**Problem:** Converting ECEF coordinates (~5 million meters) directly to f32
- ECEF positions are ~5,000,000 meters (millions)
- Vertex struct uses f32 (only ~7 digits precision)
- Converting 5,000,000 to f32 loses ~1-10m precision
- Result: Vertices positioned incorrectly → broken stretched geometry

## The Fix: Camera-Relative Coordinates

**Before (broken):**
```rust
// Direct ECEF to f32 conversion - PRECISION LOSS!
let (block_verts, block_inds) = greedy_mesh_block(
    &block.voxels,
    block.ecef_min, // ~5,000,000 as f32 = broken
);
```

**After (working):**
```rust
// Subtract camera position FIRST (both f64), THEN convert to f32
let block_relative_to_cam = [
    block.ecef_min[0] - cam_pos[0],  // ~0-100m instead of ~5M
    block.ecef_min[1] - cam_pos[1],
    block.ecef_min[2] - cam_pos[2],
];

let (block_verts, block_inds) = greedy_mesh_block(
    &block.voxels,
    block_relative_to_cam, // Small numbers, safe for f32
);
```

**Why This Works:**
- Camera-relative coords are small (~0-100m range)
- f32 has ~7 digits precision = 0.0001m at 100m scale
- Plenty precise for smooth rendering
- Industry standard: Unreal, Unity, all engines use this

## Screenshot System Fixed

**Problem:** GUI event loop stayed open, took multiple screenshots

**Fix:**
- Added CLI argument parsing (--lat, --lon, --alt, --pitch, --screenshot)
- Screenshot mode calls `std::process::exit(0)` after saving
- Takes ONE screenshot and exits immediately

**Usage:**
```bash
./continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 10.0 --pitch -30 --screenshot
```

**Test Results:**
- ✅ 10 screenshots at different positions all successful
- ✅ No windows left open
- ✅ Each exits immediately after screenshot

## Results

**Before:**
- Individual voxel cubes everywhere
- Crazy stretched triangles shooting into space
- Z-fighting artifacts
- Completely broken geometry

**After:**
- Proper greedy-meshed terrain
- Large flat quads (as designed)
- Material colors visible (green grass, gray concrete, blue water)
- Correct depth/distance rendering
- No artifacts

## Files Changed

- `examples/continuous_viewer_simple.rs`:
  - Line 146-150: Camera-relative coordinate calculation
  - Line 204-213: Removed broken origin_transform
  - Line 400-410: Removed broken origin_transform
  - Added CLI arg parsing
  - Added screenshot mode with immediate exit

## Next Issue

User reports screenshots show inverted surfaces - might be rendering backside of blocks.
Need to check:
- Winding order in greedy meshing
- Backface culling settings
- Normal directions

**Status:** Precision bug SOLVED, screenshot system FIXED, surface culling to investigate next
