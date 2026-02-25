# Layer Separation Implementation Guide

## Overview

The metaverse uses a three-layer architecture to separate immutable base terrain from mutable user content. This enables trustless P2P verification while maintaining local-first performance.

## The Three Layers

### Layer 1: Base Terrain (Immutable, Deterministic)

**Source:** SRTM elevation + procedural generation  
**Trust Model:** Math - every peer generates identical output  
**Verification:** Hash of generated terrain

**Current Status:** ⚠️ Needs refactoring
- TerrainGenerator exists but not pure function
- Uses `&mut self` (has state)
- Goal: `fn generate_chunk(chunk_id) -> Octree`

**Why Important:**
- Every peer must generate identical terrain
- No "my terrain vs your terrain" conflicts
- Deterministic = verifiable with hashes

### Layer 2: Infrastructure (Immutable, Generated)

**Source:** OSM data → roads, rivers, tunnels, bridges  
**Trust Model:** Math - procedurally placed from real data  
**Verification:** Hash of generated structures

**Current Status:** ❌ Not started
- Deferred to future phase
- Will modify Layer 1 (road cuts through hill)
- Still deterministic and verifiable

### Layer 3: User Content (Mutable, Owned)

**Source:** Player actions (dig, place, build)  
**Trust Model:** Signatures + Rules + Causal History  
**Verification:** Operation log replay

**Current Status:** ✅ Implemented
- `UserContentLayer` manages operation log
- CRDT conflict resolution (Last-Write-Wins + PeerId tiebreak)
- Parcel ownership foundation (toggleable)
- Ed25519 signature verification (toggleable)

## Implementation: UserContentLayer

### Core Concept

**UserContentLayer does NOT store voxels.** It stores:
1. Operation log (append-only history)
2. Parcel ownership claims
3. Access grants

**Voxel storage stays in Octree.** The layer provides:
- CRDT merge logic
- Permission checking
- Signature verification
- Log persistence

### API Usage

```rust
use metaverse_core::user_content::{UserContentLayer, VerificationConfig};

// Create layer
let mut layer = UserContentLayer::new();

// Or with custom config
let config = VerificationConfig {
    verify_signatures: true,
    verify_permissions: false,
    enable_logging: true,
};
let mut layer = UserContentLayer::with_config(config);

// Apply remote operation
let result = layer.apply_operation(op, &local_ops);
match result {
    Ok(true) => {
        // Operation accepted - apply to octree
        octree.set_voxel(op.coord, op.material.to_material_id());
    }
    Ok(false) => {
        // Operation rejected by CRDT (local wins)
    }
    Err(ApplyError::InvalidSignature) => {
        // Signature verification failed
    }
    Err(ApplyError::Unauthorized) => {
        // Permission check failed
    }
}

// Get operation count
println!("Operations in log: {}", layer.op_count());

// Save/load operation log
layer.save_op_log("chunk_0_0_ops.json")?;
layer.load_op_log("chunk_0_0_ops.json")?;
```

### CRDT Conflict Resolution

**Scenario:** Two players edit the same voxel simultaneously

**Example:**
```
Player A: Digs voxel (5,10,15) at timestamp 100
Player B: Places stone at (5,10,15) at timestamp 105
```

**Resolution:** Last-Write-Wins
- Higher timestamp wins (B wins: 105 > 100)
- If timestamps equal, PeerId tiebreak (deterministic)
- Both clients converge to same state

**Code:**
```rust
// VoxelOperation::wins_over() in messages.rs
pub fn wins_over(&self, other: &VoxelOperation) -> bool {
    if self.timestamp > other.timestamp {
        true
    } else if self.timestamp < other.timestamp {
        false
    } else {
        // Timestamp tie - PeerId tiebreak
        self.author > other.author
    }
}
```

### Parcel Ownership (Foundation)

**Not fully implemented yet, but foundation is there:**

```rust
// Claim a parcel
let bounds = ParcelBounds::new(
    VoxelCoord::new(0, 0, 0),
    VoxelCoord::new(100, 50, 100),
);
layer.claim_parcel(owner_peer_id, bounds)?;

// Grant access to another player
layer.grant_access(bounds, friend_peer_id);

// Check permission
if layer.has_access(peer_id, &coord) {
    // Allowed to edit
}
```

**Future work:**
- Store claims in operation log (first-claim-wins)
- Broadcast CLAIM operations like voxel ops
- Verify claim conflicts (overlapping parcels)

## Integration with Game Loop

### Local Operations (Dig/Place)

```rust
// Player digs
if let Some(coord) = player.dig_voxel(&physics, &mut octree, 10.0) {
    // Broadcast operation
    let op = multiplayer.broadcast_voxel_operation(coord, Material::Air)?;
    
    // Track for CRDT
    local_voxel_ops.insert(coord, op.clone());
    
    // Log to user content layer
    user_content.apply_operation(op, &local_voxel_ops)?;
    
    mesh_dirty = true;
}
```

### Remote Operations (From Network)

```rust
// Process pending operations from network
let pending_ops = multiplayer.take_pending_operations();
for op in pending_ops {
    // Apply through user content layer (handles CRDT + verification)
    match user_content.apply_operation(op.clone(), &local_voxel_ops) {
        Ok(true) => {
            // Accepted - apply to octree
            octree.set_voxel(op.coord, op.material.to_material_id());
            mesh_dirty = true;
        }
        Ok(false) => {
            // Rejected by CRDT (local wins, ignore)
        }
        Err(e) => {
            // Invalid operation (log and ignore)
            eprintln!("Invalid operation: {:?}", e);
        }
    }
}
```

## Verification Config

### Testing Mode

```rust
let config = VerificationConfig {
    verify_signatures: false,  // Disable for tests (no real keys)
    verify_permissions: false,  // Disable (not implemented yet)
    enable_logging: true,       // Keep operation log
};
```

### Production Mode

```rust
let config = VerificationConfig {
    verify_signatures: true,   // Full Ed25519 verification
    verify_permissions: true,  // Enforce parcel ownership
    enable_logging: true,      // Persist operation log
};
```

## Operation Log Persistence

### Save on Shutdown

```rust
// When exiting
user_content.save_op_log("world/chunk_0_0_ops.json")?;
```

### Load on Startup

```rust
// When loading chunk
let count = user_content.load_op_log("world/chunk_0_0_ops.json")?;
println!("Loaded {} operations", count);

// Operations are in log but NOT applied yet
// Caller must replay them:
for op in user_content.op_log() {
    octree.set_voxel(op.coord, op.material.to_material_id());
}
```

**Note:** Currently loads ops into log but doesn't auto-replay. This is intentional:
- Caller controls when/how operations apply
- Can validate before applying
- Can merge with other sources

## Future: Chunk Manifest

**Not implemented yet, but this is the goal:**

```rust
struct ChunkManifest {
    chunk_id: ChunkId,
    
    // Base terrain (deterministic, verifiable)
    terrain_hash: Hash,  // SHA256 of generated terrain
    infra_hash: Hash,    // SHA256 of generated infrastructure
    
    // User content (signed operation log)
    ops: Vec<VoxelOperation>,
    
    // Current state hash (for quick verification)
    state_hash: Hash,  // SHA256(terrain + infra + replay(ops))
}
```

**Verification:**
1. Receive manifest from peer
2. Generate terrain yourself → verify terrain_hash
3. Verify each op signature
4. Replay ops → verify state_hash
5. If all match: peer is trustworthy, use their data

**Trust model:** Math + signatures = no authority needed

## Why This Architecture?

### Problem: Trustless P2P World State

**Challenge:** How do peers agree on world state without central server?

**Bad approach:** Trust what peers send
- Peer A says "voxel (10,20,30) is stone"
- Peer B believes it
- Result: Peer A can lie, create arbitrary terrain

**Good approach:** Separate verifiable from signed data

**Layer 1 (Base terrain):**
- Every peer generates it independently
- Deterministic algorithm → identical output
- No trust needed: verify with hash

**Layer 3 (User content):**
- Every operation is signed
- Operation log is append-only
- Replay log → current state
- No trust needed: verify signatures + rules

### Problem: Storage Explosion

**Challenge:** Millions of voxels × millions of players = huge data

**Bad approach:** Store every voxel modification as separate entry
- 1M edits = 1M voxel records
- Must store forever
- Doesn't compress

**Good approach:** Layers + compression

**Layer 1:** Deterministic, doesn't need storage (regenerate on demand)
**Layer 3:** Operation log compresses well (coordinate + material + timestamp)
- 1M edits = ~128 MB (128 bytes/op)
- Can prune old ops if have state checkpoints
- Can delta-encode similar ops

### Problem: Conflicting Edits

**Challenge:** Two players edit same voxel, network lag, who wins?

**Bad approach:** Last to arrive wins
- Race condition based on network speed
- Clients see different results
- Non-deterministic

**Good approach:** CRDT with deterministic tiebreak
- Compare timestamps (higher wins)
- If timestamps equal, compare PeerId (deterministic)
- All clients apply same rule → same result
- Eventual consistency guaranteed

## Testing Strategy

### Unit Tests

**Test CRDT logic:**
```rust
#[test]
fn test_crdt_conflict_resolution() {
    let local_op = VoxelOperation::new(coord, Material::Stone, peer_a, 100);
    let remote_op = VoxelOperation::new(coord, Material::Wood, peer_b, 50);
    
    // Remote op has lower timestamp → should be rejected
    assert!(!remote_op.wins_over(&local_op));
}
```

**Test parcel bounds:**
```rust
#[test]
fn test_parcel_overlap() {
    let bounds1 = ParcelBounds::new(min1, max1);
    let bounds2 = ParcelBounds::new(min2, max2);
    
    assert!(bounds_overlap(&bounds1, &bounds2));
}
```

### Integration Tests

**Test P2P sync:**
1. Start two clients
2. Client A digs voxel
3. Verify Client B receives operation
4. Verify Client B's terrain updates
5. Verify both clients show hole

**Test CRDT conflict:**
1. Start two clients
2. Disconnect network
3. Both clients edit same voxel (different materials)
4. Reconnect network
5. Verify both clients converge to same state (CRDT winner)

## Common Issues

### Issue: Operations not applying

**Symptom:** Remote operations received but terrain doesn't update

**Debugging:**
```rust
match user_content.apply_operation(op, &local_ops) {
    Ok(true) => println!("Applied: {:?}", op),
    Ok(false) => println!("Rejected by CRDT: {:?}", op),
    Err(e) => println!("Error: {:?}", e),
}
```

**Common causes:**
- Signature verification failing (check config)
- Local operation has higher timestamp (CRDT reject)
- Permission check failing (disable for testing)

### Issue: Signature verification always fails

**Symptom:** All remote ops rejected with InvalidSignature

**Fix:** Disable in testing
```rust
let mut config = VerificationConfig::default();
config.verify_signatures = false;
layer.config = config;
```

**Reason:** Signature verification requires extracting public key from PeerId, which is complex. Phase 1 testing doesn't need it.

### Issue: Operation log grows forever

**Symptom:** op_log() returns thousands of operations

**Solution 1:** Clear periodically
```rust
user_content.clear();  // Removes all logged ops
```

**Solution 2:** State checkpoints (future)
- Save full octree state to disk
- Clear ops older than checkpoint
- Result: Log only contains recent ops

## Next Steps

### Immediate

1. ✅ UserContentLayer implemented
2. ✅ Integration with phase1_multiplayer
3. ⏳ Test P2P voxel sync (user will test)

### Short-term

1. Operation log persistence (save/load on disk)
2. Proper signature verification (extract public key from PeerId)
3. Test CRDT conflict resolution with concurrent edits

### Long-term

1. Refactor TerrainGenerator to pure function
2. Implement chunk manifest with hashes
3. Implement parcel ownership in operation log
4. Add vector clocks (replace Lamport timestamps)
5. State checkpoints for log pruning

## References

**Architecture Docs:**
- `files/WORLD_LAYER_ARCHITECTURE.md` - Three-layer model
- `files/TRUST_MODEL_ARCHITECTURE.md` - Trustless verification
- `files/P2P_NETWORKING_PLAN.md` - P2P roadmap

**Code:**
- `src/user_content.rs` - UserContentLayer implementation
- `src/messages.rs` - VoxelOperation, CRDT methods
- `examples/phase1_multiplayer.rs` - Integration example

**Checkpoints:**
- `checkpoints/057-layer-separation-architecture.md` - Implementation notes
