# The Multi-Resolution Synchronization Problem

**Date:** 2026-02-17  
**Critical Insight:** Detail is relative to interaction + multiple players need synchronized state

## The Scenario

**Same field, three players, three views:**

### Player A: In Airplane (1000m altitude)
**View needs:**
- Field = colored patch (green/brown pattern)
- Tractor = moving pixel/dot
- Other plane = another pixel
- Hole being dug = NOT VISIBLE (too small from this height)

**Query:** `query_terrain(field_area, detail_level: AERIAL, resolution: 10m)`

---

### Player B: In Tractor (ground level, moving)
**View needs:**
- Field = rows/furrows visible
- Tractor = detailed model (cockpit, wheels, etc)
- Plane overhead = small but visible
- Player C and hole = visible, medium detail

**Query:** `query_terrain(field_area, detail_level: GROUND, resolution: 1m)`

---

### Player C: Standing, Digging Hole (stationary, close focus)
**View needs:**
- Dirt = individual clods, texture detail
- Shovel = high-detail tool
- Tractor = medium detail as it passes
- Plane = tiny dot overhead
- Hole = FULL detail (they're digging it)

**Query:** `query_terrain(dig_site, detail_level: INTERACTION, resolution: 0.1m)`

---

## The Hard Problem

**All three players are in the SAME world:**
1. Player C digs hole → **MODIFIES terrain**
2. Player B sees it (close enough, in their query range)
3. Player A doesn't see it (too far/small, below their resolution threshold)

**But:**
- All three share the **SAME underlying world state**
- Just queried at **DIFFERENT resolutions**
- Modifications must **PROPAGATE** to all players
- But rendered at **APPROPRIATE detail level** for each viewer

## Example Interactions

### Player C Digs Hole
```
1. Player C action: dig_hole(gps_location, depth: 1m, radius: 0.5m)
2. World state updates: Modify voxels at that location → AIR
3. Propagate to network: Send signed modification op to P2P network
4. Other players query:
   - Player B (50m away): Sees hole in ground (rendered at 1m res)
   - Player A (1000m up): Doesn't see it (below visibility threshold)
```

### Player B Plows Field
```
1. Tractor moves, plowing: modify_terrain_strip(tractor_path, depth: 0.3m)
2. Creates furrows (pattern modification to terrain)
3. Propagate:
   - Player C (standing in field): Sees furrows appear as tractor passes
   - Player A (aerial): Sees darker line appear across field (tractor path)
```

### Player A Flies Overhead
```
1. Plane position updates: entity_moved(plane_id, new_gps)
2. Shadow projects on ground: visual effect, not terrain modification
3. Other players see:
   - Player B: Sees plane overhead, shadow moving
   - Player C: Sees plane as small object, hears engine sound
```

## The Architecture Required

### 1. Shared World State (Single Source of Truth)
```rust
struct WorldState {
    // Base terrain from real data
    srtm_elevation: ElevationSource,
    osm_features: FeatureSource,
    
    // Runtime modifications (P2P synchronized)
    terrain_modifications: CrdtMap<Location, Modification>,
    entity_positions: CrdtMap<EntityId, Position>,
}
```

### 2. Detail-Adaptive Queries
```rust
fn query_for_player(
    player_pos: GpsPos,
    player_view: ViewFrustum,
    detail_budget: DetailLevel
) -> RenderData {
    // Determine required detail based on:
    // - Distance to objects
    // - Screen space size
    // - Player interaction mode
    
    let detail_level = calculate_lod(player_pos, view_frustum);
    
    // Query world at appropriate resolution
    world.query(view_frustum, detail_level)
}
```

### 3. Resolution Pyramid (Like Mipmaps for World)

```
Level 0 (Interaction):  0.1m   - Digging, crafting, placing
Level 1 (Ground):       1m     - Walking, ground vehicles  
Level 2 (Aerial Low):   10m    - Low-flying, drone view
Level 3 (Aerial High):  100m   - Airplane, satellite
Level 4 (Space):        1000m  - Orbit view
```

**Same data, different resolutions:**
- Hole dug at 0.1m precision
- Stored in world state
- Player B queries at 1m → sees hole (downsampled)
- Player A queries at 10m → hole below threshold (not rendered)

### 4. Modification Propagation

```rust
// Player C digs
let modification = TerrainMod {
    location: (lat, lon, alt),
    operation: SetVoxels(AIR),
    bounds: AABB::from_radius(0.5m),
    timestamp: now(),
    signature: player_c.sign(),
};

// Broadcast to P2P network
p2p.broadcast(modification);

// Other players receive
on_receive_modification(mod) {
    // Apply to local world state
    world.apply(mod);
    
    // Invalidate affected query cache
    cache.invalidate(mod.bounds);
    
    // Next query will include this modification
}
```

## Why Continuous Queries Enable This

**Traditional chunks fail here:**
- Chunk A loads for Player B (ground level, high detail)
- Chunk A loads for Player A (aerial, low detail)
- **Same chunk, different data?** → Doesn't make sense
- Or **same high-detail chunk for both?** → Wastes bandwidth/memory for Player A

**Continuous queries solve it:**
- Player A: `query(area, resolution: 10m)` → Low-detail response
- Player B: `query(area, resolution: 1m)` → High-detail response
- Player C: `query(area, resolution: 0.1m)` → Ultra-detail response
- **Same world data, filtered/sampled at query time**

## The Pattern Recognition Element

**From earlier insight:** "Nature has patterns everywhere."

When Player C is on ground and needs grass detail:
- Don't send every blade of grass position
- Send: "grass type: field_grass, density: 0.8, seed: 12345, bounds: AABB"
- Player's client **GENERATES** grass blades procedurally
- Same seed → same grass for all players who get close enough to see it

**When Player A is overhead:**
- Sees: "green patch, texture: field_grass_aerial"
- No grass blade generation needed

**Same underlying data (grass exists here), different representation.**

## Implementation Challenges

1. **LOD transitions** - When does Player A start seeing the hole as they descend?
2. **Modification resolution** - Player C digs at 0.1m precision, how does that coarsen to 1m/10m levels?
3. **Network bandwidth** - How to send modifications efficiently to players at different detail levels?
4. **State consistency** - All players must agree on world state despite different views
5. **Procedural determinism** - Same seed must generate same grass/rocks on all clients

## Why This Is Hard But Necessary

**This is NOT a game with a fixed map.**

**This IS:**
- Real Earth (entire planet)
- Real data (SRTM, OSM, satellite imagery)
- Real interactions (dig anywhere, build anything)
- Real scale (millimeters to thousands of kilometers)
- Real multi-player (millions of concurrent users)

**No existing game engine does this** because:
- Games have fixed maps
- Known interaction areas
- Limited player counts
- Controlled detail levels
- Pre-rendered assets

**We need a completely different architecture** - one that:
- Queries detail on demand
- Synchronizes state not assets
- Generates visuals procedurally
- Adapts to viewer context
- Scales to planetary size

---

## Next Steps (After Understanding This)

1. **Prove multi-resolution queries work** (prototype)
2. **Test procedural detail generation** (grass, rocks, etc at different LODs)
3. **Implement modification propagation** (CRDT ops)
4. **Validate network efficiency** (send mods, not geometry)
5. **Profile performance** (can we actually do this in real-time?)

---

**This is the paradigm shift.**

Not better graphics. Not smoother terrain. Not faster rendering.

**A fundamentally different way of representing and querying a shared dynamic world at planetary scale with unlimited detail granularity.**

This is what "MMORPG" (Mass Multiplayer Online Real Parameter Game) actually means.
