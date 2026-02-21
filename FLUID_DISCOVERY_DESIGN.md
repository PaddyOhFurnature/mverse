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
**Use:** Configurable bootstrap list with fallbacks

```rust
pub struct BootstrapConfig {
    /// Primary bootstrap nodes (community-run relays)
    pub relays: Vec<RelayAddress>,
    
    /// Fallback DNS seeds (relay1.metaverse.org → IP lookup)
    pub dns_seeds: Vec<String>,
    
    /// Last known good peers (cached from previous session)
    pub peer_cache: Vec<PeerId>,
    
    /// Try relays in parallel, use first N that respond
    pub parallel_bootstrap: usize,
}

pub struct RelayAddress {
    pub multiaddr: String,
    pub peer_id: String,
    pub weight: f32,      // Prefer certain relays
    pub region: String,   // Geographic hint (us-east, eu-west, etc.)
}
```

**Bootstrap process:**
1. Try cached peers from last session (instant reconnect)
2. Try primary relays in parallel (first 3 to respond)
3. Fallback to DNS seeds if relays fail
4. Keep trying until connected to at least 2 bootstrap nodes

**Decentralization:**
- No single point of failure
- Anyone can run a relay
- Client tries multiple, picks best
- Community maintains list (like Bitcoin/IPFS)

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

### 3. Dynamic Regional Topics

**Instead of:** One global topic  
**Use:** Topics per region/chunk zone

```rust
pub struct TopicManager {
    /// Currently subscribed topics
    active_topics: HashMap<String, IdentTopic>,
    
    /// Player position (drives topic subscriptions)
    player_position: ECEF,
    
    /// Subscription radius (how far to subscribe)
    subscription_radius_m: f64,
}

impl TopicManager {
    /// Subscribe to topics based on player position
    pub fn update_subscriptions(&mut self, position: ECEF) {
        let current_region = position.to_region_id();
        let neighbor_regions = self.get_neighbor_regions(current_region, self.subscription_radius_m);
        
        // Calculate which topics we should be subscribed to
        let needed_topics: HashSet<String> = neighbor_regions.iter()
            .flat_map(|region_id| self.topics_for_region(*region_id))
            .collect();
        
        // Subscribe to new topics
        for topic in needed_topics.difference(&self.active_topics.keys().cloned().collect()) {
            self.subscribe(topic);
        }
        
        // Unsubscribe from topics we left
        for topic in self.active_topics.keys().filter(|t| !needed_topics.contains(*t)) {
            self.unsubscribe(topic);
        }
    }
    
    fn topics_for_region(&self, region_id: u64) -> Vec<String> {
        vec![
            format!("state:region_{}", region_id),      // General state
            format!("chunks:region_{}", region_id),     // Chunk edits
            format!("players:region_{}", region_id),    // Player movement
        ]
    }
}
```

**Topic structure:**
```
state:region_12345       → Player positions, entity updates
chunks:region_12345      → Voxel edits, chunk modifications
players:region_12345     → Player join/leave, chat in region
voice:region_12345       → Voice chat for region
global:announcements     → Global events (always subscribed)
```

**Subscription strategy:**
```
Player at chunk (0, 0, 0)
   │
   ├─ Subscribe to: state:region_0  (center)
   ├─ Subscribe to: state:region_1  (north)
   ├─ Subscribe to: state:region_2  (south)
   ├─ Subscribe to: state:region_3  (east)
   └─ Subscribe to: state:region_4  (west)

Player moves to chunk (100, 0, 0)
   │
   ├─ Unsubscribe: state:region_4  (too far)
   └─ Subscribe: state:region_5     (new neighbor)
```

**Benefits:**
- Only receive updates for chunks near you
- Bandwidth scales with local complexity, not global player count
- Move to empty region = zero network traffic
- Move to crowded city = high traffic (but only for that region)

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

## 🎯 Open Questions for Discussion

1. **Region/Tile size:** 100km² tiles reasonable? Or should it adapt to density?

2. **Voice chat routing:** Put voice in regional topics, or separate DHT key?

3. **Rare content:** What if you're the ONLY peer with a chunk? How to ensure availability?

4. **Bootstrap centralization:** Accept some community-run relays, or 100% decentralized?

5. **Cache eviction:** When to drop chunks from cache? LRU? By distance?

6. **Deterministic fallback:** Always generate terrain if no peer has it?

7. **Cross-region latency:** If closest provider is 200ms away, accept it or wait?

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
