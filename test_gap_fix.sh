#!/bin/bash
# Test expanded surface range at multiple angles/positions

echo "Testing gap fixes with expanded surface range (-4m to +2m)..."
echo ""

# Test positions around Brisbane area
positions=(
  "-27.4795 153.0331 50 -30"   # Original problem area
  "-27.4796 153.0336 50 -45"   # Steeper angle
  "-27.4790 153.0340 30 -60"   # Lower + steep
  "-27.4800 153.0330 80 -20"   # Higher + shallow
)

count=1
for pos in "${positions[@]}"; do
  read -r lat lon alt pitch <<< "$pos"
  echo "Test $count: lat=$lat, lon=$lon, alt=${alt}m, pitch=${pitch}°"
  
  target/release/examples/continuous_viewer_simple \
    --lat $lat --lon $lon --alt $alt --pitch $pitch \
    --screenshot 2>&1 | grep -E "primitives|vertices"
  
  latest=$(ls -t screenshot/continuous_*.png 2>/dev/null | head -1)
  if [ -n "$latest" ]; then
    mv "$latest" "screenshot/gap_test_${count}.png"
    echo "  ✓ Saved: gap_test_${count}.png"
  fi
  echo ""
  count=$((count + 1))
done

echo "Gap fix tests complete!"
ls -lh screenshot/gap_test_*.png
