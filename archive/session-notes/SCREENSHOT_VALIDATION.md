# Screenshot Validation - User Instructions

**Goal:** Visual confirmation that the continuous query system generates correct terrain.

---

## Quick Start

### Option 1: Manual (Recommended)
```bash
cd /home/main/metaverse/metaverse_core
export OPENTOPOGRAPHY_API_KEY=3e607de6969c687053f9e107a4796962
cargo run --example continuous_viewer_simple
```

**In the viewer:**
1. Window opens showing Kangaroo Point, Brisbane
2. Click left mouse button to capture cursor
3. Look around with mouse
4. Press **F5** to capture screenshot
5. Move with WASD + Space/Shift
6. Press **F5** again from different angle
7. Press **ESC** to exit

**Screenshots saved to:** `screenshot/continuous_<timestamp>_<lat>_<lon>.png`

### Option 2: Automated (Requires xdotool)
```bash
./capture_continuous_screenshots.sh
```

This will:
- Build the viewer
- Launch it
- Wait 8 seconds
- Auto-press F5
- Auto-press ESC
- List saved screenshots

---

## What You Should See

### Expected Rendering

**Red Cubes:**
- You should see **red cubic blocks** scattered around
- Each cube = 8m × 8m × 8m block with voxels
- ~8 cubes visible at Kangaroo Point starting position
- Cubes represent roads from OpenStreetMap data

**Background:**
- Sky blue color (RGB: 135, 206, 235)
- Clear view to horizon

**Camera Info (Title Bar):**
```
Continuous Viewer - 60 FPS | (-27.479769°, 153.033586°) 20m
```

### What the Red Cubes Represent

These are **OSM road segments** at Kangaroo Point:
- River Terrace
- Captain Cook Bridge approach roads
- Surrounding street network

The system has voxelized these roads into 8m blocks. The validation test found **753 non-AIR voxels (ASPHALT material)** across **37 blocks**, and the viewer renders 8 of those blocks as red cubes.

---

## Verification Steps

### 1. Check Screenshot Exists
```bash
ls -lh screenshot/continuous_*.png
```

Should show one or more PNG files with GPS coordinates in filename.

### 2. Open Screenshot
```bash
# Linux:
xdg-open screenshot/continuous_*.png

# Or specify image viewer:
eog screenshot/continuous_*.png
feh screenshot/continuous_*.png
```

### 3. Visual Inspection Checklist

**✓ Red cubes visible?**
- YES → Generation working
- NO → Camera in wrong location or generation broken

**✓ Cubes form linear patterns?**
- YES → Roads are being detected correctly
- NO → May be random noise or wrong features

**✓ Cubes at ground level?**
- YES → Elevation calculation correct
- NO → Terrain height wrong

**✓ Multiple cubes visible?**
- YES → Spatial queries working
- NO → Query radius too small or camera position wrong

### 4. Compare to Satellite View

**Kangaroo Point Location:**
- Latitude: -27.479769°
- Longitude: 153.033586°
- Reference: https://www.google.com/maps/@-27.479769,153.033586,17z

**Expected Features:**
- River Terrace (curved road along Brisbane River)
- Captain Cook Bridge road connections
- Residential streets
- Park areas (no roads = no cubes there)

**Validation:**
- Do cube positions match where roads appear on satellite view?
- Are cubes absent where there are no roads (parks, water)?

---

## What Different Results Mean

### ✅ PASS: Cubes visible in correct locations
**Interpretation:** System works!
- Procedural generation generates voxels
- Continuous queries return correct blocks
- Spatial positioning accurate
- Ready for Phase 3

**Next:** Continue to Phase 3 (performance tuning, LOD, etc.)

---

### ⚠️ PARTIAL: Cubes visible but positions seem wrong
**Interpretation:** Generation works but accuracy issues
- May need to adjust coordinate transformations
- May need to tune voxelization parameters
- May need better SRTM elevation data

**Next:** Debug coordinate accuracy, fix issues, re-test

---

### ❌ FAIL: No cubes visible (empty scene)
**Interpretation:** Critical bug still exists
- Generation may still be broken
- Query system may not return data
- Mesh conversion may be broken

**Next:** Debug with validation tools, check logs, fix issues

---

### ❌ FAIL: Viewer doesn't start or crashes
**Interpretation:** Rendering issue
- Check GPU/driver support
- Check error messages
- May need to debug wgpu initialization

**Next:** Review error logs, fix renderer issues

---

## Troubleshooting

### Viewer won't start
```bash
# Check if compiling:
cargo build --example continuous_viewer_simple

# Check dependencies:
cargo tree | grep -E "(winit|wgpu|image)"
```

### No screenshots saved
```bash
# Check directory exists:
mkdir -p screenshot

# Check permissions:
ls -la screenshot/

# Try manual save test:
# In viewer, press F5 and check console output
```

### Can't see anything (black screen)
**Possible causes:**
1. Camera inside terrain (move up with Space)
2. All blocks empty (wrong query location)
3. Mesh not generating (check console for "0 vertices")

**Debug:**
- Check console output for "Generated X vertices"
- Press 'R' to reload mesh
- Move camera with WASD

### Performance issues (low FPS)
**If <30 FPS:**
- Normal for first run (shader compilation)
- Check GPU usage
- May need to reduce query radius

---

## Console Output Reference

### Normal Startup
```
=== Continuous Query Viewer ===
Location: Kangaroo Point, Brisbane

Camera at Kangaroo Point:
  GPS: (-27.479769°, 153.033586°, 20m)
  ECEF: (-5046957.0, 2567827.6, -2925527.7)

Initializing renderer...
✓ Renderer ready
✓ Pipeline ready
✓ World created

[Mesh Update]
  Queried 1820 blocks
  Generated 384 vertices, 1728 indices
  ✓ Mesh updated

✓ Ready to render!
```

### Screenshot Capture (F5)
```
[F5] Capturing screenshot...
  ✓ Screenshot saved: screenshot/continuous_1739690400_-27.479769_153.033586.png
```

### Mesh Reload (R)
```
[R] Reloading mesh...

[Mesh Update]
  Queried 1820 blocks
  Generated 384 vertices, 1728 indices
  ✓ Mesh updated
```

---

## What to Report Back

**If validation passes:**
"Screenshots look good, cubes match roads on satellite view."

**If validation has issues:**
"Screenshots show [describe what you see]. Expected [describe what should be there]."

**Include:**
1. Screenshot files (attach or describe)
2. Console output (any errors or unexpected messages)
3. Visual comparison to satellite imagery
4. Any observations about positioning accuracy

---

## Next Steps After Validation

### If PASS → Phase 3
- Commit checkpoint
- Update documentation
- Begin Phase 3: Performance tuning
  - Streaming system
  - LOD (Level of Detail)
  - Chunked voxel loading
  - Mesh optimization

### If FAIL → Debug
- Review logs and screenshots
- Identify specific issues
- Fix bugs
- Re-test until passing
- Then Phase 3

---

## Summary

You're validating that:
1. ✅ Code compiles and runs
2. ✅ Viewer opens and renders
3. ✅ Geometry appears in 3D space
4. ✅ Positions match real-world roads
5. ✅ System performance acceptable (60 FPS)

This visual validation is **critical** - it catches bugs that unit tests miss. Once screenshots confirm correctness, we can confidently proceed to building more features.
