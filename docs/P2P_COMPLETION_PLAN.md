# P2P Infrastructure Completion Plan

**Status:** Phase 1 Working, Phase 2 Starting  
**Goal:** Production-ready P2P with persistence, verification, and optimization

---

## ✅ Phase 1: COMPLETE (Tested Working)

**Achievements:**
- ✅ Network infrastructure (libp2p, mDNS, gossipsub)
- ✅ Player state sync (20Hz, real-time, 60 FPS)
- ✅ Voxel operation sync (tested - user dug under player, they fell)
- ✅ UserContentLayer (operation log, CRDT foundation)
- ✅ Three-layer architecture (base terrain, infrastructure, user content)

**What works:**
- 3 clients connected simultaneously
- Real-time position sync
- Terrain modifications sync between clients
- Wireframe rendering of remote players
- Operation logging

---

## 🎯 Phase 2: Core P2P Completion (CRITICAL)

**Priority:** HIGHEST - Foundation for everything else

### 2.1 Operation Log Persistence (1-2 days)

**Why Critical:**
- Without persistence, all edits lost on shutdown
- Foundation for offline sync
- Required for chunk state verification

**Implementation:**
```rust
// On shutdown
user_content.save_op_log("world/chunks/chunk_0_0/ops.json")?;

// On startup
user_content.load_op_log("world/chunks/chunk_0_0/ops.json")?;
for op in user_content.op_log() {
    octree.set_voxel(op.coord, op.material.to_material_id());
}
```

**Files to modify:**
- `examples/phase1_multiplayer.rs` - Add save on window close, load on startup
- `src/user_content.rs` - Already has save/load methods

**Success criteria:**
- Edit terrain, quit, restart → edits persist
- Multiple chunks have separate op logs
- Old ops can be pruned (keep last 1000 per chunk)

**Todo:** `op-log-storage`

---

### 2.2 Vector Clocks for CRDT (2-3 days)

**Why Critical:**
- Lamport timestamps don't capture causality properly
- Concurrent operations need proper detection
- Foundation for deterministic conflict resolution

**Current (Lamport):**
```rust
struct VoxelOperation {
    timestamp: u64,  // Single counter
}
```

**Better (Vector Clock):**
```rust
struct VectorClock {
    clocks: BTreeMap<PeerId, u64>,  // Per-peer counter
}

impl VectorClock {
    fn happens_before(&self, other: &VectorClock) -> bool;
    fn happens_after(&self, other: &VectorClock) -> bool;
    fn concurrent(&self, other: &VectorClock) -> bool;
    fn merge(&mut self, other: &VectorClock);
    fn increment(&mut self, peer: PeerId);
}
```

**CRDT merge with vector clocks:**
```rust
fn merge(op_a: &VoxelOp, op_b: &VoxelOp) -> Material {
    if op_a.vector_clock.happens_after(&op_b.vector_clock) {
        op_a.material  // A causally after B
    } else if op_b.vector_clock.happens_after(&op_a.vector_clock) {
        op_b.material  // B causally after A
    } else {
        // Concurrent - use timestamp + PeerId tiebreak
        deterministic_tiebreak(op_a, op_b)
    }
}
```

**Files to create:**
- `src/vector_clock.rs` - VectorClock implementation with tests

**Files to modify:**
- `src/messages.rs` - Add VectorClock to VoxelOperation
- `src/multiplayer.rs` - Update clock on every operation
- `src/user_content.rs` - Use vector clock for CRDT merge

**Success criteria:**
- Concurrent edits detected correctly
- Causal ordering preserved
- All clients converge to same state
- Unit tests with 1000+ random concurrent ops

**Todo:** `vector-clock`

---

### 2.3 Proper Signature Verification (2 days)

**Why Critical:**
- Currently verify_signature() is a placeholder
- Need to prevent unauthorized edits
- Foundation for trustless P2P

**Current:**
```rust
pub fn verify_signature(&self) -> bool {
    true  // Placeholder
}
```

**Proper implementation:**
```rust
use ed25519_dalek::{PublicKey, Signature, Verifier};

pub fn verify_signature(&self) -> bool {
    // Extract public key from author PeerId
    let public_key = match extract_public_key(&self.author) {
        Some(key) => key,
        None => return false,
    };
    
    // Create signature bytes
    let sig = match Signature::from_bytes(&self.signature) {
        Ok(s) => s,
        Err(_) => return false,
    };
    
    // Verify signature on message (coord + material + timestamp)
    let message = self.serialize_for_signing();
    public_key.verify(&message, &sig).is_ok()
}

fn extract_public_key(peer_id: &PeerId) -> Option<PublicKey> {
    // libp2p PeerId contains public key (for some key types)
    // Extract and convert to ed25519_dalek::PublicKey
    // See: https://docs.rs/libp2p/latest/libp2p/identity/
}
```

**Challenge:** PeerId → PublicKey extraction
- libp2p identity::PublicKey → ed25519_dalek::PublicKey
- Need to ensure PeerId was created from Ed25519 key

**Files to modify:**
- `src/messages.rs` - Implement real verify_signature()
- `src/identity.rs` - Add public_key() method
- `src/multiplayer.rs` - Store peer public keys

**Success criteria:**
- Valid signatures pass verification
- Invalid signatures rejected
- Tampered messages detected
- Performance: <1ms per verification

**Todo:** Part of `permission-system`

---

### 2.4 Chunk Manifest System (3-4 days)

**Why Critical:**
- Foundation for trustless chunk verification
- Required for peer-to-peer chunk sharing
- Enables deterministic terrain generation verification

**Architecture:**
```rust
struct ChunkManifest {
    chunk_id: ChunkId,
    
    // Base terrain (deterministic, verifiable)
    terrain_hash: [u8; 32],  // SHA256 of generated terrain
    terrain_seed: u64,        // For deterministic regeneration
    
    // User modifications
    op_log: Vec<VoxelOperation>,
    op_log_hash: [u8; 32],   // SHA256 of op_log
    
    // Current state (for quick verification)
    state_hash: [u8; 32],    // SHA256(terrain + replay(ops))
    
    // Metadata
    created_at: u64,
    last_modified: u64,
    total_operations: usize,
}

impl ChunkManifest {
    fn verify(&self, terrain_gen: &TerrainGenerator) -> bool {
        // 1. Regenerate base terrain
        let terrain = terrain_gen.generate(self.chunk_id, self.terrain_seed);
        if sha256(&terrain) != self.terrain_hash {
            return false;  // Terrain doesn't match
        }
        
        // 2. Verify each operation signature
        for op in &self.op_log {
            if !op.verify_signature() {
                return false;  // Invalid signature
            }
        }
        
        // 3. Replay operations
        let mut state = terrain.clone();
        for op in &self.op_log {
            state.set_voxel(op.coord, op.material.to_material_id());
        }
        
        // 4. Verify final state hash
        sha256(&state) == self.state_hash
    }
}
```

**Files to create:**
- `src/chunk_manifest.rs` - ChunkManifest implementation

**Files to modify:**
- `src/user_content.rs` - Create manifest from UserContentLayer
- `src/terrain.rs` - Add deterministic seed parameter

**Success criteria:**
- Can generate manifest from chunk state
- Can verify manifest against terrain generator
- Invalid manifests rejected
- Tampered op logs detected

**Todo:** `chunk-manifest`

---

### 2.5 Pure Terrain Generation (3-4 days)

**Why Critical:**
- Required for deterministic chunk verification
- Every peer must generate identical terrain
- Foundation for trustless P2P

**Current:**
```rust
impl TerrainGenerator {
    pub fn generate_chunk(&mut self, center: GPS) -> Octree {
        // Has state, not deterministic
    }
}
```

**Goal:**
```rust
pub fn generate_chunk_deterministic(
    chunk_id: ChunkId,
    seed: u64,
    srtm_data: &SRTMData,
) -> Octree {
    // Pure function - same inputs = same output
    // No &mut self, no hidden state
    // Deterministic RNG from seed
}
```

**Challenges:**
1. **Caching** - Current generator caches SRTM data
   - Solution: Pass cache as parameter, or load per-call
2. **SRTM loading** - I/O is non-deterministic
   - Solution: Load SRTM data once at startup, pass as parameter
3. **Random terrain features** - Need seeded RNG
   - Solution: ChaCha RNG with chunk_id + seed

**Files to modify:**
- `src/terrain.rs` - Make generate_chunk() pure
- `src/elevation.rs` - Separate caching from generation

**Success criteria:**
- Same chunk_id + seed → identical Octree on any machine
- No &mut self in generation function
- All RNG seeded deterministically
- Hash verification passes

**Todo:** `pure-terrain-gen`

---

## 🎯 Phase 3: Hardening & Optimization (IMPORTANT)

**Priority:** HIGH - Production readiness

### 3.1 Permission System (2-3 days)

**Why Important:**
- Prevent unauthorized edits
- Parcel ownership enforcement
- Foundation for land economy

**Current:**
```rust
pub fn has_access(&self, peer: PeerId, coord: &VoxelCoord) -> bool {
    // Foundation exists, not enforced
    true
}
```

**Implementation:**
```rust
// Add CLAIM operation type
enum OperationType {
    SetVoxel { coord: VoxelCoord, material: Material },
    ClaimParcel { bounds: ParcelBounds },
    GrantAccess { parcel: ParcelBounds, grantee: PeerId },
}

// Verify permission before applying
fn apply_operation(&mut self, op: VoxelOperation) -> Result<bool> {
    if self.config.verify_permissions {
        match &op.operation {
            OperationType::SetVoxel { coord, .. } => {
                if !self.has_access(op.author, coord) {
                    return Err(ApplyError::Unauthorized);
                }
            }
            OperationType::ClaimParcel { bounds } => {
                // Check no existing claim overlaps
            }
        }
    }
    // Apply operation...
}
```

**Files to modify:**
- `src/messages.rs` - Add OperationType enum
- `src/user_content.rs` - Enforce permissions in apply_operation()
- `examples/phase1_multiplayer.rs` - Broadcast CLAIM operations

**Success criteria:**
- Unauthorized edits rejected
- Parcel claims work
- Access grants work
- Toggle permissions for testing

**Todo:** `permission-system`

---

### 3.2 Bandwidth Optimization (2-3 days)

**Why Important:**
- Target: <100 KB/s per connection
- Enable play over mobile/satellite
- Scale to 100+ nearby players

**Current usage (10 players):**
- Player state @ 20Hz: 12.8 KB/s
- Voxel ops @ 1/sec: 1.3 KB/s
- Total: ~16 KB/s ✅

**Optimizations:**

1. **Delta encoding** - Only send what changed
```rust
// Current: Send full state every 50ms
PlayerStateMessage {
    position: [f64; 3],  // 24 bytes
    velocity: [f32; 3],  // 12 bytes
    yaw: f32, pitch: f32,  // 8 bytes
    mode: u8,            // 1 byte
}  // 45 bytes

// Optimized: Only send deltas
if position_changed_significantly() {
    send_position_delta();  // 8 bytes (f32 deltas)
}
if rotation_changed_significantly() {
    send_rotation_delta();  // 4 bytes
}
```

2. **Rate limiting**
```rust
// Don't send if change is tiny
const MIN_POSITION_DELTA: f32 = 0.01;  // 1cm
const MIN_ROTATION_DELTA: f32 = 0.01;  // ~0.5 degrees

if (new_pos - last_sent_pos).length() < MIN_POSITION_DELTA {
    return;  // Skip update
}
```

3. **Compression**
```rust
// Use bincode instead of JSON
let bytes = bincode::serialize(&message)?;  // Smaller than JSON
```

**Files to modify:**
- `src/messages.rs` - Add delta encoding
- `src/multiplayer.rs` - Track last-sent state, implement rate limiting

**Success criteria:**
- Bandwidth reduced by 50%+ for stationary players
- Small movements don't flood network
- All clients still sync smoothly

**Todo:** `bandwidth-optimize`

---

### 3.3 Spatial Sharding (2-3 days)

**Why Important:**
- Scale to 1000+ players globally
- Bandwidth constant regardless of total players
- Only sync nearby entities

**Goal:** Only broadcast to players within 1km radius

**Implementation:**
```rust
struct SpatialBroadcaster {
    visible_range: f64,  // meters, e.g. 1000.0
    
    // Cache of which peers are nearby
    nearby_peers: HashMap<PeerId, ECEF>,
}

impl SpatialBroadcaster {
    fn update_nearby_peers(&mut self, my_position: ECEF, all_peers: &[RemotePlayer]) {
        self.nearby_peers.clear();
        for peer in all_peers {
            let distance = (peer.position - my_position).length();
            if distance < self.visible_range {
                self.nearby_peers.insert(peer.peer_id, peer.position);
            }
        }
    }
    
    fn broadcast_to_nearby(&self, message: PlayerStateMessage) {
        for peer_id in self.nearby_peers.keys() {
            network.send_to(*peer_id, message.clone());
        }
    }
}
```

**Challenge:** Gossipsub broadcasts to all peers
- Solution: Use direct messaging (request-response) instead
- Or: Dynamic gossipsub topics per zone

**Files to modify:**
- `src/network.rs` - Add request-response protocol
- `src/multiplayer.rs` - Implement spatial filtering

**Success criteria:**
- Player in Brisbane doesn't receive updates from Sydney
- Both in same area → sync works
- Bandwidth independent of global player count

**Todo:** `spatial-sharding`

---

### 3.4 NAT Traversal (3-4 days)

**Why Important:**
- Enable cross-internet connections
- Work behind firewalls/NATs
- Production deployment requirement

**Implementation:**
```rust
// Add relay transport to libp2p
use libp2p::relay;

let transport = tcp::Transport::default()
    .upgrade(Version::V1)
    .authenticate(noise::Config::new(&local_key)?)
    .multiplex(yamux::Config::default())
    .or_transport(relay::client::Transport::new(...))  // Add relay
    .boxed();
```

**Relay servers:**
- Deploy 3-5 relay nodes on cloud VPS
- Configure in client as fallback
- Relay assists NAT hole punching

**Files to modify:**
- `src/network.rs` - Add relay transport
- Add `relay_nodes.toml` - List of public relay addresses

**Success criteria:**
- Two peers behind different NATs connect
- Relay used only when direct connection fails
- <100ms additional latency via relay

**Todo:** `nat-traversal`

---

### 3.5 Determinism Audit (2 days)

**Why Important:**
- All clients must compute same results
- Critical for CRDT convergence
- Foundation for trustless verification

**Audit checklist:**

1. **HashMap → BTreeMap** where iteration order matters
2. **Seeded RNG** for all randomness
3. **Fixed timestep physics** (already using Rapier correctly)
4. **No system time** in game logic (use Lamport/vector clocks)
5. **Floating point** - ensure same operations on all platforms

**Files to audit:**
- `src/voxel.rs` - Check HashMap usage
- `src/terrain.rs` - Check RNG seeding
- `src/physics.rs` - Verify fixed timestep
- `src/multiplayer.rs` - Check timestamp sources

**Success criteria:**
- Same inputs → same outputs on different machines
- Replay test: Record ops, replay → identical state
- Cross-platform verification test

**Todo:** `determinism-audit`

---

## 📊 Implementation Order (Recommended)

**Week 1: Core Infrastructure**
1. Operation log persistence (1-2 days) ← START HERE
2. Vector clocks (2-3 days)
3. Signature verification (2 days)

**Week 2: Trustless Architecture**
4. Pure terrain generation (3-4 days)
5. Chunk manifest system (3-4 days)

**Week 3: Production Hardening**
6. Permission system (2-3 days)
7. Bandwidth optimization (2-3 days)
8. Determinism audit (2 days)

**Week 4: Scale & Deployment**
9. Spatial sharding (2-3 days)
10. NAT traversal (3-4 days)

**Total:** ~28 days (4 weeks)

---

## Success Criteria (Phase 2 Complete)

**Technical:**
- ✅ Edits persist across restarts
- ✅ Proper CRDT with vector clocks
- ✅ Signature verification working
- ✅ Deterministic terrain generation
- ✅ Chunk manifests verify correctly
- ✅ Unauthorized edits rejected
- ✅ Bandwidth <100 KB/s per peer
- ✅ Works behind NAT

**User Experience:**
- ✅ Edit offline, sync when connection returns
- ✅ 1000 global players, only see nearby 100
- ✅ Cross-internet play works
- ✅ Malicious peers detected and blocked
- ✅ All clients converge to same world state

---

## Dependencies

```
op-log-storage ─┐
                ├─→ chunk-manifest
vector-clock ───┤
                │
pure-terrain ───┘

permission-system (independent)
bandwidth-optimize (independent)
spatial-sharding (independent)
nat-traversal (independent)
determinism-audit (independent)
```

**Critical path:** op-log → vector-clock → chunk-manifest
**Can parallelize:** permissions, bandwidth, spatial, NAT

---

## Current Status

**Completed (Phase 1):** 17/30 todos ✅
**Next:** Operation log persistence ← START HERE
**Blockers:** None
**Ready to proceed:** YES

---

This is production-quality P2P infrastructure for a decade-long project.
Every component designed for scale, security, and offline-first operation.
No shortcuts. No technical debt. Built correctly from the start.
