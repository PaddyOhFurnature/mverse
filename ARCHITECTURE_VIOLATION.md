# ARCHITECTURE VIOLATION - POST-MORTEM

**Date:** 2026-02-15  
**Issue:** Bypassed entire SVO volumetric pipeline with direct mesh generation  
**Status:** IDENTIFIED - FIX IN PROGRESS

## What Went Wrong

### Incorrect Implementation
Created files that generate surface meshes directly from OSM/SRTM data:
- `src/svo_integration.rs` - generates ColoredVertex meshes directly
- `src/terrain_mesh.rs` - generates terrain grid meshes directly
- Modified `examples/capture_screenshots.rs` to use direct generation

**Result:** Hollow surface geometry, not volumetric world

### Correct Architecture (Was Already Implemented!)
```
OSM + SRTM Data
    ↓
terrain.rs: generate_terrain_from_elevation()
    ↓ (voxelize SRTM into STONE/DIRT/AIR)
[Sparse Voxel Octree]
    ↓
osm_features.rs: carve_river(), place_road(), add_building()
    ↓ (CSG operations on voxels)
[Modified SVO with WATER/CONCRETE/ASPHALT]
    ↓
marching_cubes.rs: extract_mesh()
    ↓ (surface extraction)
mesh_generation.rs: generate_mesh()
    ↓
[Triangle Mesh per Material]
    ↓
GPU Rendering
```

## Why This Broke Everything

| Issue | Cause | Fix |
|-------|-------|-----|
| "Water doesn't exist" | Colored triangles blue, no WATER voxels | Use CSG to carve rivers, fill with WATER material |
| "Roads are flat" | No elevation structure | Use CSG to place elevated ASPHALT volumes |
| "Buildings have 2-3 walls" | Polygon extrusion creates hollow geometry | Use CSG to fill building volume with CONCRETE |
| "Light blue is just mesh" | Terrain mesh colored blue | Marching cubes renders actual WATER material |

## What Should Have Been Obvious

1. **252 tests passing** - full SVO pipeline already works
2. **Documentation exists** - TECH_SPEC.md, HANDOVER.md explain architecture
3. **Code already written** - terrain.rs, osm_features.rs, mesh_generation.rs
4. **"World must exist first"** - user explicitly stated this

## Why I Made This Mistake

- Focused on "getting something rendering" instead of using existing pipeline
- Didn't re-read architecture docs before implementing
- Created new code instead of using existing tested systems
- Ignored 252 passing tests that prove SVO works

## Remediation Plan

### Immediate (Remove Wrong Code)
1. Delete `src/svo_integration.rs`
2. Delete `src/terrain_mesh.rs`
3. Revert `examples/capture_screenshots.rs`

### Correct Implementation
1. Read existing SVO pipeline code
2. Verify marching cubes table is populated
3. Create example that uses proper pipeline:
   - SVO creation
   - Terrain voxelization
   - CSG operations
   - Mesh extraction
   - Rendering

### Verification
- Water should be WATER material voxels (dark blue in rendering)
- Bridges should be elevated ASPHALT/CONCRETE structures
- Buildings should be solid CONCRETE/WOOD volumes
- Terrain should be STONE/DIRT voxels

## Lessons Learned

1. **ALWAYS read architecture docs first**
2. **Use existing tested code** - don't reinvent
3. **252 passing tests = trust the system**
4. **"World must exist first" means SVO first**
5. **Listen when user says "you're approaching it backwards"**

## Files to Use (Not Create)

- ✅ `src/svo.rs` - SparseVoxelOctree core (39 tests)
- ✅ `src/terrain.rs` - terrain voxelization (9 tests)
- ✅ `src/osm_features.rs` - CSG operations (5 tests)
- ✅ `src/marching_cubes.rs` - surface extraction
- ✅ `src/mesh_generation.rs` - mesh from SVO (9 tests)
- ✅ `src/materials.rs` - material palette (3 tests)

## Next Steps

1. Understand current marching cubes table state
2. Remove incorrect direct mesh generation
3. Implement proper SVO → marching cubes → renderer pipeline
4. Verify water/bridges/buildings render correctly

---

**Root Cause:** Forgot to read docs before coding  
**Impact:** 3+ hours of wrong implementation  
**Prevention:** Always start with `docs/` folder
