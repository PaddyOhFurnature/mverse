#!/bin/bash
# Generate all 10 reference screenshots
# Run this on a machine with a display

set -e
mkdir -p screenshot

echo "=========================================="
echo "Screenshot Generation Instructions"
echo "=========================================="
echo "This script will run the viewer 10 times with different camera positions."
echo "When you see 'READY FOR SCREENSHOT', take a screenshot and save it."
echo "Press Ctrl+C in the viewer window to move to the next position."
echo ""
echo "Positions from REFERENCE_IMAGES.md:"
echo "  1. Top-down (0° heading, 0° tilt)"
echo "  2. North horizontal (0° heading, 90° tilt)"
echo "  3. East horizontal (90° heading, 90° tilt)"
echo "  4. South horizontal (180° heading, 90° tilt)"
echo "  5. West horizontal (270° heading, 90° tilt)"
echo "  6. NE angle (45° heading, 45° tilt)"
echo "  7. SE angle (135° heading, 45° tilt)"
echo "  8. SW angle (225° heading, 45° tilt)"
echo "  9. NW angle (315° heading, 45° tilt)"
echo "  10. Ground level north (0° heading, 85° tilt, 20m alt)"
echo ""
read -p "Press Enter to start..."

# Position data: lat lon alt heading tilt output_file
declare -a SHOTS=(
    "-27.463697 153.035725 250 0 0 screenshot/01_top_down.png"
    "-27.463697 153.035725 250 0 90 screenshot/02_north_horizontal.png"
    "-27.463697 153.035725 250 90 90 screenshot/03_east_horizontal.png"
    "-27.463697 153.035725 250 180 90 screenshot/04_south_horizontal.png"
    "-27.463697 153.035725 250 270 90 screenshot/05_west_horizontal.png"
    "-27.463697 153.035725 250 45 45 screenshot/06_northeast_angle.png"
    "-27.463697 153.035725 250 135 45 screenshot/07_southeast_angle.png"
    "-27.463697 153.035725 250 225 45 screenshot/08_southwest_angle.png"
    "-27.463697 153.035725 250 315 45 screenshot/09_northwest_angle.png"
    "-27.463697 153.035725 20 0 85 screenshot/10_ground_level_north.png"
)

NAMES=(
    "01 - Top Down"
    "02 - North Horizontal"
    "03 - East Horizontal"
    "04 - South Horizontal"
    "05 - West Horizontal"
    "06 - Northeast Angle"
    "07 - Southeast Angle"
    "08 - Southwest Angle"
    "09 - Northwest Angle"
    "10 - Ground Level North"
)

for i in {0..9}; do
    echo ""
    echo "=========================================="
    echo "Screenshot $((i+1))/10: ${NAMES[$i]}"
    echo "=========================================="
    export CAMERA_PARAMS="${SHOTS[$i]}"
    
    # Run viewer - user takes screenshot when READY appears
    cargo run --example screenshot_capture --release || true
    
    echo "Screenshot $((i+1)) complete"
    sleep 1
done

echo ""
echo "=========================================="
echo "All screenshots generated!"
echo "Check the screenshot/ directory"
echo "=========================================="
ls -lh screenshot/*.png 2>/dev/null || echo "No PNGs found - did you save them?"
