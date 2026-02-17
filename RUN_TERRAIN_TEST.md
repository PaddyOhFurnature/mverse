# Quick Start - Test Terrain Fix

## Launch Viewer
```bash
./target/release/examples/continuous_viewer_async
```

## Test at Kangaroo Point Cliffs

**Controls:**
- WASD: Move around
- Mouse: Look around (click window first to capture mouse)
- Escape: Release mouse
- F12: Capture screenshot
- Close window: Exit

**Starting position:** -27.479769°, 153.033586° (30m altitude)

**Cliff locations:**
1. **Cliff top:** Move slightly north (W key)
2. **Cliff face:** Move north more, should see vertical drop
3. **River:** Move down (look down, move forward)

## What to Look For

**GOOD SIGNS:**
- Cliff appears as solid continuous surface
- Clear vertical drop visible
- Stone material forming cliff wall
- No major gaps or floating blocks

**BAD SIGNS:**
- Still looks like "asteroids game"
- Floating disconnected blocks
- Cliff still appears as gentle slope
- Chaotic mess of random cubes

## Screenshots

Press F12 at each location to save screenshot with GPS coordinates in filename.

Screenshots save to: `screenshot/async_[timestamp]_[lat]_[lon]_alt[altitude]m.png`

## Report Back

Tell me:
1. Is it **better** than before? (yes/no/about the same)
2. Is it **acceptable** quality? (yes/no/getting there)
3. What specifically still looks wrong?

Then I can:
- Commit if good
- Try smaller voxels (0.5m) if needs more detail
- Implement cliff detection if still broken
- Research other solutions

---

**The fix is in the code, cache is cleared. Just run the viewer and test!**
