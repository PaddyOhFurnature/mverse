#!/bin/bash
# Test fixed rendering at multiple distances

echo "Testing fixed rendering..."

# Multiple test positions
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 5.0 --pitch -30.0 --screenshot 2>&1 | tail -3 &
sleep 3

target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 20.0 --pitch -20.0 --screenshot 2>&1 | tail -3 &
sleep 3

target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 50.0 --pitch -15.0 --screenshot 2>&1 | tail -3 &
sleep 3

target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 100.0 --pitch -10.0 --screenshot 2>&1 | tail -3

echo "✓ Screenshots captured - checking results..."
ls -lth screenshot/*.png | head -5
