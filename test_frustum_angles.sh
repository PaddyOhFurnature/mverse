#!/bin/bash
# Test frustum culling at different camera angles

echo "Testing frustum culling effectiveness..."
echo ""

# Test at different pitch angles (looking up/down/around)
angles=(
  "-27.4796 153.0336 50 0 looking-straight"
  "-27.4796 153.0336 50 -60 looking-steep-down"
  "-27.4796 153.0336 50 -15 looking-shallow"
  "-27.4796 153.0336 100 -45 high-altitude"
)

for angle_data in "${angles[@]}"; do
  read -r lat lon alt pitch label <<< "$angle_data"
  echo "=== Test: $label (pitch=$pitch°, alt=$alt m) ==="
  
  target/release/examples/continuous_viewer_simple \
    --lat $lat --lon $lon --alt $alt --pitch $pitch \
    --screenshot 2>&1 | grep -E "blocks with LOD|Near blocks|primitives"
  
  echo ""
done

echo "Frustum culling test complete!"
