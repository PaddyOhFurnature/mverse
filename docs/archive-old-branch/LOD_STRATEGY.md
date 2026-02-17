# LOD Strategy - Proper Voxel Level of Detail

## What Went Wrong (First Attempt)

**Broken approach:** Sample every Nth voxel from dense blocks
- LOD 3 (8m): Sample every 8th voxel = 1 voxel per 8×8×8 block
- Render that 1 voxel as 8m cube = giant glitchy blocks
- Result: Unusable, 1 FPS, crashes at 22m

**Root cause:** Can't sparsely sample from dense voxel data structures.

## Correct Approaches (Research)

### Option 1: Greedy Meshing (Minecraft Method)
- **What:** Merge adjacent same-type voxels into larger quads
- **How:** Iterate over each block face, expand quads greedily
- **Result:** 10-100× fewer triangles without changing data
- **Reference:** https://github.com/vercidium-patreon/meshing
- **Complexity:** Medium (2-3 days implementation)
- **Benefit:** Works at all distances, 10× triangle reduction

### Option 2: POP Buffers (Distance-Based Vertex Clustering)
- **What:** Round vertices to power-of-2 grids based on distance
- **How:** Sort primitives by LOD level, use single buffer
- **Math:** `L_i(v) = 2^i * floor(v / 2^i)` (round to 2^i grid)
- **Reference:** https://0fps.net/2018/03/03/a-level-of-detail-method-for-blocky-voxels/
- **Complexity:** High (4-5 days implementation)
- **Benefit:** Continuous LOD transitions, no popping

### Option 3: Simple Block-Level LOD (Our Current Approach)
- **What:** Render entire far blocks as single cubes (no voxel detail)
- **How:** 
  - 0-50m: Render individual 1m voxels (current system)
  - 50-100m: Render entire 8m blocks as single cubes
  - 100m+: Don't render at all (too far)
- **Complexity:** Low (1 day)
- **Benefit:** Quick win, gets us to 100m radius

### Option 4: True SVO LOD (Future)
- **What:** Store voxel data at multiple resolutions in octree
- **How:** Interior octree nodes store aggregated child data
- **Reference:** NVIDIA SVO papers
- **Complexity:** Very High (2-3 weeks rewrite)
- **Benefit:** Proper hierarchical representation, infinite scales

## Recommended Approach: #3 Then #1

### Phase 1: Block-Level LOD (Quick Win)
**Goal:** Get 100m radius working TODAY

```rust
fn render_with_block_lod(blocks: &[(VoxelBlock, f64)]) {
    for (block, distance) in blocks {
        if distance < 50.0 {
            // Near: render individual 1m voxels (current)
            render_voxels(block);
        } else {
            // Far: render entire block as single 8m cube
            render_block_as_cube(block);
        }
    }
}
```

**Implementation:**
1. Add distance calculation to query
2. Branch in meshing loop based on distance
3. For far blocks: 1 cube per block instead of 512 voxels
4. Use lighter color for far blocks (visual debug)

**Expected results:**
- 0-50m: 374K voxels (same as now)
- 50-100m: 7K blocks = 7K cubes (was 3.2M voxels!)
- **Reduction:** 400× fewer primitives for far terrain
- **Memory:** Fits easily in GPU buffer

### Phase 2: Greedy Meshing (Proper Optimization)
**Goal:** Reduce triangle count 10× for nearby terrain

```rust
fn greedy_mesh_block(block: &VoxelBlock) -> Mesh {
    // For each face direction (6 total)
    for dir in [+X, -X, +Y, -Y, +Z, -Z] {
        // Scan slices perpendicular to direction
        for slice in 0..8 {
            // Greedily expand quads in 2D
            let quads = greedy_mesh_slice(block, dir, slice);
            mesh.extend(quads);
        }
    }
}
```

**Algorithm (per slice):**
1. Mark all voxel faces as "unvisited"
2. For each unvisited face:
   - Start a quad at this face
   - Expand right as far as possible (same voxel type)
   - Expand up as far as possible (maintaining width)
   - Mark all faces in quad as "visited"
   - Add quad to mesh
3. Result: Large quads instead of tiny 1m² faces

**Benefits:**
- 10× fewer triangles near camera
- Better GPU utilization
- Smoother performance

## Implementation Plan

### TODAY: Block-Level LOD
- [x] Add distance to query results
- [ ] Branch meshing loop on distance
- [ ] Test: 100m radius should work
- [ ] Test: FPS should improve 2-3×
- [ ] Screenshot validation

### NEXT WEEK: Greedy Meshing
- [ ] Implement greedy_mesh_slice() algorithm
- [ ] Test on single block
- [ ] Integrate with continuous world
- [ ] Benchmark: expect 10× triangle reduction
- [ ] Visual validation: no artifacts

### FUTURE: Advanced LOD
- Consider POP buffers if smooth transitions needed
- Consider true SVO LOD for infinite scale
- Frustum culling still highest priority after greedy meshing

## Key Insights from Research

1. **Never sample sparse voxels from dense blocks** - doesn't work
2. **Greedy meshing is industry standard** - Minecraft, Voxel engines
3. **LOD must work on GEOMETRY not DATA** - merge quads, don't skip voxels
4. **Distance-based is simpler than screen-space** - good first step
5. **Continuous LOD prevents popping** - POP buffers or geomorphing

## References

- **Greedy Meshing Tutorial:** https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/
- **POP Buffer Paper:** "Rapid Progressive Clustering by Geometry Quantization" (2013)
- **Blocky Voxel LOD:** https://0fps.net/2018/03/03/a-level-of-detail-method-for-blocky-voxels/
- **Implementation Example:** https://github.com/vercidium-patreon/meshing
- **NVIDIA SVO Research:** "Efficient Sparse Voxel Octrees" (2010)
