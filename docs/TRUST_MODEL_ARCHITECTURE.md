# Trust Model Architecture: Math, Not Authority

## Core Principle: The Op Log IS the Authority

**No server decides what's valid. Every peer independently verifies using:**
1. **Math** (deterministic generation + hashes)
2. **Cryptography** (Ed25519 signatures)
3. **Rules** (permission checking against causal history)

---

## Three Types of Data, Three Trust Models

### Type 1: Deterministic Data (Terrain, Infrastructure)

**Trust Model:** MATH  
**Authority:** The algorithm itself  
**Verification:** Re-run algorithm, compare output

#### Example: Base Terrain
```rust
fn generate_base_terrain(chunk_id: ChunkId) -> Mesh {
    // Pure function - same input always produces same output
    let srtm_data = load_srtm_for_chunk(chunk_id);
    let mesh = generate_terrain_mesh(srtm_data);
    mesh
}

// Verification
let received_mesh = peer_sends_me_chunk();
let computed_mesh = generate_base_terrain(chunk_id);
let received_hash = sha256(serialize(received_mesh));
let computed_hash = sha256(serialize(computed_mesh));

if received_hash == computed_hash {
    // Peer is honest (or we both have same bug)
    use_mesh(received_mesh);
} else {
    // Peer is lying or has different source data
    reject();
}
```

#### Source Data Verification
- SRTM files have known hashes published by NASA
- OSM extracts have known hashes from Geofabrik
- If source data hash doesn't match published hash → reject

**Servers' Role:** Cache pre-computed chunks for speed  
**Trust Required:** None - verify hash before using

---

### Type 2: Authored Data (User Builds)

**Trust Model:** CRYPTOGRAPHIC SIGNATURE  
**Authority:** Author's private key  
**Verification:** Check signature with public key

#### Example: Block Placement
```rust
struct VoxelOperation {
    coord: VoxelCoord,
    material: Material,
    timestamp: u64,
    author: PeerId,          // Derived from public key
    signature: Signature,     // Ed25519 signature
}

// Create operation (local player)
fn place_block(coord: VoxelCoord, material: Material) -> VoxelOperation {
    let op = VoxelOperation {
        coord,
        material,
        timestamp: lamport_clock.tick(),
        author: my_peer_id,
        signature: [0; 64], // Will sign
    };
    
    let msg = serialize(&op);
    let sig = my_keypair.sign(&msg);
    op.signature = sig;
    
    broadcast(op);
    op
}

// Verify operation (remote peer)
fn verify_operation(op: &VoxelOperation) -> bool {
    let msg = serialize_without_signature(op);
    let public_key = peer_id_to_public_key(&op.author);
    
    public_key.verify(&msg, &op.signature).is_ok()
}
```

**Question Answered:** "Did the claimed author actually create this?"  
**NOT Answered Yet:** "Was the author ALLOWED to do this?"

---

### Type 3: Permissioned Data (Parcel Edits, Ownership)

**Trust Model:** RULES + SIGNATURES + CAUSAL HISTORY  
**Authority:** The chain of signed operations  
**Verification:** Replay op chain, verify every signature and permission

#### Example: Operation Log Replay

```rust
// Initial state: world generated, all parcels unclaimed
let mut world_state = WorldState::from_deterministic_generation();

// Op #1: Player A claims parcel
let op1 = Operation {
    seq: 1,
    action: Action::ClaimParcel {
        bounds: ParcelBounds::new((0,0,0), (100,100,50)),
    },
    author: player_a,
    signature: sig_a,
    vector_clock: VectorClock::new(),
};

// Every peer verifies independently:
if verify_signature(&op1) {
    // Check rule: "First valid claim wins"
    if !world_state.parcel_claimed(&op1.action.bounds) {
        world_state.claim_parcel(player_a, op1.action.bounds); // ✅ VALID
    }
}

// Op #2: Player B tries to claim SAME parcel (concurrent)
let op2 = Operation {
    seq: 2,
    action: Action::ClaimParcel {
        bounds: ParcelBounds::new((0,0,0), (100,100,50)), // Same bounds!
    },
    author: player_b,
    signature: sig_b,
    vector_clock: VectorClock::new(),
};

if verify_signature(&op2) {
    if !world_state.parcel_claimed(&op2.action.bounds) {
        world_state.claim_parcel(player_b, op2.action.bounds);
    } else {
        // ❌ REJECTED - parcel already claimed by A
        log::warn!("Player B attempted to claim already-owned parcel");
    }
}

// Op #3: Player A edits within their parcel
let op3 = Operation {
    seq: 3,
    action: Action::SetVoxel {
        coord: (50, 50, 25), // Within A's parcel
        material: Material::Wood,
    },
    author: player_a,
    signature: sig_a,
    vector_clock: op1.vector_clock.increment(player_a),
};

if verify_signature(&op3) {
    let owner = world_state.get_parcel_owner(&op3.action.coord);
    if owner == Some(player_a) || world_state.is_free_build(&op3.action.coord) {
        world_state.set_voxel(op3.action.coord, op3.action.material); // ✅ VALID
    }
}

// Op #4: Player C tries to edit A's parcel (unauthorized)
let op4 = Operation {
    seq: 4,
    action: Action::SetVoxel {
        coord: (50, 50, 25), // A's parcel!
        material: Material::Stone,
    },
    author: player_c,
    signature: sig_c,
    vector_clock: VectorClock::new(),
};

if verify_signature(&op4) {
    let owner = world_state.get_parcel_owner(&op4.action.coord);
    if owner == Some(player_c) || world_state.is_free_build(&op4.action.coord) {
        world_state.set_voxel(op4.action.coord, op4.action.material);
    } else {
        // ❌ REJECTED - C doesn't own this parcel
        log::warn!("Player C attempted to edit parcel owned by {:?}", owner);
    }
}

// Op #5: Player A grants access to C
let op5 = Operation {
    seq: 5,
    action: Action::GrantAccess {
        parcel: op1.action.bounds,
        grantee: player_c,
    },
    author: player_a,
    signature: sig_a,
    vector_clock: op3.vector_clock.increment(player_a),
};

if verify_signature(&op5) {
    if world_state.get_parcel_owner(&op5.action.parcel) == Some(player_a) {
        world_state.grant_access(op5.action.parcel, player_c); // ✅ VALID
    }
}

// Op #6: NOW C can edit
let op6 = Operation {
    seq: 6,
    action: Action::SetVoxel {
        coord: (50, 50, 25),
        material: Material::Stone,
    },
    author: player_c,
    signature: sig_c,
    vector_clock: VectorClock::new(),
};

if verify_signature(&op6) {
    let owner = world_state.get_parcel_owner(&op6.action.coord);
    let has_access = world_state.has_access(player_c, &op6.action.coord);
    if owner == Some(player_c) || has_access || world_state.is_free_build(&op6.action.coord) {
        world_state.set_voxel(op6.action.coord, op6.action.material); // ✅ NOW VALID
    }
}
```

**Key Insight:** Every peer replays the same log and arrives at the same world state. No server needed.

---

## Chunk Manifest Structure

```rust
struct ChunkManifest {
    // IDENTITY - What chunk is this?
    chunk_id: ChunkId,
    
    // BASE STATE - Deterministic generation
    terrain_hash: Hash,  // SHA256(generate_terrain(chunk_id))
    infra_hash: Hash,    // SHA256(generate_infrastructure(chunk_id))
    
    // MODIFICATION LOG - Signed operations
    ops: Vec<Operation>,  // Append-only, ordered by vector clock
    
    // CURRENT STATE HASH - Quick verification
    state_hash: Hash,     // SHA256(terrain + infra + replay(ops))
    
    // OWNERSHIP RECORDS - Derived from op log
    parcels: HashMap<ParcelBounds, Owner>,  // From CLAIM operations
}
```

### Verification Process (When Joining Chunk)

```rust
fn verify_chunk_manifest(manifest: &ChunkManifest) -> Result<(), VerificationError> {
    // 1. Verify terrain hash
    let computed_terrain = generate_base_terrain(manifest.chunk_id);
    let computed_terrain_hash = sha256(serialize(&computed_terrain));
    if computed_terrain_hash != manifest.terrain_hash {
        return Err(VerificationError::TerrainMismatch);
    }
    
    // 2. Verify infrastructure hash
    let computed_infra = generate_infrastructure(manifest.chunk_id);
    let computed_infra_hash = sha256(serialize(&computed_infra));
    if computed_infra_hash != manifest.infra_hash {
        return Err(VerificationError::InfraMismatch);
    }
    
    // 3. Replay operations and verify each one
    let mut state = WorldState::from_base(computed_terrain, computed_infra);
    for op in &manifest.ops {
        // Verify signature
        if !verify_signature(op) {
            return Err(VerificationError::InvalidSignature(op.seq));
        }
        
        // Verify permission
        if !check_permission(&state, op) {
            return Err(VerificationError::UnauthorizedOperation(op.seq));
        }
        
        // Apply operation
        apply_operation(&mut state, op);
    }
    
    // 4. Verify final state hash
    let computed_state_hash = sha256(serialize(&state));
    if computed_state_hash != manifest.state_hash {
        return Err(VerificationError::StateMismatch);
    }
    
    Ok(()) // All checks passed - manifest is trustworthy
}
```

**If ANY check fails:** This peer is lying/corrupt. Ask a different peer.  
**If ALL checks pass:** Trust the math and signatures, use the data.

---

## What Servers Actually Do (They're NOT Authorities)

| Server Type | Function | Is Authority? | What if it dies? |
|------------|----------|---------------|------------------|
| Bootstrap node | Help new peers find P2P network | No | Use different bootstrap or mDNS |
| Cache server | Store pre-baked terrain/OSM chunks | No | Peers generate or share directly |
| SRTM/OSM mirror | Host source data files | No | Use NASA/Geofabrik directly |
| Update server | Distribute client binary updates | No (but signed) | Manual download or peer-distributed |

**Every server is replaceable.** If a server gives bad data, hash won't match → reject.  
**Servers accelerate, they don't control.**

---

## Practical Implementation Roadmap

### Phase 1: Separate the Layers (CURRENT)

**Goal:** Make base terrain generation pure and separate from user edits.

#### 1.1 Pure Terrain Generation Function
```rust
// In src/terrain.rs
pub fn generate_base_terrain(chunk_id: ChunkId) -> Octree {
    // Pure function - deterministic, no user state
    let srtm_data = load_srtm_for_chunk(chunk_id);
    let mut octree = Octree::new();
    
    for voxel in compute_voxels_from_srtm(srtm_data) {
        octree.set(voxel.coord, voxel.material);
    }
    
    octree // No user edits - just base terrain
}
```

#### 1.2 Separate User Edits SVO
```rust
// In src/voxel.rs or new src/user_content.rs
pub struct UserContentLayer {
    /// User edits stored separately from base terrain
    edits: Octree,
    
    /// Operation log for this chunk
    op_log: Vec<Operation>,
    
    /// Ownership map
    parcels: HashMap<ParcelBounds, Owner>,
}

impl UserContentLayer {
    pub fn apply_operation(&mut self, op: Operation) -> Result<(), OpError> {
        // Verify signature
        if !verify_signature(&op) {
            return Err(OpError::InvalidSignature);
        }
        
        // Check permission (can toggle this off for testing)
        if VERIFY_PERMISSIONS {
            if !self.check_permission(&op) {
                return Err(OpError::Unauthorized);
            }
        }
        
        // Apply to edits octree
        match op.action {
            Action::SetVoxel { coord, material } => {
                self.edits.set(coord, material);
            }
            Action::ClaimParcel { bounds } => {
                self.claim_parcel(op.author, bounds)?;
            }
            // ... other actions
        }
        
        // Append to op log
        self.op_log.push(op);
        
        Ok(())
    }
    
    pub fn get_voxel(&self, coord: VoxelCoord, base: &Octree) -> Material {
        // User edits override base terrain
        if let Some(material) = self.edits.get(coord) {
            material
        } else {
            base.get(coord).unwrap_or(Material::Air)
        }
    }
}
```

#### 1.3 Signed Operations (Even if Verification Toggled Off)
```rust
// In multiplayer.rs
pub fn broadcast_voxel_operation(
    &mut self,
    coord: VoxelCoord,
    material: Material,
) -> Result<()> {
    let timestamp = self.clock.tick();
    
    // ALWAYS create signed operation (even if we don't verify yet)
    let op = VoxelOperation {
        coord,
        material,
        timestamp,
        author: self.local_peer_id,
        signature: [0; 64],
    };
    
    // Sign it
    let msg = op.to_bytes_without_signature()?;
    let sig = self.identity.sign(&msg);
    op.signature = sig;
    
    // Broadcast
    let data = op.to_bytes()?;
    self.cmd_tx.send(NetworkCommand::Publish {
        topic: TOPIC_VOXEL_OPS.to_string(),
        data,
    })?;
    
    // Store in local op log
    self.op_log.push(op.clone());
    
    Ok(())
}
```

#### 1.4 Store Op Log Per Chunk
```rust
// In src/chunk.rs or src/user_content.rs
pub struct ChunkState {
    chunk_id: ChunkId,
    
    /// Base terrain (deterministic, cached)
    base_terrain: Octree,
    
    /// User modifications (from op log)
    user_content: UserContentLayer,
}

impl ChunkState {
    pub fn load_or_generate(chunk_id: ChunkId) -> Self {
        // Try loading from disk
        if let Ok(state) = Self::load_from_disk(chunk_id) {
            return state;
        }
        
        // Generate fresh
        let base_terrain = generate_base_terrain(chunk_id);
        let user_content = UserContentLayer::new();
        
        Self {
            chunk_id,
            base_terrain,
            user_content,
        }
    }
    
    pub fn save_to_disk(&self) -> Result<()> {
        // Save op log (small, append-only)
        self.user_content.save_op_log(&self.chunk_id)?;
        
        // Optionally cache base terrain (large, but deterministic)
        // Can regenerate from SRTM if needed
        
        Ok(())
    }
}
```

---

### Phase 2: Add Toggleable Verification

```rust
// Config flag
const VERIFY_SIGNATURES: bool = true;   // Toggle for testing
const VERIFY_PERMISSIONS: bool = false;  // Toggle separately

fn apply_received_operation(op: Operation) -> Result<()> {
    if VERIFY_SIGNATURES {
        if !verify_signature(&op) {
            return Err(OpError::InvalidSignature);
        }
    }
    
    if VERIFY_PERMISSIONS {
        if !check_permission(&op) {
            return Err(OpError::Unauthorized);
        }
    }
    
    // Apply to world
    apply_operation(op);
    
    Ok(())
}
```

---

### Phase 3: Implement Permission Checking

```rust
fn check_permission(world_state: &WorldState, op: &Operation) -> bool {
    match &op.action {
        Action::SetVoxel { coord, .. } => {
            // Check if author owns parcel
            if let Some(owner) = world_state.get_parcel_owner(coord) {
                return owner == op.author || world_state.has_access(op.author, coord);
            }
            
            // Check if free-build zone
            world_state.is_free_build(coord)
        }
        
        Action::ClaimParcel { bounds } => {
            // Check if parcel already claimed
            !world_state.parcel_claimed(bounds)
        }
        
        Action::GrantAccess { parcel, .. } => {
            // Only owner can grant access
            world_state.get_parcel_owner_by_bounds(parcel) == Some(op.author)
        }
    }
}
```

---

## Summary: You're Already 80% There

**What you have:**
- ✅ Base terrain generation (deterministic)
- ✅ Ed25519 signatures on operations
- ✅ Lamport clock for ordering
- ✅ Gossipsub broadcast
- ✅ Operation structure (VoxelOperation)

**What you need:**
1. Separate base terrain from user edits (pure functions)
2. Store op log per chunk
3. Implement permission checking (toggleable)
4. Implement state replay from op log

**Philosophy:**
- Files are never edited, operations are appended
- The op log IS the authority
- Every peer verifies independently
- Math + signatures + rules = trustless system
- Servers accelerate, don't control

**Next immediate step:** Refactor to separate base terrain generation into a pure function, and make user edits overlay on top in a separate SVO.

---

**Status:** Architecture validated, implementation path clear  
**Author:** User architectural insight on 2026-02-18  
**Next:** Implement layer separation and op log storage
