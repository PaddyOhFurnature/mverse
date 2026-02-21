# Discovery Design Clarifications - Working Notes

## Issue 1: What "Region" Means

### The Problem User Identified:

**My wrong assumption:**
"Player in San Francisco subscribes to SF region topics"

**Why this is wrong:**
1. Player in San Francisco (real world) might be playing in Australia (game world)
2. Player uses VPN → real-world location is masked/wrong
3. Player on satellite connection, moves 5000km physically while logged in
4. Real-world location has NO relevance to what game content they need

**Example scenarios that break real-world tracking:**
```
Scenario 1: Geographic mismatch
- Player physically in USA
- Playing in Tokyo in-game
- Needs Tokyo chunks, not USA chunks
- Real-world location is useless

Scenario 2: VPN
- Player physically in China
- VPN shows location as Netherlands
- Playing in Australia in-game
- Both real-world locations are wrong for content needs

Scenario 3: Mobile (satellite)
- Player starts in London (in-game)
- Takes laptop on plane with Starlink
- Flies to Australia (real world)
- Still playing in London (in-game)
- Real-world location changed, game location didn't
```

### What SHOULD "region" mean?

**Option A: In-Game Character Position**
```
Player character at lat/lon (37.7749, -122.4194) IN THE GAME WORLD
→ Subscribe to topics for chunks near that game position
→ Regardless of where player physically is
```

**Challenges:**
- Fast travel (fly/teleport) → rapid topic churn
- Large view distance → subscribe to many regions
- Game world IS Earth-scale → regions still need definition

**Option B: Network Proximity (User's Hint)**
```
Don't use geography at all for topics
Use network metrics:
- Ping/latency to peers
- Data rate/bandwidth available
- Closest node timing
```

**But this doesn't work because:**
- Network proximity ≠ what content you need
- Low-ping peer in Japan doesn't help if you need New York chunks

### User's Insight: Separate Two Concerns

**WHAT you want (content) vs WHO you get it from (peers)**

I think the answer is:

1. **Topic subscription** = Based on IN-GAME character position
   - "I need chunks near my character in the game world"
   - Subscribe to "chunks:tile_australia" because character is in Australia
   - Has nothing to do with player's real-world location

2. **Peer selection** = Based on network proximity
   - "Of all peers who HAVE this content, who's closest network-wise?"
   - Prefer peers with low ping, high bandwidth
   - Use: ping, data rate, node timing

**Example:**
```
Character in Australia (game) → Subscribe to "chunks:tile_australia"
Query DHT: "Who has chunks:tile_australia?"
Response: [Alice (ping: 20ms), Bob (ping: 200ms), Charlie (ping: 50ms)]
Select: Alice (lowest ping)
Request chunk from Alice

Alice might be:
- In Australia (real world) - coincidence
- In USA with good internet - doesn't matter
- Using VPN - doesn't matter
- On satellite - if ping is good, fine
```

## Issue 1: What "Region" Means - RESOLVED

### The Problem User Identified:

**My wrong assumption:**
"Player in San Francisco subscribes to SF region topics"

**Why this is wrong:**
1. Player in San Francisco (real world) might be playing in Australia (game world)
2. Player uses VPN → real-world location is masked/wrong
3. Player on satellite connection, moves 5000km physically while logged in
4. Real-world location has NO relevance to what game content they need

**Example scenarios that break real-world tracking:**
```
Scenario 1: Geographic mismatch
- Player physically in USA
- Playing in Tokyo in-game
- Needs Tokyo chunks, not USA chunks
- Real-world location is useless

Scenario 2: VPN
- Player physically in China
- VPN shows location as Netherlands
- Playing in Australia in-game
- Both real-world locations are wrong for content needs

Scenario 3: Mobile (satellite)
- Player starts in London (in-game)
- Takes laptop on plane with Starlink
- Flies to Australia (real world)
- Still playing in London (in-game)
- Real-world location changed, game location didn't
```

### SOLUTION: Hierarchical + On-Demand + Chunk Streaming Priority

**Key insight:** We ALREADY have chunk streaming priority system!
- Chunk you're in: Highest priority (load NOW)
- Chunks in direction of travel: Medium priority (prefetch)
- Chunks being culled: Lowest priority (background)

**Just tie P2P data requests to the same priority queue:**

```rust
// ChunkStreamer already does this:
1. Load chunk player is standing in (PRIORITY 1)
2. Load chunks in direction of movement (PRIORITY 2)  
3. Load chunks in view frustum (PRIORITY 3)
4. Cull distant chunks (PRIORITY 4 - cleanup)

// Add P2P layer on top:
1. Request data for current chunk FROM NETWORK (PRIORITY 1)
2. Prefetch data for next chunks FROM NETWORK (PRIORITY 2)
3. Background fetch for visible chunks FROM NETWORK (PRIORITY 3)
4. Passively share data for chunks we have (PRIORITY 4)
```

**Hierarchical topics based on in-game chunk structure:**

```
Global level (always subscribed):
  - "global:announcements"
  
Regional level (tile-based, ~100km²):
  - "state:tile_12345"         → Active players in this tile
  - "chunks:tile_12345"        → Chunk edits in this tile
  
Chunk level (on-demand, NOT topics):
  - DHT query: "Who has chunk_xyz?"
  - Request from fastest peer
  - Cache locally
```

**Bandwidth scaling (User's example):**

```
Average player walking down street:
  - 5-10 nearby players
  - Few edits per second
  - LOW bandwidth: ~10-50 KB/s

Racetrack grandstand (10,000 players):
  - 10,000 players in view
  - Every player interacting with every object
  - HIGH bandwidth: ~5-10 MB/s
  
BUT: Only players AT the racetrack see high bandwidth
     Players in empty forest: ~1 KB/s (just their own position)
     
Bandwidth scales with LOCAL complexity, not global player count
```

**The beautiful part:** This already matches our chunk streaming!
- Already prioritize chunks
- Already cull distant chunks
- Just add network requests to same queue
- Bandwidth naturally scales with what you see

### Implementation Strategy:

1. **Use existing ChunkStreamer priority queue**
   - Don't create new priority system
   - Piggyback on chunk streaming logic

2. **Hierarchical topics for regions:**
   - Tile-level topics (100km²): "state:tile_12345"
   - Subscribe to current tile + 8 neighbors (3x3 grid)
   - Update subscriptions when crossing tile boundaries

3. **On-demand DHT for specific chunks:**
   - Query DHT when ChunkStreamer requests chunk
   - "Who has chunk_xyz?" → peer list
   - Request from fastest peer
   - Cache result

4. **Passive background sharing:**
   - Announce chunks we have to DHT (low priority)
   - Serve chunks to other peers (when bandwidth available)
   - Lowest priority, doesn't block active gameplay

### Example Flow:

```
Player walking in Tokyo (in-game):

Current tile: tile_tokyo_central
Subscribe to topics:
  - state:tile_tokyo_central
  - state:tile_tokyo_north (neighbor)
  - state:tile_tokyo_south (neighbor)
  - ... (8 neighbors total)

ChunkStreamer says: "Need chunk at (100, 0, 50)"
  ↓
Query DHT: "Who has chunk_100_0_50?"
  ↓
Response: [Alice (20ms), Bob (200ms), Charlie (50ms)]
  ↓
Request from Alice (fastest)
  ↓
Receive chunk, validate, cache
  ↓
Announce to DHT: "I now have chunk_100_0_50"

Player starts flying west:
  ↓
ChunkStreamer prefetches chunks in west direction
  ↓
P2P layer prefetches data for those chunks (PRIORITY 2)
  ↓
When player arrives, chunks already loaded
  ↓
Smooth experience, no loading stutter
```

### Bandwidth Analysis (User is correct):

**Empty area (forest, desert):**
```
Player alone
  - Position update: 50 bytes/sec
  - No other players to sync
  - Total: ~0.1 KB/s
```

**Moderate area (small town):**
```
Player + 20 others
  - 20 position updates: 1 KB/s
  - Few voxel edits: 0.5 KB/s
  - Total: ~2 KB/s
```

**Dense area (city center):**
```
Player + 500 others
  - 500 position updates: 25 KB/s
  - Many edits: 5 KB/s
  - Total: ~30 KB/s
```

**Extreme area (racetrack, concert):**
```
Player + 10,000 others
  - 10,000 position updates: 500 KB/s
  - Massive interaction: 100 KB/s
  - Total: ~600 KB/s = 0.6 MB/s
```

**User is correct:** Average player uses very little bandwidth!
- Only scales with LOCAL density
- NOT with global player count
- 1,000,000 players online doesn't matter if you're in empty forest
- This is the "graceful degradation" we want

---

## Status: CONCEPT VALIDATED

Key insights:
1. ✅ Use in-game position (not real-world) for topics
2. ✅ Use network metrics for peer selection
3. ✅ Tie to existing ChunkStreamer priority
4. ✅ Hierarchical (tiles) + On-demand (chunks)
5. ✅ Bandwidth scales with local complexity

Ready to update main design document with these clarifications.

