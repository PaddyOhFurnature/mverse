#!/bin/bash
cd "$(dirname "$0")"
export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 &
XVFB_PID=$!
sleep 2
echo "Running screenshot capture..."
timeout 300 cargo run --example screenshot_worldmanager --release 2>&1 | grep -v "warning:"
kill $XVFB_PID 2>/dev/null
echo ""
echo "Screenshots in screenshot/ directory"
ls -lh screenshot/*.png 2>/dev/null | tail -10
