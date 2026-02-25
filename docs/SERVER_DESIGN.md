# NODE ARCHITECTURE — Data Model & Capabilities

**Last Updated:** 2026-02-25
**Companion docs:** MESHNET_PLATFORM.md (overall platform), GRACEFUL_DEGRADATION.md (bandwidth tiers), NETWORKING_ARCHITECTURE.md (transport layer)

---

## THE THREE BINARIES

Three separate binaries because they have three different hardware targets:

```
metaverse-relay      — Pure network infrastructure
                       Runs on Android background, cheap VPS, future ESP32/LoRa node
                       No graphics, no terrain, no game loop
                       Target: 64 MB RAM, any CPU, no GPU

metaverse-server     — Always-on world state + web dashboard
                       Runs headless on a dedicated machine or VPS
                       No graphics, full world data, REST API, TUI
                       Target: 1–32 GB RAM, multi-core CPU, large disk

metaworld_alpha      — The game client (graphical)
                       Desktop player-facing
                       Full graphics pipeline, physics, game loop
                       Target: 8+ GB RAM, GPU required
```

They share the **same protocol**, the **same data model**, and the **same identity system**
(Ed25519 keypairs, signed operations, Kademlia DHT, gossipsub). What differs is what
each binary stores, serves, and forwards.

---

## 1. WHAT EACH BINARY STORES

| Data Type | Relay | Server | Client |
|---|---|---|---|
| **Chunk ops (voxel edits)** | Hot RAM cache (recent traffic) | Full world, all time, encrypted | Disk, configurable radius + budget |
| **Player positions** | Hot RAM cache | Archive | RAM, active session |
| **Chat logs** | Hot RAM cache | Full archive | Disk, configurable history |
| **Meshsite content** | Hot RAM cache, popular pages | All pages, all versions | Disk, pages visited (LRU) |
| **Key records** | All (tiny, ~1 KB each) | All | All |
| **DHT routing table** | Full | Full | Full |
| **Chunk terrain** | — | All regions configured | Loaded + configurable radius |
| **Bootstrap list** | In-memory | Authoritative copy, serves peers | In-memory |

### The cache farm principle (client)

Clients are already receiving all the data they need to be cache providers.
Walking through a chunk → you received the ops → optionally store them → announce as DHT provider.
Reading a forum post → you received the content → optionally pin it → serve to the next reader.

This is **opt-in and configurable**. The user sets a storage budget. Within that budget,
the client silently becomes a replication node for content it touches.
Beyond the budget, LRU eviction keeps it bounded. The user's own data is never evicted.

```json
"storage_budget_gb": 10,       // how much disk to donate to network cache
"cache_radius_chunks": 5,      // store ops for chunks within N-chunk radius
"cache_chat_days": 30,         // keep chat logs for N days
"pin_visited_content": true    // cache Meshsite pages visited
```

---

## 2. REPLICATION MODEL

**Full copies, not shards.** When content is replicated to R=5 nodes, each of those 5
nodes holds a **complete, independently usable copy**. No reconstruction needed.
If 4 nodes go offline simultaneously, the 5th still serves it immediately.

This is a deliberate choice over erasure coding (K-of-N shards):
- Erasure coding: storage-efficient but requires K nodes to reconstruct
- Full replication: more storage but any single surviving copy is sufficient
- For a network built around availability and censorship resistance: full replication wins

### How replication flows

```
Node A creates content (voxel op, chat, forum post, terrain chunk)
  → stored locally
  → ChunkId::dht_key() → Kademlia start_providing
  → gossipsub fans out to mesh members (D ≈ 6 peers)
  → each receiving node with budget → writes to disk → announces as provider
  → re-announces every 30 min (keeps DHT record alive)

New node joins:
  → queries DHT for nearby content hashes
  → finds providers → fetches full copy
  → if has budget → stores → announces
  → replication count climbs naturally
```

### Replication target

`redundancy_target = 5` (per node.json). The DHT provider count for any piece of
content is observable. If count < R and a node has spare budget, it proactively
re-replicates to peers with storage capacity.

---

## 3. NODE CAPABILITIES (advertised in DHT)

Every node advertises what it can do. Other nodes use this to make intelligent
routing decisions — prefer a server for large chunk fetches, prefer a relay for
circuit establishment, use a client cache as a secondary source.

```rust
pub struct NodeCapabilities {
    pub tier: NodeTier,               // Relay | Server | Client | Light
    pub available_storage_bytes: u64, // remaining budget willing to serve
    pub bandwidth_out_bps: u32,       // approximate outbound capacity
    pub always_on: bool,              // expected to be available 24/7
    pub regions: Vec<u32>,            // SpatialShard cell IDs covered (empty = global)
    pub version: [u8; 3],             // major.minor.patch
}
```

Stored as: `provider/capabilities/{peer_id}` → bincode in Kademlia DHT.

Routing logic:
1. **Chunk fetch**: prefer Server nodes (high bandwidth, always-on), fall back to Client caches
2. **NAT traversal / circuit**: prefer Relay nodes (lightweight, purpose-built)
3. **Meshsite content**: prefer Server nodes (authoritative), Client caches as CDN
4. **Geographic preference**: nodes advertising the same SpatialShard region first

---

## 4. RELAY BINARY (metaverse-relay)

**Role:** NAT traversal, gossipsub forwarding, DHT routing, hot-path caching.
No world state. No terrain. No graphics. Headless only.

**Hardware target:** 64–256 MB RAM. Runs on:
- Any cheap VPS (512 MB RAM, 1 CPU)
- Android phone as background service
- Raspberry Pi
- Future: ESP32 with LoRa radio (extremely stripped build, LoRa packet protocol)

**Storage:** RAM cache only (by default). Optionally a small disk cache for
frequently requested content (reduces re-fetch from servers under load).

**Config (relay.json):**
```json
{
  "port": 4001,
  "node_name": "my-relay",
  "max_circuits": 100,
  "max_peers": 200,
  "max_bandwidth_mbps": 0,
  "ram_cache_mb": 256,
  "disk_cache_gb": 0,
  "always_on": true,
  "identity_file": "~/.metaverse/relay.key",
  "web_enabled": true,
  "web_port": 8080
}
```

---

## 5. SERVER BINARY (metaverse-server)

**Role:** World state authority, Meshsite host, full archive, high-availability data node.
Headless. Web dashboard. TUI. No graphics.

**Hardware target:** 1–32 GB RAM, multi-core CPU, 10 GB – multi-TB disk.

**Storage:** Large disk. Configured regions of the world. Optionally full-world.
Meshsite content for hosted sites. Full chat + interaction archive.

**Config (server.json):**
```json
{
  "port": 4001,
  "node_name": "my-server",
  "always_on": true,
  "storage_budget_gb": 500,
  "world_dir": "~/.metaverse/world_data",
  "serve_regions": [],
  "redundancy_target": 5,
  "re_announce_interval_secs": 1800,
  "max_peers": 500,
  "max_bandwidth_mbps": 0,
  "cpu_shed_threshold_pct": 90,
  "ram_shed_threshold_pct": 85,
  "web_enabled": true,
  "web_port": 8080,
  "web_bind": "0.0.0.0",
  "tui_enabled": true,
  "identity_file": "~/.metaverse/server.key",
  "log_level": "info"
}
```

**Web dashboard:** Same structure as relay — `/`, `/peers`, `/storage`, `/world`, `/config`, `/logs`.

**API:**
```
GET  /api/status
GET  /api/peers
POST /api/peers/{id}/disconnect
POST /api/peers/{id}/block
GET  /api/storage
POST /api/storage/evict
GET  /api/config
POST /api/config
POST /api/shutdown
```

---

## 6. CLIENT STORAGE (metaworld_alpha)

The game client participates in replication **optionally and within a user budget**.
Nothing is stored beyond the user's own data without explicit opt-in.

Config (in node.json or metaverse settings UI):
```json
"storage_budget_gb": 10,
"cache_radius_chunks": 5,
"cache_chat_days": 30,
"pin_visited_content": true
```

When `storage_budget_gb > 0`:
- Chunk ops within `cache_radius_chunks` are stored to disk and announced in DHT
- Visited Meshsite pages are pinned (if `pin_visited_content`)
- Chat logs retained for `cache_chat_days`
- Client shows as a DHT provider for cached content → reduces server load
- Budget enforced via LRU eviction. User's own ops are never evicted.

When `storage_budget_gb = 0` (default): client stores nothing beyond its own data.
Fully self-contained, zero contribution to network storage. That's fine.

---

## 7. PHASE ROLLOUT

### Now — Servers carry the load
Dedicated server nodes store and serve all world data.
Clients cache what they touch (chunk ops already written to disk + announced in DHT).
Relay nodes handle NAT traversal.

### Near-term — Client participation
`storage_budget_gb` config enabled in client settings UI.
Clients announce as DHT providers for cached content.
Server load drops as client count grows.
Replication count climbs naturally above R=5 in active areas.

### Long-term — Servers become high-capacity peers
Dense client populations serve most content via gossipsub + DHT.
Servers are still valuable (large storage, high bandwidth, always-on) but not load-critical.
Bootstrap can be handled by relay nodes alone.
Onion routing adds metadata privacy (requires relay redesign — see MESHNET_PLATFORM.md section 3).


---
