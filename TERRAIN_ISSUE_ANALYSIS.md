# Terrain Generation - Root Cause Analysis

**Date:** 2026-02-16  
**Status:** Terrain generation works, retrieval broken

---

## What Works ✅

1. **Terrain generation itself** - generates 505 voxels (10 GRASS, 52 DIRT, 443 STONE)
2. **Spatial index storage/retrieval** - tested: 11 GRASS in → 11 GRASS out
3. **Roads** - 753 ASPHALT voxels render correctly
4. **Block key matching** - ECEF positions align perfectly

## What's Broken ❌

**Terrain blocks don't appear in queries** despite being pre-generated.

---

## Tests Run

### Test 1: Direct Generation
```
Block at ECEF (-5046878, 2567787, -2925490)
Elevation span: 3.7m to 11.7m
Ground level: 5.0m
Result: 505 terrain voxels ✓
```

### Test 2: Index Storage
```
Generate block → insert into index → query from index
Result: 11 GRASS in, 11 GRASS out ✓
```

### Test 3: Pre-generation
```
ContinuousWorld::new() generates 10,404 blocks
Inserts into spatial index
Result: Blocks inserted ✓
```

### Test 4: Query After Pre-generation
```
Query 50m radius
Returns 2,366 blocks
Result: ALL AIR (0 GRASS, 0 DIRT, 0 STONE) ✗
```

---

## Root Cause Hypothesis

**Cache pollution during first query:**

1. Pre-generation creates blocks with terrain, inserts into index
2. First `query_range()` calls `get_or_generate_block()` for many keys
3. Some blocks not in index (e.g., elevations outside pre-gen range)
4. Those blocks regenerate as AIR, go into cache
5. Cache now has mix of index blocks (with terrain) + regenerated blocks (without terrain)
6. Subsequent queries return cached ALL-AIR blocks

**Alternative:** Blocks in index ARE empty because generation during pre-gen didn't work for some reason.

---

## Debug Evidence

### Query returns blocks from index:
```
Expected ECEF: (-5046880.000000, 2567784.000000, -2925488.000000)
Retrieved ECEF: (-5046880.000000, 2567784.000000, -2925488.000000)
Match: ✓
```

### But blocks are empty:
```
Retrieved 1 blocks
Block: 512 AIR, 0 GRASS
```

### Yet same ECEF generates terrain when tested directly:
```
Direct generation at same ECEF:
505 terrain voxels (10 GRASS, 52 DIRT, 443 STONE)
```

---

## Possible Fixes

### Option 1: Disable cache during pre-generation queries
Only use index, don't cache until after terrain is confirmed

### Option 2: Generate ALL blocks upfront, don't use on-demand
Pre-generate complete grid, disable dynamic generation

### Option 3: Fix cache/index interaction
Ensure cached blocks come from index, not regeneration

### Option 4: Separate terrain from features
Generate terrain layer separately from OSM features

---

## Current Workaround

Roads render (753 voxels) but no terrain underneath.

Viewer command:
```bash
cargo run --example continuous_viewer_simple
```

Shows: Grey lines (roads) floating in green space (no ground).

---

## Next Steps

1. Add cache bypass flag for terrain blocks
2. OR: Simplify to single-source (index OR cache, not both)
3. OR: Architectural redesign (terrain-first, features-second)

**Priority:** Fix before proceeding to Phase 3. Terrain is foundation.
