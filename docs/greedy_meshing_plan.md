# Greedy Meshing Implementation Plan

## Algorithm Understanding (from 0fps.net)

**Core Concept:** Merge adjacent same-material voxel faces into large rectangular quads

**Steps:**
1. For each of 6 face directions (±X, ±Y, ±Z):
   - Sweep through slices perpendicular to that direction
   - Build 2D mask of which voxels have visible faces
   - Greedily merge rectangular regions of same material
   - Emit quad for each merged region

2. Visible face detection:
   - Face is visible if adjacent voxel is different material
   - GRASS→AIR = visible (surface)
   - GRASS→DIRT = visible (boundary)
   - GRASS→GRASS = not visible (internal)

3. Greedy merging (2D):
   ```
   For each unprocessed cell in mask:
     - Extend rectangle horizontally as far as possible
     - Extend rectangle vertically as far as possible
     - Mark all cells in rectangle as processed
     - Emit quad for rectangle
   ```

## Implementation Plan

### Phase 1: Create greedy_mesh module (1 hour)

**File:** `src/renderer/greedy_mesh.rs`

**Functions:**
- `greedy_mesh_block(voxels: &[MaterialId; 512]) -> (Vec<Vertex>, Vec<u32>)`
  - Main entry point
  - Returns vertices and indices for one 8×8×8 block

- `extract_face_mask(voxels, axis, direction, slice_index) -> [[Option<MaterialId>; 8]; 8]`
  - Build 2D mask of visible faces for one slice
  - Check if face should be emitted (material differs from neighbor)

- `greedy_merge_slice(mask) -> Vec<Quad>`
  - Merge 2D mask into rectangular quads
  - Greedy algorithm: extend width first, then height

- `quad_to_vertices(quad, material, axis, direction, slice) -> (4 vertices, 6 indices)`
  - Convert quad to actual vertex positions
  - Apply material color
  - Calculate normal vector

**Tests:**
- Flat plane (one material) → should produce 1 quad per face
- Checkerboard pattern → no merging possible
- Staircase → multiple quads
- Empty block → no faces
- Single voxel in air → 6 faces (cube)

### Phase 2: Replace naive meshing (30 min)

**File:** `examples/continuous_viewer_simple.rs`

**Change near-LOD rendering:**
```rust
// OLD: Loop through voxels, emit 12 triangles each
for each voxel in block {
    if voxel != AIR {
        emit_cube(voxel)  // 12 triangles
    }
}

// NEW: Greedy mesh entire block
let (vertices, indices) = greedy_mesh_block(&block.voxels);
append_to_mesh(vertices, indices);
```

### Phase 3: Test and measure (30 min)

**Metrics to capture:**
- Triangle count before/after
- FPS improvement
- Mesh generation time
- Memory usage

**Expected results:**
- 10-100× fewer triangles for terrain
- 2-5× FPS improvement
- Slightly longer mesh generation (acceptable tradeoff)

## Technical Details

**Block structure:** 8×8×8 voxels = 512 voxels total

**Axis iteration:**
- X-axis: faces perpendicular to X (left/right)
- Y-axis: faces perpendicular to Y (front/back)
- Z-axis: faces perpendicular to Z (top/bottom)

**Index math:**
```rust
// 3D → 1D index
fn voxel_index(x: usize, y: usize, z: usize) -> usize {
    z * 64 + y * 8 + x
}

// Get neighbor for face visibility check
fn neighbor(x, y, z, axis, direction) -> (x', y', z')
```

**Quad representation:**
```rust
struct Quad {
    x: usize,     // Start X in slice
    y: usize,     // Start Y in slice
    width: usize, // Extent in X
    height: usize,// Extent in Y
    material: MaterialId,
}
```

## Next Steps

1. Create `src/renderer/greedy_mesh.rs`
2. Implement core algorithm with tests
3. Integrate into viewer
4. Take before/after screenshots
5. Measure performance improvement
6. Commit with proof

