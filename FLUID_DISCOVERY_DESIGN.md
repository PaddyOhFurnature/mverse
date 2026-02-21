# Fluid Discovery System Design
**Planet-Scale P2P Content Discovery for Dynamic Metaverse**

**Created:** 2026-02-21  
**Status:** DESIGN PHASE - Not yet implemented  
**Checkpoint:** `pre-discovery-design-20260221-0549`

---

## 🎯 The Problem

**Traditional BitTorrent DHT:**
```
Magnet Link → DHT Query → Tracker → Peer List → Download File
```
- ✅ Works for: Static files, global interest, complete downloads
- ❌ Fails for: Dynamic content, regional interest, partial data

**Our Metaverse:**
- Content constantly changes (player edits chunks)
- Players only care about nearby chunks (location-based interest)
- Need discovery to be **spatial** and **temporal**
- Peers come and go based on where they are

**Current State (What We Have):**
```rust
// src/network.rs
pub(crate) struct MetaverseBehaviour {
    pub(crate) kademlia: kad::Behaviour<MemoryStore>,    // ✅ Exists, ❌ Not used for content
    pub(crate) gossipsub: gossipsub::Behaviour,          // ✅ Works, ❌ One global topic
    pub(crate) mdns: mdns::tokio::Behaviour,             // ✅ Works, ❌ LAN only
}

// examples/metaworld_alpha.rs line 178
let relay_addr = "/ip4/49.182.84.9/tcp/4001/p2p/12D3Koo...";  // ❌ Hardcoded, single point
```

**Problems:**
1. ❌ **No content announcement** - DHT exists but peers don't announce what chunks they have
2. ❌ **No spatial discovery** - Can't find "who has chunk at lat/lon X,Y"
3. ❌ **Single bootstrap** - Hardcoded relay is single point of failure
4. ❌ **Global topic** - Everyone gets all state updates (doesn't scale)
5. ❌ **No chunk routing** - Can't ask network "who has chunk X?"

---

## 🏗️ Design: Fluid Discovery Architecture

### Three-Layer Discovery System

```
┌─────────────────────────────────────────────────────────────┐
│  LAYER 1: BOOTSTRAP & RENDEZVOUS                           │
│  ───────────────────────────────────────────               │
│  Get onto the network, find initial peers                  │
│                                                             │
│  • Bootstrap node list (multiple relays)                   │
│  • Well-known relay servers                                │
│  • Community-run relays                                    │
│  • Fallback: DNS seeds (relay1.metaverse.org)             │
│                                                             │
│  Purpose: Initial entry into the mesh                      │
└─────────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────┐
│  LAYER 2: SPATIAL DISCOVERY (DHT)                          │
│  ───────────────────────────────────────────────           │
│  Find peers by location/content                            │
│                                                             │
│  • Kademlia DHT for content routing                        │
│  • Announce: "I have chunks at region X"                   │
│  • Query: "Who has chunks near lat/lon Y,Z?"               │
│  • Content keys = Hash(region_id + content_type)           │
│                                                             │
│  Example DHT keys:                                         │
│    - region:tile_12345:chunks   → Peer list                │
│    - region:tile_12345:players  → Peer list                │
│    - chunk:xyz_hash:providers   → Peer list                │
│                                                             │
│  Purpose: Find WHO has WHAT WHERE                          │
└─────────────────────────────────────────────────────────────┘
           │
           ▼
┌─────────────────────────────────────────────────────────────┐
│  LAYER 3: DYNAMIC TOPICS (Gossipsub)                       │
│  ────────────────────────────────────────────              │
│  Subscribe to regions of interest                          │
│                                                             │
│  • One topic per region/chunk zone                         │
│  • Subscribe to topics near your location                  │
│  • Unsubscribe when you leave region                       │
│  • Auto-subscribe to neighbor chunks                       │
│                                                             │
│  Example topics:                                           │
│    - state:region_12345      → Regional state updates      │
│    - chunks:tile_12345       → Chunk edit notifications    │
│    - players:region_12345    → Player movement in region   │
│    - global:announcements    → Global broadcasts           │
│                                                             │
│  Purpose: Real-time data sync for active regions           │
└─────────────────────────────────────────────────────────────┘
```

---

## 📋 Component Design

### 1. Bootstrap Node List

**Instead of:** One hardcoded relay  
**Use:** Configurable bootstrap list with self-healing network

```rust
pub struct BootstrapConfig {
    /// Hardcoded starting nodes (relay1, relay2, relay3...)
    /// These are ONLY used for initial connection
    pub initial_relays: Vec<RelayAddress>,
    
    /// Fallback DNS seeds (relay.metaverse.org → IP lookup)
    pub dns_seeds: Vec<String>,
    
    /// Cached peers from previous session (instant reconnect)
    pub peer_cache: Vec<PeerId>,
    
    /// Try relays in parallel, use first N that respond
    pub parallel_bootstrap: usize,
}

pub struct RelayAddress {
    pub multiaddr: String,
    pub peer_id: String,
    pub weight: f32,      // Prefer certain relays (0.0-1.0)
    pub region: String,   // Geographic hint (us-east, eu-west, etc.)
}
```

**Self-Healing Bootstrap Network:**

```rust
// Every relay announces itself to DHT
async fn announce_relay(&mut self) {
    let key = "bootstrap:relays";
    let value = RelayInfo {
        peer_id: self.peer_id,
        multiaddr: self.public_addr.clone(),
        uptime: self.uptime_percentage(),
        region: self.region.clone(),
    };
    
    // Announce every 5 minutes (keeps us in DHT)
    self.dht.put_record(key, bincode::serialize(&value)?);
}

// Every client refreshes relay list from DHT
async fn refresh_relay_list(&mut self) {
    // Query DHT for current relay list
    let relays = self.dht.get_providers("bootstrap:relays").await?;
    
    // Update our local cache
    self.bootstrap_config.update_relays(relays);
    
    // Persist to disk for next session
    self.save_peer_cache();
}
```

**Bootstrap process (with self-healing):**
```
1. Try cached peers from last session (instant reconnect)
   ├─ If 2+ connect → DONE (fastest path)
   └─ If < 2 connect → continue

2. Try hardcoded initial relays in parallel
   ├─ Connect to first 3 that respond
   └─ If < 2 connect → continue

3. Fallback to DNS seeds
   ├─ Resolve relay.metaverse.org → IP
   └─ Connect to resolved relays

4. Once connected:
   ├─ Query DHT: "bootstrap:relays" → get full relay list
   ├─ Cache relay list locally
   └─ Refresh list every 5 minutes

5. If relay goes offline:
   ├─ DHT TTL expires (no re-announcement)
   ├─ Relay removed from list automatically
   └─ Next refresh, clients get updated list
```

**Decentralization:**
- No single point of failure (multiple hardcoded relays)
- Network self-heals (relays announce/expire via DHT)
- Anyone can run a relay (announces to DHT)
- Client tries multiple, picks best (latency, uptime)
- Community maintains list (like Bitcoin/IPFS)

**Key insight from user:** 
> "Like magnet files updating the peers, except sometimes our peers need to 
> update the location of the magnet file if that host is offline."

This is exactly what the DHT announcements do - relays announce themselves,
if they move or go offline, the announcement expires and network updates.

---

### 2. Spatial DHT Announcements

**What to announce:**
```rust
pub enum ContentAnnouncement {
    /// I have these chunks loaded
    Chunks {
        region_id: u64,              // Spatial tile ID
        chunk_ids: Vec<ChunkId>,     // List of chunks
        last_updated: u64,           // Timestamp
    },
    
    /// I'm actively playing in this region
    PlayerPresence {
        region_id: u64,
        position: ECEF,
        radius_m: f64,               // How far I can see
    },
    
    /// I have edited content in these chunks
    UserContent {
        chunk_ids: Vec<ChunkId>,
        edit_count: u32,             // How much content
        signature: Vec<u8>,          // Proof of ownership
    },
}
```

**How to announce:**
```rust
// Every 5 minutes, announce to DHT
async fn announce_content(&mut self) {
    // Announce chunks we have
    for region_id in self.loaded_regions() {
        let key = format!("region:{}:chunks", region_id);
        let value = bincode::serialize(&self.get_chunks(region_id))?;
        self.dht.put_record(key, value);
    }
    
    // Announce player presence
    let region_id = self.player_position.to_region_id();
    let key = format!("region:{}:players", region_id);
    self.dht.put_record(key, self.peer_id.to_bytes());
}

// When we need a chunk
async fn find_chunk_providers(&mut self, chunk_id: &ChunkId) -> Vec<PeerId> {
    let region_id = chunk_id.to_region_id();
    let key = format!("region:{}:chunks", region_id);
    
    // Query DHT for who has chunks in this region
    let providers = self.dht.get_providers(&key).await?;
    
    // Filter to peers that specifically have this chunk
    providers.into_iter()
        .filter(|peer| self.peer_has_chunk(peer, chunk_id))
        .collect()
}
```

**DHT key structure:**
```
region:{tile_id}:chunks        → List of peers with chunks in tile
region:{tile_id}:players       → List of active players in tile
chunk:{chunk_hash}:providers   → List of peers with specific chunk
user:{peer_id}:content         → User's owned content index
global:relays                  → List of known relay servers
```

**Efficiency:**
- Announce every 5 minutes (TTL refresh)
- Only announce regions we're actively in
- Unannounce when we leave region
- DHT naturally expires old announcements

---

### 3. Dynamic Regional Topics + Chunk Streaming Integration

**CRITICAL INSIGHT:** We already have chunk streaming priority! Don't reinvent it.

**Instead of:** One global topic OR complex topic management  
**Use:** Tile-level topics + piggyback on existing ChunkStreamer

```rust
pub struct TopicManager {
    /// Currently subscribed topics (tile-level only, ~9 active)
    active_topics: HashMap<String, IdentTopic>,
    
    /// Player position IN-GAME (not real-world!)
    player_position: ECEF,
    
    /// Current tile and neighbors (3x3 grid = 9 tiles)
    subscribed_tiles: HashSet<u64>,
}

impl TopicManager {
    /// Subscribe to topics based on IN-GAME player position
    /// 
    /// IMPORTANT: player_position is where the character is in the game world,
    /// NOT where the player is physically (VPN, satellite, etc. are irrelevant)
    pub fn update_subscriptions(&mut self, position: ECEF) {
        let current_tile = position.to_tile_id();  // ~100km² tiles
        let neighbor_tiles = self.get_neighbor_tiles_3x3(current_tile);
        
        // Subscribe to current + 8 neighbors (total: 9 tiles)
        let needed_topics: HashSet<String> = neighbor_tiles.iter()
            .flat_map(|tile_id| vec![
                format!("state:tile_{}", tile_id),      // Player positions, entity state
                format!("events:tile_{}", tile_id),     // Voxel edits, object changes
            ])
            .collect();
        
        // Subscribe to new topics
        for topic in needed_topics.difference(&self.active_topics.keys().cloned().collect()) {
            self.subscribe(topic);
        }
        
        // Unsubscribe from topics we left (moved to different tile)
        for topic in self.active_topics.keys().filter(|t| !needed_topics.contains(*t)) {
            self.unsubscribe(topic);
        }
        
        self.subscribed_tiles = neighbor_tiles;
    }
}
```

**Integration with ChunkStreamer Priority:**

```rust
// ChunkStreamer already prioritizes chunks:
// Priority 1: Current chunk (player standing in)
// Priority 2: Chunks in direction of travel (prefetch)
// Priority 3: Chunks in view frustum (visible)
// Priority 4: Distant chunks (background)

// Just add network layer on same priorities:
impl NetworkChunkProvider {
    fn request_chunk(&mut self, chunk_id: ChunkId, priority: ChunkPriority) {
        match priority {
            ChunkPriority::Immediate => {
                // Player standing in this chunk - URGENT
                self.request_immediate(chunk_id);
            }
            ChunkPriority::Prefetch => {
                // Player moving toward this chunk - IMPORTANT
                self.request_prefetch(chunk_id);
            }
            ChunkPriority::Background => {
                // Visible but not urgent - NICE TO HAVE
                self.request_background(chunk_id);
            }
            ChunkPriority::Passive => {
                // Announce we have this chunk - SHARE WITH OTHERS
                self.announce_chunk_available(chunk_id);
            }
        }
    }
    
    fn request_immediate(&mut self, chunk_id: ChunkId) {
        // Query DHT: "Who has this chunk?"
        let providers = self.dht_query_chunk_providers(chunk_id);
        
        // Select best peer (lowest ping, highest bandwidth)
        let peer = self.select_best_provider(providers);
        
        // Request chunk data
        self.send_chunk_request(peer, chunk_id, urgent: true);
    }
}
```

**Topic structure (tile-level only):**
```
state:tile_12345         → Player positions, entity updates (coarse)
events:tile_12345        → Voxel edits, object changes (coarse)
global:announcements     → Global events (always subscribed)

Chunk-level requests: NOT topics, use DHT on-demand
  DHT query: "chunk:xyz:providers" → peer list
  Request from fastest peer
  Cache locally
```

**Subscription strategy (3x3 tile grid):**
```
Player in Tokyo (in-game position):
   │
   ├─ Current tile: tile_tokyo_central
   ├─ Subscribe to 9 topics (3x3 grid):
   │    state:tile_tokyo_north
   │    state:tile_tokyo_northeast  
   │    state:tile_tokyo_east
   │    state:tile_tokyo_southeast
   │    state:tile_tokyo_south
   │    state:tile_tokyo_southwest
   │    state:tile_tokyo_west
   │    state:tile_tokyo_northwest
   │    state:tile_tokyo_central (current)
   │
   └─ Total: ~18 topics (2 types × 9 tiles)

Player moves to Osaka (different tile):
   │
   ├─ Unsubscribe: 18 Tokyo topics
   └─ Subscribe: 18 Osaka topics
      (happens at tile boundary crossing)
```

**Bandwidth scaling (USER VALIDATED):**

```
EMPTY AREA (forest, desert):
  - Player alone
  - Position update: 50 bytes/sec
  - No other players to sync
  - Total: ~0.1 KB/s

MODERATE AREA (small town):
  - Player + 20 others
  - 20 position updates: 1 KB/s
  - Few voxel edits: 0.5 KB/s
  - Total: ~2 KB/s

DENSE AREA (city center):
  - Player + 500 others
  - 500 position updates: 25 KB/s
  - Many edits: 5 KB/s
  - Total: ~30 KB/s

EXTREME AREA (racetrack, concert):
  - Player + 10,000 others in grandstand
  - 10,000 position updates: 500 KB/s
  - Massive interaction: 100 KB/s
  - Total: ~600 KB/s = 0.6 MB/s
  
BUT: Only players AT the racetrack see this bandwidth
     Player in empty forest: Still 0.1 KB/s
     
BANDWIDTH SCALES WITH LOCAL COMPLEXITY, NOT GLOBAL PLAYER COUNT
```

**This is beautiful because:**
- ✅ Bandwidth scales with what you SEE, not total online players
- ✅ 1,000,000 players online doesn't matter if you're alone
- ✅ Uses existing ChunkStreamer priority (no new system)
- ✅ Tile topics are coarse (only ~18 topics active)
- ✅ Chunk requests are fine-grained (DHT on-demand)
- ✅ Passive sharing is lowest priority (doesn't block gameplay)

---

### 4. Content Request Protocol

**When you need a chunk:**
```rust
pub async fn request_chunk(&mut self, chunk_id: ChunkId) -> Result<ChunkData> {
    // Step 1: Check local cache
    if let Some(data) = self.chunk_cache.get(&chunk_id) {
        return Ok(data.clone());
    }
    
    // Step 2: Query DHT for providers
    let providers = self.find_chunk_providers(&chunk_id).await?;
    
    if providers.is_empty() {
        // Step 3: No providers found, generate deterministically
        return Ok(self.generate_chunk_deterministic(&chunk_id));
    }
    
    // Step 4: Request from closest/fastest provider
    let provider = self.select_best_provider(&providers);
    let data = self.request_from_peer(provider, chunk_id).await?;
    
    // Step 5: Validate (hash check or signature)
    if !self.validate_chunk(&chunk_id, &data) {
        return Err("Invalid chunk data");
    }
    
    // Step 6: Cache and announce we have it
    self.chunk_cache.insert(chunk_id, data.clone());
    self.announce_chunk_available(&chunk_id);
    
    Ok(data)
}
```

**Provider selection:**
```rust
fn select_best_provider(&self, providers: &[PeerId]) -> PeerId {
    providers.iter()
        .map(|peer| (peer, self.score_provider(peer)))
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(peer, _)| *peer)
        .unwrap()
}

fn score_provider(&self, peer: &PeerId) -> f32 {
    let mut score = 1.0;
    
    // Prefer peers we have direct connection with
    if self.is_directly_connected(peer) {
        score *= 2.0;
    }
    
    // Prefer peers with low latency
    if let Some(rtt) = self.peer_latency(peer) {
        score *= 1.0 / (1.0 + rtt.as_secs_f32());
    }
    
    // Prefer peers in same region (geography)
    if self.peer_region(peer) == self.my_region() {
        score *= 1.5;
    }
    
    score
}
```

---

## 🔄 Complete Discovery Flow

### Scenario: Alice joins the metaverse at lat/lon (37.7749, -122.4194) - San Francisco

**Step 1: Bootstrap (LAYER 1)**
```
Alice starts client
  │
  ├─ Try cached peers from last session
  │  └─ Connect to 2 peers instantly
  │
  ├─ Connect to bootstrap relays in parallel:
  │    • relay-us-west.metaverse.org  ✅ Connected (50ms)
  │    • relay-eu.metaverse.org       ✅ Connected (150ms)
  │    • relay-asia.metaverse.org     ⏱️ Timeout
  │
  └─ Connected to mesh (5 peers total)
```

**Step 2: Announce Presence (LAYER 2)**
```
Alice announces to DHT:
  │
  ├─ "region:sf_downtown:players" → Alice's PeerId
  ├─ "region:sf_downtown:chunks" → (none yet, just joined)
  └─ "user:alice:presence" → {lat:37.7749, lon:-122.4194, timestamp}
```

**Step 3: Subscribe to Topics (LAYER 3)**
```
Alice subscribes to regional topics:
  │
  ├─ "state:region_sf_downtown"
  ├─ "chunks:region_sf_downtown"  
  ├─ "players:region_sf_downtown"
  ├─ "state:region_sf_north"      (neighbor)
  ├─ "state:region_sf_south"      (neighbor)
  └─ "global:announcements"       (always)
```

**Step 4: Discover Nearby Players (LAYER 2)**
```
Alice queries DHT:
  │
  ├─ "region:sf_downtown:players" → [Bob, Charlie, Diana]
  │
  └─ Connect to them directly via P2P
```

**Step 5: Request Chunks (LAYER 2 + 3)**
```
Alice needs chunk at her spawn point:
  │
  ├─ Query DHT: "region:sf_downtown:chunks"
  │  └─ Providers: [Bob, Charlie] (they're in SF)
  │
  ├─ Request chunk from Bob (closest)
  │  └─ Receive chunk data + signature
  │
  ├─ Validate chunk hash
  │  └─ Store in cache
  │
  └─ Announce: "I now have this chunk"
```

**Step 6: Real-time Sync (LAYER 3)**
```
Alice edits a voxel (digs a block):
  │
  ├─ Publish to: "chunks:region_sf_downtown"
  │  └─ All subscribers (Bob, Charlie, Diana) receive update
  │
  └─ Publish to: "state:region_sf_downtown"
     └─ Player position updates flow here
```

**Step 7: Alice Moves to Oakland**
```
Alice walks/flies to Oakland (different region):
  │
  ├─ Unsubscribe from: "state:region_sf_downtown" (too far)
  ├─ Unsubscribe from: "chunks:region_sf_downtown"
  │
  ├─ Subscribe to: "state:region_oakland"
  ├─ Subscribe to: "chunks:region_oakland"
  │
  ├─ Announce to DHT: "region:oakland:players" → Alice
  │
  └─ Query DHT: "region:oakland:chunks" → [Eve, Frank]
     └─ Connect to Eve and Frank
```

---

## 📊 Scalability Analysis

### Traditional (Current) Approach
```
Global topic: "metaverse-state-sync"
  ├─ 1,000 players online
  ├─ Each publishes 10 updates/sec
  └─ Each client receives: 1,000 × 10 = 10,000 updates/sec
     └─ ❌ Doesn't scale beyond ~100 players
```

### Fluid Discovery Approach
```
Regional topics: "state:region_{id}"
  ├─ 1,000 players online (distributed globally)
  ├─ Average 5 players per region
  ├─ Each publishes 10 updates/sec
  └─ Each client receives: 5 × 10 = 50 updates/sec
     └─ ✅ Scales to millions (if evenly distributed)
```

### DHT Query Load
```
Alice queries: "Who has chunk X?"
  ├─ DHT lookup: O(log N) hops where N = total peers
  ├─ 1 million peers → ~20 hops
  ├─ Cache results for 5 minutes
  └─ Load: ~1 query per chunk = ~10 queries when entering new region
     └─ ✅ Very efficient
```

---

## 🛠️ Implementation Plan

### Phase 1: Bootstrap List (1-2 days)
- [ ] Create `BootstrapConfig` struct
- [ ] Load bootstrap list from config file
- [ ] Implement parallel relay connection
- [ ] Add peer caching (save/load from disk)
- [ ] Fallback to DNS seeds

### Phase 2: DHT Content Announcement (2-3 days)
- [ ] Design DHT key schema
- [ ] Implement content announcement (chunks, presence)
- [ ] Implement DHT queries (find providers)
- [ ] Add TTL refresh (re-announce every 5 min)
- [ ] Add cleanup (unannounce on disconnect)

### Phase 3: Dynamic Topics (2-3 days)
- [ ] Design region/tile ID system
- [ ] Implement `TopicManager`
- [ ] Auto-subscribe based on player position
- [ ] Auto-unsubscribe when leaving region
- [ ] Migrate from global topic to regional topics

### Phase 4: Content Request Protocol (2-3 days)
- [ ] Implement `request_chunk()` flow
- [ ] Add provider selection logic
- [ ] Add chunk validation (hash/signature)
- [ ] Cache management
- [ ] Fallback to deterministic generation

### Phase 5: Testing & Tuning (2-3 days)
- [ ] Test with 10 clients on localhost
- [ ] Test with 5 clients across LAN
- [ ] Test with clients on different continents
- [ ] Measure bandwidth usage
- [ ] Tune subscription radius, TTL, cache size

**Total: ~2 weeks**

---

## 🔍 Key Design Decisions

### 1. Region Size
**Question:** How big is a "region" for topic subscriptions?

**Options:**
- **Per chunk** (1km²): Too many topics, too much overhead
- **Per tile** (100km²): Good balance, ~10-50 chunks
- **Per grid** (1000km²): Too coarse, still too much traffic in cities

**Recommendation:** Start with tile-based (100km²), make configurable

### 2. Announcement Frequency
**Question:** How often to announce to DHT?

**Trade-off:**
- Too frequent: DHT network load
- Too infrequent: Stale data, can't find peers

**Recommendation:** 5 minutes (libp2p DHT default TTL is 24h, we refresh early)

### 3. Topic Subscription Radius
**Question:** How many neighbor regions to subscribe to?

**Options:**
- **1-ring** (9 regions): Low bandwidth, risk missing updates at borders
- **2-ring** (25 regions): Medium bandwidth, safer
- **Distance-based** (all within 10km): Varies by density

**Recommendation:** Start with 1-ring (9 regions), add config for power users

### 4. Bootstrap Node Count
**Question:** How many bootstrap relays to maintain?

**Recommendation:**
- Minimum 3 (one per major region: US, EU, Asia)
- Ideal 10-20 (community-run)
- Client connects to closest 2-3
- Community curates list (like Bitcoin DNS seeds)

---

## 🔐 Security Considerations

### 1. Sybil Attacks on DHT
**Risk:** Attacker creates many fake peers to pollute DHT

**Mitigation:**
- Require proof-of-work for announcements (lightweight)
- Rate limit DHT puts per peer
- Use signed announcements (ed25519)
- Prefer peers with verified history

### 2. Content Poisoning
**Risk:** Peer provides fake chunk data

**Mitigation:**
- Layer 1 terrain: Hash validation against SRTM data
- Layer 3 user edits: Signature validation
- Cache only validated chunks
- Blacklist peers that provide invalid data

### 3. Topic Spam
**Risk:** Attacker floods regional topics

**Mitigation:**
- Gossipsub has built-in flood protection
- Rate limit messages per peer per topic
- Reputation system (mute bad actors)

---

## 📈 Success Metrics

**Before (Current):**
- ✅ Works with 2-10 players on LAN
- ❌ Doesn't scale beyond ~100 players
- ❌ No cross-region discovery
- ❌ Single point of failure (one relay)

**After (Fluid Discovery):**
- ✅ Works with 10,000+ players globally
- ✅ Bandwidth per client scales with local density, not global count
- ✅ No single point of failure (distributed bootstrap)
- ✅ Can find content and players anywhere in the world
- ✅ Graceful degradation (works offline, syncs when connected)

---

## 🎯 Design Clarifications (From Discussion)

### 1. Region/Tile Size
**ANSWER:** Real-world geography tiles (100km² based on lat/lon)
- Uses real-world coordinates (SRTM data already does this)
- Tiles are fixed geographic regions
- Players in San Francisco subscribe to SF tile topics
- NOT in-game chunks (chunks are too small for topic granularity)

### 2. Decentralization Philosophy
**ANSWER:** As decentralized as possible, community-run relays
- Many people will run relays (community infrastructure)
- Hardcoded starting nodes for initial bootstrap
- BUT nodes must propagate relay/server availability dynamically
- Like magnet files updating tracker lists, except trackers can move
- Network must be self-healing and self-organizing

### 3. Rare Content & Cold Start Problem
**ANSWER:** Dedicated servers fill the gaps when few players online
- Early days: Few players, servers cache/serve most content
- Growth: More players, P2P takes over, servers do less
- Servers = persistent cache layer for unpopular regions
- Eventually: Servers only needed for very rare content or new player bootstrapping
- This is graceful scaling from server-heavy to pure P2P

### 4. Dynamic Bootstrap Node Discovery
**NEW REQUIREMENT:** Nodes need to share "who else is a good relay"
- Start with hardcoded bootstrap nodes (relay1, relay2, relay3...)
- Nodes announce themselves to DHT as available relays
- Nodes share relay lists with each other (relay gossip)
- If a relay goes offline, others propagate updated lists
- Clients refresh relay list every X minutes from DHT
- Like BitTorrent tracker lists that update themselves

**Implementation:**
```rust
// DHT key for relay discovery
"bootstrap:relays" → [relay1, relay2, relay3, ...]

// Every relay announces itself
relay.announce_to_dht("bootstrap:relays", my_multiaddr);

// Clients query for current relay list
let relays = dht.get("bootstrap:relays");

// Clients prefer relays with:
// - Low latency
// - High uptime (reputation)
// - Geographic proximity
```

### 5. Cache eviction:** When to drop chunks from cache? LRU? By distance?

### 6. Deterministic fallback:** Always generate terrain if no peer has it?

### 7. Cross-region latency:** If closest provider is 200ms away, accept it or wait?

---

## 📚 References

**Similar systems:**
- **IPFS:** DHT for content-addressed storage
- **BitTorrent:** Magnet links + DHT announces
- **Ethereum:** Kademlia DHT for node discovery
- **Yggdrasil:** Mesh routing with distance vectors
- **Freenet:** Distributed content routing

**Our unique requirements:**
- Content is **dynamic** (changes over time)
- Interest is **spatial** (location-based)
- Network is **graceful** (works at all bandwidths)

This is like IPFS + spatial indexing + graceful degradation.

---

**Status:** DESIGN COMPLETE - Ready for review and feedback

---

## 📝 UPDATED DESIGN CLARIFICATIONS

### Issue 1: What "Region" Actually Means ✅ RESOLVED

**CRITICAL MISTAKE IN ORIGINAL DESIGN:**
"Player in San Francisco subscribes to SF region topics"

**This is WRONG because:**
- San Francisco = real-world location
- In-game position might be Tokyo, Australia, anywhere
- VPN masks location, satellite moves 5000km while playing
- Real-world location is IRRELEVANT to game content needs

**CORRECT APPROACH:**
- "Region" = in-game character position (NOT real-world player location)
- Player character in Tokyo (game) → subscribe to `state:tile_tokyo`
- Peer selection = network metrics (ping, bandwidth, not geography)

**Example:**
```
Real-world: Player in USA with VPN through Netherlands
In-game: Character in Tokyo at lat/lon (35.6762, 139.6503)

Subscribe to topics:
  ✅ state:tile_tokyo (based on in-game position)
  ❌ NOT state:tile_usa (real-world is irrelevant)

Query DHT: "Who has chunks near Tokyo (in-game)?"
Response: [Alice: 20ms, Bob: 200ms, Charlie: 50ms]
Select: Alice (best network metrics)

Alice might be:
  - In Japan (low latency coincidence)
  - In USA on fiber (good connection)
  - Anywhere - doesn't matter, just fast
```

### Issue 2: Integration with Existing Systems ✅ RESOLVED

**CRITICAL INSIGHT:** We already have chunk streaming priority!

**Don't create new priority system - piggyback on ChunkStreamer:**

```rust
// ChunkStreamer already prioritizes (src/chunk_streaming.rs):
Priority 1: Current chunk (player standing in)
Priority 2: Direction of travel (prefetch for smooth movement)
Priority 3: View frustum (visible chunks)
Priority 4: Background culling (cleanup)

// Just add P2P layer on SAME priority queue:
Priority 1: Request from network URGENT (current chunk)
Priority 2: Prefetch from network IMPORTANT (smooth movement)
Priority 3: Background fetch NICE TO HAVE (fill in visible)
Priority 4: Announce availability PASSIVE (share with others)
```

**This is beautiful because:**
- ✅ No new priority logic needed
- ✅ Network requests match chunk loading priority
- ✅ Prefetching in direction of travel (smooth experience)
- ✅ Passive sharing doesn't block gameplay

### Issue 3: Bandwidth Scaling ✅ VALIDATED BY USER

**USER CONFIRMED:** Bandwidth scales with LOCAL complexity, not global player count

```
Empty forest (just you):
  - ~0.1 KB/s (only your position)

Small town (20 players):
  - ~2 KB/s (20 positions + few edits)

City center (500 players):
  - ~30 KB/s (500 positions + many edits)

Racetrack grandstand (10,000 players):
  - ~600 KB/s = 0.6 MB/s
  - But ONLY for players AT the racetrack!
  - Player in empty forest: still 0.1 KB/s
```

**This validates the hierarchical + on-demand design:**
- Tile-level topics (coarse) - only active players in tile
- Chunk-level DHT (fine) - on-demand when needed
- Bandwidth scales with what you SEE, not total online players

### Issue 4: Server Role During Growth ✅ RESOLVED

**USER CLARIFIED:** Servers fill gaps, then fade as P2P grows

```
Week 1:  Server serves 95% of chunks (few players, cold start)
Month 1: Server serves 60% of chunks (growing P2P coverage)
Year 1:  Server serves 20% of chunks (mostly P2P now)
Year 5:  Server serves 5% of chunks (rare content only)
```

**Server role:**
- Early: Main content source (few peers have content)
- Growth: Backup for unpopular regions (P2P handles popular areas)
- Mature: Fallback for rare content (P2P handles 95%)

**This is graceful scaling from centralized to decentralized**

### Issue 5: Self-Healing Bootstrap ✅ RESOLVED

**USER INSIGHT:** 
> "Like magnet files updating the peers, except sometimes our peers need to 
> update the location of the magnet file if that host is offline."

**Implementation:**
```
1. Hardcoded initial relays for cold start (relay1, relay2, relay3)
2. Relays announce to DHT every 5 minutes: "bootstrap:relays" → my_address
3. Clients query DHT every 5 minutes: "bootstrap:relays" → relay_list
4. If relay goes offline → TTL expires → removed from DHT
5. If relay changes IP → re-announces → clients get new address
6. Network self-heals without code changes
```

**Benefits:**
- ✅ Decentralized (anyone can run relay, announces to DHT)
- ✅ Self-healing (offline relays expire, network updates)
- ✅ Dynamic (relays can move IPs, clients auto-update)
- ✅ No central authority (community maintains via DHT)

---

## 🎯 Remaining Design Questions

### Cache Eviction Strategy
**Question:** When to drop chunks from local cache?

**Options:**
- LRU (Least Recently Used) - drop oldest accessed
- Distance-based - drop chunks far from player
- Hybrid - LRU + distance + usage frequency

### Deterministic Fallback
**Question:** If no peer has a chunk, always generate deterministically?

**For Layer 1 (terrain):**
- ✅ YES - deterministic from SRTM data + noise
- Always works offline
- Validate with hash when peer provides it

**For Layer 3 (user edits):**
- ❌ Can't generate - user-created content
- Must get from peer or server
- Server acts as persistent storage

### Voice Chat Routing
**Question:** How to route voice in this system?

**Options:**
- A) Voice in regional topics (simple, reuses infrastructure)
- B) Separate voice DHT routing (better QoS)
- C) Direct P2P after discovery (lowest latency)

**Defer until after base implementation**

---

**Status:** Design clarified and validated. Ready for phased implementation.
