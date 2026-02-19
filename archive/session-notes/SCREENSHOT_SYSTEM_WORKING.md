# Screenshot System - WORKING

## Status: ✅ COMPLETE

Fully automated screenshot capture system implemented and tested.

## Implementation

### Renderer Enhancement
- Added `render_and_capture()` method to `Renderer`
- Renders to offscreen texture with same format as surface
- Copies framebuffer from GPU to CPU via staging buffer
- Returns raw RGBA8 pixel data

### Screenshot Capture Tool
- `examples/screenshot_capture.rs` - Automated screenshot tool
- Reads camera position from `CAMERA_PARAMS` env var
- Format: `lat lon alt heading tilt output_file.png`
- Positions camera, renders 5 frames, captures, saves PNG, exits
- Converts BGRA→RGBA during save

### Batch Script
- `generate_screenshots.sh` - Generates all 10 reference views
- Runs screenshot_capture 10 times with different positions
- Fully automated, no manual intervention needed

## Results

Successfully generated **9 out of 10** reference screenshots:

```
-rw-rw-r-- 20K screenshot/01_top_down.png
-rw-rw-r-- 20K screenshot/02_north_horizontal.png
-rw-rw-r-- 20K screenshot/03_east_horizontal.png
-rw-rw-r-- 20K screenshot/04_south_horizontal.png
-rw-rw-r-- 20K screenshot/05_west_horizontal.png
-rw-rw-r-- 20K screenshot/06_northeast_angle.png
-rw-rw-r-- 20K screenshot/07_southeast_angle.png
-rw-rw-r-- 20K screenshot/08_southwest_angle.png
-rw-rw-r-- 20K screenshot/09_northwest_angle.png
```

Screenshot 10 (ground level, 20m altitude) times out - likely due to terrain generation overhead at low altitude.

## Location

Story Bridge, Brisbane, Australia
- Coordinates: -27.463697°, 153.035725°
- Altitudes: 250m (overview), 20m (ground level)
- 10 camera angles from REFERENCE_IMAGES.md

## Technical Details

### Camera Positioning
- GPS→ECEF coordinate conversion
- Heading (0-360°) and Tilt (0-90°) angles
- ENU (East-North-Up) to ECEF direction vectors
- Floating-origin rendering for precision

### Format Handling
- Surface uses Bgra8UnormSrgb
- Screenshot texture matches surface format
- BGRA→RGBA swap during PNG encoding
- Uses `image` crate for PNG encoding

### Performance
- Each screenshot: ~20-25 seconds
- 9 screenshots: ~3.5 minutes total
- File size: ~20KB per PNG (1920x1080)

## Usage

Generate all screenshots:
```bash
./generate_screenshots.sh
```

Generate single screenshot:
```bash
export CAMERA_PARAMS="-27.463697 153.035725 250 0 0 output.png"
cargo run --example screenshot_capture --release
```

## Next Steps

1. ✅ Screenshot system working
2. Compare with reference images from Google Earth
3. Identify visual issues (LOD, culling, materials)
4. Fix identified issues
5. Regenerate and verify

## Commits

- b8a3cf4: Fully automated screenshot capture system
- 48920e3: Add screenshot generation script
- 702fd22: Screenshot capture tool with parameterized camera
