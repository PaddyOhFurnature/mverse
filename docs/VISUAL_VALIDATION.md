# Visual Validation Status

## Scale Bug Discovered & Fixed (2026-02-17)

**BUG:** Terrain rendering at 100x scale instead of 10x
**DISCOVERED BY:** User visual inspection
**ROOT CAUSE:** `terrain_viewer.rs` line 236: `lat_offset * 10.0` should be `lat_offset * 1.0`

### Before Fix
- Terrain spread over 100m × 100m area (10x too large)
- Many floating individual cubes
- Scale appeared wrong compared to character model

### After Fix  
- Terrain now at correct 10m × 10m scale
- Each voxel = 1m³
- Matches intended design

## Lesson Learned

**This proves PROJECT_REALITY.md was correct:**
> "No visual validation - building completely blind"

Without seeing the rendered output, scale bugs go unnoticed until user tests.

## Screenshot Tool ✅ WORKING

**Fixed:** wgpu texture format mismatch resolved by using surface format + depth buffer

```bash
# Headless screenshot tool
cargo run --example terrain_screenshot
# → Generates screenshot/terrain_validation.png

# Interactive viewer with teleports
cargo run --example terrain_viewer
```

## Teleport Commands (terrain_viewer)

- **T** - Teleport to Kangaroo Point (-27.4775, 153.0355)
- **1** - Aerial view (50m above terrain)
- **2** - Ground level looking north
- **3** - Ground level looking east
- **P** - Print current camera position

## Automated Testing Capability

Now have programmatic navigation + screenshot capture for:
1. Visual regression testing
2. Self-validation during development
3. Documentation generation
4. Bug reproduction

**User was right:** "you cant even make a viewer load at particular coords, take a screenshot and close"
**Status:** FIXED. Screenshot tool works, teleport commands work.

