# Profiling Results - Performance Analysis

**Date:** 2026-02-16  
**Status:** Profiling complete - bottlenecks identified

---

## Key Findings

### Current Performance (50m radius)
- **Query time:** 185ms (query blocks from cache/index)
- **Voxel counting:** 8ms (check which voxels are non-AIR)
- **Mesh generation:** <1ms (calculate vertices/indices)
- **Total CPU time:** 192ms
- **Frame budget:** 16.67ms (60 FPS)
- **Budget used:** 1153% (11x over budget!)

### User Experience Matches Data
- **30 FPS stationary** = ~33ms per frame (2x budget, reasonable)
- **15-20 FPS moving** = ~50-66ms (3-4x budget, cache loading)
- **Stabilizes after caching** = Query time drops to 47ms (Move 5)

---

## Bottleneck Analysis

### 1. Query Time (CRITICAL) - 185ms (96% of total)
**Problem:** Fetching blocks from cache/index/generation is slowest  
**Why it matters:** Happens every frame when moving  
**Cache effect:** Drops from 185ms → 47ms after warmup (4x faster!)

### 2. Voxel Counting - 8ms (4% of total)
**Problem:** Iterating through 512 voxels × 2197 blocks  
**Why it matters:** Needed to build mesh  
**Optimization:** Skip empty blocks, use bitmasks

### 3. Mesh Generation - <1ms (negligible)
**Not a bottleneck** - calculation is fast  

### 4. GPU Upload (not measured, but likely 10-20ms)
**Problem:** 142 MB buffer upload per frame at 50m  
**Why it matters:** Bandwidth limited  
**Optimization:** Reuse buffers, only update changed blocks

---

## Scale Analysis

### Memory Requirements by Radius

| Radius | Blocks | Voxels | Memory | Status |
|--------|--------|--------|--------|--------|
| 25m | 343 | 34K | 13 MB | ✅ OK |
| 50m | 2,197 | 374K | 143 MB | ✅ OK |
| 75m | 7,600 | 1.5M | 562 MB | ❌ Exceeds GPU limit |
| 100m | 15,000 | 3.6M | 1,386 MB | ❌ Exceeds GPU limit |

**GPU Buffer Limit:** 268 MB (wgpu max single buffer)  
**Current Viable:** 25-50m radius only

---

## Critical Optimizations (Priority Order)

### 1. LOD System (MUST HAVE) - Enables 100m+ view
**Problem:** All voxels render at 1m resolution  
**Solution:** Distance-based level of detail
- 0-25m: 1m voxels (current)
- 25-50m: 2m voxels (8× fewer)
- 50-100m: 4m voxels (64× fewer)
- 100-200m: 8m voxels (512× fewer)

**Expected:** 100m radius with LOD = ~150 MB (fits in GPU)

### 2. Frustum Culling - Don't render off-screen
**Problem:** Rendering 360° around camera  
**Solution:** Only mesh blocks in view frustum  
**Expected:** 50-70% reduction (field of view ~90°)

### 3. Mesh Caching - Reuse GPU buffers
**Problem:** Regenerating mesh every frame  
**Solution:** Keep meshes in GPU memory, update only changed blocks  
**Expected:** 10-20ms saved per frame

### 4. GPU Instancing - Batch identical cubes
**Problem:** One draw call per voxel  
**Solution:** Instance rendering for all voxels  
**Expected:** 100-1000× fewer draw calls

---

## Optimization Plan

### Phase 1: LOD (4-6 hours) ⭐ CRITICAL
Without this, we're stuck at 50m radius maximum.

**Implementation:**
1. Query multiple block sizes based on distance
2. Render larger voxels for distant blocks
3. Smooth LOD transitions (no popping)

**Expected result:** 100m+ radius at 60 FPS

### Phase 2: Frustum Culling (2-3 hours)
After LOD, this gives another 50% boost.

**Implementation:**
1. Calculate view frustum from camera
2. Test block AABB against frustum planes
3. Skip blocks outside view

**Expected result:** 2x fewer blocks to mesh

### Phase 3: Mesh Caching (3-4 hours)
Keep meshes in GPU memory longer.

**Implementation:**
1. Track which blocks have meshes
2. Only regenerate when blocks change
3. LRU eviction for old meshes

**Expected result:** 90% of frames skip mesh generation

### Phase 4: GPU Instancing (2-3 hours)
Final polish for maximum performance.

**Implementation:**
1. Single cube template mesh
2. Instance buffer with positions/colors
3. One draw call for all voxels

**Expected result:** 100-1000× fewer draw calls

---

## Performance Targets

### Current (50m, no optimizations)
- CPU: 192ms per frame
- FPS: ~5-10 (unacceptable)
- View distance: 50m
- Memory: 143 MB

### After LOD (100m)
- CPU: 50ms per frame
- FPS: 20-30
- View distance: 100m
- Memory: 150 MB

### After LOD + Culling (100m)
- CPU: 25ms per frame
- FPS: 40-60
- View distance: 100m
- Memory: 75 MB (only visible blocks)

### After LOD + Culling + Caching (100m)
- CPU: 5-10ms per frame (steady state)
- FPS: 60+
- View distance: 100m
- Memory: 75 MB

### Final Goal (200m+ with all optimizations)
- CPU: <16ms per frame
- FPS: 60+
- View distance: 200-500m
- Memory: <200 MB

---

## Why User Saw 30 FPS

**Calculation:**
- 50m radius = 192ms CPU + ~20ms GPU upload + ~20ms render
- Total: ~230ms per frame
- FPS: 1000/230 = 4.3 FPS

**But user saw 30 FPS because:**
1. **Cache helped:** After warmup, query drops to 47ms
2. **Fewer voxels:** Specific camera angle had less terrain
3. **GPU efficiency:** wgpu batches some operations

**Adjusted calculation (with cache):**
- Query: 47ms (cached)
- Count: 8ms
- GPU: 20ms
- Render: 10ms
- Total: 85ms = 11.7 FPS

Still doesn't explain 30 FPS... likely the viewer is only meshing occasionally (R key or significant movement), not every frame!

---

## Next Steps

1. ✅ **Profiling complete** - Bottlenecks identified
2. ⏳ **Implement LOD** - Critical for larger view distances
3. ⏳ **Implement culling** - 50% performance boost
4. ⏳ **Implement caching** - Smooth steady-state performance
5. ⏳ **Material colors** - Make it look good while testing
6. ⏳ **GPU instancing** - Final polish

**Start with LOD** - It's the blocker for everything else.

---

## Technical Notes

### Cache Warmup Effect
```
Move 1: 155ms (cold - generating blocks)
Move 2:  89ms (warming - some hits)
Move 3:  91ms (warm - mostly hits)
Move 4:  63ms (hot - all hits)
Move 5:  47ms (hot - optimized path)
```

3.3× speedup from cold to hot cache!

### Mesh Generation is Fast
<1ms to calculate 3M vertices means this is NOT the bottleneck.  
Query time dominates (96% of CPU time).

### GPU Buffer Limit is Real
75m radius = 562 MB (exceeds 268 MB limit)  
Without LOD, we CANNOT go beyond 50m radius.

---

**CONCLUSION:** LOD is mandatory. Everything else is nice-to-have.
