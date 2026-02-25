# Graceful Degradation — The Data Always Gets Through

## READ THIS FIRST

> The data will win. If the internet dies, fall back to LAN.
> If LAN dies, fall back to dialup. If dialup is gone, use packet radio.
> If all you have is LoRa — 200 bytes, 250 bps — you still exist in the world.
> Your house is local. Your presence is a 200-byte packet. Your text message
> fits in that packet alongside it. The data always gets through.

This is not a feature. This is the architecture. Every system decision must
be evaluated against: "does this work over LoRa?" If the answer is no,
the data is too big and must be decomposed until it is.

---

## 1. The Philosophy

### 1.1 The Network Is Never "Down"

Conventional software treats network failure as an error state. This platform
treats it as a normal operating condition. There is always SOME channel available —
even if it's a USB drive carried across town. The question is never "is the network
up?" — it's "what can we send given what we have?"

```
Network state is not binary (up/down).
Network state is a bandwidth budget.
The client's job is to prioritise ruthlessly within that budget.
```

### 1.2 Your Home Exists Without a Network

Your chunk files are local. Your key is local. Your signed op-log is local.
When you open the client with zero connectivity, your home loads from disk.
You can walk around it. Every block you place is signed locally and queued.
When connectivity returns — any connectivity — the queue drains.

You are never "offline" in the sense of "the game doesn't work."
You are sometimes "local-only" in the sense of "your changes haven't propagated yet."

### 1.3 Presence Over Completeness

At every bandwidth tier, the priority is:
1. **You know I exist** (presence beacon — who I am, where I am)
2. **You can hear me** (text message)
3. **You can see me** (movement)
4. **You can see what I've done** (voxel ops, terrain changes)
5. **You can see the whole world** (full terrain sync)

Each tier adds richness. None of them are required for the ones above.
Over LoRa, 1 and 2 are always possible. Everything else waits.

---

## 2. The Bandwidth Stack

```
Tier 6: LAN / Gigabit
  ├─ Speed: 100 Mbps+
  ├─ Latency: <1ms
  ├─ What works: Everything. Full world. Live terrain sync. Video. Audio.
  └─ Compression: Optional (zstd for terrain still saves bandwidth and CPU)

Tier 5: Broadband (home fibre / cable)
  ├─ Speed: 10–100 Mbps
  ├─ Latency: 5–50ms
  ├─ What works: Full game experience. All features.
  └─ Compression: zstd terrain, delta position encoding

Tier 4: Mobile / 4G
  ├─ Speed: 1–10 Mbps (bursting), often constrained
  ├─ Latency: 20–100ms, variable
  ├─ What works: Full game. Terrain sync may be slower on large areas.
  └─ Compression: Aggressive. Tokenised peer IDs. Delta-only position.

Tier 3: Constrained (3G, satellite, poor mobile, VPN overhead)
  ├─ Speed: 100 kbps – 1 Mbps
  ├─ Latency: 100–600ms
  ├─ What works: Movement, chat, voxel ops. Terrain sync paused / on-demand.
  └─ Compression: Maximum. Session tokens. Binary protocol only. No terrain.

Tier 2: Very Constrained (dialup 56k, slow satellite, GPRS)
  ├─ Speed: 10–56 kbps
  ├─ Latency: 200–800ms
  ├─ What works: Presence, chat, op queue draining (1 op/sec). No terrain.
  └─ Compression: Tokenised everything. 1-byte action codes. VectorClock compressed.

Tier 1: Minimal (packet radio, slow HF, mesh radio)
  ├─ Speed: 1–10 kbps
  ├─ Latency: 1–30 seconds
  ├─ What works: Presence beacon (1/min). Text chat. Signed ops queued locally.
  └─ Compression: Bitpacked. 200-byte max packet size. Signatures truncated or batched.

Tier 0: LoRa (868 MHz / 915 MHz, license-free)
  ├─ Speed: 250–5000 bps (spreading factor dependent)
  ├─ Latency: seconds to minutes
  ├─ Packet size: 200–255 bytes MAX (LoRa physical layer limit)
  ├─ What works: Presence + short text chat. That's it. But that's enough.
  └─ Compression: Extreme. See Section 4.
```

---

## 3. What Each Tier Sends

The client constantly measures available bandwidth and switches tiers dynamically.
Tier is not configured — it's detected. The bandwidth budget determines what
gets sent, in strict priority order.

### 3.1 Priority Layers

```
Priority 0 — ALWAYS (even LoRa)
  ├─ Presence beacon: "I am PeerID X, at position Y, timestamp Z, signed"
  └─ Text chat: "From X: <message>" — if message fits in remaining packet space

Priority 1 — Tier 2+ (dialup and above)
  ├─ Player movement: delta position + rotation
  ├─ Voxel op queue drain: signed ops the world hasn't seen yet
  └─ Key registry updates: new/updated KeyRecords

Priority 2 — Tier 3+ (constrained mobile and above)
  ├─ Object state: position of nearby objects
  ├─ On-demand chunk ops: "what happened in chunk X?" fetch
  └─ Forum/chat history sync: new posts since last seen

Priority 3 — Tier 4+ (good mobile / broadband)
  ├─ Terrain sync: compressed chunk data (zstd)
  ├─ Full world state: chunk manifests, missing ops
  └─ Asset sync: textures, models, blueprints

Priority 4 — Tier 5+ (broadband / LAN)
  ├─ Live terrain sync: immediate chunk updates as they happen
  ├─ Audio (future)
  └─ High-frequency position: 20Hz broadcast (current default)
```

### 3.2 Automatic Tier Switching

```rust
pub enum BandwidthTier {
    LoRa,         // < 1 kbps
    Minimal,      // 1–10 kbps (packet radio)
    VeryConstrained, // 10–56 kbps (dialup)
    Constrained,  // 56 kbps – 1 Mbps (3G/satellite)
    Normal,       // 1–10 Mbps (4G/broadband)
    LAN,          // 10 Mbps+ (local network)
    Auto,         // detect and switch (default)
}
```

The client measures round-trip time and throughput every 5 seconds.
If throughput drops below a tier threshold, it switches down immediately.
If throughput recovers above a higher tier for 30 seconds, it switches up.
Hysteresis prevents oscillation.

---

## 4. The LoRa Packet — 200 Bytes

This is the design constraint that keeps everything honest. If it doesn't fit
in 200 bytes, it needs to be rethought.

### 4.1 Presence + Chat Packet Layout

```
Byte  0:      Protocol version (1 byte) = 0x01
Byte  1:      Message type (1 byte)
              0x01 = Presence only
              0x02 = Presence + Text
              0x03 = Voxel op (no text)
              0x04 = Presence + Voxel op

Bytes 2–3:    Session token (2 bytes) — established at last full handshake
              Replaces the 38-byte PeerId for peers who know us already
              Fallback: 0x0000 = "I am new, here is my full PeerId below"

Bytes 4–9:    Position (6 bytes)
              x: int16 (metres, relative to last known anchor, cm precision with 0.01 scale)
              y: int16
              z: int16
              Range: ±327m from anchor. Anchor stored locally and updated periodically.

Byte 10:      Rotation (1 byte) — yaw only (0–255 maps to 0–360°)
              Pitch not critical for presence/chat.

Bytes 11–14:  Timestamp (4 bytes) — unix seconds (good until 2106)

Bytes 15–18:  Lamport clock (4 bytes) — causal ordering

Bytes 19–82:  Ed25519 signature (64 bytes)
              Signs bytes 0–18 (the presence fields above)

[Presence-only packet: 83 bytes total. 117 bytes remaining for payload.]

Bytes 83–N:   Payload (variable, up to 117 bytes)
              For text chat (0x02): 1 byte length + UTF-8 text
              117 bytes = 117 characters. Plenty for a chat message.
              For voxel op (0x03): see 4.2 below.

[Total: 200 bytes maximum]
```

### 4.2 Voxel Op in LoRa Budget

If a player places a block, that op needs to propagate. Within the 200-byte budget:

```
After presence header (83 bytes):

Byte 83:      Action code (1 byte)
              0x01 = SetVoxel, 0x02 = RemoveVoxel, etc.

Bytes 84–89:  VoxelCoord (6 bytes)
              x,y,z as int16 relative to current chunk anchor

Byte 90:      Material ID (1 byte) — 256 materials, enough for now

Bytes 91–96:  Op ID (6 bytes) — truncated UUID (first 6 bytes of UUIDv4)
              Collision probability at network scale: negligible

[Voxel op addition: 14 bytes. Total: 97 bytes. 103 bytes still spare.]
```

A presence beacon + voxel op + remaining space for a short message: **97 bytes**.
We're not even close to the limit with the essential data.

### 4.3 Tokenisation

Tokenisation is the key to fitting meaning into tiny packets.

**Session tokens:** Instead of `12D3KooWXyz...` (38 bytes), use a 2-byte token
established during the last full-bandwidth handshake. Both sides have a mapping
table: `token 0x0042 → 12D3KooWXyz...`. Saves 36 bytes per peer reference.

**Action codes:** Instead of serialising `Action::SetVoxel { coord, material }`,
use 1 byte for the action type (0x01) + minimal field encoding. Saves ~20 bytes
per op vs bincode serialisation.

**Position deltas:** Instead of absolute world coordinates (8+ bytes), send delta
from last known position. If you moved 3 metres, that's int8 × 3 = 3 bytes.
Saves 5+ bytes per position update.

**Dictionary compression for chat:** A pre-shared word dictionary (common English
words + metaverse-specific terms) mapped to 1–2 byte tokens. "hello" = 0xA1.
"your house" = 0xB2 0xC4. Short messages compress to 50–70% of raw UTF-8.

**Signature batching (Tier 1 / LoRa):** At extreme bandwidth constraints, a node
can batch multiple ops and send a single signature over the batch hash, rather
than 64 bytes per op. The batch is signed, and individual ops inherit validity
from the batch. Reduces signature overhead from 64 bytes/op to 64 bytes/batch.

---

## 5. Local-First — The Home Exists Without Network

### 5.1 What's Always Local

```
~/.metaverse/
  identity.key          Your key — always local, never sent anywhere
  identity.keyrec        Your public record
  chunks/               Your chunk files — loaded from disk, playable offline
    <chunk_id>.chunk    Every chunk you've loaded, stored locally
  op_log/              Your signed op queue
    pending/            Ops not yet propagated (queued for next connection)
    confirmed/          Ops seen by the network (safe to archive)
  key_cache/            Keys of peers you've met — usable offline
```

When you open the client with no network:
1. Your chunks load from disk → your home is there
2. You can walk around, place blocks, everything
3. All ops are signed and written to `pending/`
4. When connectivity returns, `pending/` drains in priority order

### 5.2 What "Offline Mode" Looks Like

At Tier 0 / LoRa, your experience is:
- Full 3D access to your locally-stored chunks (your home, nearby areas you've visited)
- Text chat with anyone in LoRa range (few km radius)
- Presence: the world knows you exist, where you are, that you're active
- Voxel ops you make are queued and will propagate when bandwidth improves
- You see other LoRa-range peers' presence beacons
- You do NOT see terrain you haven't loaded yet (it hasn't arrived)
- You do NOT see ops from peers outside LoRa range (they'll sync when bandwidth returns)

This is not a degraded experience. It is the experience appropriate to the
bandwidth available. The world is always there — just with different resolution
depending on your connection.

### 5.3 The Queue — Ops Always Propagate Eventually

```
Player places a block over LoRa:
  ├─ Op signed locally, written to pending/
  ├─ Presence beacon includes: "I have 1 pending op (hash: abc...)"
  │   (other LoRa-range peers note they should fetch this when bandwidth allows)
  ├─ If LoRa-range peer has better connectivity, they relay it
  │   (mesh forwarding — peers forward ops for each other)
  └─ When player's own connection improves, pending/ drains automatically

Player returns home after a week with no connectivity:
  ├─ pending/ may have 1000 ops (they were building all week)
  ├─ On reconnect, ops drain at current bandwidth rate
  ├─ Priority: voxel ops in chunk-ID order (others can merge them properly)
  └─ World converges to final state via CRDT merge (already implemented)
```

---

## 6. Routing Fallback Chain

The client tries connection methods in order. Each failed method falls back
to the next. None of this is manual — it's automatic.

```
Attempt 1: Direct P2P (mDNS + known peers)
  ├─ LAN peers auto-discovered via mDNS (already implemented)
  ├─ Known peers dialed directly
  └─ Fallback if: no route to peer

Attempt 2: Relay via circuit relay (already implemented)
  ├─ libp2p circuit relay through trusted relay nodes
  ├─ Works through CGNAT, NAT, firewalls
  └─ Fallback if: no relay reachable

Attempt 3: DHT peer discovery (already implemented)
  ├─ Kademlia DHT finds new peers to relay through
  └─ Fallback if: DHT bootstrap fails

Attempt 4: Offline mesh (future — Tier 1/0)
  ├─ Bluetooth / WiFi Direct to nearby devices
  ├─ Store-and-forward via mesh
  └─ Fallback if: even this fails

Attempt 5: Packet radio / LoRa (future — Tier 0)
  ├─ Hardware bridge: local LoRa transceiver
  ├─ Client detects hardware and switches to Tier 0 protocol
  └─ Everything in Section 4 applies

At every level: local-first. The client always works with what it has.
```

---

## 7. Data Architecture — Groups That Can Travel Independently

The reason this graceful degradation works is that data is deliberately grouped
into independently-transmissible units. Nothing is entangled in a way that
requires everything to arrive at once.

```
Group A: Identity (tiny, ~200 bytes, LoRa-capable)
  ├─ Presence beacon (who, where, when, signed)
  └─ KeyRecord updates

Group B: Social (small, ~1–5 kB per item, Tier 2+)
  ├─ Text chat messages
  ├─ Direct messages
  └─ Forum posts (text only)

Group C: Operations (medium, ~100 bytes per op, Tier 2+)
  ├─ Voxel ops (SetVoxel, RemoveVoxel, etc.)
  ├─ Object ops (place, move, remove)
  └─ Commerce ops (listings, contracts)

Group D: Manifests (medium, ~1–10 kB, Tier 3+)
  ├─ Chunk manifests (what ops exist for a chunk)
  └─ Content index updates

Group E: Terrain (large, ~50–500 kB per chunk, Tier 4+)
  ├─ Compressed chunk data (zstd, already implemented)
  └─ Terrain mesh updates

Group F: Assets (very large, Tier 5+)
  ├─ Textures, models, blueprints
  └─ Avatar data, audio
```

The client transmits Group A first, always. Then B if budget allows. Then C.
Then D. Then E. Then F. If the connection drops mid-Group C, Groups A and B
already made it. Nothing is lost. The world knows you exist and can hear you.

---

## 8. Encryption + Compression Stack

At every bandwidth tier, the same pipeline applies — just with more or less
aggressive settings:

```
Raw data
  │
  ▼
Tokenise (session tokens, action codes, position deltas, dictionary)
  │
  ▼
Serialise (custom binary format at Tier 0/1, bincode at Tier 2+)
  │
  ▼
Compress (zstd level 1–19; higher level at lower bandwidth)
  │
  ▼
Encrypt (ChaCha20-Poly1305 for transit; libp2p noise protocol already handles this)
  │
  ▼
Sign (Ed25519; 64 bytes; unavoidable but worth every byte — it's proof)
  │
  ▼
Send

At Tier 0 (LoRa): every byte is accounted for. The 64-byte signature is ~32%
of the budget. It is not negotiable. The signature is why the data is trusted.
Without it, anyone on the LoRa frequency can inject fake ops.
```

### 8.1 How Much We Can Actually Fit

Real examples at different tiers:

```
200-byte LoRa packet:
  83 bytes:  Presence header (session token + position + timestamp + Lamport + sig)
  117 bytes: "I built a wall today — 47 blocks placed" = 40 bytes text
             + 14 bytes per voxel op × 5 ops = 70 bytes
             + 7 bytes spare
  RESULT: Presence + short chat + 5 voxel ops. In 200 bytes.

1 kB dialup packet (burst):
  83 bytes:  Presence header
  200 bytes: 10 voxel ops
  400 bytes: 4 forum posts (100 chars each, compressed)
  300 bytes: 3 KeyRecord updates (compressed)
  RESULT: Full presence + significant world update in 1 kB.

10 kB mobile packet:
  Everything above + 5 chunk manifest entries + 1 compressed terrain delta
  RESULT: Starting to see the world change around you.
```

---

## 9. LoRa Hardware Integration (Future)

A client running on hardware with a LoRa radio (Raspberry Pi + LoRa hat,
or a custom embedded device) would:

```
1. Detect LoRa hardware via serial/USB
2. Switch to BandwidthTier::LoRa automatically
3. Use 200-byte packet format (Section 4)
4. Broadcast presence beacon every 30–60 seconds
5. Listen for other presence beacons on the LoRa frequency
6. Display nearby LoRa peers on a minimal UI (text-only)
7. Queue all other ops for when higher bandwidth returns
```

The metaverse node + LoRa radio becomes a standalone device. No internet.
No phone. Just power and the radio. You exist in the world, you can chat,
you can build locally, and your presence is real.

This is not a theoretical edge case. It is the design floor. Everything
above it is a bonus.

---

## 10. Connection to the Key System

Even at LoRa bandwidth, every packet is signed. The key is the identity.
The signature is the proof. Without it, the packet is noise.

The 64-byte Ed25519 signature takes 32% of a 200-byte LoRa packet.
It is worth every byte:

- Nobody can fake your presence beacon
- Nobody can inject voxel ops in your name
- Nobody can forge a chat message attributed to you
- The CRDT merge at the other end can trust the op because the signature verifies

Your key is a 32-byte file. It works over LoRa. It works on a Raspberry Pi.
It works with no internet. It is your identity in the world regardless of
what bandwidth the world has given you today.

---

## 11. Open Design Questions

**11.1 LoRa Frequency / Channel Plan**
Which frequency band? 868 MHz (EU), 915 MHz (AU/US), 433 MHz (global ISM)?
Multiple channels for different data groups (presence on one, chat on another)?

**11.2 Signature Batching Threshold**
At what point do we batch-sign ops vs. individually sign?
Suggested: batch when >3 ops in 1 packet. Individual otherwise.

**11.3 Mesh Forwarding**
When a peer can hear you over LoRa and has better upward connectivity,
should they automatically forward your ops? Opt-in vs. always-on?

**11.4 Presence Beacon Interval**
30 seconds at LoRa (battery life concern for mobile devices)?
Or 60 seconds? Or adaptive (faster when moving, slower when still)?

**11.5 Chunk Anchor Compression**
Position deltas work only if both sides agree on the anchor.
How often do we re-broadcast the absolute anchor? On chunk crossing?
Or every N minutes as a fallback?

---

*Status: Architecture vision — the design floor for all bandwidth decisions*
*Every feature, every message format, every protocol decision must be evaluated
against: "does this work in 200 bytes?" If not, decompose it until it does.*
