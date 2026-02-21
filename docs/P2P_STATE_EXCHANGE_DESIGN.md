# P2P State Exchange Design

## Problem Statement

When a player joins the network AFTER other players have modified the world, they don't receive historical operations. This breaks the shared world experience.

**Scenario:**
1. Alice digs holes, logs out
2. Bob joins network
3. Bob loads only his own `world_data_bob/operations.json`
4. Bob never receives Alice's historical edits
5. Bob sees incomplete world state

## Production Requirements

### Functional Requirements
1. **Complete State**: New players must receive all relevant historical operations
2. **Chunk-Based**: Only request operations for chunks actually loaded
3. **Incremental**: Sync new chunks as player explores
4. **Deduplication**: Don't apply same operation twice
5. **CRDT Semantics**: Proper conflict resolution via vector clocks

### Non-Functional Requirements
1. **Scalable**: Works with millions of operations per chunk
2. **Bandwidth Efficient**: Only transfer needed data
3. **Resilient**: Handle peers going offline mid-sync
4. **Deterministic**: Same result across all peers
5. **Secure**: Verify signatures on received operations

### Planet-Scale Considerations
1. **Spatial Sharding**: Operations grouped by chunk
2. **Incremental Loading**: Request chunks as needed, not all at once
3. **Priority Queue**: Spawn chunk first, then expand by distance
4. **DHT Ready**: Design compatible with future DHT replication
5. **Bandwidth Budget**: Respect network limits (Priority 2: State Sync)

## Architecture

### Message Protocol

```rust
/// Request historical operations for specific chunks
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChunkStateRequest {
    /// Chunks to request operations for
    pub chunk_ids: Vec<ChunkId>,
    
    /// Requester's vector clock (for filtering already-known ops)
    pub requester_clock: VectorClock,
}

/// Response with operations for requested chunks
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChunkStateResponse {
    /// Operations grouped by chunk
    pub operations: HashMap<ChunkId, Vec<VoxelOperation>>,
    
    /// Responder's vector clock (for causality tracking)
    pub responder_clock: VectorClock,
}
```

### Network Topics

```rust
const TOPIC_STATE_REQUEST: &str = "metaverse/state/request";
const TOPIC_STATE_RESPONSE: &str = "metaverse/state/response";
```

### Flow Diagram

```
BOB JOINS NETWORK:

1. Bob loads chunks
   load_chunks_immediate(&spawn_chunk, 2, &world_dir)
   → 19 chunks loaded

2. Bob discovers Alice (mDNS)
   NetworkEvent::PeerConnected { peer_id: alice }

3. Bob requests state
   Send to alice: ChunkStateRequest {
     chunk_ids: [chunk_45099_44843_115846, ...],
     requester_clock: bob's_clock
   }

4. Alice receives request
   Filters her op_log for requested chunks
   Excludes ops Bob already has (via vector clock comparison)

5. Alice responds
   Send to bob: ChunkStateResponse {
     operations: {
       chunk_45099_44843_115846: [op1, op2, ...],
       chunk_45100_44843_115846: [op3, op4, ...]
     },
     responder_clock: alice's_clock
   }

6. Bob receives operations
   For each operation:
     - Verify signature
     - Check if already applied (deduplication)
     - Apply to appropriate chunk octree
     - Add to local op_log
     - Mark chunk dirty for mesh regeneration
   
7. Bob merges vector clocks
   bob's_clock.merge(&alice's_clock)
   
8. Bob's world state now complete
```

## Implementation Plan

### Phase 1: Message Types
**File:** `src/messages.rs`

Add `ChunkStateRequest` and `ChunkStateResponse` structs with proper serialization.

### Phase 2: Multiplayer API
**File:** `src/multiplayer.rs`

```rust
impl MultiplayerSystem {
    /// Request chunk state from all connected peers
    pub fn request_chunk_state(&mut self, chunk_ids: Vec<ChunkId>) -> Result<()>;
    
    /// Handle incoming state request (called by update loop)
    fn handle_state_request(&mut self, peer_id: PeerId, request: ChunkStateRequest);
    
    /// Handle incoming state response (called by update loop)
    fn handle_state_response(&mut self, peer_id: PeerId, response: ChunkStateResponse);
    
    /// Take pending received operations (for game loop to apply)
    pub fn take_pending_state_operations(&mut self) -> Vec<VoxelOperation>;
}
```

### Phase 3: ChunkManager Integration
**File:** `src/chunk_manager.rs`

```rust
impl ChunkManager {
    /// Get list of loaded chunk IDs
    pub fn get_loaded_chunk_ids(&self) -> Vec<ChunkId>;
    
    /// Merge received operations (deduplication + application)
    pub fn merge_received_operations(&mut self, ops: Vec<VoxelOperation>);
}
```

### Phase 4: Game Loop Integration
**File:** `examples/phase1_multiplayer.rs`

```rust
// After loading chunks
let loaded_chunk_ids = chunk_manager.get_loaded_chunk_ids();
multiplayer.request_chunk_state(loaded_chunk_ids)?;

// In update loop
let state_ops = multiplayer.take_pending_state_operations();
if !state_ops.is_empty() {
    chunk_manager.merge_received_operations(state_ops);
}
```

## Deduplication Strategy

Use operation signature as unique key:

```rust
// In ChunkManager or UserContentLayer
struct OperationTracker {
    seen_signatures: HashSet<[u8; 64]>,
}

impl ChunkManager {
    fn is_duplicate(&self, op: &VoxelOperation) -> bool {
        self.seen_signatures.contains(&op.signature)
    }
    
    fn mark_seen(&mut self, op: &VoxelOperation) {
        self.seen_signatures.insert(op.signature);
    }
}
```

## Vector Clock Filtering

Optimize bandwidth by not sending operations peer already has:

```rust
fn filter_operations(
    all_ops: &[VoxelOperation],
    requester_clock: &VectorClock,
) -> Vec<VoxelOperation> {
    all_ops.iter()
        .filter(|op| !requester_clock.happened_before(&op.vector_clock))
        .cloned()
        .collect()
}
```

If requester's clock shows they already saw an operation, don't send it.

## Bandwidth Analysis

### Worst Case (Cold Join)
- Player loads 19 chunks (radius 2)
- Each chunk has 100 operations (heavy editing)
- Each operation: ~160 bytes

**Total:** 19 chunks × 100 ops × 160 bytes = ~304 KB

This fits well within Priority 2 bandwidth budget (1-5 KB/s sustained).

### Typical Case
- Most chunks have 0-10 operations
- Only spawn area heavily edited
- Average: ~50 KB on join

### Incremental Sync
As player explores:
- Request new chunks as loaded
- Amortized over time
- No bandwidth spikes

## Error Handling

### Peer Goes Offline
- Request timeout after 5 seconds
- Retry with other peers
- Continue with partial state (better than nothing)

### Invalid Operations
- Verify signature before applying
- Skip invalid operations
- Log warning for debugging

### Chunk Not Loaded
- Queue operations for when chunk loads
- Or discard (will request again if chunk loads later)

## Testing Strategy

### Unit Tests
1. Message serialization/deserialization
2. Operation filtering by vector clock
3. Deduplication logic
4. Chunk ID extraction from operations

### Integration Tests
1. Two peers: Alice edits, logs out, Bob joins
2. Three peers: Alice+Bob edit, Charlie joins
3. Incremental: Bob joins, explores, gets more chunks
4. Conflict resolution: Alice and Bob edit same voxel offline

### Production Testing
- Run 3+ instances on same machine
- Test various join/leave scenarios
- Verify bandwidth usage
- Check for duplicate operations

## Future Enhancements

### Priority Queue
Prioritize chunks by distance from player:
1. Spawn chunk (immediate)
2. Adjacent chunks (high priority)
3. Visible chunks (medium priority)
4. Loaded but distant chunks (low priority)

### DHT Integration
When DHT implemented:
- Request from DHT nodes responsible for chunk
- Multiple sources for redundancy
- Gossip protocol for propagation

### Compression
- Delta encoding (only changes from base terrain)
- zstd compression for network transmission
- Batch operations to same chunk

### Operation Log Compaction
- Remove redundant operations (set same voxel multiple times)
- Keep only final state
- Reduce storage and bandwidth

## Success Criteria

1. **Completeness**: Bob sees all of Alice's edits after joining
2. **Correctness**: No duplicate applications, proper CRDT merge
3. **Performance**: <1 second sync time for typical join
4. **Scalability**: Works with 1000+ operations per chunk
5. **Robustness**: Handles peer disconnections gracefully

## Non-Goals (For Now)

- Real-time partial state updates (request all loaded chunks at once)
- DHT-based storage (direct peer-to-peer only)
- Operation log compaction (send all operations as-is)
- Encryption (signatures provide authenticity, not confidentiality)

These can be added in future iterations as needed.
