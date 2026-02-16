# Chunk Boundary Investigation - ROOT CAUSE FOUND

## Test Results

### East/West Boundaries: PERFECT ✓
- Camera chunk east edge: lon 153.02795018°
- East neighbor west edge: lon 153.02795018°
- Gap: **0.0m** (f64 precision perfect alignment)

### North/South Boundaries: BROKEN ✗
- Camera chunk north edge: lat -27.46454086°
- North neighbor south edge: lat -27.46568657°
- Gap: **128.4m** (MASSIVE misalignment)

## Root Cause

**Quad-sphere chunking on face 1 (Asia) creates non-square chunks.**

When projecting cube UV coordinates onto sphere:
- UV square [-1,1] × [-1,1] on cube face
- Quadtree subdivides UV space evenly
- But sphere projection distorts geometry
- Result: Chunks are rectangular, not square
- Different aspect ratios in lat vs lon

At Brisbane (face 1, depth 14):
- Camera chunk: 676.8m north-south × 547.1m east-west
- Ratio: 1.24:1 (24% elongated)

### Why East/West Perfect But North/South Has Gap?

Looking at neighbor finding logic in chunks.rs:
```rust
// East neighbor: u_mid + u_size (moves in UV space)
// North neighbor: v_mid + v_size (moves in UV space)
```

UV space is UNIFORM on cube, but GPS space is DISTORTED on sphere.

**Hypothesis**: The neighbor calculation assumes uniform UV grid, but:
1. Moving +u_size in UV doesn't equal moving +lat_size in GPS
2. Sphere distortion causes cumulative errors
3. East/west happens to work (maybe longitude wrapping helps?)
4. North/south accumulates misalignment

## The Real Problem

Not coordinate system errors - **quad-sphere chunking math needs revision**.

Current approach:
1. Define chunks in UV space (cube coordinates)
2. Project to sphere (ECEF)
3. Convert to GPS for bounds

This creates gaps because UV space quadtree doesn't map cleanly to spherical geodesic grid.

## Why User Is Right

> "you cant walk down the street and be teleported 2m because you crossed a chunk line"

With 128m gaps between chunks, structures WOULD have discontinuities:
- Building spanning boundary: 128m gap in middle
- Road crossing boundary: 128m missing section
- Player walking north: teleports 128m when chunk loads

This absolutely breaks the 1:1 world fidelity.

## Solution Options

### Option 1: Fix Quad-Sphere Math (HARD)
Account for sphere distortion in neighbor calculations:
- Calculate geodesic distance for chunk size
- Adjust UV stepping to maintain GPS continuity
- Complex math, may introduce new issues

### Option 2: Use GeodeticCHUNKING (BETTER)
Instead of UV quadtree, use lat/lon grid:
- Define chunks as lat/lon rectangles (e.g., 0.006° × 0.006°)
- Chunks align to GPS grid naturally
- No projection distortion issues
- Neighbors trivial: ±0.006° lat or lon

**Problem**: Chunks not uniform size (cos(lat) scaling)
- Equator chunks: ~667m × ~667m
- Brisbane chunks: ~667m × ~545m (18% narrower)
- Polar chunks: ~667m × ~0m (infinitely narrow!)

### Option 3: Hybrid Approach (PRAGMATIC)
- Keep quad-sphere for spatial organization (face + path addressing)
- But calculate chunk boundaries from GEODESIC centers
- Each chunk gets GPS center + radius
- Neighbors overlap by 1 voxel intentionally
- Marching cubes uses overlapping data → seamless

### Option 4: Accept Overlap, Handle In Generation (CURRENT FIX)
The 128m gap might actually be OVERLAP (chunks extend past theoretical boundaries).

If chunks overlap:
- Generate terrain in BOTH chunks for overlap region
- Marching cubes sees same voxels from both sides
- Meshes naturally match at boundary
- "Gap" is visual artifact from missing neighbor data, not actual gap

## Recommended Fix

**Option 4 + enable multi-chunk loading.**

Test hypothesis:
1. Load camera chunk + north neighbor
2. Generate terrain in BOTH
3. If they overlap, terrain should be identical in overlap
4. Marching cubes should generate seamless mesh

If this works, "gap" was never coordinate misalignment - just marching cubes needing neighbor data (which I originally thought, but for wrong reason).

## Next Action

Enable 3x3 chunk loading, test if boundaries become seamless.

If yes: Coordinate system is fine, just need neighbor data.
If no: Quad-sphere chunking math needs deeper fix.
