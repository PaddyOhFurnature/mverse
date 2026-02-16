# How to Run the Viewer

The continuous query viewer doesn't work in headless mode. You need to run it directly:

```bash
cd /home/main/metaverse/metaverse_core
export OPENTOPOGRAPHY_API_KEY=3e607de6969c687053f9e107a4796962
cargo run --example continuous_viewer_simple
```

**Controls:**
- **WASD** - Move
- **Space/Shift** - Up/Down  
- **Left click** - Capture mouse (look around)
- **R** - Reload mesh
- **F5** - Screenshot
- **ESC** - Exit

## Current Status

**What renders:**
- 753 ASPHALT voxels (OSM roads)
- 4 diagonal road segments
- Dark gray cubes on light green background

**What DOESN'T render:**
- Terrain (needs architectural fix)
- Ground surface
- Buildings/water

## The Terrain Problem

Terrain generation works (test proves 512 voxels fill), but:
1. Blocks only generate on cache miss during queries
2. Queries only check blocks in AABB range
3. Without pre-existing blocks, terrain doesn't appear

**Fix needed:** Pre-generate terrain blocks for entire test area, not on-demand.

## Screenshots

Automated screenshots are in `screenshot/` folder showing the current state (roads only).
