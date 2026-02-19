# Missing Terrain Data Analysis

## Current Surface Detection
- Range: -2.0m to +1.0m from SRTM elevation
- Surface layer: -0.5m to +1.0m (GRASS)
- Sub-layer 1: -1.5m to -0.5m (DIRT)  
- Sub-layer 2: -2.0m to -1.5m (STONE)

## Potential Issues

### 1. Block Boundary Gaps
If terrain elevation is between two 8m blocks, surface might fall through gap:
- Block A: 0-8m elevation
- Block B: 8-16m elevation
- Terrain at 7.8m might miss both blocks

### 2. Surface Detection Too Narrow
Current: Only fills voxels within 3m vertical range
- Voxel size: 1m
- Block size: 8m vertical
- If surface crosses block at edge, might miss entirely

### 3. Elevation Sampling Position
Currently sampling at block center (Z=4m):
```rust
z: ecef_min[2] + 4.0, // Middle of block
```
This might not represent the actual surface position in that block.

## Proposed Fixes

### Option A: Sample elevation per voxel XYZ (expensive but accurate)
- Revert to per-voxel elevation query
- But only fill surface ±2m range

### Option B: Expand surface range to ±4m
- Ensures overlap between adjacent blocks
- More conservative, won't miss surfaces

### Option C: Sample multiple heights per XY column
- Check elevation at min, mid, max Z of block
- Fill if ANY sample is near surface

### Option D: Add hysteresis - thicker surface layer
- Make surface detection more forgiving: -3m to +2m range
- 5 layers thick instead of 3

## Recommended: Option D (simple + effective)
Change thresholds to catch more edge cases without expensive queries.
