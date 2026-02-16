#!/bin/bash
# Screenshot tool - takes 10 screenshots at different positions
# Usage: ./take_screenshot.sh

set -e

echo "=== Screenshot Tool - Taking 10 different shots ==="

# Build viewer first
cargo build --release --example continuous_viewer_simple 2>&1 | grep -E "(Finished|error)"

# 10 different test positions around Kangaroo Point
POSITIONS=(
    "-27.4796 153.0336 5.0 -40"     # Ground level, looking down steep
    "-27.4796 153.0336 20.0 -30"    # Low altitude, moderate angle
    "-27.4796 153.0336 50.0 -20"    # Medium altitude
    "-27.4796 153.0336 100.0 -10"   # High altitude, shallow angle
    "-27.4800 153.0330 20.0 -25"    # Different location, west
    "-27.4790 153.0340 20.0 -25"    # Different location, east
    "-27.4796 153.0336 10.0 -45"    # Ground level, very steep
    "-27.4796 153.0336 30.0 -15"    # Medium-low, shallow
    "-27.4785 153.0335 40.0 -20"    # North position
    "-27.4805 153.0338 40.0 -20"    # South position
)

for i in "${!POSITIONS[@]}"; do
    POS=(${POSITIONS[$i]})
    LAT=${POS[0]}
    LON=${POS[1]}
    ALT=${POS[2]}
    PITCH=${POS[3]}
    
    echo ""
    echo "[$((i+1))/10] Position: lat=$LAT lon=$LON alt=${ALT}m pitch=${PITCH}°"
    
    # Run viewer in screenshot mode (will take screenshot and need manual close)
    timeout 15 target/release/examples/continuous_viewer_simple \
        --lat "$LAT" --lon "$LON" --alt "$ALT" --pitch "$PITCH" \
        2>&1 | grep -E "(Screenshot|vertices|Mesh)" || true
    
    sleep 1
done

echo ""
echo "✓ Done! Check screenshot/ folder:"
ls -lth screenshot/*.png | head -12
