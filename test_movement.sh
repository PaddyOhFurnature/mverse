#!/bin/bash
# Quick test - see what's happening with mesh updates
timeout 8 target/release/examples/continuous_viewer_simple \
  --lat -27.4796 --lon 153.0336 --alt 100.0 --pitch -30 2>&1 | \
  grep -E "Mesh Update|moved" | head -20
