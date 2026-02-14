#!/bin/bash
# Automated screenshot capture script
# 
# This script documents the 10 test positions
# You can manually fly to each and press F12 to screenshot
# OR we can modify the viewer to cycle through these automatically

cat << 'EOF'
=== AUTOMATED SCREENSHOT TEST POSITIONS ===

Take screenshots from these 10 positions to verify rendering:

1. GROUND LEVEL (spawn)
   - Position: -27.4698, 153.0251, 2m elevation
   - Look at: buildings ahead (north)
   - Should see: Building WALLS with depth, 3D roads with thickness
   
2. EYE LEVEL (50m)
   - Position: -27.4698, 153.0251, 50m
   - Look at: CBD buildings at angle
   - Should see: Building sides, roof details, 3D structure

3. LOW AERIAL (100m)
   - Position: -27.4698, 153.0251, 100m
   - Look at: angled down at city
   - Should see: Building tops AND sides, road depth visible

4. STREET VIEW (5m)
   - Position: -27.4698, 153.0251, 5m
   - Look at: down a street
   - Should see: Buildings as solid volumes, not flat shapes

5. ROOFTOP (30m)
   - Position: -27.4708, 153.0241, 30m
   - Look south
   - Should see: Nearby buildings at eye level

6. AERIAL (500m)
   - Position: -27.4698, 153.0251, 500m
   - Look at angle
   - Should see: City layout, buildings should still have volume

7. TOP-DOWN (200m)
   - Position: -27.4698, 153.0251, 200m
   - Look straight down
   - Should see: Building tops (expected to be flat from this angle)

8. RIVER VIEW (20m)
   - Position: -27.4748, 153.0301, 20m
   - Look at river
   - Should see: Water surface, buildings with depth

9. DIAGONAL (150m)
   - Position: -27.4668, 153.0221, 150m
   - Look diagonal across city
   - Should see: 3D cityscape with perspective

10. LOW OBLIQUE (80m)
    - Position: -27.4678, 153.0231, 80m
    - Look at angle
    - Should see: Buildings with clear 3D structure

WHAT TO CHECK IN EACH SCREENSHOT:
- Are buildings 3D VOLUMES with walls, or flat rectangles?
- Do roads have THICKNESS (30cm) or are they flat lines?
- Is there depth/shadow/perspective?
- Can you see building SIDES or just tops?
- Does it look like GTA V or like a 2D map?

SAVE AS: screenshot/test_01_ground_level.png (etc.)

EOF
