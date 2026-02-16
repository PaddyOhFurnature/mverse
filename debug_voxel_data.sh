#!/bin/bash
# Debug: Check voxel data in blocks
cargo run --release --example continuous_viewer_simple -- --lat -27.4796 --lon 153.0336 --alt 20.0 --pitch -30 --screenshot 2>&1 | grep -E "primitives|vertices|blocks"
