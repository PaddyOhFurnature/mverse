# Research Questions & Validation Strategy

**Date:** 2026-02-17  
**Status:** Planning phase - understanding what we don't know

## The Core Problem

**"If we're doing something that hasn't been done, how do we know we are doing it?"**

We can't afford to be 100,000 lines deep before discovering the foundation is wrong.

---

## Critical Questions That Must Be Answered FIRST

### 1. Data Requirements

**What data exists?**
- ✅ SRTM elevation: 90m resolution (we have)
- ✅ OSM buildings/roads: Vector data (we have)
- ❓ Higher resolution elevation: LiDAR? ALOS? Where available?
- ❓ Satellite imagery: Resolution? Coverage? Cost?
- ❓ Geological data: Rock types, soil composition?
- ❓ Vegetation data: Tree locations? Species? Density maps?
- ❓ Underground data: Caves, tunnels, aquifers?
- ❓ Real-time data: Weather, traffic, construction?

**What data do we NEED vs NICE-TO-HAVE?**
- NEED: Base terrain (elevation)
- NEED: Buildings/roads (OSM sufficient?)
- NEED: Material types (rock, dirt, water, asphalt)
- NICE: Vegetation specifics
- NICE: Geological details
- NICE: Real-time updates

**What can we infer vs must be explicit?**
- Can infer: Grass on flat areas, rocks on slopes
- Can infer: Dirt below grass, stone below dirt
- Must be explicit: Specific building locations
- Must be explicit: Road paths
- ??? Cave systems - infer from geology or need explicit data?

### 2. Representation Rules

**What is the substrate (the actual world)?**
- Volumetric field? (voxels, SDF, something else?)
- Height field + modifications? (2.5D base + 3D edits?)
- Pure mesh? (all triangles?)
- Hybrid? (different representations at different scales?)

**How do we represent at different scales?**
- Millimeter (digging sand): ???
- Meter (walking): Voxels? Mesh? SDF?
- 10m (vehicle): Simplified mesh?
- 100m (aerial): Texture-mapped heightmap?
- 1000m (satellite): Satellite image tiles?

**What is modifiable vs static?**
- Static: Bedrock geology?
- Modifiable: Surface terrain (dig/build)
- Modifiable: Vegetation (cut trees, plant)
- Modifiable: Buildings (player-built)
- ??? Dynamic: Water flow, erosion?

### 3. Detail Generation Rules

**Procedural generation patterns:**
- Grass placement: Perlin noise? Density based on terrain?
- Rock distribution: Fractal? Based on geology?
- Terrain micro-detail: Fractal subdivision? Noise overlay?
- Cave formation: Geological simulation? Pre-generated?

**Determinism requirements:**
- Same seed → same grass layout? (YES - players must see same)
- Same location → always same base terrain? (YES)
- Modifications → deterministic results? (YES - CRDT rules)

**Level of detail thresholds:**
- At what distance do we stop rendering grass blades?
- When does a building become a box vs detailed model?
- When does terrain become satellite image vs 3D geometry?

### 4. Query System Design

**What can be queried?**
- Terrain elevation at point
- Material at point
- Objects in volume (buildings, trees, rocks)
- Modifications in area
- Entities (players, vehicles) in area

**Query granularity:**
- Can we query single points? (get_material(gps))
- Must we query volumes? (get_voxels(aabb))
- What's the minimum/maximum query size?

**Query performance requirements:**
- How fast must queries be? (<16ms for 60fps?)
- How many concurrent queries per player?
- What's cacheable vs must be fresh?

### 5. Synchronization Rules

**What gets synchronized?**
- Player modifications (dig, build) - YES
- Procedural generation seeds - YES (or deterministic?)
- Terrain base layer - NO (from source data)
- Entity positions - YES
- ??? Dynamic terrain (erosion, water) - ???

**How is state represented?**
- CRDT operations (set_voxel, fill_region, etc)
- Delta encoding (only changes)
- Full state snapshots (for new players)

**Conflict resolution:**
- Two players dig same spot simultaneously?
- Player A sees grass, Player B already cut it?
- Modification ordering (timestamp? Vector clock?)

---

## Testing Strategy (How Do We Validate Without 100k Lines?)

### Phase 0: Data Inventory (1-2 days)
**Goal:** Know what data actually exists and is accessible

**Tests:**
1. Survey elevation data sources (SRTM, ALOS, LiDAR coverage)
2. Test OSM data completeness for test area
3. Research satellite imagery APIs
4. Document what exists, what costs money, what has gaps

**Success criteria:** 
- Know exactly what data we can access
- Know resolution/coverage limits
- Know costs if any

---

### Phase 1: Multi-Resolution Query Proof (1 week)
**Goal:** Prove same data can be queried at different resolutions efficiently

**Prototype:**
- Small test area (1km²)
- Generate test data at 0.1m resolution
- Implement queries at: 0.1m, 1m, 10m, 100m resolutions
- Measure: query time, memory usage, result quality

**Tests:**
1. Query same location at 4 different resolutions
2. Verify results are consistent (lower res is downsampled higher res)
3. Profile performance (can we hit 60fps?)
4. Test cache effectiveness

**Success criteria:**
- Query time <16ms for any resolution
- Memory usage reasonable (<1GB for 1km²)
- Visual quality acceptable at each level

**Failure modes to watch for:**
- Can't downsample efficiently
- Cache misses too frequent
- Memory explosion
- Quality degradation unacceptable

---

### Phase 2: Procedural Detail Generation (1 week)
**Goal:** Prove we can generate convincing detail procedurally

**Prototype:**
- Grass generation at 3 LODs (close/medium/far)
- Rock placement procedurally
- Terrain micro-detail (fractal subdivision)

**Tests:**
1. Same seed generates same results
2. Transitions between LODs are smooth
3. Performance acceptable (generation time)
4. Visual quality convincing

**Success criteria:**
- Deterministic (same input → same output)
- Fast enough to generate on-demand
- Looks natural/convincing

**Failure modes:**
- Too slow to generate
- Results look artificial/repetitive
- Non-deterministic (desyncs between players)

---

### Phase 3: Modification Propagation (1 week)
**Goal:** Prove modifications can sync between multiple viewers at different LODs

**Prototype:**
- Simulate 3 players (aerial, ground, interaction)
- Player digs hole
- Verify all see it at appropriate detail level

**Tests:**
1. Modification at one LOD visible at others
2. CRDT operations merge correctly
3. Network bandwidth acceptable
4. Latency acceptable

**Success criteria:**
- <100ms modification propagation
- <1KB per modification network cost
- No desyncs between players

**Failure modes:**
- Bandwidth explosion
- Modification conflicts
- Visual inconsistencies

---

### Phase 4: Scale Test (1 week)
**Goal:** Prove approach scales beyond test area

**Prototype:**
- Expand to 100km² area
- Test memory usage, query performance
- Test data loading/streaming

**Tests:**
1. Query performance stays <16ms
2. Memory usage stays reasonable
3. Data loads on-demand
4. No loading screens

**Success criteria:**
- Performance maintains
- Memory doesn't explode
- Seamless experience

**Failure modes:**
- Memory/performance cliffs
- Data loading bottlenecks
- System thrashing

---

## Decision Points (When Do We Pivot?)

**After Phase 1:**
- ✅ Continue → Multi-resolution queries work
- ❌ Pivot → Fall back to fixed-LOD chunks

**After Phase 2:**
- ✅ Continue → Procedural generation convincing
- ❌ Pivot → Use more static assets

**After Phase 3:**
- ✅ Continue → Synchronization works
- ❌ Pivot → Rethink networking model

**After Phase 4:**
- ✅ Continue → Approach scales
- ❌ Pivot → Hybrid approach or scale limits

---

## Key Unknowns (Research Needed)

### Technical
1. **What's the best substrate representation?**
   - Research: Voxels vs SDF vs hybrid vs pure mesh
   - Test: Small prototype of each
   - Measure: Memory, query speed, modification cost

2. **How to handle LOD transitions smoothly?**
   - Research: Geomipmapping, CLOD, ROAM algorithms
   - Test: Implement simple transition
   - Measure: Visual quality, performance

3. **What procedural algorithms work at scale?**
   - Research: Fractal terrain, Perlin noise, Worley noise
   - Test: Generate test terrain
   - Measure: Quality, speed, determinism

### Data
1. **What elevation data quality is sufficient?**
   - Research: Compare SRTM vs ALOS vs LiDAR
   - Test: Generate terrain from each
   - Measure: Visual quality, detail level

2. **Can we infer vegetation or need explicit data?**
   - Research: Vegetation density maps, climate data
   - Test: Procedural placement vs real data
   - Measure: Realism, coverage

3. **What's the minimal required data set?**
   - Research: What can be generated vs must be sourced
   - Test: Build world with minimal data
   - Measure: Quality, accuracy

### Performance
1. **What hardware can we target?**
   - Research: GPU requirements, memory needs
   - Test: Profile on range of hardware
   - Measure: FPS, memory, load times

2. **What's the network bandwidth reality?**
   - Research: Typical player bandwidth
   - Test: Measure modification sync costs
   - Measure: Acceptable for home internet?

---

## The Meta-Question

**"How do we know we're doing it right when nobody has done this?"**

**Answer:** We don't. But we can:

1. **Test incrementally** - Validate each assumption before building on it
2. **Measure objectively** - FPS, memory, bandwidth, quality metrics
3. **Fail fast** - If Phase 1 doesn't work, pivot before Phase 2
4. **Compare to reality** - Does it LOOK real? FEEL real?
5. **Prototype alternatives** - Try voxels AND SDF, pick winner

**The only way to know is to BUILD AND MEASURE.**

But build SMALL prototypes, not the whole system.

---

## Immediate Next Steps

**Option 1: Answer data questions** (research phase)
- Survey elevation data sources
- Test OSM completeness
- Document what's possible

**Option 2: Prove multi-resolution queries** (prototype phase)
- Implement simple multi-res query system
- Test on 1km² area
- Measure performance

**Option 3: Research substrate representation** (research phase)
- Study voxels vs SDF vs mesh approaches
- Build tiny prototype of each
- Compare objectively

**Which should we start with?**
