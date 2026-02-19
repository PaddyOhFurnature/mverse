# Diagnosis: Why Greedy Meshing Shows Individual Blocks

## Observations from Screenshots
1. Can see individual voxel blocks in checkerboard pattern
2. Blocks appear as individual cubes, not merged terrain
3. Looks like Minecraft without greedy meshing

## Data Analysis
- 7959 blocks queried in 100m radius
- 2,188,249 voxels (primitives) total
- Average: 275 voxels per block (out of 512)
- Blocks are ~50% full

## Output Metrics
- 1,001,148 vertices
- Average: 125 vertices per block
- With 512 voxels/block × 50% filled = 256 voxels
- 256 voxels × 24 vertices/voxel (6 faces × 4 verts) = 6,144 vertices if NO merging
- Actual: 125 vertices per block

**WAIT - this means greedy meshing IS working!** (98% reduction per block)

## But Why Do Screenshots Show Individual Blocks?

Theory: We're rendering SOLID FILLED blocks, not terrain surfaces!

**Problem:** Terrain generator is creating completely filled 8×8×8 cubes of material, not hollow surfaces.

**What we should have:** Mostly air blocks with surface voxels only  
**What we actually have:** Solid blocks of grass/stone

**Why screenshots show checker pattern:**  
- Each 8m block is a solid cube
- Greedy meshing merges within a block
- But adjacent blocks don't merge across boundaries
- Result: Grid of 8×8×8m cubes = checkerboard pattern

## The Real Bug

Terrain generation is filling entire blocks instead of creating surfaces!

Check: `src/procedural_generator.rs` - is it filling blocks or creating surfaces?
