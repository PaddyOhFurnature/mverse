# LOCAL-FIRST ARCHITECTURE PHILOSOPHY
## The Foundation of the Metaverse

**Created:** 2026-02-18  
**Purpose:** Core architectural philosophy — read this when you forget "what the hell you're doing"

---

## 🎯 The Core Principle

> **"Your world exists on YOUR machine first. The network is just a sync layer that operates at whatever level it can."**

This is not "multiplayer with offline mode."  
This is **"single-player with optional sync."**

---

## 🧠 The Mental Model

### Wrong Way to Think About It:
```
❌ Client → Server → Other Clients
   (depends on server being up)
   
❌ Multiplayer game with "offline fallback"
   (treats offline as degraded/broken state)
```

### Right Way to Think About It:
```
✅ Your Machine = Source of Truth
   Network = Sync Layer (best-effort)
   Other Peers = Mirrors (eventually consistent)
   
✅ Single-player game with "multiplayer enhancement"
   (treats network as optional quality improvement)
```

**You are not connecting to "the metaverse."**  
**You ARE the metaverse. Others are syncing with you.**

---

## 📚 The Snow Crash Parallel

Neal Stephenson described this in 1992:

> In *Snow Crash*, Hiro's home exists on his machine. The Street exists because everyone's machines collectively serve it. If your connection is bad, the Metaverse degrades:
> - **Cheap public terminals:** Black-and-white, low-res avatars
> - **Rich connections:** Full color, detailed avatars, complex buildings

**This project is the same idea:**
- **"Home"** = Your owned parcels (stored locally, always available)
- **"The Street"** = Shared world (P2P served, fidelity depends on bandwidth)
- **"Cheap terminal"** = Offline/2G mode (wireframe, text chat, queued sync)
- **"Rich connection"** = Fiber/5G (full AAA, real-time everything)

**The critical insight:** It's all the same client, same code. Data availability drives rendering fidelity.

---

## 🏗️ Architectural Consequences

This philosophy has deep technical implications:

### 1. Every System Needs a Degraded Mode

**Renderer:**
- Full: PBR materials, textures, lighting
- Degraded: Flat-shaded geometry
- Minimal: Wireframe outlines
- None: Grey fog

**Entities:**
- Full: Skeletal animation, clothing, facial expressions
- Degraded: Capsule with nametag
- Minimal: Dot on map
- None: Hidden

**Communication:**
- Full: Voice + text + emotes
- Degraded: Text only
- Minimal: Queued text (delivered on next burst)
- None: Local-only (no sync)

**Physics:**
- Full: Real-time sync (you see their jumps immediately)
- Degraded: Interpolated position updates (smooth but delayed)
- Minimal: Teleport to last-known position
- None: Ghost outline (frozen at disconnect)

### 2. Bandwidth is a Budget, Not a Requirement

Traditional multiplayer: **"You must have 1 Mbps to play"** ❌

This architecture: **"It works on ANY connection, just at different fidelity"** ✅

The BandwidthBudget allocator treats network capacity like a scarce resource:
- Measure available bandwidth
- Allocate to channels by priority
- Lower-priority channels gracefully degrade
- Player sees fidelity reduction, not error screens

### 3. Local Storage is Not Optional

The cache is not a "performance optimization."  
The cache is **the foundation of offline mode.**

```
~/.metaverse/
├── parcels/       # Your owned land (full data, always available)
├── cache/         # Visited areas (last-known state)
├── op_queue/      # Your edits (waiting to sync)
└── identity.key   # Your cryptographic identity
```

**Workflow:**
1. You dig a voxel → Applied locally, queued for sync
2. Network available → Sync op to peers
3. Network unavailable → Op stays in queue
4. Network returns → Queue flushes, CRDT merge resolves conflicts

This is **asynchronous collaboration**, like Git for voxels.

### 4. CRDT is Not Just for Conflicts

Most multiplayer games use CRDT to resolve "two players edited same thing" conflicts.

This architecture uses CRDT for **offline sync:**
- You edit for 3 hours with no connection
- Friend edits same area while you're offline
- You reconnect
- CRDT merge: both sets of edits converge to deterministic state
- No data loss, no manual conflict resolution

**It's not "multiplayer" — it's "collaborative editing with arbitrary latency."**

### 5. Determinism is Non-Negotiable

In client-server games, server is source of truth.  
In P2P, **all peers must compute identical results.**

**Already deterministic ✅:**
- ECEF coordinate math (Cartesian, no precision drift)
- Rapier physics (fixed timestep, ordered operations)
- DDA voxel raycast (integer math)
- Marching cubes (deterministic triangulation)
- CRDT merge rules (pure function)

**Must fix ⚠️:**
- HashMap iteration (use BTreeMap or sort keys)
- Random number generation (seeded RNG, sync seeds)
- Floating-point edge cases (consistent compiler flags)
- System timestamps (use Lamport clocks)

**Test strategy:**
- Record input sequence on Client A
- Replay on Client B
- Compare states byte-for-byte
- Hash mismatch = determinism bug

---

## 🔢 The Six-Priority Bandwidth Stack

Every piece of data has a priority. Lower priorities degrade when bandwidth shrinks:

```
Priority 1: LOCAL STATE (zero bandwidth)
  Your parcels, edits, identity
  Stored on disk. Works offline. Works forever.
  ✅ ALWAYS WORKS

Priority 2: STATE SYNC (1-5 KB/s)
  Player positions, voxel changes, text messages
  Works on ANY connection (dial-up, 2G, packet radio)
  Examples: "Player X at Y" = 50 bytes
            "Voxel changed" = 32 bytes

Priority 3: VOICE (10-50 KB/s)
  Opus codec: 6-12 KB/s per speaker
  Works on 3G+
  Drops first when bandwidth shrinks

Priority 4: GEOMETRY (50-500 KB/s)
  Chunk meshes, building data
  Works on 4G+
  Without this: wireframe or last-cached-state

Priority 5: TEXTURES (500 KB/s - 5 MB/s)
  PBR materials, normal maps
  Works on fast broadband, 5G
  Without this: flat colors

Priority 6: RICH MEDIA (5-50 MB/s)
  Video on TVs, complex animations
  Works on fiber, fast 5G
  Without this: static images
```

**Every layer is optional except Priority 1.**

---

## 🌐 What the Player Experiences

### Fiber/5G (All 6 Priorities)
- World looks like AAA game
- Other players fully animated
- TVs play videos, doors animate
- Voice chat works
- Voxel edits appear instantly

### 4G/WiFi (Priorities 1-4)
- World geometry present but textures low-res
- Other players are simple capsules
- Voice chat maybe choppy
- Voxel edits delayed 1-5 seconds

### 2G/Satellite (Priorities 1-2)
- Your area: full detail (from cache)
- Beyond: wireframe or old snapshot
- Other players: dots on map
- Text chat works (queued)
- No voice, no real-time geometry

### Offline (Priority 1 Only)
- Your area: fully functional
- Beyond: frozen at last session
- Other players: gone or ghost outlines
- Edits queue locally
- CRDT merge when connection returns

**The player never sees "Connection Failed" — just different visual fidelity.**

---

## 🛠️ Implementation Implications

### For Rendering:
```rust
pub enum DataAvailability {
    Full,           // Local data or full cache
    GeometryOnly,   // Have mesh, no textures
    StateOnly,      // Last-known position, no mesh
    Nothing,        // Never seen this area
}

fn render_chunk(chunk: &Chunk, availability: DataAvailability) {
    match availability {
        Full => render_full_fidelity(chunk),
        GeometryOnly => render_flat_shaded(chunk),
        StateOnly => render_wireframe(chunk),
        Nothing => render_fog(chunk),
    }
}
```

### For Networking:
```rust
pub struct BandwidthBudget {
    available_bps: u64,
    channels: Vec<SyncChannel>,  // Ordered by priority
}

impl BandwidgetBudget {
    fn allocate(&mut self) {
        let mut remaining = self.available_bps;
        for channel in &mut self.channels {
            if remaining >= channel.min_bps {
                channel.enabled = true;
                remaining -= channel.allocated_bps;
            } else {
                channel.enabled = false;  // Gracefully degrade
            }
        }
    }
}
```

### For Storage:
```rust
// Your edits always saved locally FIRST
fn dig_voxel(&mut self, coord: VoxelCoord) {
    // 1. Apply locally (instant feedback)
    self.region.set_voxel(coord, Material::Air);
    
    // 2. Write to op queue (durable storage)
    let op = VoxelOperation::new(coord, Material::Air, &self.identity);
    write_to_op_queue(&op);
    
    // 3. Try to sync (best-effort)
    if self.network.is_connected() {
        self.network.broadcast("state-sync", &op);
    }
    // If not connected: op stays in queue, syncs later
}
```

---

## 🎯 Why This Architecture Wins

### Traditional Multiplayer (Client-Server):
```
❌ Requires constant connection
❌ Server costs scale with players
❌ Server is single point of failure
❌ Player has no control over their data
❌ Game dies when servers shut down
```

### This Architecture (Local-First P2P):
```
✅ Works offline (your areas always available)
✅ Zero server costs (P2P only)
✅ No single point of failure (decentralized)
✅ Player owns their data (local storage)
✅ Game never dies (peers can persist it)
✅ Graceful degradation (works on ANY connection)
```

### Bonus Properties:
- **Censorship-resistant** (no central authority)
- **Privacy-respecting** (data on your disk, not company servers)
- **Archival by default** (every peer is a backup)
- **Community-owned** (players host relay nodes, bootstrap nodes)

---

## 🚨 Common Misconceptions

### "This is just P2P multiplayer"
**No.** Traditional P2P multiplayer still requires all players online simultaneously.

This is **asynchronous collaboration** — you can edit for hours offline, friend edits same area, you both reconnect → CRDT merge → consistency.

### "Offline mode is a fallback"
**No.** Offline is the **default mode.** Network is the enhancement.

Your parcels exist on your disk. They don't "come from the server." They ARE the source of truth.

### "Cache is just for performance"
**No.** Cache is the **foundation of the architecture.**

Without cache:
- Offline mode wouldn't work (no local data)
- Bandwidth budget wouldn't work (nothing to fall back to)
- Graceful degradation wouldn't work (no low-fidelity render path)

### "CRDT is just for conflict resolution"
**No.** CRDT enables **time-shifted collaboration.**

Two players edit same voxel:
- With different latencies → CRDT handles it
- While one is offline → CRDT handles it
- Days apart with delayed sync → CRDT handles it

It's Git for voxels, not just "multiplayer anti-cheat."

---

## 📖 Further Reading

### Papers:
- "Local-First Software" (Kleppmann et al., 2019)
- "A comprehensive study of CRDTs" (Shapiro et al., 2011)
- "Time, Clocks, and the Ordering of Events" (Lamport, 1978)

### Books:
- *Snow Crash* (Neal Stephenson, 1992) — The vision
- *Designing Data-Intensive Applications* (Martin Kleppmann) — The engineering

### Projects:
- **IPFS** — Content-addressed storage (inspiration for chunk distribution)
- **Dat/Hypercore** — P2P data sync (inspiration for op logs)
- **Automerge** — CRDT library (inspiration for merge logic)

---

## 💡 The One-Sentence Summary

> **Your machine is the metaverse; the network is just a mirror.**

Everything else follows from this.

---

**Remember:** When you're 6 months into implementation and wondering "what the hell am I doing," read this document. The architecture makes sense. The philosophy is sound. Trust the process.
