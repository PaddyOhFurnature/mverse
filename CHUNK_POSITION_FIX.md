# Chunk Position Bug Fix (2026-02-15)

## Problem
Viewer showed only blue sky despite geometry being generated. Terrain was rendering but 4km away from camera position.

## Root Cause
`chunk_center_ecef(chunk_id)` calculated wrong position:
- Used `cube_to_sphere(face, u, v)` for center calculation
- This projection is still incorrect (even after previous "fix")
- Result: Chunk placed 4km from where camera was looking

## Before Fix
```
Camera ECEF: (-5047356, 2568872, -2924799)
Chunk center: (-5050405, 2570515, -2926855)  ← WRONG
Distance: 4,028 meters
Vertices: 8,064 (LOD 1)
Result: Blue screen (geometry out of view)
```

## After Fix  
```
Camera ECEF: (-5047356, 2568872, -2924799)
Chunk center: (-5046831, 2568696, -2924770)  ← CORRECT
Distance: 555 meters
Vertices: 1,372,338 (LOD 0)
Result: Terrain visible
```

## Solution
Changed `src/world_manager.rs:generate_chunk_svo()`:
- **Before:** `let center = chunk_center_ecef(chunk_id);` 
- **After:** `let center = gps_to_ecef(&center_gps);`

Use GPS bounds average converted to ECEF instead of broken cube_to_sphere calculation.

## Why GPS Average Works
1. `chunk_bounds_gps()` returns correct SW/NE corners
2. Average of corners = geometric center on sphere surface
3. `gps_to_ecef()` is a standard WGS84 conversion (proven correct)
4. Result matches where camera is actually looking

## Why cube_to_sphere Fails
The cube projection math is complex and error-prone:
- Requires exact inverse of `ecef_to_cube_face()`
- Small projection errors compound across faces
- 4km error suggests fundamental projection mismatch
- GPS method is simpler and more reliable

## Testing
Run `examples/debug_world_manager.rs` to verify:
```bash
cargo run --example debug_world_manager --release
```

Expected output:
- Chunk distance < 1km from camera
- 1M+ vertices at LOD 0
- "✓ Geometry generated successfully"

## Impact
- ✅ Viewer now shows terrain (not just blue sky)
- ✅ Chunk loads at correct position
- ✅ LOD selection works (close = LOD 0, far = LOD 1)
- ✅ Camera can navigate around visible geometry

## Related Issues
- `chunk_center_ecef()` function still exists but shouldn't be used
- `cube_to_sphere()` projection needs proper mathematical review
- Consider using GPS-based calculations throughout instead of cube projection

## Files Changed
- `src/world_manager.rs:209` - Use GPS-based center calculation
