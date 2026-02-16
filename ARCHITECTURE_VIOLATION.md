# CRITICAL: Chunk Seam Misalignment = Architecture Failure

## User's Point (CORRECT)

> "chunk seams, they have to be perfect... you cant be off by a voxel... you cant walk down the street and be teleported 2m because you crossed a chunk line... EVERY DETAIL IS THE WORLD. the generation, coords, scale, joining... the final output that the user sees is because all that is correct"

## The Mistake I Made

I treated visible chunk boundary artifacts as "visual polish to fix later."

**THIS IS WRONG.**

Chunk boundary discontinuities aren't rendering bugs - they're evidence of:
1. **Coordinate system errors** - Voxel positions misaligned between chunks
2. **Chunk grid misalignment** - Chunks don't share exact boundaries
3. **Floating point precision loss** - Coordinates drifting across boundaries
4. **Transform errors** - GPS→ECEF→ENU→Voxel chain breaks at seams

## Why This Is Critical

In a true 1:1 world:
- A building spanning 2 chunks MUST be continuous (zero gap, zero overlap)
- A road crossing a chunk boundary MUST align perfectly (same elevation, same slope)
- A player walking across chunk seam MUST NOT notice (zero teleport, zero height jump)
- Voxel at chunk boundary (127, y, z) in chunk A MUST match voxel (0, y, z) in chunk B

**If these fail, the coordinate system is fundamentally broken.**

## Root Cause Analysis Required

The visible "recessed edges" prove something is wrong:

### Possible Causes

1. **Chunk bounds not aligned to voxel grid**
   - GPS bounds of chunks don't fall on exact voxel boundaries
   - Chunk A's east edge != Chunk B's west edge at voxel precision
   - Fix: Snap chunk bounds to voxel grid OR make voxel size fit chunk size exactly

2. **Voxel coordinate origin mismatch**
   - Each chunk's SVO has (0,0,0) at different ECEF positions
   - Voxel (127, y, z) in chunk A doesn't map to same ECEF as voxel (0, y, z) in chunk B
   - Fix: Ensure chunk centers are calculated consistently, voxel math identical

3. **Floating point precision loss**
   - GPS→ECEF uses f64, but voxel positions use f32
   - Accumulated error causes drift at boundaries
   - Fix: Keep f64 precision through entire chain until final GPU transform

4. **ENU tangent plane discontinuity**
   - Each chunk uses different tangent plane (at its center)
   - Plane mismatch causes height/position discontinuity at edges
   - Fix: Use shared tangent plane for adjacent chunks OR account for plane rotation

5. **Marching cubes boundary assumption**
   - Algorithm assumes AIR outside SVO bounds → creates boundary faces
   - This is CORRECT behavior if voxel data actually stops at boundary
   - Real issue: voxel data SHOULD continue seamlessly into neighbor
   - Fix: Voxel grids must overlap by 1 voxel at boundaries

6. **Chunk size doesn't divide evenly**
   - 677m chunk / 128 voxels = 5.289m per voxel
   - Non-integer division causes cumulative rounding errors
   - Voxel 127 in chunk A doesn't land at exact boundary
   - Fix: Choose SVO depth that divides chunk size evenly (or vice versa)

## The Correct Approach

Not "fix rendering artifacts later" - **Fix coordinate math NOW**.

### Step 1: Verify Coordinate Correctness
Test at chunk boundary (e.g., Brisbane chunk edge):
```
Chunk A east edge: GPS bounds end at lon X
Chunk B west edge: GPS bounds start at lon X
→ These MUST be identical (f64 precision)

Chunk A voxel (127, 64, 64): Calculate ECEF position
Chunk B voxel (0, 64, 64): Calculate ECEF position  
→ Distance between them MUST be exactly 1 voxel width

Generate terrain at boundary:
Chunk A: SRTM elevation at (x=127, z=64) → voxel Y position
Chunk B: SRTM elevation at (x=0, z=64) → voxel Y position
→ Y positions MUST match (same elevation → same voxel)
```

### Step 2: Fix Misalignment Root Cause
Based on what test reveals, fix the actual problem:
- If GPS bounds mismatch → fix chunk boundary calculation
- If ECEF positions mismatch → fix voxel→ECEF transform
- If voxel sizes mismatch → fix voxel size calculation
- If ENU planes cause discontinuity → use shared plane or account for rotation

### Step 3: Validate Seamlessness
Generate 2 adjacent chunks, verify:
- Same terrain elevation at boundary (SRTM query returns same value)
- Same voxel Y coordinate at boundary (coords_fn returns same elevation)
- Same ECEF position at boundary (transforms match)
- Marching cubes generates matching faces (no gaps, no overlaps)

### Step 4: Test Across Chunk Traversal
Place player at chunk boundary, move across:
- Position (x,y,z) should change smoothly (no teleport)
- Ground elevation should be continuous (no jump)
- Mesh faces should connect seamlessly (no visual pop)

## Why User Is Right

> "the final output that the user sees is because all that is correct"

You can't fake this. Either:
- The math is correct → seams are invisible → world is seamless
- The math is wrong → artifacts appear → world is broken

No amount of "visual polish" fixes broken math. The artifacts are TELLING US the coordinate system has errors.

## Priority: ABSOLUTE HIGHEST

This blocks EVERYTHING:
- ❌ Can't add buildings (they'd be misaligned across chunks)
- ❌ Can't add roads (they'd have gaps at seams)
- ❌ Can't enable multi-chunk (seams would multiply)
- ❌ Can't claim "1:1 world" (it's not 1:1 if chunks don't align)

**Fix the coordinate math first. Then everything else can be built on solid foundation.**

## Next Action

Run diagnostic test:
1. Load 2 adjacent chunks
2. Check GPS bounds at shared edge (must be identical)
3. Check voxel→ECEF at boundary (must be continuous)
4. Check terrain elevation at boundary (must match)
5. Identify which step breaks

Then fix the root cause, not the symptom.
