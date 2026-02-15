#!/bin/bash
set -e
mkdir -p screenshot

# Position data: lat lon alt heading tilt outputfile
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

for shot in "${SHOTS[@]}"; do
    export CAMERA_PARAMS="$shot"
    echo "Capturing: $shot"
    cargo run --example screenshot_capture --release
    sleep 1
done
