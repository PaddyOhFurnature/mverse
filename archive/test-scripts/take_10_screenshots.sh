#!/bin/bash
# Take 10 screenshots at different positions

echo "=== Taking 10 screenshots at different positions ==="

# Clean old screenshots
rm -f screenshot/shot_*.png 2>/dev/null

# 10 different positions
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 5.0 --pitch -40 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 20.0 --pitch -30 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 50.0 --pitch -20 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 100.0 --pitch -10 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4800 --lon 153.0330 --alt 20.0 --pitch -25 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4790 --lon 153.0340 --alt 20.0 --pitch -25 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 10.0 --pitch -45 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 30.0 --pitch -15 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4785 --lon 153.0335 --alt 40.0 --pitch -20 --screenshot &
sleep 1
target/release/examples/continuous_viewer_simple --lat -27.4805 --lon 153.0338 --alt 40.0 --pitch -20 --screenshot

wait

echo ""
echo "✓ Done! Here are the screenshots:"
ls -lth screenshot/shot_*.png 2>/dev/null || ls -lth screenshot/*.png | head -12
