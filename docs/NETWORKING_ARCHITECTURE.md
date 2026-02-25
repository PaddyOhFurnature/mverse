# Metaverse Networking Architecture

## Overview

This document defines the complete networking architecture for the planet-scale P2P metaverse.
It covers what data is sent, when, to whom, over what path, at what priority, and how the
system degrades gracefully across bandwidth constraints ranging from gigabit LAN to LoRa radio.

This builds on the existing spatial sharding design (SPATIAL_SHARDING_DESIGN.md) and controlled
chaos redundancy model (SCALABILITY_ARCHITECTURE.md). Read those first.

---

## The Core Question: What Data, When, To Whom

Every network event in the game falls into one of these categories:

| Event | Changed data | Recipients |
|-------|-------------|------------|
| Standing still | Nothing | Nobody — suppress entirely |
| Walking/looking | Player position + rotation | Peers with overlapping render distance |
| Place/dig block | VoxelOp (chunk X) | Peers subscribed to chunk X's topic |
| Toggle light | VoxelOp (light material, chunk X) | Peers subscribed to chunk X AND adjacent chunks within light radius |
| New peer connects | Chunk manifest | That peer only (point-to-point) |
| Player enters new chunk | Subscribe to new chunk topics, unsubscribe from distant ones | — |
| Someone walks near me | Their state update arrives | Me (they send, I receive — not my concern) |

**Key principle:** The sender is responsible for knowing who needs the data. The network layer
does not broadcast to everyone — it publishes to topic subscribers who have declared interest.

---

## Layer 1: Message Priority Classes

Every message type carries a priority class. This controls behaviour under bandwidth pressure.

```rust
pub enum MessagePriority {
    /// Presence beacon, auth tokens, peer identity.
    /// ALWAYS sent, even at LoRa speeds.
    Critical,

    /// Chat, voxel operations (block edits, lights, doors).
    /// Sent immediately. Never dropped. Queued if congested.
    High,

    /// Player position + rotation updates.
    /// Sent when delta exceeds threshold. Dropped if queue full.
    Normal,

    /// Chunk terrain transfers, asset preloads.
    /// Sent only when bandwidth headroom exists. Background only.
    Low,
}
```

### Bandwidth Tier Profiles

```rust
pub enum BandwidthProfile {
    /// ~250 bps (LoRa, ham radio mesh)
    /// Critical only: presence beacon + text chat
    /// Player position: suppressed
    /// Chunk terrain: suppressed
    LoRa,

    /// ~10 KB/s (dialup, satellite, degraded 4G)
    /// Critical + High + Normal (reduced rate: 2Hz position)
    /// Chunk terrain: suppressed
    Constrained,

    /// ~100 KB/s (normal 4G, NBN, cable)
    /// All tiers active. Position at 10Hz. Chunk terrain queued.
    Normal,

    /// Unlimited (LAN, gigabit)
    /// All tiers active. Position at 20Hz. Chunk terrain immediate.
    /// Preloading adjacent chunks.
    LAN,

    /// Automatic: measure actual throughput, classify, adapt.
    Auto,
}
```

Under `Auto`, the system measures round-trip time and throughput to the nearest relay every
30 seconds and reclassifies. Tier transitions are hysteretic (require 3 consecutive measurements
to upgrade, 1 to downgrade) to prevent flapping.

---

## Layer 2: Area of Interest (AOI) — What Gets Sent Where

### Per-Chunk Gossipsub Topics

Replace global topics with chunk-addressed topics:

```
player-state-{chunk_x}-{chunk_y}-{chunk_z}     # position updates for players in this chunk
voxel-ops-{chunk_x}-{chunk_y}-{chunk_z}         # block edits in this chunk
chunk-terrain-{chunk_x}-{chunk_y}-{chunk_z}     # terrain data for this chunk
```

Chat, presence, and state-request/response remain global topics (they're low volume and
need to reach all peers regardless of location).

### Render Distance Subscription Management

As a player moves through the world, the multiplayer layer maintains a sliding window of
subscribed chunk topics:

```
On position update:
  1. Calculate set of chunks within render distance (R)
  2. new_chunks  = current_chunks - previously_subscribed
  3. old_chunks  = previously_subscribed - current_chunks
  4. Subscribe to topics for new_chunks
  5. Unsubscribe from topics for old_chunks (after grace period of 5s)
  6. Update previously_subscribed
```

Grace period on unsubscribe prevents topic thrash when crossing chunk boundaries.

### Light Propagation

Lights are just VoxelOps with a light-emitting material. The light radius is bounded
(max 32 voxels = ~32m). Peers subscribed to the chunk containing the light source AND
any adjacent chunks that overlap the light radius will receive the op. Since render
distance >> light radius, any peer who can see the light is already subscribed to
the relevant chunk topics. No special case needed.

---

## Layer 3: Delta Filtering — When to Send

### Player State (Position/Rotation)

Replace 20Hz always-on with threshold-based delta:

```rust
const POSITION_DELTA_THRESHOLD: f64 = 0.05;   // 5cm
const ROTATION_DELTA_THRESHOLD: f32 = 0.017;  // ~1 degree
const MAX_SILENCE_INTERVAL: Duration = Duration::from_millis(500);
```

Logic:
- If `|new_pos - last_sent_pos| > POSITION_DELTA_THRESHOLD` → send
- If `|new_rot - last_sent_rot| > ROTATION_DELTA_THRESHOLD` → send
- If neither, but `elapsed > MAX_SILENCE_INTERVAL` → send keepalive (prevents peer timeout)
- Standing still for >500ms: send one update then suppress until something changes

At normal walking speed this reduces from 20Hz to ~10-15Hz. Standing still: 2Hz keepalive.
This is a ~5x reduction in player state bandwidth with zero perceivable impact.

### Chunk Terrain

Only send to peers who:
1. Are subscribed to that chunk's topic (in render distance)
2. Have an older `last_modified` timestamp than our copy (from manifest comparison)
3. Have not received this chunk from us in the last 60 seconds

---

## Layer 4: Geographic Relay Routing

### The Problem

Without routing intelligence, every peer maintains direct connections to every other peer.
A player in Australia talking to a player in Europe has:
- N Australian players × M European players = N×M relay circuits
- Each circuit is individually maintained across a high-latency link

### The Solution: Relay Aggregation

```
AU Player 1 ─┐                          ┌─ EU Player 1
AU Player 2 ─┤                          ├─ EU Player 2
AU Player 3 ─┼─► AU Relay ──────────► EU Relay ─┼─ EU Player 3
AU Player 4 ─┘    (aggregates)   (aggregates)    └─ EU Player 4
```

A single AU↔EU relay link carries all AU↔EU player traffic. The relay mesh is already built —
this optimisation just ensures clients prefer their geographically closest relay.

### Implementation

**Client relay selection:**
- Bootstrap nodes carry `region` tag (already in bootstrap.json: `"region": "AU"`)
- On startup, client measures latency to each bootstrap relay (3 pings)
- Connects to lowest-latency relay as primary, keeps one secondary
- Announces preferred relay in identify protocol so peers know how to reach us

**Relay-to-relay topology:**
- Relays dial each other (already implemented via `--peer` flag)
- Gossipsub naturally propagates through the relay mesh
- Geographic aggregation emerges from the topology: AU peers cluster on AU relay,
  EU peers cluster on EU relay, relay gossip carries cross-region traffic

**For now:** Relay selection is manual (bootstrap.json priority field). Auto-selection
based on measured latency is a future enhancement.

---

## Layer 5: Jitter Buffer & Dead Reckoning

### The Problem

Network packets arrive at irregular intervals. Without smoothing, other players'
avatars stutter and teleport. Current state: no smoothing at all.

### Jitter Buffer

Hold incoming player state updates in a timestamp-ordered buffer. Release them at a
fixed cadence 100ms behind the current "network time".

```
Packet arrives at t=0ms  (sent at sender's t=0)
Packet arrives at t=80ms (sent at sender's t=50ms) ← late
Packet arrives at t=110ms (sent at sender's t=100ms)

Without buffer: avatar jumps at t=0, stalls 80ms, jumps at t=80ms, etc.
With 100ms buffer: avatar moves smoothly at t=100, t=150, t=200 — one steady cadence
```

Buffer depth = 100ms. This adds 100ms of display lag (not input lag — your own character
is unaffected). For a metaverse (non-twitch game) this is imperceptible.

Peers with < 100ms RTT: perfectly smooth, always.
Peers with 100-200ms RTT: mostly smooth, occasional hiccup.
Peers with > 200ms RTT: reduced stutter compared to now, visible but tolerable.

### Dead Reckoning

Between received state updates, predict where the remote peer is:

```rust
predicted_position = last_known_position
    + last_known_velocity * elapsed_since_last_update
```

With velocity included in PlayerStateMessage (already there), this is straightforward.
Correct on next received update with a brief lerp (50ms blend) to avoid snapping.

Combined with the jitter buffer, remote avatars will appear to move continuously and
smoothly even at 5Hz update rates.

---

## Layer 6: Session Token (Hot-Path Optimisation)

PeerId is 39 bytes. It's included in every PlayerStateMessage (39% of the 100-byte packet).
After handshake, peers know each other's PeerIds. There is no need to repeat it.

Replace PeerId in hot-path messages with a 2-byte session token assigned at connect:

```
Handshake: peer announces PeerId (full 39 bytes) once
Session token: assigned by local node, 0x0001–0xFFFF (65535 peers max)
Hot path: PlayerStateMessage uses 2-byte token instead of 39-byte PeerId
Saving: 37 bytes per packet × 20Hz × 10 peers = 7.4 KB/s
```

Lookup table maps token → PeerId for auth and signing verification.

---

## Implementation Order

These are listed in dependency order — each builds on the previous.

### Phase 1: Delta Sending (immediate, isolated change)
- `src/multiplayer.rs`: Track last-sent position/rotation, suppress send if delta < threshold
- `src/multiplayer.rs`: 500ms keepalive when standing still
- **Benefit:** ~5x reduction in player state traffic, zero gameplay impact

### Phase 2: Jitter Buffer + Dead Reckoning (client-side receive path)
- New `src/jitter_buffer.rs`: Timestamped ring buffer, 100ms release delay
- `src/multiplayer.rs`: Feed incoming PlayerState into jitter buffer instead of direct apply
- `src/player_state.rs`: Dead reckoning extrapolation between updates
- **Benefit:** Smooth remote avatars regardless of connection quality

### Phase 3: Per-Chunk AOI Topics (gossipsub topic restructure)
- `src/multiplayer.rs`: Replace global topics with chunk-addressed topics
- `src/multiplayer.rs`: Subscribe/unsubscribe management as player moves
- `src/messages.rs`: Update topic name helpers
- **Benefit:** Players only receive data for chunks they can see. Scales to 10k+ concurrent.

### Phase 4: Message Priority + Bandwidth Profiles (congestion management)
- New `src/bandwidth.rs`: BandwidthProfile enum, throughput measurement, tier detection
- `src/multiplayer.rs`: Priority tagging on outbound messages, queue management
- `src/messages.rs`: Add priority metadata to message types
- User config: `bandwidth_profile` in relay.json / client config
- **Benefit:** System works on LoRa. Graceful degradation across all bandwidth tiers.

### Phase 5: Session Tokens (encoding optimisation)
- `src/messages.rs`: SessionToken type (u16), hot-path message variants
- `src/multiplayer.rs`: Session token assignment and lookup table
- **Benefit:** ~37 bytes saved per player-state packet

### Phase 6: Geographic Relay Routing (infrastructure)
- `src/bootstrap.rs`: Latency measurement to relay candidates
- `src/network.rs`: Primary/secondary relay selection, prefer nearest
- `bootstrap.json`: Expand region tags
- **Benefit:** Aggregates cross-region traffic through single relay links, reduces global latency

---

## Data Flow Summary

```
[Player input]
     │
     ▼
[Delta filter] ── no change ──► suppress
     │
     ▼ change detected
[Priority tag] → Critical/High/Normal/Low
     │
     ▼
[AOI filter] → which chunk topic(s)?
     │
     ▼
[Bandwidth tier check] → enough headroom for this priority?
     │ yes                              │ no
     ▼                                 ▼
[Nearest relay]                   queue / drop
     │
     ▼
[Relay mesh] → gossip to region subscribers
     │
     ▼
[Jitter buffer] (receive side, remote peers)
     │
     ▼
[Dead reckoning] → smooth interpolation
     │
     ▼
[Render]
```

---

## What This Achieves

| Scenario | Before | After |
|----------|--------|-------|
| Standing still | 2 KB/s wasted | 0.2 KB/s keepalive |
| Walking, 10 nearby players | 20 KB/s | 8-10 KB/s |
| Block edit | Broadcast to all | Chunk subscribers only |
| AU↔EU players | N×M relay circuits | 1 relay link, aggregated |
| Remote avatar on 80ms ping | Stutters | Smooth (jitter buffered) |
| 4G degraded connection | Chunk terrain causes lag | Terrain suppressed, positions only |
| LoRa / ham radio | Unusable | Presence + chat only |

---

## Relation to Existing Systems

- **spatial_sharding.rs** — Geographic region IDs already implemented. AOI topics
  use the same `RegionId` grid. Phase 3 extends this to per-chunk granularity.

- **chunk_streaming.rs** — Chunk load/unload events are the trigger for topic
  subscribe/unsubscribe in Phase 3.

- **vector_clock.rs** — CRDT ordering unchanged. Priority/AOI operate at the
  transport layer, not the CRDT layer.

- **messages.rs** — VoxelOperation signatures unchanged. Priority metadata is
  added as a wrapper, not embedded in the signed payload.

- **bootstrap.rs** — Region tags already exist in bootstrap schema. Phase 6
  adds latency measurement to relay selection.

---

## Non-Goals (Out of Scope for This Architecture)

- **Voice/Video:** Requires WebRTC, separate from libp2p stack. Noted for future work.
- **Asset streaming (textures, models):** Bitswap/IPFS-style protocol. Post-server-mode.
- **Server-authoritative anti-cheat:** Requires dedicated server node. Post-server-mode.
- **Sub-chunk voxel-level topics:** Over-engineered for current scale. Chunk-level is sufficient.
