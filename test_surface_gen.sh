#!/bin/bash
# Test surface-only generation at multiple altitudes

rm -f screenshot/test_surface_*.png

echo "Taking screenshots at different altitudes..."

for i in 1 2 3 4 5; do
  alt=$((250 - i * 40))
  pitch=-30
  
  echo "  Altitude: ${alt}m..."
  target/release/examples/continuous_viewer_simple \
    --lat -27.4796 --lon 153.0336 --alt $alt --pitch $pitch \
    --screenshot 2>&1 | grep -E "Screenshot saved|vertices|blocks"
  
  # Find and rename latest screenshot
  latest=$(ls -t screenshot/continuous_*.png 2>/dev/null | head -1)
  if [ -n "$latest" ]; then
    mv "$latest" "screenshot/test_surface_${alt}m.png"
    echo "    ✓ Saved: test_surface_${alt}m.png"
  fi
  sleep 1
done

echo ""
echo "Screenshots saved:"
ls -lh screenshot/test_surface_*.png 2>/dev/null || echo "No screenshots found"
