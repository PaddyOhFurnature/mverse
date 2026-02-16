#!/bin/bash
# Check for missing terrain data issues

echo "=== Checking SRTM cache ==="
cache_dir="$HOME/.metaverse/cache/srtm"
if [ -d "$cache_dir" ]; then
  echo "Cache directory exists: $cache_dir"
  echo "Cached tiles:"
  ls -lh "$cache_dir"/*.hgt 2>/dev/null || echo "  No .hgt files found"
else
  echo "Cache directory NOT found: $cache_dir"
fi

echo ""
echo "=== Test run at Brisbane coords ==="
target/release/examples/continuous_viewer_simple \
  --lat -27.479532 --lon 153.033142 --alt 50.0 --pitch -30 \
  --screenshot 2>&1 | grep -E "primitives|blocks|elevation|Failed|Error" | head -15
