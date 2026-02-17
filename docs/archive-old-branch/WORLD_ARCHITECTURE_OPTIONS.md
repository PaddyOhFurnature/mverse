# World Architecture Options - Comprehensive Analysis

**Created:** 2026-02-16  
**Status:** Architectural decision needed  
**Context:** Need perfect chunk boundaries for 1:1 scale. Current quad-sphere has 128m gaps.

## Requirements Summary

**Non-negotiable:**
- 1:1 scale Earth (40,075km circumference)
- Voxel-based (SVO) for destructibility
- P2P networking (libp2p + CRDT)
- Perfect seam alignment (no visible boundaries)
- Context-aware detail (sitting vs driving vs flying)
- Deterministic simulation

**Scale:**
- Surface area: 510M km²
- If 1m³ voxels: ~510 trillion voxels
- Brisbane chunk (depth 14): 677m × 547m × 512m deep = ~189M voxels
- Global chunks at depth 14: ~1.5 million chunks

---

## Option 1: Pure Continuous Queries (Database-First)

**Architecture:**
- World is distributed spatial database (no chunks at all)
- Query: "Give me all voxels in this AABB/frustum"
- P2P nodes run distributed R-tree or similar
- Each voxel has (ECEF_x, ECEF_y, ECEF_z, material, metadata)

**How it works:**
```rust
// Game logic requests arbitrary volume
let bounds = Bounds::from_frustum(camera);
let voxels = world_db.query_range(bounds);

// Rendering generates mesh from query results
let mesh = marching_cubes(voxels);

// Modification updates spatial index
world_db.insert_voxel(ecef_pos, material);
```

**Perfect alignment:** YES - no chunks to misalign  
**Context-aware:** YES - query exactly what you need  
**Scalability:** Database handles it  

### PROS:
1. **Mathematically perfect** - no chunk boundaries exist
2. **Truly context-aware** - query interior room (5m³) or hemisphere (20,000km³) with same API
3. **Elegant simplicity** - one abstraction for everything
4. **Natural P2P fit** - DHT already does distributed lookups
5. **Efficient modifications** - update single voxel, not entire chunk
6. **No overlap needed** - continuous by definition
7. **LOD is natural** - query resolution matches need

### CONS:
1. **Spatial range queries in DHT not solved** (⚠️ SHOWSTOPPER)
   - Kademlia does key-value lookups, not range scans
   - Need distributed R-tree or quadtree overlay
   - Research paper territory (2015-2020 papers exist but not production)
   
2. **Caching is hard** without discrete units
   - What do you cache? Individual voxels? Arbitrary regions?
   - Cache invalidation across infinite permutations
   - Memory management becomes complex
   
3. **Network efficiency concerns**
   - Requesting 1M voxels = 1M DHT lookups?
   - Need batching/aggregation layer (basically chunks by another name)
   
4. **State sync complexity**
   - CRDT logs tied to... what? Individual voxels?
   - How do you gossip "region has updates" without regions?
   
5. **Index maintenance overhead**
   - Every voxel modification updates spatial index
   - R-tree rebalancing on distributed system
   
6. **No production reference** - nobody has built this at scale
   
7. **Generation is complex** - can't generate "chunks", must generate on-demand
   
8. **Timeline: 6-12 months minimum** for research + implementation

### REALISTIC ASSESSMENT:
This is the "correct" computer science solution, but requires solving unsolved distributed systems problems. You'd need to:
- Implement distributed R-tree over libp2p
- Design spatial gossip protocol
- Handle replication/sharding of spatial regions (wait, that's chunks again)
- Extensive testing of edge cases

**Risk:** HIGH - could spend a year and fail  
**Timeline:** 6-12 months research + 6-12 months implementation  
**Correctness:** PERFECT (if it works)

---

## Option 2: Hidden Chunks (Minecraft/GTA Approach)

**Architecture:**
- Chunks exist internally (storage, networking, caching)
- Game logic sees continuous world
- Generation samples across chunk boundaries
- Rendering builds meshes without chunk awareness

**How it works:**
```rust
// Internal: chunks stored as discrete units
struct ChunkStorage {
    chunks: HashMap<ChunkId, SVO>
}

// External API: continuous
fn get_voxel(ecef: Vec3) -> Material {
    let chunk = find_chunk_containing(ecef);
    let local = ecef_to_chunk_local(ecef, chunk);
    chunk.svo.sample(local)
}

// Generation queries neighbors
fn generate_terrain(chunk_id: ChunkId) {
    let neighbors = get_neighbor_chunks(chunk_id);
    let extended_bounds = chunk_bounds(chunk_id).expand(10m);
    let elevation = query_srtm(extended_bounds); // overlaps neighbors
    voxelize_with_blending(elevation, neighbors);
}
```

**Perfect alignment:** YES - with overlapping generation  
**Context-aware:** PARTIAL - need additional LOD system  
**Scalability:** PROVEN - all major games use this  

### PROS:
1. **Proven at scale** - Minecraft, GTA V, Google Earth all use variants
2. **Incremental implementation** - can iterate chunk by chunk
3. **Clear caching boundaries** - chunk = cache unit
4. **Network efficient** - chunk = transfer unit
5. **P2P friendly** - chunk = gossip unit, natural DHT key
6. **State sync straightforward** - CRDT log per chunk
7. **Can solve boundary problem** with overlapping generation
8. **Existing code mostly preserved**
9. **Timeline: 6-8 weeks** for full implementation

### CONS:
1. **Chunk size is architectural constraint**
   - Too big: memory waste, network inefficiency
   - Too small: overhead, index size
   - Can't change easily later
   
2. **Overlap increases storage** - each boundary duplicated
   - 10m overlap on 500m chunk = ~4% overhead per chunk
   - 6 faces = potentially 24% overhead if naive
   
3. **Still have chunk-scale granularity** for some operations
   - Cache: load whole chunk or nothing
   - Network: request whole chunk
   - Can't efficiently stream "just this building interior"
   
4. **LOD transitions** need extra work - not automatic
   
5. **Interior spaces awkward** - apartment room spans multiple chunks?
   
6. **Quadtree/quad-sphere distortion** still exists
   - Either fix math (hard) or accept non-uniform chunk sizes
   
7. **Not truly continuous** - just hidden well

### REALISTIC ASSESSMENT:
This is the "practical engineering" solution. Well understood, proven to work, can be implemented incrementally. The overlap strategy solves the seam problem. Main limitation is chunks impose a granularity on everything.

**Risk:** LOW - known problem with known solutions  
**Timeline:** 6-8 weeks full implementation  
**Correctness:** VERY GOOD - visually perfect if done right

---

## Option 3: Hybrid Adaptive (Context-Dependent Architecture)

**Architecture:**
- Multiple systems running simultaneously
- Interior/detail: Pure continuous queries (small scale)
- Exterior/terrain: Hidden chunks (large scale)
- Transition zones: Hybrid
- System chooses based on context

**How it works:**
```rust
enum WorldRegion {
    Interior { spatial_db: RTree },  // Building interiors, caves
    Exterior { chunks: ChunkSystem },  // Outdoor terrain
    Transition { both: HybridSystem }  // Doors, cave entrances
}

fn get_voxel(ecef: Vec3) -> Material {
    let region = classify_region(ecef);
    match region {
        Interior => spatial_db.query_point(ecef),
        Exterior => chunk_system.query_point(ecef),
        Transition => blend(spatial_db, chunk_system, ecef)
    }
}
```

**Perfect alignment:** YES - continuous in interiors, overlapping in exteriors  
**Context-aware:** EXCELLENT - different systems for different needs  
**Scalability:** GOOD - uses best tool for each job  

### PROS:
1. **Best of both worlds** - continuous where needed, chunks where practical
2. **Optimized per use case:**
   - Coffee shop interior: Continuous, high detail, small volume
   - Open field: Chunked, lower detail, large volume
   - Dense city: Hybrid, varied detail
3. **Efficient at all scales** - right tool for right job
4. **Future-proof** - can add more region types
5. **Natural LOD** - system type implies detail level
6. **Memory efficient** - interior spaces don't waste chunk storage
7. **Network efficient** - small queries for interiors, bulk for exteriors

### CONS:
1. **COMPLEX** - multiple systems to maintain (⚠️ MAJOR)
2. **Transition zones are hard** - blending two different representations
3. **Classification system needed** - how to decide region type?
   - Density-based? (buildings = interior)
   - User-marked? (requires data)
   - Automatic? (complex algorithm)
4. **State sync complexity** - different CRDT strategies per region
5. **Testing surface area** - must test all combinations
6. **Debugging nightmare** - which system caused the bug?
7. **No reference implementation** - inventing new approach
8. **Timeline: 12-16 weeks** minimum
9. **Code complexity high** - harder to understand/maintain

### REALISTIC ASSESSMENT:
This is the "ideal" solution that optimizes everything, but at cost of high complexity. Risk of over-engineering. Could work brilliantly or become unmaintainable mess.

**Risk:** MEDIUM-HIGH - complexity risk, no references  
**Timeline:** 12-16 weeks minimum  
**Correctness:** EXCELLENT - if implemented correctly

---

## Option 4: Octree-First (Natural SVO Structure)

**Architecture:**
- World IS an octree at all scales
- Earth is root node
- Subdivide recursively to voxel level
- Chunks are just "octree nodes we cache/network"

**How it works:**
```rust
struct OctreeNode {
    bounds: AABB,
    children: Option<[Box<OctreeNode>; 8]>,
    data: Option<SVO>, // leaf nodes have voxel data
}

// Natural quadtree on sphere surface
// Depth 0: whole earth (1 node)
// Depth 5: ~1000km regions (1024 nodes)  
// Depth 10: ~30km regions (~1M nodes)
// Depth 15: ~1km regions (~33M nodes)
// Depth 20: ~30m regions (~1B nodes)

fn query_voxels(bounds: AABB) -> Vec<Voxel> {
    let nodes = octree.query_range(bounds);
    nodes.flat_map(|n| n.data.voxels())
}
```

**Perfect alignment:** YES - octree has no seams  
**Context-aware:** GOOD - natural LOD hierarchy  
**Scalability:** EXCELLENT - sparse octree scales to planet  

### PROS:
1. **Mathematically elegant** - one structure, all scales
2. **Natural LOD** - octree IS LOD hierarchy
3. **Sparse storage** - empty space costs ~nothing
4. **Perfect alignment** - octree subdivides uniformly
5. **Query efficiency** - tree traversal is O(log n)
6. **Already using SVO** - just extend to global scale
7. **Standard algorithm** - well understood
8. **Network friendly** - nodes are natural transfer units
9. **Cache friendly** - nodes are natural cache units
10. **P2P friendly** - node hash is DHT key

### CONS:
1. **Sphere doesn't map to octree cleanly** (⚠️ PROBLEM)
   - Octree is Cartesian (xyz)
   - Earth is spherical
   - Either waste space (bounding cube) or use complex projection
   
2. **Depth determines granularity** - locked to powers of 2
   - Can't have "chunks" that are e.g. 500m
   - Must be 512m, 256m, 128m, etc.
   
3. **Rebalancing on sphere is complex** - not uniform space
   
4. **Still need to choose "chunk depth"** - which octree level to cache/network?
   
5. **Memory overhead** - tree nodes have pointers
   
6. **Modification ripples up tree** - updating leaf affects ancestors

### REALISTIC ASSESSMENT:
Very clean conceptually, but octree and sphere are awkward fit. The quad-sphere (cube faces) approach is a workaround for this. Could work if we accept power-of-2 sizes and spherical distortion.

**Risk:** MEDIUM - sphere/octree mismatch  
**Timeline:** 8-10 weeks  
**Correctness:** GOOD - octree is continuous, but sphere projection still distorts

---

## Option 5: Streaming Database with Spatial Indexes

**Architecture:**
- World stored in proper spatial database (PostGIS, custom)
- Local cache of recent queries
- Stream voxels on-demand
- P2P nodes replicate popular regions

**How it works:**
```rust
struct WorldDB {
    local_cache: RTree<Voxel>,
    remote_peers: Vec<PeerId>,
    db_connection: DatabaseConnection, // fallback
}

fn get_voxels(bounds: AABB) -> Vec<Voxel> {
    // 1. Check local cache
    if let Some(voxels) = local_cache.query(bounds) {
        return voxels;
    }
    
    // 2. Query P2P peers
    if let Some(voxels) = p2p_query(bounds) {
        local_cache.insert(voxels);
        return voxels;
    }
    
    // 3. Query central DB (fallback)
    let voxels = db.spatial_query(bounds);
    local_cache.insert(voxels);
    voxels
}
```

**Perfect alignment:** YES - database has no chunks  
**Context-aware:** EXCELLENT - query arbitrary bounds  
**Scalability:** DEPENDS - on database architecture  

### PROS:
1. **Proven tech** - PostGIS has spatial indexes
2. **SQL is powerful** - complex queries, joins, etc.
3. **Replication solved** - databases handle this
4. **Backup/recovery** - standard database tools
5. **Debugging tools** - can inspect with SQL
6. **Gradual loading** - query more detail as needed
7. **Modification tracking** - databases have transaction logs

### CONS:
1. **Central database** - conflicts with P2P goal (⚠️ ARCHITECTURAL)
2. **Network latency** - query across internet for each view
3. **Throughput limits** - database becomes bottleneck
4. **Not deterministic** - network timing affects state
5. **Who runs the database?** - centralization problem
6. **Cost at scale** - hosting 510M km² of voxel data
7. **Doesn't solve P2P problem** - just moves it

### REALISTIC ASSESSMENT:
This works for client-server architecture but conflicts with P2P requirement. Could have hybrid: central database seeds data, P2P replicates active regions. But then we're back to chunking for P2P.

**Risk:** MEDIUM - architectural conflict with P2P  
**Timeline:** 10-12 weeks  
**Correctness:** GOOD - but centralized

---

## Option 6: Micro-Chunks with Aggressive Streaming

**Architecture:**
- Very small chunks (10m × 10m × 10m)
- Massive quantity (millions of chunks)
- Stream aggressively, cache aggressively
- Accept high overhead for fine granularity

**How it works:**
```rust
// At 10m chunks, Earth surface has ~510M chunks
// Brisbane 1km² = 10,000 chunks

struct MicroChunk {
    id: ChunkId,
    svo: CompressedSVO, // highly compressed
    neighbors: [ChunkId; 26], // 3D neighbors
}

// Stream only visible chunks
fn update_visible_chunks(frustum: Frustum) {
    let visible = spatial_index.query_frustum(frustum);
    for chunk_id in visible {
        if !cache.has(chunk_id) {
            stream_chunk(chunk_id);
        }
    }
}
```

**Perfect alignment:** GOOD - 10m boundaries less noticeable  
**Context-aware:** GOOD - fine-grained loading  
**Scalability:** CHALLENGING - millions of chunks  

### PROS:
1. **Fine-grained control** - stream exactly what's needed
2. **Small network transfers** - 10m³ chunk is tiny
3. **Efficient modifications** - update only affected micro-chunk
4. **Good for P2P** - small chunks, small deltas
5. **Boundaries less visible** - 10m seams easier to hide than 500m
6. **Natural for interiors** - room-sized chunks
7. **Caching is efficient** - evict unused chunks quickly

### CONS:
1. **HUGE chunk count** - 510M+ chunks globally (⚠️ MAJOR)
2. **Index overhead** - tracking millions of chunks
3. **Memory overhead** - each chunk has metadata
4. **Network overhead** - millions of DHT keys
5. **Generation overhead** - generate millions of chunks
6. **Still have seams** - just smaller
7. **Pointer chasing** - rendering visits many chunks
8. **Not truly continuous** - just finer-grained chunking

### REALISTIC ASSESSMENT:
This is "chunking but smaller". Helps with some problems (granularity) but amplifies others (count, overhead). At some point, the overhead of managing millions of chunks exceeds the benefit.

**Risk:** MEDIUM - overhead concerns  
**Timeline:** 8-10 weeks  
**Correctness:** GOOD - small seams easier to hide

---

## Option 7: Lazy Evaluation / Procedural Generation

**Architecture:**
- Nothing exists until queried
- Generate voxels on-demand from source data
- Cache generated results
- Modifications stored as deltas

**How it works:**
```rust
struct ProceduralWorld {
    modifications: HashMap<ECEF, Material>, // user changes
    cache: LRU<AABB, Vec<Voxel>>,
}

fn get_voxels(bounds: AABB) -> Vec<Voxel> {
    // Check cache first
    if let Some(voxels) = cache.get(bounds) {
        return voxels;
    }
    
    // Generate from source data
    let elevation = query_srtm(bounds);
    let buildings = query_osm(bounds);
    let mut voxels = procedural_generate(elevation, buildings);
    
    // Apply user modifications
    for (pos, material) in modifications {
        if bounds.contains(pos) {
            voxels.set(pos, material);
        }
    }
    
    cache.insert(bounds, voxels);
    voxels
}
```

**Perfect alignment:** YES - generated continuously  
**Context-aware:** EXCELLENT - generate at any resolution  
**Scalability:** EXCELLENT - only exists what's needed  

### PROS:
1. **Infinite detail** - generate at any resolution
2. **Zero base storage** - only modifications stored
3. **Perfect alignment** - no chunk boundaries
4. **True 1:1 scale** - source data is 1:1
5. **Network efficient** - transfer modifications only
6. **Deterministic** - same input = same output
7. **Natural LOD** - generate detail level needed
8. **Memory efficient** - cache only active regions

### CONS:
1. **Generation cost** - every query generates
   - Expensive: query SRTM, query OSM, voxelize, marching cubes
   - Must be FAST or cache hit rate must be HIGH
   
2. **Modification tracking complex**
   - How to efficiently find "which modifications affect this query?"
   - Spatial index of modifications needed (chunking by another name)
   
3. **Determinism requirement** - must guarantee same results
   - Floating point consistency
   - Algorithm stability
   
4. **Caching strategy critical** - determines performance
   - What boundaries for cache regions? (chunks again)
   - When to evict?
   
5. **P2P replication** - what to replicate? Modifications? Cache?
   
6. **Source data access** - SRTM/OSM must be local or cached

### REALISTIC ASSESSMENT:
This is elegant but moves complexity to caching layer. The "cache regions" become chunks by another name. Generation cost could be prohibitive without excellent caching. Works best with hybrid: chunk-cached procedural generation.

**Risk:** MEDIUM - caching complexity, performance risk  
**Timeline:** 10-12 weeks  
**Correctness:** EXCELLENT - truly continuous

---

## Comparison Matrix

| Approach | Alignment | Scalability | P2P Fit | Timeline | Risk | Correctness |
|----------|-----------|-------------|---------|----------|------|-------------|
| Pure Continuous | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ | 12-24mo | HIGH | ⭐⭐⭐⭐⭐ |
| Hidden Chunks | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | 6-8wk | LOW | ⭐⭐⭐⭐ |
| Hybrid Adaptive | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | 12-16wk | MED-HIGH | ⭐⭐⭐⭐⭐ |
| Octree-First | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | 8-10wk | MEDIUM | ⭐⭐⭐⭐ |
| Streaming DB | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐ | 10-12wk | MEDIUM | ⭐⭐⭐⭐ |
| Micro-Chunks | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | 8-10wk | MEDIUM | ⭐⭐⭐ |
| Lazy/Procedural | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | 10-12wk | MEDIUM | ⭐⭐⭐⭐⭐ |

---

## Hybrid Recommendation: Procedural + Hidden Chunks

**Combine the best of Options 2 and 7:**

```rust
// Chunks are STORAGE and NETWORK units only
struct ChunkStore {
    chunks: HashMap<ChunkId, CachedSVO>,
}

// Generation is CONTINUOUS across boundaries
fn generate_terrain(bounds: AABB) -> SVO {
    // Query source data (overlaps multiple chunks)
    let elevation = query_srtm(bounds.expand(10m));
    let buildings = query_osm(bounds);
    
    // Generate voxels procedurally
    let voxels = procedural_voxelize(elevation, buildings);
    
    // Store in affected chunks
    for chunk_id in chunks_overlapping(bounds) {
        let chunk_voxels = voxels.clip_to_chunk(chunk_id);
        chunk_store.insert(chunk_id, chunk_voxels);
    }
    
    voxels
}

// Query is CONTINUOUS
fn query_voxels(bounds: AABB) -> Vec<Voxel> {
    let chunks = find_chunks_intersecting(bounds);
    
    // If all chunks cached, return quickly
    if chunks.iter().all(|c| chunk_store.has(c)) {
        return merge_chunk_voxels(chunks, bounds);
    }
    
    // Otherwise generate on-demand
    generate_terrain(bounds)
}
```

**This gives us:**
1. ✅ Perfect alignment - continuous generation
2. ✅ Efficient caching - chunks as cache units
3. ✅ P2P friendly - chunks as network units
4. ✅ Scalable - proven chunk architecture
5. ✅ Context-aware - query arbitrary bounds
6. ✅ Deterministic - procedural generation
7. ✅ Incremental - can implement step by step

**Timeline: 8-10 weeks**
- Week 1-2: Decouple generation from chunk boundaries
- Week 3-4: Implement continuous query API
- Week 5-6: Overlapping generation and blending
- Week 7-8: Testing and optimization
- Week 9-10: P2P integration and stress testing

**Risk: LOW-MEDIUM** - combines proven techniques

---

## Decision Criteria

**Choose Pure Continuous if:**
- You have 12+ months for research
- You're willing to risk complete failure
- You want to publish research papers
- Correctness > timeline

**Choose Hidden Chunks if:**
- You need working system in 6-8 weeks
- You want proven, low-risk approach
- You're okay with "very good" instead of "perfect"
- Practical > theoretical

**Choose Hybrid Adaptive if:**
- You need perfect interior spaces
- You have 12-16 weeks
- You can handle high complexity
- You want optimal for all cases

**Choose Procedural + Hidden Chunks if:** ⭐ **RECOMMENDED**
- You want continuous generation with proven storage
- You need working system in 8-10 weeks
- You want good balance of correctness and practicality
- You want incremental implementation path

---

## My Honest Recommendation

**Start with Procedural + Hidden Chunks (Option 7 + 2 hybrid)**

Reasons:
1. **Achievable timeline** - 8-10 weeks vs 12-24 months
2. **Low risk** - combines proven techniques
3. **Incremental** - can see progress weekly
4. **Correct enough** - continuous generation solves alignment
5. **Future-proof** - can evolve to pure continuous later
6. **Practical** - chunks solve caching, networking, P2P elegantly

The pure continuous query system is mathematically beautiful, but requires solving research problems that might not have solutions. The procedural+chunks hybrid gets you 95% of the benefits with 10% of the risk.

**Implementation priority:**
1. Decouple terrain generation from chunk alignment (2 weeks)
2. Implement continuous query API (2 weeks)  
3. Add overlapping generation (2 weeks)
4. Testing and edge cases (2 weeks)
5. Optimization and P2P integration (2 weeks)

Then you have working system with perfect seams. If you still want pure continuous queries after that, you can refactor—but at least you'll have proven the concept first.

---

## Questions for User

1. **Risk tolerance:** Research project (high risk, high reward) or practical system (proven approach)?
2. **Timeline:** Is 6-8 weeks fast enough, or do you need it sooner?
3. **Interior spaces:** How important are building interiors vs outdoor terrain?
4. **Complexity tolerance:** Willing to maintain complex hybrid system or prefer simpler architecture?
5. **Future evolution:** Want to build simple first and enhance later, or build "correct" from start?
