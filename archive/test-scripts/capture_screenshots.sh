#!/bin/bash
# Screenshot capture using xvfb (headless rendering)

cd "$(dirname "$0")"

echo "=== Screenshot Capture System ==="
echo ""
echo "Starting Xvfb virtual display..."
Xvfb :99 -screen 0 1920x1080x24 &
XVFB_PID=$!
export DISPLAY=:99
sleep 2

echo "Building viewer..."
cargo build --example viewer --release 2>&1 | grep -E "(Compiling metaverse|Finished|error)" || true

if [ $? -ne 0 ]; then
    echo "Build failed!"
    kill $XVFB_PID 2>/dev/null
    exit 1
fi

echo ""
echo "Capturing screenshots..."
echo "This will take about 60 seconds (generating terrain for each view)"
echo ""

# Run viewer in screenshot mode (needs to be implemented)
timeout 120 cargo run --example viewer --release -- --screenshot-mode 2>&1 | tee /tmp/screenshot_output.log

echo ""
echo "Cleaning up..."
kill $XVFB_PID 2>/dev/null

echo ""
echo "=== Results ==="
if [ -d "screenshot" ]; then
    echo "Screenshots saved to screenshot/:"
    ls -lh screenshot/*.png 2>/dev/null | awk '{print "  " $9, "-", $5}' || echo "  No screenshots found!"
    
    echo ""
    echo "Compare with reference photos:"
    echo "  reference/01_top_down.png         vs screenshot/01_top_down.png"
    echo "  reference/02_north_horizontal.png vs screenshot/02_north_horizontal.png"
    echo "  (etc)"
else
    echo "screenshot/ directory not found"
fi
