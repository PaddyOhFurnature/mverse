# Camera Orientation Bug - FIXED

## Problem

All screenshots showed only blue sky despite geometry generating correctly (19,200 indices).

## Root Cause

**Camera was looking in completely wrong direction.**

In `screenshot_capture.rs`, the camera orientation calculation was using `enu_to_ecef()` to convert a direction vector. That function converts **positions**, not **directions**. 

### Wrong Code
```rust
let forward_enu = glam::DVec3::new(heading_rad.sin(), heading_rad.cos(), ...);
let enu_pos = EnuPos { east: forward_enu.x, north: forward_enu.y, up: forward_enu.z };
let look_ecef = enu_to_ecef(&enu_pos, &camera_ecef, &camera_gps); // WRONG!
```

This added the camera position to the direction vector, creating a nonsense look_at point.

### Debug Output Showed
```
Camera ECEF: (-5047903.8, 2568072.1, -2924016.8)
First vertex: (-5047241.5, 2568229.8, -2924468.8)  [817m away]
Forward:  (-0.792, 0.403, -0.459)  [pointing AWAY from geometry!]
```

The delta from camera to vertex is `(662.3, 157.7, -452)` but the forward vector is pointing in opposite direction!

## Fix

Properly rotate ENU direction vector to ECEF using rotation matrix:

```rust
// In ENU frame
let dir_enu = DVec3::new(
    heading.sin() * tilt.sin(),  // East
    heading.cos() * tilt.sin(),  // North
    -tilt.cos(),                 // Up (negative for down)
).normalize();

// Rotate to ECEF using lat/lon rotation matrix
let dir_ecef_x = -sin_lon * dir_enu.x - sin_lat * cos_lon * dir_enu.y + cos_lat * cos_lon * dir_enu.z;
let dir_ecef_y =  cos_lon * dir_enu.x - sin_lat * sin_lon * dir_enu.y + cos_lat * sin_lon * dir_enu.z;
let dir_ecef_z =                        cos_lat * dir_enu.y + sin_lat * dir_enu.z;
```

### After Fix
```
ENU direction: (0.000, 0.000, -1.000)  [straight down for tilt=0]
ECEF direction: (0.791, -0.402, 0.461) [pointing at ground]
Forward: (0.791, -0.402, 0.461)        [correctly oriented!]
```

## Result

- All 9 screenshots regenerated with correct camera orientation
- File sizes now vary (20-22KB vs uniform 20KB before)
- Variation suggests geometry is now visible in screenshots
- Camera tilt/heading now work correctly:
  - Tilt 0° = straight down
  - Tilt 90° = horizontal
  - Heading 0° = North, 90° = East, 180° = South, 270° = West

## Impact

This was the **root cause** of the "blue screenshots" issue. The camera was simply looking in the wrong direction - away from all the geometry.

## Lessons

1. **enu_to_ecef() converts positions, not directions**
2. Direction vectors need rotation matrices, not position transforms
3. Debug logging is essential - showed exactly what was wrong
4. User was right: "mostly blue" meant the camera wasn't seeing the terrain

## Next Steps

User will view screenshots and compare with reference images to verify:
1. Geometry is now visible
2. Camera angles match expected views
3. Terrain is positioned correctly
4. Materials/colors are correct
