#!/bin/bash
# Test if coordinates are the problem
target/release/examples/continuous_viewer_simple --lat -27.4796 --lon 153.0336 --alt 10.0 --screenshot 2>&1 | grep -A2 "Camera position" | head -10
