# Scalable P2P Architecture - Spatial Sharding

## The Problem You Identified

**Current (doesn't scale):**
```
world_data/operations.json  ← ONE FILE for entire world
  ↓
All 100,000 players load it
  ↓
Gigabytes of data
  ↓
BROKEN
```

## The Solution: Chunk-Based Operation Logs

**Future (scales infinitely):**
```
world_data/
  chunks/
    chunk_0_0/
      operations.json     ← Only edits in this 1km² area
    chunk_0_1/
      operations.json     ← Different area
    chunk_1_0/
      operations.json
    ... (millions of chunks)
```

### Why This Scales

**You only load what you need:**
- Player in Brisbane loads Brisbane chunks (5-10 chunks)
- Player in Sydney loads Sydney chunks (5-10 chunks)
- They never load each other's data
- **Bandwidth: Constant regardless of world size**

**Storage distribution:**
- Each chunk has ~1-100 operations (tiny)
- Players cache chunks they visit
- Share chunks with nearby players (P2P)
- No single bottleneck

## How Players Get Data

### Scenario: New Player Joins

```
Player spawns in Brisbane
  ↓
1. Query: "Who has chunk_brisbane_cbd?"
   - Ask known peers (DHT or gossipsub topic)
   - Multiple peers respond: "I have it!"
  ↓
2. Download chunk data from ANY peer:
   - Base terrain (deterministic, verifiable by hash)
   - operations.json (signed, verifiable by signatures)
  ↓
3. Verify chunk:
   - Regenerate base terrain → check hash
   - Replay operations → check signatures
   - If mismatch: try different peer
  ↓
4. Player now has chunk, can share it with others
```

**No central server needed!**

### Scenario: 100,000 Players Worldwide

```
Players distributed:
  - 10,000 in Brisbane area (50 chunks)
  - 10,000 in Sydney area (50 chunks)
  - 10,000 in London area (50 chunks)
  - ... etc

Brisbane player:
  - Loads only Brisbane chunks (50 × 10KB = 500KB)
  - Never loads Sydney or London data
  - Bandwidth: 500KB regardless of total players

London player:
  - Loads only London chunks (50 × 10KB = 500KB)
  - Never loads Brisbane or Sydney data
  - Bandwidth: 500KB regardless of total players
```

**Total bandwidth: O(local area) NOT O(world size)**

## Spatial Sharding for Operations

**Current (doesn't scale):**
- You dig in Brisbane
- Broadcast to ALL 100,000 players
- 99,990 of them don't care

**Future (scales):**
```
You dig voxel at GPS(-27.47, 153.02)
  ↓
Calculate chunk: chunk_brisbane_cbd
  ↓
Broadcast ONLY to players in nearby chunks:
  - Same chunk: Always receive
  - Adjacent chunks: Receive (might see the edit)
  - 1km away: Don't receive (too far to see)
  ↓
Total recipients: 10-50 players (not 100,000)
```

**Bandwidth: O(nearby players) NOT O(total players)**

## Data Structure

```
World (infinite)
  ↓
Divided into Chunks (1km × 1km × 500m)
  ↓
Each chunk:
  - Base terrain (generated, cached, ~50KB)
  - operations.json (edits, ~1-100KB)
  - Total: ~150KB per chunk
  ↓
Player loads ~10 chunks = 1.5MB
  ↓
Scales to infinite world size
```

## Implementation Plan

### Phase 1: Chunk-Based Files (Next)
```rust
// Instead of:
world_data/operations.json

// Do:
world_data/chunks/chunk_0_0/operations.json
world_data/chunks/chunk_0_1/operations.json
// etc
```

**Changes needed:**
1. Calculate chunk ID from GPS coordinates
2. Save operations to chunk-specific file
3. Load operations from chunk-specific file
4. (Already done: chunk-based terrain generation)

### Phase 2: Spatial Query System
```rust
// Get chunks near player position
fn get_nearby_chunks(player_pos: ECEF) -> Vec<ChunkId> {
    // Return chunks within 1km radius
}

// Only load operations for nearby chunks
for chunk_id in get_nearby_chunks(player_pos) {
    let ops = load_chunk_operations(chunk_id);
    apply_operations(ops);
}
```

### Phase 3: Spatial Broadcast
```rust
// When player edits voxel
fn broadcast_voxel_operation(coord: VoxelCoord) {
    let chunk_id = coord_to_chunk(coord);
    
    // Find peers in nearby chunks
    let nearby_peers = get_peers_in_chunks(
        nearby_chunks(chunk_id, radius=1)
    );
    
    // Broadcast ONLY to them (not everyone)
    for peer in nearby_peers {
        send_to(peer, operation);
    }
}
```

### Phase 4: DHT for Chunk Discovery
```rust
// When player needs chunk data
fn request_chunk(chunk_id: ChunkId) {
    // Ask DHT: "Who has this chunk?"
    let providers = dht.get_providers(chunk_id);
    
    // Download from ANY provider
    for peer in providers {
        if let Some(data) = request_from(peer, chunk_id) {
            // Verify data
            if verify_chunk(data) {
                return data;
            }
        }
    }
}
```

## Bandwidth Analysis

**Current (doesn't scale):**
- 100,000 players
- Each edits 1 voxel/sec
- Broadcast to all: 100,000 × 100,000 × 160 bytes = 1.6 TB/sec
- **BROKEN**

**With Spatial Sharding:**
- 100,000 players distributed worldwide
- Each edits 1 voxel/sec
- Broadcast to nearby (50 players): 100,000 × 50 × 160 bytes = 800 MB/sec
- **Divided among 100,000 clients = 8 KB/sec each**
- **WORKS**

## Why This Wasn't Implemented Yet

We're building foundation first:
1. ✅ P2P networking
2. ✅ Operation logging
3. ✅ Vector clocks
4. ⏳ Chunk-based files ← **NEXT**
5. ⏳ Spatial sharding
6. ⏳ DHT discovery

**You're right to ask about scalability NOW**
- Prevents building the wrong thing
- Chunk-based files are next priority
- All our architecture supports this (designed for it)

## Immediate Action

Let me implement chunk-based operation logs right now:
1. Calculate chunk ID from voxel coordinates
2. Save to `world_data/chunks/{chunk_id}/operations.json`
3. Load only chunks near player
4. Test with 3 clients in same area (should work same as now)
5. Foundation for future spatial sharding

**This is exactly the right question to ask. Want me to implement chunk-based files now?**
