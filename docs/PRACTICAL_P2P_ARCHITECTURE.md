# Practical P2P Architecture - With Real Concessions

## The Concessions That Make This Possible

### 1. PVE Only (HUGE Impact)
```
PVP requires:
  - Frame-perfect hit detection
  - No rollback allowed (unfair advantage)
  - Strict consistency (both see same thing)
  - Low latency (< 50ms or feels laggy)

PVE allows:
  - Approximate synchronization (NPCs don't complain)
  - Rollback is fine (NPC position corrected = invisible to player)
  - Eventual consistency (sync over 1-2 seconds is OK)
  - Higher latency tolerable (100-200ms is fine)
```

**This removes the strictest consistency requirement!**

### 2. 25 FPS Tick Rate (Even Better)
```
Client rendering: 60+ FPS (smooth, local)
Network tick rate: 25 FPS (40ms per update)
  
Benefits:
  - 2.4x less bandwidth than 60 FPS
  - More time for packet retransmission
  - Client interpolates between network updates
  - Feels smooth (client handles presentation)
  
Example:
  Frame 0ms: Receive position A
  Frame 40ms: Receive position B (but might arrive at 60ms due to lag)
  Client: Interpolate A→B smoothly over 40-60ms
  Player: Sees smooth motion (doesn't notice network lag)
```

**Halves bandwidth, adds latency tolerance.**

### 3. 100ms Input Buffer (Genius)
```
Traditional:
  - You press W
  - Broadcast immediately
  - Packet lost
  - Position desyncs

With 100ms buffer:
  - You press W at t=0
  - Buffer until t=100ms
  - Send input at t=100ms
  - If packet lost, resend at t=120ms
  - Still arrives before client needs it (t=140ms)
  
Benefits:
  - Absorbs packet loss (can retry)
  - Absorbs reordering (buffer sorts)
  - Absorbs jitter (smooth out variance)
  - Allows TCP-like reliability without blocking
```

**Makes unreliable network feel reliable!**

### 4. Quality Scaling
```
Fiber (50 Mbps):
  - Full quality: 500 entities @ 25 FPS
  - Full animations, physics, interactions

4G (5 Mbps):
  - Reduced: 200 entities @ 25 FPS
  - Simplified animations, interpolated physics

Rural (1 Mbps):
  - Minimal: 50 entities @ 10 FPS
  - Static poses, position-only updates

Dial-up (56 Kbps): [joke, but...]
  - Text mode: "There are 3 people nearby"
```

**Adaptive degradation instead of failure.**

### 5. Local Caching for High-Traffic Areas
```
Brisbane CBD (high traffic):
  - Chunk data: 50 MB (detailed)
  - NPC spawn points: 1000 entries
  - Traffic patterns: pre-computed paths
  - Building interiors: fully loaded
  - Cached locally (download once)
  
Outback (low traffic):
  - Chunk data: 500 KB (basic terrain)
  - NPC spawn points: 10 entries
  - No traffic patterns
  - No building interiors
  - Generated on-the-fly
  
Network traffic:
  - CBD: 50 MB download once, then only player positions (50 KB/sec)
  - Outback: 500 KB download, player positions (5 KB/sec)
```

**Front-load static data, minimize dynamic sync.**

### 6. Approximation for Low-Movement
```
Sitting at coffee shop:
  - Position: Static (not moving)
  - Animation: Looping (drinking coffee)
  - Update rate: 1 FPS (nothing changing)
  
Bandwidth:
  - Instead of: 64 bytes × 25 FPS = 1.6 KB/sec
  - Actual: 64 bytes × 1 FPS = 64 bytes/sec
  - 25x reduction!
  
Detection:
  - If velocity < 0.1 m/s for 5 seconds → low-movement mode
  - Reduce update rate automatically
  - When movement detected → back to 25 FPS
```

**Most entities aren't moving most of the time!**

---

## The Movie Streaming Problem (BRILLIANT Example)

### Scenario
```
You: Invite friends to watch movie in your house
Friend 1: Sits on couch
Friend 2: Sits on couch
Friend 3: Gets popcorn from kitchen

Movie: 2-hour film, 1080p, 5 GB file
```

### Naive Approach (BROKEN)
```
You: Stream movie data to friends
  - 5 GB / 2 hours = 2.5 MB/sec per friend
  - 3 friends = 7.5 MB/sec upload
  - You look away → culling stops stream
  - Friends see: Black screen (WTF?)
```

### Smart Approach (Hybrid Authority)

**Step 1: Metadata Sync (Lightweight)**
```rust
// You start movie
struct MovieEvent {
    movie_id: "sha256:abc123...",  // IPFS/torrent hash
    start_time: SystemTime,         // Wall-clock sync point
    playback_position: 0.0,         // Seconds into movie
    state: Playing,
}

// Broadcast to friends (tiny)
broadcast(MovieEvent {
    movie_id: "sha256:abc123...",
    start_time: now(),
    playback_position: 0.0,
    state: Playing,
});

// Bandwidth: ~200 bytes one-time
```

**Step 2: Friends Fetch Movie (P2P)**
```rust
// Friend 1 receives event
on_movie_event(event) {
    // Do I have this movie?
    if local_cache.has(event.movie_id) {
        // Yes! Load from disk
        load_from_cache(event.movie_id);
    } else {
        // No - need to download
        
        // Query DHT: "Who has this movie?"
        let providers = dht.get_providers(event.movie_id);
        
        // Providers might include:
        //   - You (the host)
        //   - Friend 2 (already downloaded it)
        //   - Content cache node (CDN-like)
        //   - IPFS gateway
        
        // Download from fastest provider
        download_from(providers[0], event.movie_id);
    }
    
    // Calculate playback position accounting for download time
    let elapsed = now() - event.start_time;
    start_playback_at(elapsed);
}
```

**Step 3: Playback Sync (Minimal)**
```rust
// You (host) broadcast sync events occasionally
every 5 seconds:
    broadcast(MovieEvent {
        movie_id: "sha256:abc123...",
        start_time: original_start,
        playback_position: current_time,
        state: Playing,
    });
    
// Friends adjust playback to stay in sync
on_sync_event(event) {
    let expected_pos = (now() - event.start_time).as_secs_f32();
    let actual_pos = local_player.position();
    let drift = (expected_pos - actual_pos).abs();
    
    if drift > 1.0 {  // More than 1 second out of sync
        local_player.seek(expected_pos);  // Hard sync
    } else if drift > 0.1 {
        local_player.adjust_rate(1.01);  // Slightly speed up to catch up
    }
}

// Bandwidth: 200 bytes every 5 sec = 40 bytes/sec
```

**Step 4: Handle Edge Cases**
```rust
// You pause movie
broadcast(MovieEvent { state: Paused, playback_position: 45.2 });
// Friends pause their local playback

// You leave (get up to do dishes)
// Friends: Keep watching! (movie is playing locally)
// Sync events stop arriving
// Friends continue playing with last known state
// When you return: Send sync event, friends adjust

// Your game crashes
// Friends: Keep watching
// Elect new "host" (whoever has been there longest)
// New host broadcasts sync events

// Friend has slow internet
// Download movie in background while playing
// Start with low-res version (fast download)
// Upgrade to high-res as more downloads
```

### Bandwidth Analysis
```
Naive (streaming):
  - You → 3 friends: 2.5 MB/sec × 3 = 7.5 MB/sec (continuous)
  - Total: 7.5 MB/sec for 2 hours = 54 GB

Smart (metadata):
  - Initial metadata: 200 bytes × 3 friends = 600 bytes
  - Sync events: 200 bytes every 5 sec = 40 bytes/sec
  - Control events (pause/resume): ~100 bytes/event
  - Total: ~1 MB for entire 2-hour movie
  
Friends download movie:
  - From you: 0 bytes (they get from cache/DHT)
  - From cache node: 5 GB each (one-time)
  - From each other: Share chunks (BitTorrent-style)
```

**54 GB → 1 MB = 54,000x reduction!**

---

## Shared Media Authority Pattern

### The Pattern
```
1. Host broadcasts: "I'm starting X" (metadata only)
2. Clients fetch X independently (DHT/cache/IPFS)
3. Host sends sync signals (state, position, rate)
4. Clients play locally, adjust to stay in sync
5. If host leaves: Elect new host OR continue solo
```

### Applications
```
Movies:
  - Host: Plays from local library
  - Clients: Download and play locally
  - Sync: Playback position every 5 sec

Music:
  - Host: Starts song
  - Clients: Download song
  - Sync: Position + BPM

Screen sharing:
  - Host: Streams screen (no way around this)
  - Clients: Receive stream
  - Bandwidth: High (video compression helps)

Presentation:
  - Host: Shows slide 5
  - Clients: Download presentation file
  - Sync: Current slide number (tiny)

Whiteboard:
  - Host: Draws strokes
  - Clients: Receive stroke data (vector, small)
  - Sync: CRDT merge (like voxel ops)
```

---

## The Real Architecture (Finally Concrete)

### Data Layers (Ordered by Update Frequency)

**Layer 0: Content (Static Files)**
```
Type: Movies, music, models, textures
Storage: IPFS/DHT + local cache
Bandwidth: One-time download, then zero
Authority: Cryptographic hash (SHA256)
Example: Movie file, building model, avatar mesh

Update rate: NEVER (immutable content)
Sync method: Hash reference only
```

**Layer 1: World Terrain (Static, Deterministic)**
```
Type: Base terrain, rivers, elevation
Storage: Generated from SRTM + cached
Bandwidth: One-time generation, then zero
Authority: Deterministic function (same input = same output)
Example: Brisbane terrain, river geometry

Update rate: NEVER (re-generated from source)
Sync method: Verify hash of generated mesh
```

**Layer 2: Infrastructure (Semi-Static)**
```
Type: Roads, buildings, bridges, tunnels
Storage: Generated from OSM + cached
Bandwidth: One-time generation + rare updates
Authority: Deterministic + signed ops for edits
Example: Road network, building placement

Update rate: Weekly (OSM updates)
Sync method: Version number + hash
```

**Layer 3: User Constructions (Slow-Changing)**
```
Type: Player-built structures, terrain edits
Storage: CRDT operation log (persistent)
Bandwidth: Operation log (append-only)
Authority: Signature + ownership proof
Example: Your house, quarry excavation

Update rate: Seconds to minutes (building)
Sync method: CRDT merge (like we have now)
```

**Layer 4: Dynamic Objects (Medium-Changing)**
```
Type: Vehicles, doors, interactive objects
Storage: State snapshots + deltas
Bandwidth: Delta updates at 10-25 FPS
Authority: Owner (player controlling it)
Example: Your car, coffee shop door

Update rate: 10-25 FPS (40-100ms)
Sync method: Owner-authoritative + prediction
```

**Layer 5: Characters (Fast-Changing)**
```
Type: Player avatars, NPC humans/animals
Storage: Position + animation state
Bandwidth: Full updates at 25 FPS
Authority: Owner (for players) or deterministic (NPCs)
Example: Your avatar, friend's avatar, dog NPC

Update rate: 25 FPS (40ms)
Sync method: Owner broadcasts, others interpolate
```

**Layer 6: Physics Effects (Very Fast, Ephemeral)**
```
Type: Particles, sound, temporary FX
Storage: NOT stored (local simulation only)
Bandwidth: Event triggers only (tiny)
Authority: Local (each client simulates)
Example: Dust from car tires, water splash

Update rate: 60+ FPS (local)
Sync method: Trigger event only, rest is deterministic
```

### Bandwidth Budget (With Concessions)

**High-traffic area (Brisbane CBD):**
```
Layer 0 (Content): 0 KB/sec (cached)
Layer 1 (Terrain): 0 KB/sec (cached)
Layer 2 (Infrastructure): 0 KB/sec (cached)
Layer 3 (Constructions): 1 KB/sec (rare edits)
Layer 4 (Vehicles): 50 × 128 bytes × 25 FPS = 160 KB/sec
Layer 5 (Characters): 200 × 64 bytes × 25 FPS = 320 KB/sec
Layer 6 (Effects): 50 × 16 bytes = 0.8 KB/sec (events only)

Total: ~482 KB/sec ≈ 3.86 Mbps
```

**Low-traffic area (Coffee shop):**
```
Layer 0-3: 0 KB/sec (cached)
Layer 4: 2 × 128 × 1 FPS = 256 bytes/sec (parked cars, static)
Layer 5: 5 × 64 × 1 FPS = 320 bytes/sec (sitting, low-movement)
Layer 6: 10 × 16 = 160 bytes/sec (occasional)

Total: ~736 bytes/sec ≈ 5.9 Kbps
```

**With quality scaling:**
```
Fiber (good): Full quality (482 KB/sec)
4G (medium): Reduce Layer 4+5 by 50% (241 KB/sec)
Rural (poor): Reduce Layer 4+5 by 80% (96 KB/sec)
```

---

## Handling the Movie Scenario (Complete)

### Your Movie Library
```
Your local library:
  - movies/matrix.mkv (SHA256: abc123...)
  - movies/inception.mkv (SHA256: def456...)
  
When you start movie:
  1. Don't stream file
  2. Broadcast: "Playing sha256:abc123 at position 0"
  3. Advertise to DHT: "I have sha256:abc123"
```

### Friends Join
```
Friend 1:
  - Has file cached (watched it before)
  - Loads from local disk (instant)
  - Syncs playback position
  - Bandwidth: 40 bytes/sec (sync only)
  
Friend 2:
  - Doesn't have file
  - Queries DHT: "Who has sha256:abc123?"
  - Gets: [You, Friend 1, CacheNode]
  - Downloads chunks from all three (BitTorrent-style)
  - Starts playing low-res preview while downloading
  - Upgrades to full quality when download completes
  - Bandwidth: 5 GB download + 40 bytes/sec sync
  
Friend 3:
  - Downloads like Friend 2
  - Slow connection
  - Gets compressed version (1 GB instead of 5 GB)
  - Plays at 720p (still good)
```

### You Look Away (Do Dishes)
```
Your client:
  - Culling: No longer rendering TV
  - Movie playback: STILL RUNNING (audio continues)
  - Sync broadcasts: STILL SENDING (every 5 sec)
  
Friends:
  - See TV screen (in their view)
  - Movie playing locally
  - Receive sync events
  - Everything works normally
  
Why:
  - Movie isn't "streamed from you"
  - Friends have the file
  - They're playing it locally
  - You're just coordinating (play/pause/position)
```

### You Leave Entirely
```
Your client:
  - Exits game
  - Stops sending sync events
  
Friends:
  - Notice: No sync for 15 seconds
  - Elect new host: Friend 1 (oldest viewer)
  - Friend 1 becomes authority
  - Broadcasts sync events now
  - Movie continues seamlessly
  
Or (simpler):
  - No new host elected
  - Everyone keeps playing locally
  - No more sync (might drift slightly)
  - Close enough for casual watching
```

---

## Hybrid Nodes (Your Instinct is Right)

### Types of Nodes

**Player Clients (Everyone)**
```
Role:
  - Run full simulation
  - Authority for their own entities
  - Cache content they've seen
  - Share cached content with others
  
Always present: Yes (all players)
Bandwidth: Variable (based on location/activity)
```

**Cache Nodes (Optional, Helpful)**
```
Role:
  - Store popular content (movies, models, chunks)
  - Act as CDN for downloads
  - Bootstrap new players (provide initial data)
  - NOT authoritative (just storage)
  
Who runs them:
  - You (development/testing)
  - Community volunteers
  - Players with spare bandwidth
  - Anyone (open protocol)
  
Bandwidth: High (serving downloads)
Authority: ZERO (content verified by hash)
```

**Sync Coordinators (For High-Traffic Areas)**
```
Role:
  - Relay messages for players in same area
  - Reduce P2P connection count (100 players don't need 99 connections each)
  - Batch updates (aggregate nearby player positions)
  - NOT authoritative (just relay)
  
Example:
  - 100 players in Brisbane CBD
  - Without coordinator: 100 × 99 / 2 = 4,950 P2P connections
  - With coordinator: 100 × 1 = 100 connections to coordinator
  
Authority: ZERO (just forwarding signed messages)
```

**Bootstrap Nodes (Essential)**
```
Role:
  - Help new players find P2P network
  - Provide list of active peers
  - Advertise available cache nodes
  - NOT authoritative
  
Who runs them:
  - You (official bootstraps)
  - Community (unofficial bootstraps)
  
Bandwidth: Low (just peer lists)
Authority: ZERO
```

---

## The Key Insight

**You said:**
> "the entire way data is transmitted and stored locally and between nodes and players is required"

**You're absolutely right.**

Traditional thinking:
```
Server stores data → Clients request → Server sends
```

This thinking:
```
Data exists everywhere (IPFS/DHT)
Clients have what they've seen (local cache)
Metadata syncs (tiny) coordinate shared experience
Content never moves (hash references)
```

**Example:**
```
Traditional: You show friend a video
  → You stream 5 GB to them
  → 5 GB bandwidth used

New way: You share video hash
  → Friend has it cached (0 GB)
  → Friend doesn't have it (downloads from cache node)
  → Friend partially has it (downloads missing chunks)
  → You broadcast: 200 bytes (hash + position)
```

---

## Next Steps

**I think we should implement:**

1. **Chunk-based operation files** (spatial sharding)
2. **DHT integration** (content discovery)
3. **25 FPS tick rate** (with client interpolation)
4. **100ms input buffer** (latency tolerance)
5. **Low-movement detection** (automatic rate reduction)

**This gives us:**
- ✅ Practical bandwidth (< 500 KB/sec)
- ✅ Works on 4G (with quality scaling)
- ✅ Handles packet loss (input buffer)
- ✅ Smooth rendering (client interpolation)
- ✅ Content sharing (DHT)

**Want me to start with #1 (chunk-based files)?** 

This is the foundation everything else builds on.
