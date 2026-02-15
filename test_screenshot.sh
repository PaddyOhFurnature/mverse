#!/bin/bash
# Simple test: Can we render ONE frame with WorldManager and save it?

cd "$(dirname "$0")"

echo "Testing screenshot capture..."
echo ""

# Set up xvfb
export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 &
XVFB_PID=$!
sleep 2

echo "Running simple screenshot test..."
timeout 60 cargo run --example screenshot_test --release 2>&1 | tail -30

echo ""
echo "Cleaning up..."
kill $XVFB_PID 2>/dev/null

if [ -f "screenshot/test.png" ]; then
    echo "✓ Screenshot saved: screenshot/test.png ($(du -h screenshot/test.png | cut -f1))"
    file screenshot/test.png
else
    echo "✗ No screenshot generated"
    exit 1
fi
