#!/bin/bash
# Generate all 10 reference screenshots automatically
set -e

mkdir -p screenshot

echo "=========================================="
echo "Generating 10 Screenshots"
echo "=========================================="

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
    "Top Down"
    "North Horizontal"
    "East Horizontal"
    "South Horizontal"
    "West Horizontal"
    "Northeast Angle"
    "Southeast Angle"
    "Southwest Angle"
    "Northwest Angle"
    "Ground Level North"
)

for i in {0..9}; do
    echo ""
    echo "[$((i+1))/10] ${NAMES[$i]}"
    export CAMERA_PARAMS="${SHOTS[$i]}"
    
    # Run screenshot_capture (exits automatically after saving)
    cargo run --example screenshot_capture --release 2>&1 | grep -E "(saved|Error)" || true
done

echo ""
echo "=========================================="
echo "All screenshots complete!"
echo "=========================================="
ls -lh screenshot/*.png

