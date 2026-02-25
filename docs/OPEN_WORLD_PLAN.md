# Open World Infrastructure - Full Implementation Plan

## Philosophy: Plan to Fail

**Reality:** This is a massive system. Things WILL break. Plan for it.

**Approach:**
1. Build incrementally - each piece testable in isolation
2. Expect failure - design fallbacks and degradation
3. Measure everything - know what breaks and when
4. Iterate quickly - fix what breaks, move forward

---

## Phase 1: Chunk Streaming (Foundation)

**Goal:** Player can walk infinitely, chunks load/unload dynamically

### Components

#### 1.1 Chunk Loading Strategy
**What:** Load chunks in radius around player, unload distant chunks

**Implementation:**
```rust
struct ChunkStreamer {
    load_radius: f64,      // e.g., 500m (load chunks within this)
    unload_radius: f64,    // e.g., 1000m (unload chunks beyond this)
    max_loaded_chunks: usize, // Hard limit (e.g., 100 chunks)
    
    loaded_chunks: HashMap<ChunkId, ChunkData>,
    loading_queue: VecDeque<ChunkId>,  // Chunks to load (priority queue)
    unloading_queue: Vec<ChunkId>,     // Chunks to unload
}

impl ChunkStreamer {
    fn update(&mut self, player_pos: ECEF) {
        // 1. Calculate which chunks should be loaded
        let desired_chunks = self.chunks_in_radius(player_pos, self.load_radius);
        
        // 2. Find chunks to unload (too far away)
        let to_unload = self.chunks_beyond_radius(player_pos, self.unload_radius);
        
        // 3. Find chunks to load (missing from loaded set)
        let to_load = desired_chunks - loaded_chunks;
        
        // 4. Prioritize by distance (closest first)
        to_load.sort_by_distance(player_pos);
        
        // 5. Queue operations
        self.loading_queue.extend(to_load);
        self.unloading_queue.extend(to_unload);
    }
    
    fn process_queues(&mut self, budget_ms: f64) {
        // Budget time per frame (e.g., 5ms for chunk ops)
        let start = Instant::now();
        
        // Unload first (free memory)
        while let Some(chunk_id) = self.unloading_queue.pop() {
            self.unload_chunk(chunk_id);
            if start.elapsed().as_secs_f64() * 1000.0 > budget_ms {
                break;
            }
        }
        
        // Load second (use freed memory)
        while let Some(chunk_id) = self.loading_queue.pop_front() {
            self.load_chunk(chunk_id);
            if start.elapsed().as_secs_f64() * 1000.0 > budget_ms {
                break;
            }
        }
    }
}
```

**What Will Break:**
- ❌ Loading 100 chunks at once → freeze/OOM
- ❌ Unloading chunks player is standing on → fall through world
- ❌ Chunk boundaries → seams, gaps, z-fighting
- ❌ Coordinate precision → jitter at far distances (>10km from origin)

**Fallbacks:**
- ✅ Time budget per frame (max 5ms for streaming ops)
- ✅ Max loaded chunks limit (drop furthest if exceeded)
- ✅ Keep chunks in 3x3 grid around player always loaded (never unload under player)
- ✅ Placeholder mesh for loading chunks (low-poly cube)

#### 1.2 Asynchronous Loading
**What:** Load chunks in background thread, don't block main thread

**Implementation:**
```rust
// Main thread
fn update() {
    // Queue chunk load request
    chunk_streamer.request_load(chunk_id);
    
    // Check for completed loads
    while let Some((chunk_id, chunk_data)) = load_rx.try_recv() {
        chunk_manager.insert(chunk_id, chunk_data);
    }
}

// Background thread
fn chunk_loader_thread() {
    while let Ok(chunk_id) = load_rx.recv() {
        // Generate terrain (slow)
        let octree = terrain_gen.generate_chunk(chunk_id);
        
        // Generate mesh (slow)
        let mesh = marching_cubes(octree);
        
        // Send back to main thread
        complete_tx.send((chunk_id, ChunkData { octree, mesh }));
    }
}
```

**What Will Break:**
- ❌ Race conditions (chunk loaded twice, mesh outdated)
- ❌ Player moves fast → chunks never finish loading
- ❌ Too many background threads → CPU thrashing

**Fallbacks:**
- ✅ Deduplication (don't load same chunk twice)
- ✅ Cancellation (stop loading chunk if player moved away)
- ✅ Thread pool (max N loader threads, e.g., 4)

#### 1.3 Chunk Persistence
**What:** Save/load chunks from disk, don't regenerate every time

**Implementation:**
```rust
fn load_chunk(&mut self, chunk_id: ChunkId) -> Result<ChunkData> {
    // Try disk first
    if let Ok(data) = self.load_from_disk(chunk_id) {
        return Ok(data);
    }
    
    // Generate if not on disk
    let octree = self.terrain_gen.generate_chunk(chunk_id);
    let mesh = marching_cubes(&octree);
    
    // Save for next time
    self.save_to_disk(chunk_id, &octree)?;
    
    Ok(ChunkData { octree, mesh })
}
```

**What Will Break:**
- ❌ Disk I/O too slow → stutter
- ❌ Corrupted chunk files → crash
- ❌ Disk full → can't save new chunks

**Fallbacks:**
- ✅ Disk I/O in background thread
- ✅ Validate chunk on load (skip if corrupted, regenerate)
- ✅ LRU eviction (delete oldest chunks if disk full)

---

## Phase 2: LOD System (Performance)

**Goal:** Far chunks use less detail, near chunks use full detail

### Components

#### 2.1 Distance-Based LOD
**What:** Adjust mesh resolution based on distance from player

**Implementation:**
```rust
enum LODLevel {
    High,    // 0-200m: Full resolution (all voxels)
    Medium,  // 200-500m: Half resolution (skip every other voxel)
    Low,     // 500-1000m: Quarter resolution (skip 3/4 voxels)
    Lowest,  // 1000m+: Eighth resolution (skip 7/8 voxels)
}

impl ChunkStreamer {
    fn determine_lod(&self, chunk_id: ChunkId, player_pos: ECEF) -> LODLevel {
        let distance = chunk_id.center_ecef().distance_to(&player_pos);
        
        match distance {
            d if d < 200.0 => LODLevel::High,
            d if d < 500.0 => LODLevel::Medium,
            d if d < 1000.0 => LODLevel::Low,
            _ => LODLevel::Lowest,
        }
    }
}
```

**What Will Break:**
- ❌ LOD popping (ugly transitions when LOD changes)
- ❌ Mesh regeneration spam (LOD changes every frame as player moves)
- ❌ Memory usage (need multiple LOD meshes per chunk)

**Fallbacks:**
- ✅ Hysteresis (delay LOD transitions to prevent flickering)
- ✅ Blend zones (fade between LOD levels)
- ✅ LOD caching (don't regenerate if LOD hasn't changed)

#### 2.2 Horizon Culling
**What:** Don't render chunks below horizon (can't see them anyway)

**Implementation:**
```rust
fn is_below_horizon(&self, chunk_pos: ECEF, player_pos: ECEF) -> bool {
    // Earth curvature: chunks beyond ~5km are below horizon
    let distance = player_pos.distance_to(&chunk_pos);
    let horizon_distance = 4500.0; // Approximate for player at ground level
    
    distance > horizon_distance && chunk_pos.z < player_pos.z
}
```

**What Will Break:**
- ❌ Pop-in when chunk appears over horizon
- ❌ Incorrect culling (visible chunks marked as below horizon)

**Fallbacks:**
- ✅ Gradual fade-in for chunks appearing over horizon
- ✅ Conservative culling (only cull if definitely below horizon)

---

## Phase 3: Frustum Culling (Optimization)

**Goal:** Only render chunks in front of camera, not behind

### Components

#### 3.1 Camera Frustum Calculation
**What:** Calculate which chunks are visible based on camera direction

**Implementation:**
```rust
struct Frustum {
    planes: [Plane; 6],  // Near, far, left, right, top, bottom
}

impl Frustum {
    fn from_camera(view: Mat4, proj: Mat4) -> Self {
        // Calculate frustum planes from view-projection matrix
        let vp = proj * view;
        
        // Extract planes (standard frustum extraction)
        // ... math ...
        
        Self { planes }
    }
    
    fn contains_chunk(&self, chunk_bbox: AABB) -> bool {
        // Test if chunk bounding box intersects frustum
        for plane in &self.planes {
            if plane.distance_to_box(&chunk_bbox) < 0.0 {
                return false; // Outside this plane
            }
        }
        true // Inside all planes
    }
}
```

**What Will Break:**
- ❌ Chunks pop out when turning camera (frustum culling too aggressive)
- ❌ Chunks visible at edge of screen get culled (incorrect plane calculation)

**Fallbacks:**
- ✅ Expanded frustum (cull only if definitely outside)
- ✅ Conservative culling (keep chunks at edges)

#### 3.2 Occlusion Culling (Future)
**What:** Don't render chunks hidden behind other chunks

**Note:** This is HARD. Skip for now, add later if needed.

---

## Phase 4: Authority Management (Multiplayer)

**Goal:** Control who can edit what

### Components

#### 4.1 Parcel System
**What:** Divide world into parcels, assign ownership

**Implementation:**
```rust
struct Parcel {
    id: ParcelId,
    owner: Option<PeerId>,  // None = public land
    bounds: GeoBounds,       // GPS bounding box
    permissions: Permissions,
}

struct Permissions {
    allow_public_edit: bool,
    allowed_editors: HashSet<PeerId>,
}

impl ChunkManager {
    fn can_edit(&self, peer_id: PeerId, coord: VoxelCoord) -> bool {
        let parcel = self.get_parcel_at(coord);
        
        match parcel.owner {
            None => true,  // Public land
            Some(owner) if owner == peer_id => true,  // Owner can edit
            Some(_) => {
                // Check permissions
                parcel.permissions.allow_public_edit || 
                parcel.permissions.allowed_editors.contains(&peer_id)
            }
        }
    }
}
```

**What Will Break:**
- ❌ Permission checks too slow (every voxel edit)
- ❌ Griefing (players spam edits to lag server)
- ❌ Parcel boundaries (what if edit spans two parcels?)

**Fallbacks:**
- ✅ Cache permission checks (chunk-level, not voxel-level)
- ✅ Rate limiting (max X edits per second per player)
- ✅ Reject cross-parcel edits (must be entirely within one parcel)

#### 4.2 Admin Override
**What:** Admins can edit anywhere

**Implementation:**
```rust
struct Identity {
    peer_id: PeerId,
    is_admin: bool,  // Set from config file or admin list
}

fn can_edit(&self, identity: &Identity, coord: VoxelCoord) -> bool {
    identity.is_admin || self.check_parcel_permission(identity.peer_id, coord)
}
```

**What Will Break:**
- ❌ Admin flag spoofing (malicious peer claims admin)

**Fallbacks:**
- ✅ Admin list stored server-side or in signed certificate
- ✅ Reject operations from non-admin claiming admin flag

---

## Phase 5: Graceful Fallbacks (Reliability)

**Goal:** System keeps working even when things break

### Fallback Strategies

#### 5.1 Chunk Load Failure
```rust
fn load_chunk_safe(&mut self, chunk_id: ChunkId) -> ChunkData {
    match self.load_chunk(chunk_id) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to load chunk {:?}: {}", chunk_id, e);
            // Return placeholder
            ChunkData::empty_placeholder()
        }
    }
}
```

#### 5.2 Network Disconnect
```rust
fn handle_disconnect(&mut self, peer_id: PeerId) {
    // Remove from active players
    self.remote_players.remove(&peer_id);
    
    // Keep their operations (CRDT will merge when they reconnect)
    
    // Continue playing locally (local-first!)
}
```

#### 5.3 Operation Flood
```rust
fn apply_operation(&mut self, op: VoxelOperation) -> Result<()> {
    // Check queue size
    if self.pending_ops.len() > MAX_PENDING_OPS {
        eprintln!("Operation queue full, dropping operation");
        return Err(QueueFullError);
    }
    
    // Rate limit per peer
    let rate = self.op_rate_tracker.get_rate(op.peer_id);
    if rate > MAX_OPS_PER_SECOND {
        return Err(RateLimitError);
    }
    
    self.pending_ops.push(op);
    Ok(())
}
```

#### 5.4 Memory Exhaustion
```rust
fn update(&mut self) {
    // Check memory usage
    if self.loaded_chunks.len() > self.max_loaded_chunks {
        // Emergency unload (furthest chunks first)
        let to_unload = self.loaded_chunks.len() - self.max_loaded_chunks;
        self.emergency_unload(to_unload);
    }
}
```

---

## Phase 6: Testing Strategy

**Goal:** Know what breaks and when

### Test Scenarios

#### 6.1 Scale Tests
- **1 chunk:** Baseline
- **10 chunks:** Small area
- **100 chunks:** Medium area (1km radius)
- **1000 chunks:** Large area (10km radius)
- **10000 chunks:** Stress test (100km radius)

**Measure:**
- FPS
- Memory usage
- Load time
- Frame time budget

#### 6.2 Movement Tests
- **Walk 1km:** Normal movement
- **Fly 10km:** Fast movement
- **Teleport 100km:** Instant transition

**Measure:**
- Chunk pop-in
- Loading stutter
- Unloading lag

#### 6.3 Multiplayer Tests
- **2 players, same chunk:** Baseline
- **2 players, 100m apart:** Same region
- **2 players, 10km apart:** Different regions
- **10 players, same chunk:** Crowding
- **100 players, scattered:** Global scale

**Measure:**
- Bandwidth usage
- Operation latency
- Sync accuracy

#### 6.4 Stress Tests
- **Spam edits:** 1000 ops/sec, measure queue size
- **Network disconnect:** Rejoin, measure sync time
- **Corrupted chunk file:** Measure recovery
- **OOM scenario:** Measure emergency unload

---

## Implementation Order (Incremental)

### Week 1: Chunk Streaming Core
**Days 1-2:** Chunk loading/unloading logic
**Days 3-4:** Asynchronous loading (background thread)
**Days 5-7:** Testing + fixes

**Milestone:** Player can walk infinitely, chunks load/unload

### Week 2: LOD + Frustum Culling
**Days 1-3:** Distance-based LOD
**Days 4-5:** Frustum culling
**Days 6-7:** Testing + fixes

**Milestone:** 60 FPS with 100+ chunks loaded

### Week 3: Authority + Fallbacks
**Days 1-3:** Parcel system + permissions
**Days 4-5:** Graceful fallbacks
**Days 6-7:** Testing + fixes

**Milestone:** Multiplayer editing with permissions

### Week 4: Stress Testing + Polish
**Days 1-7:** Run all test scenarios, fix what breaks

**Milestone:** System handles 1000 chunks, 100 players, graceful degradation

---

## What WILL Break (Predictions)

### High Confidence (90%)
1. **Chunk boundaries** - Seams, gaps, z-fighting at borders
2. **Coordinate precision** - Jitter at >10km from origin
3. **Memory usage** - OOM with >1000 chunks
4. **Loading stutter** - Frame drops when many chunks load at once
5. **Network flood** - Bandwidth explosion with many players in same chunk

### Medium Confidence (50%)
6. **LOD popping** - Ugly transitions when LOD changes
7. **Permission lag** - Slow permission checks on every edit
8. **Mesh regeneration** - Too many mesh updates, CPU thrashing
9. **Background thread deadlock** - Race conditions in async loading
10. **Parcel conflicts** - Edits spanning parcel boundaries

### Low Confidence (25%)
11. **Frustum culling bugs** - Visible chunks get culled
12. **Horizon culling errors** - Incorrect horizon calculations
13. **CRDT merge conflicts** - Determinism breaks with 100+ concurrent edits
14. **Spatial sharding isolation** - Players in same chunk but different regions can't communicate

---

## Contingency Plans

For each predicted failure, we have a plan:

1. **Chunk boundaries** → Overlap chunks slightly, blend at boundaries
2. **Coordinate precision** → Use double precision, origin rebasing
3. **Memory usage** → Hard limit, emergency unload
4. **Loading stutter** → Time budget per frame (5ms max)
5. **Network flood** → Rate limiting, spatial sharding already implemented
6. **LOD popping** → Hysteresis, blend zones
7. **Permission lag** → Cache at chunk level
8. **Mesh regeneration** → Debounce, dirty flag
9. **Background thread deadlock** → Separate load/save threads, channels
10. **Parcel conflicts** → Reject cross-parcel edits
11. **Frustum culling bugs** → Conservative culling, expanded frustum
12. **Horizon culling errors** → Conservative culling, gradual fade
13. **CRDT merge conflicts** → Deterministic tie-breaking already implemented
14. **Spatial sharding isolation** → Subscribe to neighbor regions (already done)

---

## Success Metrics

After implementation, system should:
- ✅ Load 1000 chunks at 60 FPS
- ✅ Support 100 concurrent players
- ✅ Handle 1000 ops/sec without lag
- ✅ Gracefully degrade under stress (not crash)
- ✅ Recover from network disconnect
- ✅ Maintain 60 FPS with 10 players editing same chunk

If any metric fails, we iterate and fix.

---

## Next Steps

**Ready to start Week 1 (Chunk Streaming Core)?**

Or want to refine the plan first?
