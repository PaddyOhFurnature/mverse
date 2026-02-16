# Next Steps - Phase 3 Optimization

**Current Status:** SRTM data pipeline working ✅  
**Git Tag:** `checkpoint-srtm-pipeline-fixed`  
**Date:** 2026-02-16

## What We Have Now

✅ Real Brisbane terrain data loading (24m-579m elevation range)  
✅ Continuous query system (200× better than target)  
✅ Basic LOD rendering (near: 1m voxels, far: 8m blocks)  
✅ Interactive viewer working (~30 FPS)  
✅ 262,270 primitives from 15,000 blocks in 100m radius

**User Feedback:** "i can kind of see it, enough to understand the concept"

## Phase 3 Priorities (Optimization & Visual Quality)

### 1. Material Colors (Highest Impact, Lowest Effort)
**Why first:** Instant visual improvement, helps user understand terrain  
**Task:** Add RGB colors per material ID  
**Implementation:**
- Green grass (0.2, 0.8, 0.2)
- Brown dirt (0.6, 0.4, 0.2)
- Gray stone (0.5, 0.5, 0.5)
- Blue water (0.2, 0.4, 0.8)
- Concrete (0.7, 0.7, 0.7)

**Files to modify:**
- `src/renderer/pipeline.rs` - Add color to Vertex struct
- `src/renderer/shaders/basic.wgsl` - Use vertex color
- `examples/continuous_viewer_simple.rs` - Map material → color

**Estimated effort:** 30 minutes  
**Expected result:** Terrain looks like terrain, not gray blocks

---

### 2. Greedy Meshing (Major Performance Win)
**Why second:** 10-100× triangle reduction for solid terrain  
**Current:** Every voxel = 6 faces = 12 triangles (wasteful)  
**Better:** Merge adjacent same-material faces into large quads

**Research:**
- [0fps.net - Meshing in a Minecraft Game](https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/)
- [Greedy Meshing Algorithm](https://devforum.roblox.com/t/consume-everything-how-greedy-meshing-works/452717)

**Implementation approach:**
1. Generate voxel grid for block
2. For each axis (X, Y, Z):
   - Sweep through slices
   - Find rectangular regions of same material
   - Emit quad for each region (not per voxel)
3. Benefits:
   - Flat terrain: 1 quad instead of 1000 voxels
   - Reduces vertices from millions to thousands
   - Massive FPS improvement

**Files to create:**
- `src/renderer/greedy_mesh.rs` - Meshing algorithm
- Tests for correctness (flat plane, stairs, mixed materials)

**Estimated effort:** 2-3 hours  
**Expected result:** 10-100× fewer triangles, 60+ FPS

---

### 3. Frustum Culling (50% Additional Savings)
**Why third:** Don't render what camera can't see  
**Current:** Rendering blocks behind camera (wasted)

**Implementation:**
- Extract camera frustum planes from projection matrix
- Test AABB (block bounds) against frustum planes
- Only render blocks inside frustum

**Files to modify:**
- `src/renderer/camera.rs` - Add frustum extraction
- `examples/continuous_viewer_simple.rs` - Filter blocks by frustum

**Estimated effort:** 1 hour  
**Expected result:** ~50% fewer blocks rendered

---

### 4. Skybox & Better Lighting
**Why fourth:** Visual context and depth perception  
**Current:** Black background is disorienting

**Implementation:**
- Simple gradient skybox (blue→cyan at horizon)
- Directional sunlight (consistent shadows)
- Ambient occlusion approximation

**Files to modify:**
- `src/renderer/shaders/basic.wgsl` - Lighting calculations
- `examples/continuous_viewer_simple.rs` - Render skybox

**Estimated effort:** 1-2 hours  
**Expected result:** Easier to understand spatial relationships

---

### 5. UI/HUD for Debugging
**Why last:** Developer experience, not user-facing yet

**Display:**
- FPS counter
- GPS coordinates (lat, lon, elevation)
- Voxel count (rendered / total)
- Cache hit rate
- Memory usage

**Implementation:**
- Use `egui` crate (immediate mode GUI)
- Overlay on 3D viewport

**Estimated effort:** 2 hours  
**Expected result:** Better debugging and performance monitoring

---

## Recommended Order

1. ✅ **Material colors** (30 min) - Quick visual win
2. ⏭️ **Greedy meshing** (2-3 hr) - Major performance boost
3. **Frustum culling** (1 hr) - Additional optimization
4. **Skybox/lighting** (1-2 hr) - Visual polish
5. **UI/HUD** (2 hr) - Developer experience

**Total estimated time:** 7-9 hours of focused work

## Success Criteria

✅ Terrain is visually distinct (green grass, brown dirt)  
✅ 60 FPS sustained at ground level  
✅ 100-200m view distance working smoothly  
✅ User can explore and understand the space  

## Research Resources

**Greedy Meshing:**
- https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/
- https://github.com/roboleary/GreedyMesh (reference implementation)
- https://voxels.blogspot.com/2011/12/meshing-greedy.html

**Frustum Culling:**
- https://learnopengl.com/Guest-Articles/2021/Scene/Frustum-Culling
- Fast AABB-plane test (SAT theorem)

**Voxel Rendering:**
- https://developer.nvidia.com/gpugems/gpugems3/part-v-physics-simulation/chapter-33-lcp-algorithms-collision-detection
- https://research.nvidia.com/publication/2010-02_efficient-sparse-voxel-octrees

## User Directive

> "lets move on.. remember to keep on track, update and stay familiar with documentation, research what ever is required"

**Interpretation:** 
- Move forward with optimization
- Document decisions and progress
- Research best practices before implementing
- Keep code quality high (tests, documentation)

## Next Session Goals

1. Implement material colors (quick win)
2. Start greedy meshing research and implementation
3. Take screenshots at each stage to show progress
4. Update documentation with findings

---

**Ready to proceed with Phase 3!** 🚀
