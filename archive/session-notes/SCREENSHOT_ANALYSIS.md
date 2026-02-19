# Screenshot Analysis - Issues Found

## Status: Screenshots Generated, Problems Identified

Generated 9 out of 10 reference screenshots successfully.
All screenshots show geometry rendering (19,200 indices per shot).

## Critical Bug Found

**Screenshot #10 (Ground Level, 20m altitude) causes complete hang:**
- Altitude: 20m (vs 250m for others)
- Tilt: 85° (near-horizontal view)
- Symptom: Application hangs during initialization, never reaches "Camera from env" print
- FPS drops to 0.1 according to user
- This suggests a critical performance or infinite loop bug triggered at low altitude

## Likely Causes of Bug

1. **Terrain generation at low altitude** - More chunks visible/needed
2. **LOD selection issue** - Low altitude might trigger LOD 0 (full detail) everywhere
3. **Elevation download** - Ground level might trigger excessive SRTM tile requests
4. **Infinite loop in chunk loading** - find_chunks_in_range() might return too many chunks
5. **Memory issue** - Trying to generate too much geometry

## Next Steps to Debug #10

1. Add logging to find_chunks_in_range() to see how many chunks at 20m
2. Check LOD distance calculation at low altitude
3. Monitor memory usage during initialization
4. Test intermediate altitudes (50m, 100m, 150m) to find threshold
5. Add chunk count limit / distance limit

## Screenshots Generated (01-09)

All at **250m altitude**, Brisbane Story Bridge:
- 01: Top-down (0° heading, 0° tilt)
- 02: North horizontal (0°, 90°)
- 03: East horizontal (90°, 90°)
- 04: South horizontal (180°, 90°)
- 05: West horizontal (270°, 90°)
- 06: NE angle (45°, 45°)
- 07: SE angle (135°, 45°)
- 08: SW angle (225°, 45°)
- 09: NW angle (315°, 45°)

Each: ~20KB, 1280x720, 19,200 indices

## Visual Issues to Investigate

User reports "mostly blue" - need to check screenshots to determine:
1. Is geometry rendering at all?
2. Are coordinates correct?
3. Is camera positioned/oriented correctly?
4. Are materials displaying?
5. Is LOD working correctly?

## Comparison with Reference

Next: Compare screenshot/*.png with reference/*.png from Google Earth
to identify specific visual problems and fix them.
