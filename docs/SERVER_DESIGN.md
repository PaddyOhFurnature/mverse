# NODE ARCHITECTURE — Unified Node Model

**Last Updated:** 2026-02-25
**Supersedes:** old SERVER_DESIGN.md (separate binaries thinking)
**Companion docs:** MESHNET_PLATFORM.md (overall platform), GRACEFUL_DEGRADATION.md (bandwidth tiers), NETWORKING_ARCHITECTURE.md (transport layer)

---

## THE CORE IDEA

Every participant runs the **same binary**. The difference between a "client", a "relay",
and a "server" is nothing more than how much disk you've allocated and whether you leave
it running 24/7. There is no special server code. There is no privileged relay code.
One binary, one protocol, one network.

```
                    ┌─────────────────────────────────────┐
                    │        metaworld_alpha               │
                    │   (the one and only binary)          │
                    │                                      │
                    │  NodeTier = Light | Client | Relay   │
                    │                │  | Server           │
                    │                │                     │
                    │  configured by node.json + CLI flags │
                    └─────────────────────────────────────┘
```

**Why this matters:** In the early network there are a handful of always-on server nodes.
As client count grows, each client passively absorbs storage and forwarding load. By the
time there are thousands of clients, the dedicated servers are optional — the clients ARE
the network. No migration needed. It was always the same code.

---

## 1. NODE TIERS

```
NodeTier::Light   — no persistent storage, RAM cache only
                    Mobile phones, IoT sensors, LoRa edge nodes.
                    Never expected to serve data to others.
                    Storage budget: 0 GB disk, 64–256 MB RAM.

NodeTier::Client  — user-configurable disk budget, caches what it touches
                    The default for desktop players.
                    Stores: nearby chunks (configurable radius), chat logs,
                    player interactions from active sessions.
                    Passively becomes a cache provider for content it reads.
                    Storage budget: 0–50 GB disk (user sets it), rest in RAM.

NodeTier::Relay   — always-on, RAM + small disk, hot-path forwarding
                    A VPS with 1 CPU and 1 GB RAM. Pure network infrastructure.
                    Stores: routing table, hot content cache (last N GB of traffic),
                    no world state beyond what recently passed through.
                    Storage budget: 1–10 GB disk, 512 MB–2 GB RAM.

NodeTier::Server  — large disk, full replication, high availability
                    Always-on, high-bandwidth, large storage.
                    Stores: full world state for configured regions, full Meshsite
                    content, complete chat/interaction archives.
                    Storage budget: 10 GB – multi-TB disk, 4–32 GB RAM.
```

---

## 2. WHAT EACH TIER STORES

### Data types and ownership

| Data Type | Light | Client | Relay | Server |
|---|---|---|---|---|
| **Chunk ops (voxel edits)** | RAM only, nearby | Disk, configurable radius | Hot cache (recent) | Full world, all time |
| **Player positions** | RAM, active session | RAM, active session | Hot cache | Archive |
| **Chat logs** | RAM, current session | Disk, configurable history | Hot cache | Full archive |
| **Meshsite content** | RAM, pages open now | Disk, pages visited (LRU) | Hot cache, popular pages | All pages, all versions |
| **Key records** | All (tiny, 1KB each) | All | All | All |
| **DHT routing table** | All nodes | All nodes | All nodes | All nodes |
| **Chunk terrain** (base + edits) | RAM, loaded only | Disk, loaded + radius | — | All regions configured |
| **Bootstrap list** | In-memory | In-memory | Serves to new peers | Authoritative copy |

### The cache farm principle

Clients are already receiving everything they need to be cache providers:
- Walking through a chunk → you have the ops → store them → announce as DHT provider
- Reading a forum post → you have the content → pin it → serve to the next reader
- Chatting → you have the log → keep it → searchable offline, restorable by peers

This happens **automatically within the configured budget**. The user sets:
```
storage_budget_gb: 10   # use up to 10 GB for network cache
cache_radius_chunks: 5  # store ops for chunks within 5-chunk radius of my position
cache_chat_days: 30     # keep chat logs for 30 days
pin_visited_content: true  # pin Meshsite pages I visit
```

When approaching the budget limit, eviction is LRU — oldest least-accessed data goes first.
User data (your own voxel edits, your own chat) is never evicted.

---

## 3. REPLICATION MODEL

**Full copies, not shards.** When content is replicated to R=5 nodes, each of those 5
nodes has a **complete, independently usable copy**. No reconstruction needed. If 4 nodes
vanish simultaneously, the 5th still serves it instantly.

This is different from erasure coding (where you need K-of-N shards to reconstruct).
Erasure coding optimises for storage efficiency. Full replication optimises for
availability and simplicity. We optimise for availability — the whole point is the
network works even when half of it is gone.

### How replication works in practice

1. **Node A** creates content (voxel op, chat message, forum post, terrain chunk)
2. Node A writes it locally + announces as DHT provider for `content_id`
3. Gossipsub propagates to all `D` mesh members (typically 6 peers)
4. Each receiving node that has **remaining storage budget** writes to disk + announces
5. After N minutes, each provider re-announces (keeps DHT record alive)
6. New node joining: queries DHT for nearby content → finds providers → fetches → stores if budget allows

### Replication target (R)

`redundancy_target = 5` (configurable in `node.json`)

The network aims for 5 independent copies of every piece of content. DHT provider
counts are observable — if a piece of content has fewer than R providers, nodes
that have budget capacity will proactively re-replicate to new peers.

### Content addressing

Every piece of content is addressed by `SHA-256(content_bytes)`. The DHT key is
`content_hash`. This means:
- Content is verifiable anywhere (hash it, compare)
- Duplicates are automatically deduplicated (same hash = same content)
- No single authority controls what "exists" — if you have the hash, you can find it

---

## 4. NODE CAPABILITIES ADVERTISEMENT

Every node advertises its capabilities in the Kademlia DHT alongside its PeerId.

```rust
pub struct NodeCapabilities {
    /// What tier this node operates at
    pub tier: NodeTier,

    /// Remaining storage capacity this node is willing to serve (bytes)
    pub available_storage_bytes: u64,

    /// Approximate bandwidth capacity (bytes/sec outbound)
    pub bandwidth_out_bps: u32,

    /// Whether this node is expected to be always-on
    pub always_on: bool,

    /// Geographic regions this node covers (for spatial routing)
    /// Empty = global (no regional preference)
    pub regions: Vec<u32>,  // SpatialShard cell IDs

    /// Node software version
    pub version: [u8; 3],  // major, minor, patch
}
```

Stored in DHT as: `provider/capabilities/{peer_id}` → bincode-encoded `NodeCapabilities`

Clients use this when deciding who to request data from:
1. **Prefer servers** for initial chunk fetches (high bandwidth, always-on)
2. **Use clients** as secondary sources (lower bandwidth but fine for redundancy)
3. **Avoid Light nodes** for large fetches (they may not have the data)
4. **Geographic preference**: nodes covering the same region first

---

## 5. CONFIGURATION (node.json)

One config file covers all tiers. Tier is set by `node_tier`.

```json
{
  // ─── Identity ─────────────────────────────────────────────
  "identity_file": "~/.metaverse/identity.key",
  "temp_identity": false,

  // ─── Node Role ────────────────────────────────────────────
  "node_tier": "client",        // "light" | "client" | "relay" | "server"
  "node_name": "my-node",
  "always_on": false,            // true = advertise as always-on in DHT

  // ─── Network ──────────────────────────────────────────────
  "port": 4001,
  "external_addr": null,        // set if behind NAT with known external IP
  "max_peers": 50,              // 500 for relay/server
  "max_bandwidth_mbps": 0,      // 0 = unlimited

  // ─── Storage Budget ───────────────────────────────────────
  "storage_budget_gb": 10,      // total disk budget for network cache
  "world_dir": "~/.metaverse/world_data",
  "cache_radius_chunks": 5,     // store chunk ops within this radius
  "cache_chat_days": 30,        // keep chat logs for N days
  "pin_visited_content": true,  // cache Meshsite pages visited
  "max_loaded_chunks": 200,     // RAM limit for loaded chunk octrees

  // ─── Replication ──────────────────────────────────────────
  "redundancy_target": 5,       // aim for R copies of everything we store
  "re_announce_interval_secs": 1800,  // re-announce DHT providers every 30min

  // ─── Server-only: regions to serve ────────────────────────
  // Empty = serve all regions. Set for geographic load distribution.
  "serve_regions": [],          // SpatialShard cell IDs (Level 1 cells)

  // ─── Relay Limits (relay + server tier) ───────────────────
  "max_circuits": 100,
  "max_circuit_duration_secs": 3600,
  "max_circuit_bytes": 1073741824,

  // ─── Load Shedding ────────────────────────────────────────
  "cpu_shed_threshold_pct": 90,
  "ram_shed_threshold_pct": 85,

  // ─── Web Dashboard (relay + server tier) ──────────────────
  "web_enabled": false,
  "web_port": 8080,
  "web_bind": "127.0.0.1",

  // ─── TUI ──────────────────────────────────────────────────
  "tui_enabled": true,          // auto-detected: true in terminal, false when piped

  // ─── Logging ──────────────────────────────────────────────
  "log_level": "info"
}
```

---

## 6. WEB DASHBOARD (relay + server tier)

Port: configurable (default 8080). Lightweight axum server.

### Pages
- `/` — status: tier, peers, uptime, bandwidth in/out, storage used/budget, CPU/RAM
- `/peers` — connected peers table: PeerID, tier, ping, data served/received
- `/storage` — what's stored: chunk count, content count, size breakdown, LRU candidates
- `/world` — world regions served, chunk op counts, save status
- `/config` — editable config (POST saves, hot reload via SIGHUP)
- `/logs` — live log stream (SSE)

### API (JSON)
```
GET  /api/status                 — health + stats
GET  /api/peers                  — peer list with capabilities
POST /api/peers/{id}/disconnect  — disconnect peer
POST /api/peers/{id}/block       — add to blocklist
GET  /api/storage                — storage stats + LRU list
POST /api/storage/evict          — manually evict LRU content
GET  /api/config                 — current config
POST /api/config                 — update + hot reload
POST /api/shutdown               — graceful shutdown
```

---

## 7. TUI DASHBOARD

Pages (all tiers that have a terminal):
- **[m] Main** — tier, peer count, bandwidth in/out, storage used, CPU/RAM, uptime
- **[p] Peers** — PeerID (short), tier, ping, data exchanged
- **[s] Storage** — content pinned, chunk ops stored, LRU cache status
- **[w] World** — regions covered, chunks loaded, ops/sec (server/client tiers)
- **[l] Log** — scrollable log with level filter
- **[h] Help** — keybindings, config path, version

---

## 8. DEPLOYMENT

All tiers run the same binary:

```sh
# Desktop client (default)
./metaworld_alpha

# Dedicated relay (headless, small VPS)
./metaworld_alpha --headless --tier relay --storage-budget 5 --port 4001

# Full server (large disk, always-on)
./metaworld_alpha --headless --tier server --storage-budget 500 --always-on --web-port 8080

# Light node (mobile, no storage)
./metaworld_alpha --tier light
```

Config file overrides defaults; CLI overrides config file.

### systemd unit (relay/server)
```ini
[Unit]
Description=Metaverse Node
After=network.target

[Service]
ExecStart=/usr/local/bin/metaworld_alpha --headless --config /etc/metaverse/node.json
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

---

## 9. PHASE ROLLOUT

### Now (v0.1.x) — Servers carry the load
- Dedicated server nodes store and serve all world data
- Clients cache what they touch (already implemented via DHT provider advertisement)
- Relay nodes handle NAT traversal
- `NodeCapabilities` advertisement lets clients pick good sources

### Near-term — Client participation
- `storage_budget_gb` config option enabled
- Clients announce as DHT providers for content they've cached
- Server load drops proportionally with client count
- Replication count naturally climbs above R=5 in active areas

### Long-term — Servers become optional
- Dense client populations serve all content locally via gossipsub + DHT
- Servers become "super-clients" — high bandwidth, large storage, but not special
- Bootstrap nodes can be relays (no world state needed)
- Onion routing adds metadata privacy layer (see MESHNET_PLATFORM.md section 3)

---

## 10. WHAT CHANGES IN THE CODE

### Immediate
- `NodeCapabilities` struct → advertised in DHT at startup + on change
- `node_tier` config field → gates which subsystems initialise
- Storage budget enforcement in `UserContentLayer` (evict LRU when over budget)
- `--headless` + `--tier` CLI flags in `metaworld_alpha`

### Near-term
- Content-addressed store: `store_content(hash, bytes)` / `fetch_content(hash)` API
- Proactive re-replication: if provider count < R, push to peers with available budget
- LRU eviction: track access times, evict oldest non-user-data first

### Later (onion routing prerequisites)
- Relay-tier circuit establishment protocol
- 3-hop onion encryption (ChaCha20-Poly1305 per hop)
- Anonymous DHT queries (query through relay circuits, not directly)


---
