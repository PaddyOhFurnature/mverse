# Continuous Queries: Full Volumetric Generation Required

**Date:** 2026-02-17  
**Branch:** feature/continuous-queries-prototype

## Problem

Current `ProceduralGenerator` only generates 30m surface shell, cannot represent cliffs/caves/waterfalls.

## Solution For Continuous Query Architecture

Generate **full vertical column** for each VoxelBlock (8×8×8m), not just thin surface.

### Current (Broken)
```rust
// Only fill if near SRTM ground elevation
if dist_from_surface >= -20.0 && dist_from_surface < 10.0 {
    // 30m shell only
}
```

### Fixed (Needed)
```rust
// Fill ENTIRE block based on altitude vs SRTM
for z in 0..8 {
    let voxel_altitude = ecef_to_gps(voxel_ecef).elevation_m;
    let ground_elev = get_elevation(lat, lon);
    
    voxels[idx] = match () {
        _ if voxel_altitude < ground_elev - 50.0 => STONE,  // Deep
        _ if voxel_altitude < ground_elev - 2.0  => DIRT,   // Shallow
        _ if voxel_altitude < ground_elev        => STONE,  // Surface rock (cliffs!)
        _ if voxel_altitude < 0.0                => WATER,  // Below sea level
        _ => AIR,                                           // Above ground
    };
}
```

## Why This Works

- **Cliff:** Blocks at different Z get different fill patterns
  - Block 0-8m: Stone (underground)
  - Block 8-16m: Stone (cliff face)  
  - Block 24-32m: Grass (cliff top)
- **Query:** R-tree finds all blocks in vertical range
- **Render:** Greedy mesh creates continuous wall

## Implementation

Update `src/procedural_generator.rs::fill_terrain()` to remove surface layer constraint and fill full volume.
