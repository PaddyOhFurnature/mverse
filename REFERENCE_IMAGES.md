# Reference Image Coordinates

## Location: Queen Street Mall, Brisbane CBD
**GPS Coordinates:** -27.469800, 153.025100, 50m elevation

This is a single location viewed from 6 different angles. Download reference images from Google Earth at these exact coordinates.

---

## View 1: Top Down
**Camera:** Looking straight down (pitch: -90°)
**Google Earth Link:** https://earth.google.com/web/@-27.4698,153.0251,50a,0d,90y,0h,0t,0r
**Filename:** `reference/01_top_down.png`
**What you see:** Bird's eye view of Queen Street Mall, building rooftops, street layout

---

## View 2: North
**Camera:** Looking north (horizontal, yaw: 0°)
**Google Earth Link:** https://earth.google.com/web/@-27.4698,153.0251,50a,100d,0y,0h,0t,0r
**Filename:** `reference/02_north.png`
**What you see:** Looking towards Brisbane River, buildings at eye level

---

## View 3: East
**Camera:** Looking east (horizontal, yaw: 90°)
**Google Earth Link:** https://earth.google.com/web/@-27.4698,153.0251,50a,100d,90y,0h,0t,0r
**Filename:** `reference/03_east.png`
**What you see:** Looking towards financial district, tall buildings

---

## View 4: South  
**Camera:** Looking south (horizontal, yaw: 180°)
**Google Earth Link:** https://earth.google.com/web/@-27.4698,153.0251,50a,100d,180y,0h,0t,0r
**Filename:** `reference/04_south.png`
**What you see:** Looking away from river, city blocks

---

## View 5: West
**Camera:** Looking west (horizontal, yaw: 270°)
**Google Earth Link:** https://earth.google.com/web/@-27.4698,153.0251,50a,100d,270y,0h,0t,0r
**Filename:** `reference/05_west.png`
**What you see:** Looking towards Roma Street area

---

## View 6: 45-Degree Angle
**Camera:** Looking north-east at 45° down (yaw: 45°, pitch: -45°)
**Google Earth Link:** https://earth.google.com/web/@-27.4698,153.0251,50a,100d,45y,0h,45t,0r
**Filename:** `reference/06_angle_45.png`
**What you see:** Angled view showing building sides and rooftops, good perspective

---

## How to Get Reference Images

1. **Open each Google Earth link above**
2. **Let it load the 3D buildings**
3. **Take a screenshot (1920x1080 if possible)**
4. **Save as the filename specified**
5. **Put all in `reference/` folder in the project**

## Automated Comparison

Once reference images are in place:

```bash
# Run auto-screenshot to generate current renders
cargo run --example auto_screenshot

# This will create:
#   screenshot/01_top_down.png
#   screenshot/02_north.png
#   screenshot/03_east.png
#   screenshot/04_south.png
#   screenshot/05_west.png
#   screenshot/06_angle_45.png

# Compare with:
#   reference/01_top_down.png (etc.)
```

## What to Compare

For each pair of images:

✅ **Good (should match):**
- Building shapes and positions
- Street layout
- Relative heights
- 3D structure visible
- Building walls showing (not just tops)
- Depth and perspective

❌ **Bad (current issues):**
- Flat 2D appearance
- Only building tops visible
- No depth perception
- Looks like a map, not 3D world
- Buildings as rectangles, not volumes

---

## Google Earth Parameters Explained

In the URL: `@lat,lon,alt a,range d,yaw y,heading h,pitch t,roll r`

- **lat,lon**: GPS coordinates
- **alt**: Altitude (50m)
- **a**: Altitude units
- **range d**: Distance from point (100d = 100 meters away)
- **yaw y**: Horizontal rotation (0=north, 90=east, 180=south, 270=west)
- **heading h**: Camera heading
- **pitch t**: Vertical angle (0=horizontal, 90=down, -45=angled)
- **roll r**: Camera roll

These parameters match exactly with the auto_screenshot camera angles.
