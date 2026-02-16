#!/bin/bash
# Test LOD hysteresis - slowly move through LOD boundary

cargo build --release --example continuous_viewer_simple 2>&1 | grep -E "(Finished|error)"

# Coordinates near threshold (test entering/exiting LOD 0 at ~25m)
echo "Testing LOD hysteresis - slowly approaching..."

# Start at 35m (LOD 1), move to 25m (boundary), then to 15m (LOD 0), back to 25m
lats=("-27.479700" "-27.479670" "-27.479640" "-27.479610" "-27.479580" "-27.479610" "-27.479640")
lons=("153.033700" "153.033700" "153.033700" "153.033700" "153.033700" "153.033700" "153.033700")

for i in "${!lats[@]}"; do
    lat="${lats[$i]}"
    lon="${lons[$i]}"
    
    echo "Position $i: $lat, $lon"
    
    target/release/examples/continuous_viewer_simple \
        --lat "$lat" \
        --lon "$lon" \
        --alt 3.0 \
        --pitch -20.0 \
        --screenshot \
        2>&1 | grep -E "(Screenshot|vertices|Near|LOD)"
    
    sleep 1
done

echo "Screenshots saved. Check for consistent mesh between frames."
