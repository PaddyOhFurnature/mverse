# Next Steps - User Decision Required

## Current State ✅

**Terrain rendering WORKING!** User validated:
- ✓ Terrain elevation visible (SRTM 76-97m)
- ✓ Material layers working (dirt, stone)
- ✓ Voxelized surface at ~5m resolution
- ✓ Performance good (46-48 FPS)
- ✓ Screenshot system operational

## Known Issues 🔧

### HIGH Priority
1. **Chunk boundary artifacts** - Recessed edges on all 4 sides
   - Cause: Marching cubes missing neighbor voxel data
   - Fix: Load 3x3 chunk grid + neighbor voxel padding
   - Time: 2-3 hours

2. **Single chunk loading** - Only camera chunk, not FOV-based
   - User wants: Multiple chunks based on view direction
   - Fix: Frustum culling OR 3x3 grid loading
   - Time: 1-2 hours

3. **LOD 1+ marching cubes bug** - Voxel stepping skips surfaces
   - Currently disabled (LOD 0 only)
   - Fix: Algorithm rewrite or mesh decimation
   - Time: 3-4 hours

## Remaining TODO Items (SQL Database)

Only 2 pending items:
1. `srtm-procedural-gaps` - Make procedural fill gaps only (not urgent)
2. `srtm-usgs-3dep` - Implement USGS 3DEP client (not urgent)

**40 items completed!** All core systems working.

## What's NOT Yet Implemented

Looking at project scope, still need:
- **OSM Buildings** - Voxelized into SVO (code exists, not wired into viewer yet?)
- **OSM Roads** - 3D road volumes (code exists, needs integration?)
- **OSM Water** - Rivers, lakes (code exists, needs integration?)
- **Trees/Vegetation** - Procedural placement based on land use
- **Lighting system** - Day/night cycle, sun/moon
- **Textures** - Currently solid colors, need PBR materials
- **Shadows** - Basic shadow mapping
- **Physics** - Collision detection, player controller
- **Networking** - P2P chunk sync, CRDT ops
- **Entity system** - NPCs, vehicles, interactables

## Decision: What Next?

### Option A: Polish Rendering (2-4 hours)
**Fix visible issues before adding more content**

Tasks:
1. Enable 3x3 chunk loading (1 hour)
2. Implement neighbor voxel padding for seamless boundaries (2 hours)
3. Test at depth 8 for better resolution (1 hour)

**Pros:**
- Clean, polished terrain before adding buildings
- No visual artifacts
- Multi-chunk viewing working

**Cons:**
- Delays gameplay content
- OSM features remain invisible

**Best if:** User prioritizes visual quality, wants clean foundation

---

### Option B: Add World Content (4-8 hours)
**Populate world with OSM features**

Tasks:
1. Wire up existing OSM building voxelization (2 hours)
2. Add road volumes to chunks (1 hour)
3. Add water features (1 hour)
4. Test with buildings visible (1 hour)
5. Debug any new issues (2-3 hours)

**Pros:**
- More complete, interesting world to test
- Validates full SVO pipeline
- Buildings, roads, water visible

**Cons:**
- Chunk boundaries still visible
- May introduce new rendering issues
- Single chunk loading limits what's visible

**Best if:** User wants to see the full world generation working

---

### Option C: Hybrid Approach (3-5 hours)
**Quick multi-chunk fix, then add content**

Tasks:
1. Enable 3x3 chunk grid loading (1 hour)
   - Simple distance check, no frustum culling
   - Accepts visible boundaries for now
2. Wire up OSM building rendering (2 hours)
3. Test with buildings + terrain (1 hour)
4. Return to polish boundaries later

**Pros:**
- Gets both multi-chunk AND content quickly
- Validates full pipeline end-to-end
- Leaves polish for later

**Cons:**
- Chunk boundaries remain visible
- Not as clean as Option A
- Not as much content as Option B

**Best if:** User wants balanced progress on both fronts

---

### Option D: Focus on Missing Core Systems
**Address bigger gaps in project**

Based on HANDOVER.md, implement:
1. Lighting system (day/night) - 3-4 hours
2. Basic physics/collision - 4-6 hours
3. Texture system (PBR materials) - 4-6 hours
4. Entity system foundation - 6-8 hours

**Pros:**
- Moves toward playable experience
- Addresses gameplay fundamentals

**Cons:**
- Leaves rendering issues unfixed
- More complex systems
- Longer timelines

**Best if:** User wants to tackle bigger architecture challenges

---

## Recommendation

Based on user's feedback ("so far it seems pretty good"), I recommend:

**Option C (Hybrid)** - Quick 3x3 chunk loading + OSM buildings

Reasoning:
1. User specifically mentioned wanting "multiple chunks depending on FOV"
2. User noted "nothing except terrain at this point"
3. Terrain is working well enough to build on
4. Can polish boundaries later when it becomes annoying

This gets the most visible progress (buildings appear!) with modest time investment.

---

## User: Which Option?

**A**: Polish rendering first (clean boundaries, multi-chunk)
**B**: Add OSM content first (buildings, roads, water)  
**C**: Hybrid (quick multi-chunk + buildings)
**D**: Focus on core systems (lighting, physics, textures)

Or describe a different priority?
