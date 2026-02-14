# Test Camera Positions for Visual Verification

These are the exact GPS coordinates where screenshots should be taken.
Match these with Google Street View to get reference images.

## Position 1: Ground Level - Queen Street Mall
**GPS:** -27.469800, 153.025100, 2.0m
**Google Street View Link:** https://www.google.com/maps/@-27.469800,153.025100,3a,75y,0h,90t/data=!3m6!1e1
**Description:** Ground level in Queen Street Mall pedestrian area
**Look Direction:** North (towards Brisbane River)
**What to expect:** Buildings directly ahead, pedestrian mall, should see building walls and depth

## Position 2: Street View - Edward Street
**GPS:** -27.467800, 153.026100, 2.0m
**Google Street View Link:** https://www.google.com/maps/@-27.467800,153.026100,3a,75y,90h,90t/data=!3m6!1e1
**Description:** Street level on Edward Street
**Look Direction:** East (along the street)
**What to expect:** Street corridor with buildings on both sides, road with thickness

## Position 3: Low Aerial - Above Queen Street
**GPS:** -27.469800, 153.025100, 30.0m
**Description:** 30m above Queen Street Mall (approximately 10-story height)
**Look Direction:** North-East at 45° angle down
**What to expect:** Rooftops of nearby buildings, can see building sides, street layout visible

## Position 4: Mid Aerial - Above CBD
**GPS:** -27.469800, 153.025100, 100.0m
**Description:** 100m above CBD center
**Look Direction:** Looking north-east at 30° angle down
**What to expect:** Multiple city blocks visible, building tops AND sides visible, 3D cityscape

## Position 5: River View - South Bank
**GPS:** -27.475000, 153.020000, 10.0m
**Google Street View Link:** https://www.google.com/maps/@-27.475000,153.020000,3a,75y,0h,90t/data=!3m6!1e1
**Description:** South Bank area near river
**Look Direction:** North across river towards CBD
**What to expect:** River in foreground, CBD skyline in background

## Position 6: Eagle Street - Financial District
**GPS:** -27.468500, 153.029500, 2.0m
**Google Street View Link:** https://www.google.com/maps/@-27.468500,153.029500,3a,75y,270h,90t/data=!3m6!1e1
**Description:** Eagle Street in financial district
**Look Direction:** West towards tall buildings
**What to expect:** Skyscrapers, modern office buildings, street-level view

## Position 7: Victoria Bridge View
**GPS:** -27.471500, 153.022000, 5.0m
**Google Street View Link:** https://www.google.com/maps/@-27.471500,153.022000,3a,75y,45h,90t/data=!3m6!1e1
**Description:** On Victoria Bridge
**Look Direction:** North-East towards CBD
**What to expect:** Bridge deck, city skyline ahead, river visible

## Position 8: Roma Street - Low Angle
**GPS:** -27.465000, 153.018000, 2.0m
**Google Street View Link:** https://www.google.com/maps/@-27.465000,153.018000,3a,75y,135h,90t/data=!3m6!1e1
**Description:** Roma Street area
**Look Direction:** South-East towards CBD
**What to expect:** Street view with buildings, rail infrastructure possibly visible

## Position 9: Elevated View - King George Square
**GPS:** -27.467500, 153.023500, 50.0m
**Description:** 50m above King George Square
**Look Direction:** 45° angle looking east
**What to expect:** Mid-height view of CBD, building details visible, square layout below

## Position 10: Corner View - Ann Street
**GPS:** -27.469000, 153.027000, 2.0m
**Google Street View Link:** https://www.google.com/maps/@-27.469000,153.027000,3a,75y,180h,90t/data=!3m6!1e1
**Description:** Ann Street corner
**Look Direction:** South
**What to expect:** Street intersection, buildings on corners, 3D structure visible

---

## How to Use This File

1. **For Screenshot Testing:**
   - Fly to each GPS coordinate listed above
   - Match the altitude (elevation_m)
   - Face the specified direction
   - Take screenshot
   - Name it: `test_01_queen_street_ground.png` etc.

2. **For Reference Images:**
   - Open each Google Street View link
   - Take screenshot of the Street View
   - This is what the metaverse SHOULD look like
   - Compare with rendered output

3. **What to Check:**
   - ✅ Buildings are 3D volumes with walls (not flat rectangles)
   - ✅ Roads have thickness (not just lines)
   - ✅ Proper depth and perspective
   - ✅ Building sides visible when not looking straight down
   - ✅ Similar layout/structure to Google Street View
   - ❌ Flat 2D shapes
   - ❌ No depth or volume
   - ❌ Buildings look like top-down map

---

## Expected Visual Quality

**Minimum acceptable (GTA V level):**
- Buildings are solid 3D volumes
- Can see building walls, not just roofs
- Roads have visible thickness
- Proper lighting and shadows
- Depth perception from any angle
- Looks like a 3D world, not a 2D map

**Current issues to fix:**
- Everything appears flat/2D
- Only seeing building tops
- No visible depth or volume
- Looks like a map drawing, not 3D city
