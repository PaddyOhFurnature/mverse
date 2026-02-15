# CURRENT PROBLEMS - Updated After Screenshot System

**Date:** 2026-02-15  
**Status:** Screenshot system working, new issues identified

## ✅ Fixed: Screenshot System
- Fully automated PNG capture from framebuffer
- `Renderer::render_and_capture()` implemented
- `screenshot_capture` example positions camera and saves PNG
- `generate_screenshots.sh` generates all reference views
- **9 out of 10 screenshots generated successfully**

## 🔴 CRITICAL: Low Altitude Performance Bug

**Problem:** Screenshot #10 at 20m altitude causes complete hang
- FPS drops to 0.1
- Application never completes initialization
- Hangs before reaching "Camera from env" print

**Likely Causes:**
1. Too many chunks visible at low altitude
2. LOD 0 forced everywhere (excessive geometry)
3. Elevation download overflow
4. Infinite loop in chunk loading
5. Memory/geometry buffer overflow

**Impact:** Blocks ground-level rendering entirely

## 🟡 HIGH PRIORITY: Screenshots Mostly Blue

**Problem:** All 9 generated screenshots show mostly blue (sky color)
- Geometry IS being generated (19,200 indices per screenshot)
- Camera positions from CAMERA_PARAMS correctly
- Terrain mesh created and uploaded to GPU

**Possible Causes:**
1. Camera position/orientation incorrect (look_at calculation wrong)
2. Geometry positioned incorrectly in world space
3. Frustum culling too aggressive (culling visible geometry)
4. Depth buffer configuration issue
5. LOD selection placing geometry out of view
6. Floating-origin transform incorrect

**Impact:** Cannot visually verify terrain rendering

## Remaining Known Issues

### Issue 1: Marching Cubes LOD Broken
- LOD 2+ returns ZERO triangles for terrain
- Voxel-skipping misses thin features (1-2 voxels thick)
- Can only use LOD 0-1
- Need: interpolation during sampling OR mesh decimation

### Issue 2: No Frustum Culling
- Rendering ALL geometry regardless of camera view
- User requested this explicitly multiple times
- Status: NOT IMPLEMENTED

### Issue 3: Only 1 Chunk Loads
- `find_chunks_in_range()` only returns camera chunk
- Should be: 9+ chunks (3x3 grid minimum)
- Current: 1 chunk at 291m away

### Issue 4: SVO Resolution
- Current: 512³ voxels for 400m chunk = 0.78m/voxel
- At LOD 1: 1.56m resolution (marginal)
- At LOD 2+: Misses features entirely

## Investigation Plan

### Step 1: Debug Blue Screenshots
1. Add logging to camera view-projection calculation
2. Log vertex positions being sent to GPU
3. Check if geometry is behind camera
4. Verify depth buffer clear value
5. Test with simple known geometry (cube at origin)

### Step 2: Debug Low Altitude Hang
1. Add logging to find_chunks_in_range() - count returned chunks
2. Check LOD distance calculation at 20m altitude
3. Monitor memory usage during initialization
4. Test intermediate altitudes (50m, 100m, 150m)
5. Add chunk count limit / distance limit

### Step 3: Fix Remaining Issues
- Once visual verification works, tackle LOD, culling, multi-chunk

## Current State Summary

- ✅ Screenshot system working (9/10 generated)
- ✅ Chunk positioning fixed (291m from camera)
- ✅ Terrain generates (19,200 indices per shot)
- ❌ **CRITICAL:** 20m altitude hang (0.1fps)
- ❌ **HIGH:** Screenshots show only blue sky
- ❌ Marching cubes LOD 2+ broken
- ❌ No frustum culling
- ❌ Only 1 chunk loads

