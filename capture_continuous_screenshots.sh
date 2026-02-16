#!/bin/bash
# Automated screenshot capture for continuous query viewer

echo "=== Continuous Query Screenshot Capture ==="
echo ""
echo "This script will:"
echo "  1. Run the continuous viewer"
echo "  2. Wait for initialization"
echo "  3. Press F5 to capture screenshot"
echo "  4. Exit"
echo ""

# Ensure screenshot directory exists
mkdir -p screenshot

# Set API key
export OPENTOPOGRAPHY_API_KEY=3e607de6969c687053f9e107a4796962

# Build first
echo "Building viewer..."
cargo build --example continuous_viewer_simple --quiet 2>/dev/null

# Run with timeout and automated input
echo ""
echo "Running viewer..."
echo "(Viewer will open, capture screenshot, then close)"
echo ""

# Launch viewer in background
timeout 30 cargo run --example continuous_viewer_simple 2>&1 &
VIEWER_PID=$!

# Wait for initialization
sleep 8

# Simulate F5 keypress using xdotool (if available)
if command -v xdotool &> /dev/null; then
    echo "Sending F5 command..."
    WID=$(xdotool search --name "Continuous" | head -1)
    if [ ! -z "$WID" ]; then
        xdotool windowactivate $WID
        sleep 1
        xdotool key F5
        sleep 2
        xdotool key Escape
    else
        echo "Window not found. Screenshots must be captured manually with F5."
    fi
else
    echo "xdotool not available. Manual screenshot capture required."
    echo "Press F5 in the viewer window to capture, then ESC to exit."
fi

# Wait for viewer to finish
wait $VIEWER_PID

# List captured screenshots
echo ""
echo "Screenshots captured:"
ls -lh screenshot/continuous_*.png 2>/dev/null || echo "No screenshots found."
echo ""
echo "Done!"
