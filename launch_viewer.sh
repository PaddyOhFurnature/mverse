#!/bin/bash
# Launch the interactive continuous viewer

echo "=== Launching Continuous Viewer ==="
echo ""
echo "NOTE: First launch will be slow (~10 seconds)"
echo "  - Generating 10,404 terrain blocks"
echo "  - Building initial mesh"
echo ""
echo "If you see a black screen or nothing:"
echo "  1. Clear cache: rm -rf ~/.cache/metaverse/blocks"
echo "  2. Try again"
echo ""
echo "Controls:"
echo "  WASD - Move horizontally"
echo "  Space - Move up"
echo "  Shift - Move down"
echo "  Mouse - Look around (click to capture)"
echo "  R - Reload mesh around camera"
echo "  F5 - Take screenshot"
echo "  ESC - Exit"
echo ""
echo "Expected: Gray voxel terrain, 50m radius visible"
echo ""
read -p "Press Enter to launch..."

# Clear cache if old
if [ -d ~/.cache/metaverse/blocks ]; then
    echo "Found existing cache, clearing for clean start..."
    rm -rf ~/.cache/metaverse/blocks
fi

cargo run --example continuous_viewer_simple
