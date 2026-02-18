# P2P NETWORKING IMPLEMENTATION PLAN
## Phase 1.5: Local-First Architecture with Graceful Degradation

**Created:** 2026-02-18  
**Updated:** 2026-02-18 (added bandwidth budget architecture)  
**Status:** Ready to implement  
**Timeline:** 3-4 weeks full-time  
**Priority:** CRITICAL - Architectural foundation for entire project

---

## 🎯 Core Philosophy: Local-First with Graceful Degradation

> _"Your world exists on YOUR machine first. The network is just a sync layer that operates at whatever level it can."_

**The Fundamental Principle:**
- **Your machine is the source of truth** for your parcels, edits, and identity
- **Network is a sync layer** that operates at whatever bandwidth is available
- **Client never "fails"** — it just renders at lower fidelity
- **Works offline** — edits queue locally, sync when connection returns

This is not "multiplayer with offline mode" — it's **"single-player with optional sync."**

### Why Now? (User's Insights)

> _"I think with the complexity of this project, it needs to be implemented much earlier for my sanity because almost everything we do from this point has to be interactable by potentially millions of players."_

**The Problem:**
- Original plan: Networking in Phase 6 (after chunks, LOD, real-world data)
- Reality: Building single-player first = massive refactoring later
- Every system from this point needs to work with concurrent players
- **NEW INSIGHT:** Every system also needs a degraded mode for limited bandwidth

**The Solution:**
- Build P2P foundation NOW (Phase 1.5)
- All future systems designed with bandwidth budgets in mind
- Every feature has a "fidelity level" based on data availability
- Incremental complexity instead of big-bang integration

### The Snow Crash Parallel

In *Snow Crash*, Hiro's home exists on his machine. The Street exists because everyone's machines collectively serve it. If your connection is bad, the Metaverse degrades:
- **Cheap public terminals:** Black-and-white, low-res avatars
- **Rich connections:** Full color, detailed avatars, complex buildings

**This project:**
- **"Home"** = Your owned parcels, stored locally, always available
- **"The Street"** = Shared world, served by P2P peers, available at whatever fidelity
- **"Cheap terminal"** = Packet radio/offline mode — wireframe world, text chat, queued sync
- **"Rich connection"** = Fiber — full AAA, real-time everything

**The difference:** It's all the same client, same code, just different data availability driving different rendering paths.

---

## 📊 The Six-Priority Bandwidth Stack

Every piece of data has a priority. The sync engine allocates bandwidth top-down. Lower priorities gracefully degrade when bandwidth shrinks.

```
┌──────────────────────────────────────────────────────────────┐
│  PRIORITY 1: Local State (ZERO bandwidth)                   │
│  ────────────────────────────────────────                   │
│  Your home, inventory, builds, position                     │
│  Stored on YOUR disk. Works offline. Works forever.         │
│  💾 Storage: ~/.metaverse/parcels/, ~/.metaverse/cache/     │
│  ✅ ALWAYS WORKS                                             │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  PRIORITY 2: State Sync (BYTES per packet)                  │
│  ────────────────────────────────────────                   │
│  Who is where. What changed. Text messages.                 │
│  Works over packet radio, 2G, dial-up.                      │
│  📡 Examples:                                                │
│    "Player X at coords Y" = 50 bytes                        │
│    "Voxel Z changed to stone" = 32 bytes                    │
│    "Chat: hello" = 20 bytes                                 │
│  🔌 Required: 1-5 KB/s (works on ANY connection)            │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  PRIORITY 3: Voice (KILOBYTES/sec)                          │
│  ────────────────────────────────────────────                │
│  Opus codec: 6-12 KB/s per speaker                          │
│  Works over 3G, slow broadband                              │
│  🔇 Drops first when bandwidth shrinks                       │
│  🔌 Required: 10-50 KB/s (10 nearby speakers)               │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  PRIORITY 4: Geometry Sync (KILOBYTES-MEGABYTES)            │
│  ────────────────────────────────────────────                │
│  Chunk meshes, building data, terrain detail                │
│  Works over broadband, 4G+                                   │
│  🔲 Without this: wireframe or last-cached-state            │
│  🔌 Required: 50-500 KB/s (loading new chunks)              │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  PRIORITY 5: Textures & High-Fidelity Assets (MEGABYTES)    │
│  ────────────────────────────────────────────                │
│  PBR materials, normal maps, detail textures                │
│  Works over fast broadband, 5G                               │
│  🎨 Without this: flat colors or low-res fallbacks          │
│  🔌 Required: 500 KB/s - 5 MB/s                             │
└──────────────────────────────────────────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────────┐
│  PRIORITY 6: Rich Media Streaming (MEGABYTES/sec)           │
│  ────────────────────────────────────────────                │
│  Video on TVs, live audio, complex animations               │
│  Works over fiber, fast 5G                                   │
│  📺 Without this: TVs show static or "no signal"            │
│  🔌 Required: 5-50 MB/s                                      │
└──────────────────────────────────────────────────────────────┘
```

**Key Insight:** Every layer is optional except Priority 1. The client adapts in real-time.

---

## 🏗️ What the Player Sees at Each Bandwidth Level

### Full Connection (Fiber / 5G) — ALL 6 PRIORITIES
```
✅ Full-color, textured, lit world
✅ Other players fully rendered with animations
✅ TVs play video, doors animate, NPCs walk
✅ Voice chat, text chat
✅ Real-time voxel sync (dig → they see it instantly)
```

### Degraded Connection (Slow 4G / Bad WiFi) — PRIORITIES 1-4
```
⚠️ World geometry present but textures low-res or flat color
⚠️ Other players are capsules/silhouettes with nametags
⚠️ TVs show static image
✅ Voice chat works, maybe choppy
⚠️ Voxel sync delayed by 1-5 seconds
```

### Minimal Connection (2G / Packet Radio / Satellite) — PRIORITIES 1-2
```
✅ Your local area (home, owned parcels): full detail from cache
⚠️ Beyond your area: wireframe or last-cached snapshot (hours/days old)
⚠️ Other players: dots on map or stick figures at last-known position
✅ Text chat works (queued, delivered on next burst)
❌ No voice, no video, no geometry streaming
✅ Your edits queue locally, sync on next burst
```

### No Connection (Offline) — PRIORITY 1 ONLY
```
✅ Your local area: fully functional, fully interactable
⚠️ Beyond your area: whatever was cached last session (frozen in time)
❌ Other players: gone (or ghost outlines at last-known position)
✅ Your edits saved locally, signed, timestamped
✅ When connection restores: CRDT merge — converged state
```

**Critical Design Rule:** The client never shows an error screen. It just degrades fidelity.

---

## 📊 What We're Building (Updated)

### End Goal (4 weeks from now)

**Deliverable:** Two players can:
1. Connect P2P (no central server)
2. See each other move in real-time
3. Collaboratively dig and place voxels
4. Handle conflicts (both dig same voxel)
5. Maintain eventual consistency
6. Survive network interruptions

**Demo Video:** 5 minutes showing all of the above working

---

## 🏗️ Architecture Overview

### Current State (Single Player)
```
┌─────────────────────────────────────┐
│  phase1_week1.rs                    │
│  ├─ Player (local input only)       │
│  ├─ VoxelRegion (local mutations)   │
│  └─ Renderer (local state)          │
└─────────────────────────────────────┘
```

### Target State (P2P Multiplayer with Bandwidth Budget)
```
┌─────────────────────────────────────────────────────────────────────┐
│                       YOUR MACHINE                                  │
│                                                                     │
│  ┌────────────────────────────────────────────────────────┐        │
│  │             LOCAL WORLD STATE (Priority 1)             │        │
│  │                                                         │        │
│  │  Your parcels: full SVO + entities                     │        │
│  │  Cached chunks: last-known state                       │        │
│  │  Op log: your edits (signed, queued for sync)          │        │
│  │  Identity: your Ed25519 keypair                        │        │
│  │                                                         │        │
│  │  💾 Storage: ~/.metaverse/parcels/                     │        │
│  │              ~/.metaverse/cache/                       │        │
│  │              ~/.metaverse/identity.key                 │        │
│  │                                                         │        │
│  │  ✅ THIS ALWAYS WORKS. NO NETWORK NEEDED.              │        │
│  └────────────────────────────────────────────────────────┘        │
│                             │                                       │
│                             ▼                                       │
│  ┌────────────────────────────────────────────────────────┐        │
│  │           BANDWIDTH BUDGET ALLOCATOR                   │        │
│  │                                                         │        │
│  │  Measures available bandwidth every few seconds        │        │
│  │  Allocates budget to channels by priority:             │        │
│  │    1. State sync (always on if ANY connection)         │        │
│  │    2. Voice (drops first when bandwidth shrinks)       │        │
│  │    3. Geometry (wireframe fallback)                    │        │
│  │    4. Textures (flat color fallback)                   │        │
│  │    5. Rich media (off unless bandwidth abundant)       │        │
│  │                                                         │        │
│  │  Each channel queues packets, sends what it can        │        │
│  │  Adapts in real-time to changing conditions            │        │
│  └────────────────────────────────────────────────────────┘        │
│                             │                                       │
│                             ▼                                       │
│  ┌────────────────────────────────────────────────────────┐        │
│  │              NETWORK NODE (libp2p)                     │        │
│  │                                                         │        │
│  │  Swarm (TCP + Noise encryption)                        │        │
│  │  Kademlia DHT (peer discovery)                         │        │
│  │  Gossipsub (pubsub messaging)                          │        │
│  │                                                         │        │
│  │  Channels:                                             │        │
│  │    - "state-sync"   (Priority 2: always on)            │        │
│  │    - "voice"        (Priority 3: optional)             │        │
│  │    - "geometry"     (Priority 4: optional)             │        │
│  │    - "textures"     (Priority 5: optional)             │        │
│  │    - "media"        (Priority 6: optional)             │        │
│  └────────────────────────────────────────────────────────┘        │
│                             │                                       │
│                             ▼                                       │
│  ┌────────────────────────────────────────────────────────┐        │
│  │                  RENDERER                              │        │
│  │                                                         │        │
│  │  Fidelity level driven by data availability:           │        │
│  │    - Full: PBR materials, textures, entities           │        │
│  │    - GeometryOnly: Flat-shaded mesh                    │        │
│  │    - StateOnly: Wireframe outline                      │        │
│  │    - Nothing: Grey fog / procedural placeholder        │        │
│  │                                                         │        │
│  │  Local player: always full fidelity (local data)       │        │
│  │  Remote players: fidelity depends on bandwidth         │        │
│  │  Terrain: fidelity depends on cache + bandwidth        │        │
│  └────────────────────────────────────────────────────────┘        │
└─────────────────────────────────────────────────────────────────────┘
                              │
           ┌──────────────────┴──────────────────┐
           │      WHATEVER PIPE EXISTS           │
           │                                     │
           │  Fiber?         All 6 priorities    │
           │  4G?            Priorities 1-4      │
           │  2G?            Priorities 1-2      │
           │  Packet radio?  Priorities 1-2      │
           │  Nothing?       Priority 1 only     │
           │                                     │
           └──────────────────┬──────────────────┘
                              │
                              ▼
           ┌──────────────────────────────────────┐
           │          OTHER PEERS                 │
           │  (same architecture, same priority   │
           │   stack, same graceful degradation)  │
           └──────────────────────────────────────┘
```

---

## 📅 4-Week Implementation Timeline

### WEEK 1: Foundation & Player Movement

#### Days 1-3: libp2p Connection
**Goal:** Two processes connect P2P on localhost and exchange messages

**Tasks:**
1. Add dependencies to Cargo.toml:
   - `libp2p` (core, tcp, noise, mplex, kad, gossipsub)
   - `ed25519-dalek` (identity)
   - Update `serde`, `bincode` (serialization)

2. Create `src/identity.rs`:
   ```rust
   pub struct Identity {
       keypair: Keypair,  // Ed25519
       peer_id: PeerId,
   }
   
   impl Identity {
       pub fn load_or_create() -> Self {
           // Load from ~/.metaverse/identity.key or generate new
       }
       
       pub fn sign(&self, data: &[u8]) -> Signature {
           // Sign with private key
       }
       
       pub fn verify(peer_id: &PeerId, data: &[u8], sig: &Signature) -> bool {
           // Verify signature
       }
   }
   ```

3. Create `src/network.rs`:
   ```rust
   pub struct NetworkNode {
       swarm: Swarm<CombinedBehaviour>,
       identity: Identity,
       peer_id: PeerId,
   }
   
   pub struct CombinedBehaviour {
       kademlia: Kademlia,  // DHT for peer discovery
       gossipsub: Gossipsub, // PubSub for messaging
   }
   
   impl NetworkNode {
       pub fn new(identity: Identity) -> Self {
           // Setup TCP transport with Noise encryption
           // Add Kademlia for DHT
           // Add Gossipsub for pubsub
       }
       
       pub fn poll(&mut self) -> Option<NetworkEvent> {
           // Process swarm events
       }
       
       pub fn broadcast(&mut self, topic: &str, msg: Vec<u8>) {
           // Publish to Gossipsub topic
       }
   }
   ```

4. Create `examples/two_peers.rs`:
   - Spawn two NetworkNodes
   - Connect on localhost
   - Peer A sends "hello"
   - Peer B receives and responds "world"

**Success Metric:**
```bash
# Terminal A
cargo run --example two_peers -- --port 4001
> Listening on /ip4/127.0.0.1/tcp/4001
> Connected to peer 12D3KooWABC... 
> Sent: hello
> Received: world

# Terminal B  
cargo run --example two_peers -- --port 4002 --connect /ip4/127.0.0.1/tcp/4001
> Listening on /ip4/127.0.0.1/tcp/4002
> Connected to peer 12D3KooWXYZ...
> Received: hello
> Sent: world
```

---

#### Days 4-7: Player State Sync
**Goal:** Two players see each other move in real-time

**Tasks:**
1. Design message format:
   ```rust
   #[derive(Serialize, Deserialize, Clone)]
   pub struct PlayerStateMessage {
       pub position: [f64; 3],      // ECEF coordinates
       pub yaw: f32,                // Radians
       pub pitch: f32,              // Radians
       pub velocity: [f32; 3],      // Local space
       pub mode: MovementMode,      // Walk or Fly
       pub timestamp: u64,          // Lamport clock
   }
   ```

2. Create `src/entity.rs`:
   ```rust
   pub struct NetworkedPlayer {
       pub peer_id: PeerId,
       pub position: DVec3,  // ECEF
       pub yaw: f32,
       pub pitch: f32,
       pub velocity: Vec3,
       pub mode: MovementMode,
       pub last_update: Instant,
   }
   
   pub struct RemotePlayerManager {
       players: HashMap<PeerId, NetworkedPlayer>,
   }
   
   impl RemotePlayerManager {
       pub fn update(&mut self, peer_id: PeerId, state: PlayerStateMessage) {
           // Update or insert player
           // TODO: Add interpolation for smooth movement
       }
       
       pub fn render(&self, physics: &PhysicsWorld, ...) {
           // Render all remote players as wireframe capsules
       }
   }
   ```

3. Modify `src/physics.rs`:
   ```rust
   impl Player {
       pub fn get_network_state(&self, timestamp: u64) -> PlayerStateMessage {
           PlayerStateMessage {
               position: self.body_position.to_array(),
               yaw: self.yaw,
               pitch: self.pitch,
               velocity: self.velocity.to_array(),
               mode: self.mode,
               timestamp,
           }
       }
   }
   ```

4. Update rendering:
   - Different color for remote players (e.g., red capsules vs green for local)
   - Convert remote ECEF → local space using FloatingOrigin
   - Render name tags (PeerId shortened) above capsules

**Success Metric:**
- Run two instances of `phase1_week1` with networking
- Move WASD in window A → see movement in window B
- Smooth interpolation (no jitter)
- Latency < 100ms on localhost

---

### WEEK 2: Voxel Sync & CRDT Foundation

#### Days 8-11: Voxel Modification Sync
**Goal:** Dig/place in one client → both clients update terrain

**Tasks:**
1. Design voxel operation message:
   ```rust
   #[derive(Serialize, Deserialize, Clone)]
   pub struct VoxelOperation {
       pub coord: VoxelCoord,
       pub material: Material,
       pub author: PeerId,
       pub timestamp: u64,        // Lamport clock
       pub vector_clock: VectorClock,  // For CRDT (added later)
       pub signature: Vec<u8>,    // Ed25519 signature
   }
   
   impl VoxelOperation {
       pub fn new(coord: VoxelCoord, material: Material, identity: &Identity) -> Self {
           let mut op = Self {
               coord,
               material,
               author: identity.peer_id,
               timestamp: get_lamport_time(),
               vector_clock: VectorClock::new(),
               signature: vec![],
           };
           
           // Sign the operation
           let data = bincode::serialize(&(&op.coord, &op.material, op.timestamp)).unwrap();
           op.signature = identity.sign(&data).to_bytes().to_vec();
           op
       }
       
       pub fn verify(&self) -> bool {
           let data = bincode::serialize(&(&self.coord, &self.material, self.timestamp)).unwrap();
           Identity::verify(&self.author, &data, &self.signature)
       }
   }
   ```

2. Modify `src/voxel.rs`:
   ```rust
   pub struct VoxelRegion {
       // ... existing fields
       operation_log: Vec<VoxelOperation>,  // CRDT history
       applied_ops: HashSet<(PeerId, u64)>, // Deduplication
   }
   
   impl VoxelRegion {
       pub fn apply_operation(&mut self, op: VoxelOperation) -> bool {
           // Check if already applied (idempotent)
           if self.applied_ops.contains(&(op.author, op.timestamp)) {
               return false;
           }
           
           // Verify signature
           if !op.verify() {
               eprintln!("Invalid signature from {:?}", op.author);
               return false;
           }
           
           // Apply voxel change
           self.set_voxel(op.coord.x, op.coord.y, op.coord.z, op.material);
           
           // Add to log
           self.operation_log.push(op.clone());
           self.applied_ops.insert((op.author, op.timestamp));
           
           true
       }
   }
   ```

3. Integrate with physics:
   ```rust
   impl Player {
       pub fn dig_voxel(&mut self, ..., network: &mut NetworkNode) -> Option<VoxelOperation> {
           // ... existing raycast logic
           
           if let Some(hit) = raycast(...) {
               let coord = hit.voxel_coord;
               region.set_voxel(coord.x, coord.y, coord.z, Material::Air);
               
               // Create and broadcast operation
               let op = VoxelOperation::new(coord, Material::Air, &network.identity);
               network.broadcast("voxel-ops", bincode::serialize(&op).unwrap());
               
               return Some(op);
           }
           None
       }
   }
   ```

4. Handle incoming operations:
   ```rust
   // In main loop
   while let Some(event) = network.poll() {
       match event {
           NetworkEvent::Message { peer_id, topic, data } if topic == "voxel-ops" => {
               if let Ok(op) = bincode::deserialize::<VoxelOperation>(&data) {
                   if region.apply_operation(op) {
                       // Mark mesh as dirty for regeneration
                       mesh_dirty = true;
                   }
               }
           }
           // ... other events
       }
   }
   ```

**Success Metric:**
- Dig hole in client A → hole appears in client B within 100ms
- Place block in client B → block appears in client A
- Mesh regenerates correctly on both clients
- Collision mesh updates on both clients

---

#### Days 12-16: CRDT Conflict Resolution
**Goal:** Two players modify same voxel → deterministic convergence

**The Problem:**
```
Time: t=0
Both clients see: Voxel(0,0,0) = STONE

Time: t=1
Client A: Dig → Material::Air
Client B: Place → Material::Dirt

Network latency: Messages arrive out-of-order

Client A receives B's place AFTER already digging
Client B receives A's dig AFTER already placing

WITHOUT CRDT:
Client A final state: Dirt (B wins)
Client B final state: Air (A wins)
❌ DESYNC! Different states!

WITH CRDT:
Both clients merge using deterministic rules
✅ Both converge to SAME state (Air or Dirt, but consistent)
```

**Tasks:**
1. Create `src/sync.rs`:
   ```rust
   use std::collections::HashMap;
   
   #[derive(Serialize, Deserialize, Clone, Debug)]
   pub struct VectorClock {
       clocks: HashMap<PeerId, u64>,
   }
   
   impl VectorClock {
       pub fn new() -> Self {
           Self { clocks: HashMap::new() }
       }
       
       pub fn increment(&mut self, peer_id: PeerId) {
           *self.clocks.entry(peer_id).or_insert(0) += 1;
       }
       
       pub fn merge(&mut self, other: &VectorClock) {
           for (peer, &count) in &other.clocks {
               let current = self.clocks.entry(*peer).or_insert(0);
               *current = (*current).max(count);
           }
       }
       
       pub fn happens_after(&self, other: &VectorClock) -> bool {
           // True if self is strictly after other (all clocks >=, at least one >)
           let mut has_greater = false;
           
           for (peer, &other_count) in &other.clocks {
               let self_count = self.clocks.get(peer).copied().unwrap_or(0);
               if self_count < other_count {
                   return false;
               }
               if self_count > other_count {
                   has_greater = true;
               }
           }
           
           has_greater
       }
       
       pub fn is_concurrent(&self, other: &VectorClock) -> bool {
           !self.happens_after(other) && !other.happens_after(self)
       }
   }
   
   pub fn merge_voxel_ops(op_a: &VoxelOperation, op_b: &VoxelOperation) -> Material {
       // Causal ordering check
       if op_a.vector_clock.happens_after(&op_b.vector_clock) {
           return op_a.material;  // A is causally after B
       }
       if op_b.vector_clock.happens_after(&op_a.vector_clock) {
           return op_b.material;  // B is causally after A
       }
       
       // Concurrent operations - use tie-breaking rules
       if op_a.timestamp != op_b.timestamp {
           // Higher timestamp wins
           if op_a.timestamp > op_b.timestamp {
               return op_a.material;
           } else {
               return op_b.material;
           }
       }
       
       // Same timestamp (rare) - use PeerId as deterministic tie-breaker
       if op_a.author > op_b.author {
           op_a.material
       } else {
           op_b.material
       }
   }
   ```

2. Update VoxelOperation creation:
   ```rust
   impl VoxelOperation {
       pub fn new(coord: VoxelCoord, material: Material, identity: &Identity, 
                  vector_clock: &mut VectorClock) -> Self {
           // Increment our own clock
           vector_clock.increment(identity.peer_id);
           
           let mut op = Self {
               coord,
               material,
               author: identity.peer_id,
               timestamp: get_lamport_time(),
               vector_clock: vector_clock.clone(),
               signature: vec![],
           };
           
           // Sign it
           let data = bincode::serialize(&(&op.coord, &op.material, &op.vector_clock)).unwrap();
           op.signature = identity.sign(&data).to_bytes().to_vec();
           op
       }
   }
   ```

3. Update VoxelRegion to handle conflicts:
   ```rust
   impl VoxelRegion {
       pub fn apply_operation(&mut self, op: VoxelOperation) -> bool {
           // Check for existing conflicting op at same voxel
           if let Some(existing) = self.operation_log.iter()
               .find(|existing_op| existing_op.coord == op.coord) {
               
               // Merge to determine winner
               let winning_material = merge_voxel_ops(&op, existing);
               
               // Apply winning material
               self.set_voxel(op.coord.x, op.coord.y, op.coord.z, winning_material);
               
               // Keep both ops in log (for future sync)
               self.operation_log.push(op);
               
               return true;
           }
           
           // No conflict - just apply
           self.set_voxel(op.coord.x, op.coord.y, op.coord.z, op.material);
           self.operation_log.push(op);
           true
       }
   }
   ```

4. Write extensive tests:
   ```rust
   #[cfg(test)]
   mod tests {
       #[test]
       fn test_concurrent_ops_converge() {
           // Create two operations at same voxel with concurrent clocks
           let op_a = /* ... */;
           let op_b = /* ... */;
           
           // Both clients merge in different orders
           let result_ab = merge(merge(STONE, op_a), op_b);
           let result_ba = merge(merge(STONE, op_b), op_a);
           
           // MUST be same (commutative)
           assert_eq!(result_ab, result_ba);
       }
       
       #[test]
       fn test_1000_random_concurrent_ops() {
           // Stress test with random operations
           // All clients must converge to same state
       }
   }
   ```

**Success Metric:**
- Run two clients
- Both dig same voxel simultaneously
- Both see SAME final state (no desync)
- Can verify with MD5 hash of terrain state
- Passes 1000-op randomized stress test

---

### WEEK 3-4: Hardening & Optimization

#### Days 17-19: Security & Anti-Cheat Foundation
**Goal:** Prevent malicious peers from corrupting world state

**Tasks:**
1. Signature verification (already implemented, but add enforcement):
   ```rust
   impl NetworkNode {
       pub fn handle_voxel_op(&mut self, peer_id: PeerId, op: VoxelOperation) {
           // Verify signature
           if !op.verify() {
               self.report_malicious_peer(peer_id, "invalid_signature");
               return;
           }
           
           // Verify author matches sender
           if op.author != peer_id {
               self.report_malicious_peer(peer_id, "author_mismatch");
               return;
           }
           
           // Check rate limit (prevent spam)
           if self.is_rate_limited(peer_id) {
               self.report_malicious_peer(peer_id, "rate_limit_exceeded");
               return;
           }
           
           // Apply operation
           self.apply_voxel_op(op);
       }
   }
   ```

2. Peer reputation system:
   ```rust
   struct PeerReputation {
       peer_id: PeerId,
       invalid_signatures: u32,
       rate_limit_violations: u32,
       last_violation: Instant,
   }
   
   impl NetworkNode {
       fn report_malicious_peer(&mut self, peer_id: PeerId, reason: &str) {
           let rep = self.reputation.entry(peer_id).or_default();
           
           match reason {
               "invalid_signature" => rep.invalid_signatures += 1,
               "rate_limit_exceeded" => rep.rate_limit_violations += 1,
               _ => {}
           }
           
           rep.last_violation = Instant::now();
           
           // Ban if reputation too low
           if rep.invalid_signatures > 5 {
               self.ban_peer(peer_id);
           }
       }
   }
   ```

---

#### Days 20-21: NAT Traversal
**Goal:** Connect peers behind different routers/NATs

**Tasks:**
1. Add relay support:
   ```rust
   use libp2p_relay as relay;
   
   // In network setup
   let relay_behaviour = relay::client::Behaviour::new(local_peer_id);
   swarm.behaviour_mut().add_relay(relay_behaviour);
   ```

2. Setup relay nodes:
   - Use public relay servers (libp2p bootstrap nodes)
   - Or run own relay on VPS
   - Test hole-punching between two NAT'd clients

**Success Metric:**
- Two clients on different home networks connect
- No port forwarding required
- Connection stable for 1+ hour

---

#### Days 22-24: Bandwidth Optimization
**Goal:** Reduce network usage to < 100 KB/s per peer

**Current Bandwidth (worst case):**
- Player state: 100 bytes × 20 Hz = 2 KB/s per remote player
- 10 nearby players = 20 KB/s
- Voxel ops: 150 bytes × 10 ops/sec × 10 players = 15 KB/s
- **Total: ~35 KB/s** ✅ Already under budget!

**Optimizations:**
1. Delta compression for player state:
   ```rust
   pub struct PlayerStateDelta {
       position_delta: Option<[f32; 3]>,  // Only if moved > 0.1m
       yaw_delta: Option<f32>,            // Only if rotated > 1°
       // ...
   }
   ```

2. Only send when values change:
   ```rust
   if player.position.distance(last_broadcast_position) > 0.1 {
       broadcast_player_state();
       last_broadcast_position = player.position;
   }
   ```

3. Bincode compression already pretty efficient

**Success Metric:**
- Monitor bandwidth with 10 concurrent players
- Average < 50 KB/s per connection
- Spikes < 100 KB/s during intensive building

---

#### Days 25-28: Spatial Sharding Prep
**Goal:** Only sync with nearby players (scalability foundation)

**Tasks:**
1. Calculate visible range:
   ```rust
   const VISIBLE_RANGE_M: f64 = 1000.0;  // 1km radius
   
   fn is_nearby(pos_a: DVec3, pos_b: DVec3) -> bool {
       pos_a.distance(pos_b) < VISIBLE_RANGE_M
   }
   ```

2. Only send player state to nearby peers:
   ```rust
   fn broadcast_player_state(&mut self, my_position: DVec3) {
       for (peer_id, remote_player) in &self.remote_players {
           if is_nearby(my_position, remote_player.position) {
               self.network.send_to(peer_id, player_state);
           }
       }
   }
   ```

3. Implement "zone topics" (precursor to chunk topics):
   ```rust
   fn calculate_zone(position: DVec3) -> ZoneId {
       // Divide world into 10km × 10km zones
       let x = (position.x / 10000.0).floor() as i32;
       let y = (position.y / 10000.0).floor() as i32;
       let z = (position.z / 10000.0).floor() as i32;
       ZoneId { x, y, z }
   }
   
   fn update_zone_subscriptions(&mut self, old_zone: ZoneId, new_zone: ZoneId) {
       if old_zone != new_zone {
           self.unsubscribe_zone(old_zone);
           self.subscribe_zone(new_zone);
       }
   }
   ```

**Success Metric:**
- Player A in Brisbane, Player B in Sydney → no player state sync
- Both move to Brisbane → auto-discover and sync
- Bandwidth scales with nearby_players, not total_players

---

### WEEK 4: Integration & Polish

#### Final Example: `two_player_demo.rs`

Complete interactive demo showing everything working:

```rust
// Main loop structure
loop {
    // 1. Poll network
    while let Some(event) = network.poll() {
        match event {
            NetworkEvent::PlayerState(peer_id, state) => {
                remote_players.update(peer_id, state);
            }
            NetworkEvent::VoxelOp(op) => {
                if region.apply_operation(op) {
                    mesh_dirty = true;
                }
            }
            // ...
        }
    }
    
    // 2. Update local player
    player.update(dt, &region, input);
    
    // 3. Broadcast state (20Hz)
    if broadcast_timer.elapsed() > Duration::from_millis(50) {
        let state = player.get_network_state(lamport_clock);
        network.broadcast("player-state", &state);
        broadcast_timer.reset();
    }
    
    // 4. Handle input (dig/place)
    if input.just_pressed(KeyE) {
        if let Some(op) = player.dig_voxel(&mut region, &network) {
            network.broadcast("voxel-ops", &op);
            mesh_dirty = true;
        }
    }
    
    // 5. Regenerate mesh if needed
    if mesh_dirty {
        mesh = generate_mesh(&region);
        collision_mesh = update_collision(&mut physics, &region);
        mesh_dirty = false;
    }
    
    // 6. Render
    render_terrain(&mesh);
    remote_players.render(&physics, &camera);
    
    // 7. Sleep to maintain 60 FPS
    frame_limiter.sleep();
}
```

---

## 🎯 Success Criteria Checklist

### Technical Milestones
- [ ] Two peers connect via libp2p (localhost)
- [ ] Player movement syncs in real-time (<100ms latency)
- [ ] Remote players render as colored capsules
- [ ] Voxel dig/place syncs between clients
- [ ] Mesh regenerates on both clients
- [ ] Concurrent voxel ops resolve deterministically (CRDT)
- [ ] Signature verification prevents fake ops
- [ ] Connection survives network interruptions
- [ ] NAT traversal works (different networks)
- [ ] Bandwidth < 100 KB/s per connection (10 players)
- [ ] Spatial sharding limits sync to nearby players

### Demo Video (5 minutes)
1. **Setup** - Show two terminal windows side-by-side
2. **Connection** - Both start, auto-discover, show PeerIDs
3. **Movement** - Walk around in window A, see in window B
4. **Collaboration** - Build structure together (one digs, one places)
5. **Conflict** - Both dig same voxel → show convergence
6. **Recovery** - Disconnect one client, reconnect → re-sync works
7. **Scale** - Show bandwidth stats with 10 simulated peers

---

## 🔧 Technical Deep Dives

### The Bandwidth Budget Allocator (Core Engine)

This is the "central nervous system" of the networking layer. Every frame, it:
1. Measures available bandwidth (probe or estimate)
2. Allocates budget across channels by priority
3. Each channel sends what it can, queues the rest
4. Lower-priority channels shut off automatically when bandwidth drops
5. Channels reactivate when bandwidth returns, flushing queued data

```rust
/// Real-time bandwidth budget allocator
/// Treats network bandwidth as a scarce resource to distribute
pub struct BandwidthBudget {
    /// Measured available bandwidth in bytes/sec
    /// Updated every few seconds by probing or estimating from send rates
    available_bps: u64,

    /// Priority channels, ordered by importance
    /// Each channel requests bandwidth, gets allocated what's available
    /// after higher-priority channels take their share
    channels: Vec<SyncChannel>,
    
    /// Bandwidth measurement history (for smoothing)
    bandwidth_history: VecDeque<(Instant, u64)>,
}

pub struct SyncChannel {
    priority: u8,           // 1 = highest (state sync), 6 = lowest (rich media)
    name: String,           // "state_sync", "voice", "geometry", "textures", "media"
    min_bps: u64,           // Minimum to be useful (0 = can be fully off)
    desired_bps: u64,       // Ideal bandwidth if unlimited
    allocated_bps: u64,     // What it actually gets this cycle
    queue: VecDeque<Packet>, // Outbound packets waiting to send
    enabled: bool,          // False if below min_bps
}

impl BandwidthBudget {
    /// Allocate bandwidth to channels by priority
    /// Higher priority channels get their desired bandwidth first
    /// Lower priority channels get what's left (or nothing)
    pub fn allocate(&mut self) {
        let mut remaining = self.available_bps;

        // Sort by priority (highest first)
        self.channels.sort_by_key(|c| c.priority);

        for channel in &mut self.channels {
            if remaining >= channel.min_bps {
                // Give this channel what it needs, up to desired or remaining
                channel.allocated_bps = channel.desired_bps.min(remaining);
                channel.enabled = true;
                remaining -= channel.allocated_bps;
            } else {
                // Not enough bandwidth — this channel goes dark
                channel.allocated_bps = 0;
                channel.enabled = false;
                
                // Log degradation
                eprintln!(
                    "⚠️  Channel '{}' (priority {}) disabled - insufficient bandwidth",
                    channel.name, channel.priority
                );
            }
        }
    }
    
    /// Measure available bandwidth by probing
    /// Send small test packets, measure round-trip time and delivery rate
    pub fn measure_bandwidth(&mut self) {
        // TODO: Implement bandwidth measurement
        // For now: estimate from successful send rates
        let bytes_sent_last_second = self.get_bytes_sent_last_second();
        self.available_bps = bytes_sent_last_second;
        
        // Smooth with moving average
        self.bandwidth_history.push_back((Instant::now(), self.available_bps));
        
        // Keep only last 10 seconds
        while self.bandwidth_history.len() > 10 {
            self.bandwidth_history.pop_front();
        }
        
        // Calculate smoothed average
        let sum: u64 = self.bandwidth_history.iter().map(|(_, bps)| bps).sum();
        self.available_bps = sum / self.bandwidth_history.len() as u64;
    }
    
    /// Each frame: channels send what they're allocated
    pub fn tick(&mut self, network: &mut NetworkNode) {
        for channel in &mut self.channels {
            if !channel.enabled {
                continue;
            }
            
            let mut bytes_to_send = channel.allocated_bps / 60; // Per-frame budget (60 FPS)
            
            while bytes_to_send > 0 && !channel.queue.is_empty() {
                if let Some(packet) = channel.queue.front() {
                    if packet.size() <= bytes_to_send {
                        // Send this packet
                        let packet = channel.queue.pop_front().unwrap();
                        network.send(packet);
                        bytes_to_send -= packet.size() as u64;
                    } else {
                        // Packet too big for this frame — wait
                        break;
                    }
                }
            }
        }
    }
}

/// Example usage in main loop
fn main() {
    let mut budget = BandwidthBudget::new();
    
    // Setup channels with priorities
    budget.add_channel(SyncChannel {
        priority: 2,
        name: "state_sync".to_string(),
        min_bps: 1_000,        // 1 KB/s minimum
        desired_bps: 5_000,    // 5 KB/s ideal
        allocated_bps: 0,
        queue: VecDeque::new(),
        enabled: true,
    });
    
    budget.add_channel(SyncChannel {
        priority: 3,
        name: "voice".to_string(),
        min_bps: 8_000,        // 8 KB/s minimum (for one speaker)
        desired_bps: 50_000,   // 50 KB/s ideal (multiple speakers)
        allocated_bps: 0,
        queue: VecDeque::new(),
        enabled: false,
    });
    
    budget.add_channel(SyncChannel {
        priority: 4,
        name: "geometry".to_string(),
        min_bps: 0,            // Can be fully off
        desired_bps: 500_000,  // 500 KB/s ideal
        allocated_bps: 0,
        queue: VecDeque::new(),
        enabled: false,
    });
    
    // ... more channels
    
    loop {
        // Measure bandwidth every second
        if bandwidth_timer.elapsed() > Duration::from_secs(1) {
            budget.measure_bandwidth();
            budget.allocate();
            bandwidth_timer.reset();
        }
        
        // Queue outbound messages to appropriate channels
        if let Some(player_state) = get_player_state() {
            budget.queue_message("state_sync", player_state);
        }
        
        if let Some(voice_data) = get_voice_data() {
            budget.queue_message("voice", voice_data);
        }
        
        // Send what we can this frame
        budget.tick(&mut network);
        
        // ... rest of game loop
    }
}
```

**Why This Matters:**
- Works on **ANY connection** (degrades gracefully)
- No "connection failed" error screens
- Player doesn't know if they're on fiber or 2G — UI just shows lower fidelity
- Future-proof: add new channels (AR, VR, haptics) without changing allocator

---

### Data Availability & Render Fidelity

Every chunk/entity has a "data availability level" that drives rendering:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DataAvailability {
    /// Full local data (your parcels, cached chunks)
    /// Source: Local disk
    Full,
    
    /// Have geometry but no textures
    /// Source: Received geometry sync, but textures not downloaded
    GeometryOnly,
    
    /// Have last-known state sync but no geometry
    /// Source: State sync messages, but no geometry received
    StateOnly,
    
    /// Have nothing — never seen this area, or cache expired
    /// Source: N/A
    Nothing,
}

impl Chunk {
    /// Determine what data we have for this chunk
    pub fn get_data_availability(&self) -> DataAvailability {
        if self.is_local_parcel || self.is_cached_full() {
            DataAvailability::Full
        } else if self.has_geometry() && !self.has_textures() {
            DataAvailability::GeometryOnly
        } else if self.has_last_known_state() {
            DataAvailability::StateOnly
        } else {
            DataAvailability::Nothing
        }
    }
}

/// Render a chunk at appropriate fidelity level
pub fn render_chunk(
    chunk: &Chunk,
    availability: DataAvailability,
    render_pass: &mut RenderPass,
) {
    match availability {
        DataAvailability::Full => {
            // PBR materials, textures, entities, lights — the works
            render_full_fidelity(chunk, render_pass);
        }
        
        DataAvailability::GeometryOnly => {
            // Flat-shaded or vertex-colored mesh
            // No textures, no PBR
            // Entities as simple shapes (capsules, boxes)
            render_low_fidelity(chunk, render_pass);
        }
        
        DataAvailability::StateOnly => {
            // Wireframe outline of last-known geometry
            // Or simple bounding boxes for entities
            // Players as dots or stick figures at last-known position
            render_wireframe(chunk, render_pass);
        }
        
        DataAvailability::Nothing => {
            // Grey fog, or procedural placeholder (flat terrain at sea level)
            // Or just empty — the void beyond what you know
            render_fog(chunk, render_pass);
        }
    }
}

/// Similar system for entities (players, NPCs, vehicles)
pub fn render_entity(
    entity: &Entity,
    availability: DataAvailability,
    render_pass: &mut RenderPass,
) {
    match availability {
        DataAvailability::Full => {
            // Full skeletal animation, textures, clothing
            render_full_entity(entity, render_pass);
        }
        
        DataAvailability::GeometryOnly => {
            // Simple capsule or low-poly model
            // No animations, just position/rotation
            render_simple_capsule(entity, render_pass);
        }
        
        DataAvailability::StateOnly => {
            // Stick figure or dot at last-known position
            // Maybe a nametag
            render_stick_figure(entity, render_pass);
        }
        
        DataAvailability::Nothing => {
            // Don't render at all
        }
    }
}
```

**Key Insight:** The renderer doesn't care WHY it doesn't have data (network down, bandwidth limited, chunk not loaded). It just renders what it has.

---

### Local-First Storage Architecture

The disk cache is not optional. It's the foundation of the entire system.

```
~/.metaverse/
├── identity.key              # Your Ed25519 keypair (NEVER sync this)
│
├── parcels/                  # Your owned volumetric parcels
│   ├── 0x1a2b3c4d/           # Parcel ID (derived from your PeerId + coords)
│   │   ├── metadata.json     # Coords, size, creation date
│   │   ├── svo.bin           # Sparse Voxel Octree (your builds)
│   │   ├── entities.json     # Entities you've placed
│   │   └── op_log.bin        # All operations (for CRDT history)
│   └── 0x5e6f7g8h/
│       └── ...
│
├── cache/                    # Chunks you've visited (not owned)
│   ├── chunk-0-1000-2000/    # ChunkId
│   │   ├── geometry.bin      # Mesh data
│   │   ├── state.json        # Last-known voxel state
│   │   ├── textures/         # Texture files (if downloaded)
│   │   └── timestamp.txt     # When this was cached
│   └── chunk-0-1001-2000/
│       └── ...
│
├── op_queue/                 # Operations waiting to sync
│   ├── pending/              # Not yet sent
│   │   ├── op_00001.bin      # VoxelOperation (signed, timestamped)
│   │   ├── op_00002.bin
│   │   └── ...
│   └── sent/                 # Sent but not yet ACKed
│       └── ...
│
└── config.json               # User settings, bootstrap nodes, etc.
```

**Workflow:**
1. **You dig a voxel:**
   - Applied to local state immediately (instant feedback)
   - Signed VoxelOperation written to `op_queue/pending/`
   - Mesh regenerates from local state
   
2. **Network available:**
   - BandwidgetBudget allocates bandwidth to "state_sync" channel
   - Ops from `op_queue/pending/` sent via Gossipsub
   - Move to `op_queue/sent/` until ACKed
   
3. **Network unavailable:**
   - Ops stay in `op_queue/pending/`
   - You keep playing (edits accumulate)
   - When connection returns: ops sent in order
   
4. **Remote op received:**
   - CRDT merge with local state
   - If conflict: deterministic tie-break
   - Applied to local state, mesh regenerates
   - Saved to cache or parcel (if it's yours)

**This is not "multiplayer" — it's "asynchronous collaboration."** Like Git, but for voxels.

---

### Why ECEF for Network?

**Problem:** Floating-point precision varies across platforms
**Solution:** ECEF coordinates are deterministic

```rust
// ✅ GOOD - Deterministic serialization
#[derive(Serialize)]
struct NetworkPosition {
    ecef: [f64; 3],  // Exact values, no precision loss
}

// ❌ BAD - Non-deterministic
struct NetworkPosition {
    lat: f64,   // Computed from ECEF, might differ by platform
    lon: f64,   // Different compilers = different float behavior
}
```

Each client converts ECEF → local space for rendering (relative to their FloatingOrigin).

---

### Why Vector Clocks?

**Lamport timestamps alone are insufficient:**

```
Example: Network partition
Group A: Players 1, 2 (disconnected from Group B)
Group B: Players 3, 4 (disconnected from Group A)

Both groups modify Voxel(0,0,0)
Group A: timestamp=100, Material::Air
Group B: timestamp=100, Material::Dirt

When network reconnects:
Using ONLY timestamps → Cannot determine order!
Using vector clocks → Can detect concurrent modification
```

Vector clocks track causality across all peers.

---

### Bandwidth Budget Breakdown

**Per-connection budget: 100 KB/s**

**Player State Messages:**
- Size: ~100 bytes (ECEF pos + rotation + velocity + metadata)
- Frequency: 20 Hz (50ms updates)
- 10 nearby players: 10 × 100 × 20 = 20,000 bytes/sec = **20 KB/s**

**Voxel Operations:**
- Size: ~150 bytes (coord + material + signature + vector clock)
- Frequency: ~1 op/sec per player (average)
- 10 nearby players: 10 × 150 × 1 = 1,500 bytes/sec = **1.5 KB/s**

**Protocol Overhead:**
- libp2p framing: ~10% overhead
- Gossipsub metadata: ~5 KB/s
- Total overhead: **~7 KB/s**

**Total: 20 + 1.5 + 7 = 28.5 KB/s** ✅ Well under 100 KB/s budget!

**Headroom:** 71.5 KB/s for future features (chat, voice, entity sync)

---

### Determinism Audit Checklist

**Critical for P2P:** All clients MUST compute identical results

**✅ Already Deterministic:**
- ECEF coordinate math (Cartesian, no trig)
- Rapier physics (fixed timestep, no HashMap for logic)
- DDA voxel raycast (integer math)
- Marching cubes (deterministic triangulation)
- CRDT merge rules (pure function of inputs)

**⚠️ Needs Fixing:**
- [ ] HashMap iteration order (use BTreeMap or sort keys)
- [ ] Random number generation (use seeded RNG, sync seeds)
- [ ] Floating-point edge cases (ensure same compiler flags)
- [ ] System time (use Lamport clocks, not wall time)

**Testing Strategy:**
1. Record input sequence on Client A
2. Replay inputs on Client B
3. Compare terrain state byte-for-byte
4. Hash mismatch = determinism bug

---

## 📂 File Structure After Implementation

```
metaverse_core/
├── src/
│   ├── lib.rs                      (updated: export new modules)
│   ├── coordinates.rs              (unchanged)
│   ├── elevation.rs                (unchanged)
│   ├── marching_cubes.rs           (unchanged)
│   ├── materials.rs                (unchanged)
│   ├── mesh.rs                     (unchanged)
│   ├── terrain.rs                  (unchanged)
│   ├── physics.rs                  (modified: add get_network_state())
│   ├── voxel.rs                    (modified: add operation log)
│   │
│   ├── network.rs                  ⭐ NEW - libp2p networking
│   ├── identity.rs                 ⭐ NEW - Ed25519 identity
│   ├── sync.rs                     ⭐ NEW - CRDT & vector clocks
│   └── entity.rs                   ⭐ NEW - NetworkedPlayer
│
├── examples/
│   ├── phase1_week1.rs             (unchanged - single player demo)
│   ├── two_peers.rs                ⭐ NEW - Connection test
│   └── two_player_demo.rs          ⭐ NEW - Full P2P demo
│
├── Cargo.toml                      (modified: add libp2p deps)
│
└── docs/
    └── P2P_ARCHITECTURE.md         ⭐ NEW - Technical documentation
```

---

## 🚨 Risks & Mitigations

### Risk 1: libp2p Learning Curve
**Probability:** High  
**Impact:** Medium (delays implementation)

**Mitigation:**
- Start with minimal TCP example (skip libp2p initially)
- Prove concept with simple sockets
- Then migrate to libp2p once architecture validated

---

### Risk 2: CRDT Merge Logic Bugs
**Probability:** Medium  
**Impact:** Critical (causes desync)

**Mitigation:**
- Extensive unit tests (1000s of random scenarios)
- Property-based testing (quickcheck)
- Formal verification of merge function
- Conservative tie-breaking (prefer consistency over "fairness")

---

### Risk 3: NAT Traversal Failure
**Probability:** Medium  
**Impact:** Medium (some users can't connect)

**Mitigation:**
- Use libp2p-relay (hole-punching assistance)
- Fallback to relay servers for difficult NATs
- Document port forwarding for advanced users
- Future: Run community relay nodes

---

### Risk 4: Bandwidth Explodes
**Probability:** Low  
**Impact:** High (unusable on mobile/rural internet)

**Mitigation:**
- Spatial sharding (only nearby players)
- Delta compression (only send changes)
- Monitoring/profiling early and often
- Rate limiting per peer

---

### Risk 5: Determinism Breaks
**Probability:** Medium  
**Impact:** Critical (subtle desync bugs)

**Mitigation:**
- Determinism audit (checklist above)
- Regression tests (record/replay)
- CI testing on multiple platforms (Linux, macOS, Windows)
- Avoid floating-point in critical paths

---

## 📚 Resources & References

### libp2p Documentation
- https://docs.libp2p.io/
- https://docs.rs/libp2p/latest/libp2p/

### CRDT Theory
- "A comprehensive study of CRDTs" (Shapiro et al.)
- https://crdt.tech/

### Vector Clocks
- "Time, Clocks, and the Ordering of Events" (Lamport, 1978)

### Deterministic Simulation
- https://gafferongames.com/post/deterministic_lockstep/

---

## 🎬 Next Steps

**Ready to start? Here's the first task:**

1. **Add libp2p dependencies to Cargo.toml**
2. **Create src/identity.rs with Ed25519 keypair**
3. **Create src/network.rs with basic Swarm**
4. **Create examples/two_peers.rs to test connection**

**Estimated time for first working connection: 1-2 days**

Want me to start implementing? I'll begin with the Cargo.toml dependencies and identity module.
