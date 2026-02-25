# MESHNET PLATFORM — Architecture & Full Roadmap

**Last Updated:** 2026-02-25
**Status:** Living architecture document
**Companion docs:** NETWORKING_ARCHITECTURE.md (transport detail), IDENTITY_SYSTEM.md (identity detail), SPATIAL_SHARDING_DESIGN.md (sharding detail)

---

## THE IDEA IN ONE PARAGRAPH

The metaverse is not a game that runs on the internet. It IS a network,
and the 3D world is its interface. When you run the game, you become a node.
You can walk around anonymously, or walk up to an in-game terminal and interact
with the content layer — forums, wikis, marketplace, profiles — all served from
the mesh. The mesh runs over whatever transport is available: internet, Tor, VPN,
cellular, anything. There is no canonical server. There is no single domain that
can be taken down. The network is the game and the game is the network.

---

## 1. DESIGN INVARIANTS

These rules cannot be broken by any design decision, ever.

```
1.  NO SINGLE POINT OF FAILURE
    Removing any node, including all bootstrap nodes, must not break
    the network for already-connected peers.

2.  CONTENT IS VERIFIABLE ANYWHERE
    Any piece of content can be verified by anyone who knows its hash,
    regardless of where they received it from or who served it.

3.  SELF-SOVEREIGN IDENTITY
    No authority grants identity. Generating a keypair IS creating an
    identity. No signup required to exist on the network.

4.  TRANSPORT IS IRRELEVANT
    The application layer must not care what carries the packets.
    Tor, TCP, UDP, cellular, LoRa, Bluetooth — all equivalent.

5.  PRIVACY BY DEFAULT
    What you read, build, and say is not visible to infrastructure
    operators unless you choose to make it public.

6.  UNIFIED DATA MODEL
    A forum post, a voxel edit, a marketplace listing, a profile update —
    all are SignedOperation. Same infrastructure, same trust model,
    same replication. One thing.
```

---

## 2. CURRENT STATE (What Already Exists)

### ✅ Foundation (Built)
- 3D voxel world at near-1:1 Earth scale (SRTM elevation, OSM data)
- Physics: gravity, collision, terrain streaming (Rapier 3D)
- P2P networking: libp2p full stack (TCP + QUIC, Gossipsub, Kademlia DHT, mDNS, Noise+TLS, Yamux, relay/DCUTR hole-punching)
- Key system: 8 key types (Genesis→Trial), Ed25519 signing, DHT-propagated KeyRecords
- Relay nodes: NAT traversal, gossipsub forwarding, binary release on GitHub
- Server nodes: REST API v1, web dashboard, key registry SQLite
- Multiplayer: player positions, voxel operations, state sync
- Bootstrap: static JSON + GitHub Gist relay list, relay-based discovery
- **The Construct**: bundled lobby scene, 6 Meshsite module rooms, world portal, signup terminal, debug HUD
- **GameMode separation**: Construct vs OpenWorld — terrain/physics gated, clean spawn
- Identity tiers redesigned: Trial(7)/Guest(6)/User(5)/Business(4) — wire-format stable discriminants
- Signup screen: egui overlay, all 4 flows, key file generation

### ❌ Platform Layer (Not Yet Built)
- No in-game terminal (no PTY renderer, no mesh browser)
- No content types beyond voxel ops (no forum/wiki/marketplace SignedOps)
- No onion routing (traffic is Noise-encrypted P2P but no anonymisation circuits)
- No transport agnosticism (TCP + QUIC only — no Tor, no WebRTC, no LoRa)
- No large-content distribution (no block-chunked DHT)
- No screen sharing / media streaming

---

## 2.5 DATA LAYER AUDIT (What's Built vs What's Wired)

The data infrastructure components exist in isolation. The **wiring** between them is incomplete.

| Component | Built | Wired | Notes |
|---|---|---|---|
| **Chunking** | ✅ ChunkId, ChunkManager, ChunkStreamer, 100³ voxels | ❌ | Not wired to DHT. Edits don't persist across sessions. |
| **P2P Transport** | ✅ Full libp2p stack (see above) | ✅ | Gossipsub messages, player state, voxel ops — all live |
| **DHT (Kademlia)** | ✅ Kademlia behaviour in swarm | ❌ | Chunk IDs not advertised as DHT providers. Peers don't fetch missing chunks from DHT. |
| **Compression** | ✅ zstd on all gossipsub messages | ❌ | Chunk terrain binary not compressed before P2P send |
| **Encryption in transit** | ✅ Noise protocol (all connections auth + encrypted) | ✅ | |
| **Encryption at rest** | ❌ | ❌ | Chunk files stored plaintext. ChaCha20 imported but unused. |
| **Sharding** | ✅ `spatial_sharding.rs`: L0–L3 geographic cells, `redundancy_target=5` | ❌ | Topology built but no runtime subscription/redistribution. |
| **Signed operations** | ✅ Ed25519 signing on all VoxelOps | ✅ | Verified on receive |
| **CRDT / vector clocks** | ✅ `vector_clock.rs`, CRDT merge semantics | ❌ | Peers don't persist received ops to disk. No quorum checks. |
| **Session tokens** | ❌ | ❌ | Ed25519 keypairs exist but no short-lived session tokens. PeerId repeated in every hot-path message (39 bytes). |
| **Onion routing** | ❌ | ❌ | Not started. Design in section 3 / Layer 2. |
| **Redundancy** | ✅ Architecture (`redundancy_target=5`, gossipsub propagation) | ❌ | No node actually writes received ops to disk. Data exists only on originating client. |

### The Core Wiring Gap

Everything feeds into `operations.json` (single global file) instead of per-chunk DHT:

```
Voxel edit → signed op → operations.json   ← works today
                       ↓
              DHT publish → peers store per-chunk  ← NOT WIRED
                       ↓
              Chunk file ← chunk streamer   ← NOT WIRED
```

The single highest-value fix: **replace global `operations.json` with per-chunk files stored and fetched via DHT**. Sharding, compression, redundancy, and replication all plug in around that.

### Next Data Layer Work (Sequenced by Dependency)

1. **Chunk→DHT wiring** — ChunkId advertised as DHT provider; peers request missing chunks from DHT, not regenerate locally
2. **Chunk persistence** — Per-chunk op files survive sessions; new joiner can fetch world state
3. **Replication** — On voxel edit, write received op to disk + gossip to `redundancy_target` peers
4. **Chunk compression** — zstd on chunk binary before DHT store/fetch
5. **At-rest encryption** — Key-derived encryption per parcel/chunk before writing to disk
6. **Session tokens** — Short-lived 2-byte tokens to replace PeerId in hot-path messages
7. **Onion routing** — 3-hop circuits through Relay key holders (see Layer 2 below)

---

## 3. THE FULL ARCHITECTURE

Seven layers, each buildable independently, each depending on the ones below it.

```
┌────────────────────────────────────────────────────────────────┐
│  LAYER 6 — MESHNET SERVICES                                    │
│  Mesh-hosted websites, email, video, arbitrary services        │
├────────────────────────────────────────────────────────────────┤
│  LAYER 5 — LARGE CONTENT DISTRIBUTION                         │
│  Block-chunked DHT, parallel fetch, integrity verification     │
├────────────────────────────────────────────────────────────────┤
│  LAYER 4 — IN-GAME TERMINAL + MESH BROWSER                    │
│  PTY renderer as 3D surface, TUI content browser              │
├────────────────────────────────────────────────────────────────┤
│  LAYER 3 — CONTENT TYPES + SIGNED OPS                         │
│  Forum/wiki/marketplace/profile as SignedOperation             │
├────────────────────────────────────────────────────────────────┤
│  LAYER 2 — ONION ROUTING (PEER-ROUTED)                        │
│  3-hop circuits through game peers, no dedicated relays        │
├────────────────────────────────────────────────────────────────┤
│  LAYER 1 — TRANSPORT ABSTRACTION                               │
│  TCP + Tor + WebRTC + cellular + LoRa + anything               │
├────────────────────────────────────────────────────────────────┤
│  LAYER 0 — FOUNDATION (EXISTING)                               │
│  libp2p, gossipsub, Kademlia DHT, signed ops, key system       │
└────────────────────────────────────────────────────────────────┘
```

---

### Layer 0: Foundation (Existing)

libp2p handles peer identity (peer ID = hash of public key), connection
multiplexing (yamux), security (noise protocol — all connections authenticated
and encrypted), gossipsub for broadcast, Kademlia for DHT lookups.

The peer ID is derived from the public key. The transport address is just HOW
you reach that peer ID, not WHO they are. Same peer, reachable via TCP today
and Tor tomorrow. This separation is already built in.

---

### Layer 1: Transport Abstraction

**Current state:** TCP only.

**Target state:** Any IP-capable transport, plus non-IP transports.

```
Transport          Status     Notes
──────────────────────────────────────────────────────────────────
TCP/IP             ✅ done    Current only transport
QUIC/UDP           📋 next    Lower latency, better for mobile
TLS                ✅ done    Via noise protocol (libp2p)
Tor (via SOCKS5)   📋 needed  Local tor daemon → SOCKS5 proxy → libp2p transport
                              Relay nodes publish .onion addresses
                              Censorship resistance, NAT bypass
WebRTC             📋 later   Browser-accessible peers, no app required
LoRa/Bluetooth     📋 future  Non-internet transports, text-only mode
```

**Implementation note:** libp2p has a pluggable `Transport` trait. Adding Tor means
detecting/starting a local Tor daemon and routing connections through its SOCKS5 proxy.
The rest of the stack doesn't change. Relay nodes publish their `.onion` address
alongside their TCP address in their KeyRecord.

**Bootstrap resilience:** Current bootstrap requires a known IP (GitHub Gist relay list).
Fix: peer exchange (PEX) — every peer caches its peer list locally after first connection.
Future runs reconnect via cached peers without any bootstrap server. DNS seeding for
truly first-time connections (multiple DNS operators, decentralised).

---

### Layer 2: Onion Routing (Peer-Routed — The Novel Part)

**This is where no one has been before.**

Tor has dedicated relay nodes run by volunteers and a directory authority that
certifies them. That creates a centralised chokepoint (the directory) and limits
who can contribute routing capacity.

This system routes through the game players themselves. Every Relay key holder
COMMITS to routing traffic. Regular players can opt in. The relays are the
mesh — not a separate network.

#### Circuit Construction

```
Node A wants to anonymously fetch content C.

Step 1: A selects 3 relay peers from its known peer set
        - R1: first hop (close to A in DHT keyspace = fast)
        - R2: middle hop (far from A = no correlation)
        - R3: exit hop (different continent if possible)
        - Relay key holders preferred; any peer with routing opt-in allowed

Step 2: A builds layered encryption (onion):
        outer  = encrypt({"forward": R2_addr, "payload": middle},  R1_pubkey)
        middle = encrypt({"forward": R3_addr, "payload": inner},   R2_pubkey)
        inner  = encrypt({"fetch": content_hash},                  R3_pubkey)

Step 3: A sends outer to R1
        R1 peels outer layer → sees "forward to R2" → forwards middle
        R2 peels middle layer → sees "forward to R3" → forwards inner
        R3 peels inner layer → sees "fetch content_hash" → fetches, returns

At no point does any relay know both:
  - Who asked (A's identity)
  - What was asked (content_hash)
```

#### Circuit Maintenance

Transient player peers create a reliability problem: if R2 logs off mid-circuit,
the circuit breaks. Solution: **tiered circuit design**.

```
Backbone hops (R1, R3):  Relay key holders only — long-lived, stable
Optional middle hops:     Any peer with opt-in — provides additional obfuscation
                          If they disconnect, circuit rebuilds without them
                          Circuit never DEPENDS on transient peers
```

A Relay key holder who goes offline has already proven they're a bad relay via
their uptime record. DHT-tracked uptime scores influence relay selection.

#### Spatial Correlation Resistance

Players near you in 3D space know you're in the area. DHT proximity doesn't
correlate with physical location. Circuit selection rules:

```
- First hop:  close in DHT keyspace (performance), but NOT nearby in 3D world
- Middle hop: far in DHT keyspace from both A and R1
- Exit hop:   different autonomous system (ISP) from A if detectable
```

---

### Layer 3: Content Types

All platform content is a `SignedOperation`. The existing voxel op infrastructure
handles signing, propagation, deduplication, and CRDT merge. Platform content
reuses all of that — just new `Action` variants.

#### New Action Variants (to add to messages.rs)

```rust
// Forum / discussion
PostContent     { topic_id, body: String, parent_id: Option<[u8;16]> }
EditContent     { target_op_id: [u8;16], new_body: String }
DeleteContent   { target_op_id: [u8;16] }
ReportContent   { target_op_id: [u8;16], reason: ReportReason }

// Identity / profile
UpdateProfile   { display_name: String, bio: String, avatar_hash: Option<[u8;32]> }

// Marketplace
CreateListing   { item_desc: String, price: u64, quantity: u32, images: Vec<[u8;32]> }
AcceptListing   { listing_op_id: [u8;16] }
CancelListing   { listing_op_id: [u8;16] }

// Wiki
PublishWikiPage { slug: String, title: String, body: String }
EditWikiPage    { page_id: [u8;16], new_body: String, summary: String }

// Governance
CreatePoll      { question: String, options: Vec<String>, duration_hours: u32 }
CastVote        { poll_op_id: [u8;16], option_index: u8 }

// Moderation
BanFromRegion   { target_peer_id: PeerId, region_id: RegionId, reason: String }
ModerateContent { target_op_id: [u8;16], action: ModerationAction }
```

Content gossipsub topics (to add to the existing topic set):
```
content/forum/<topic_id>    — forum posts for a specific topic
content/wiki                — wiki pages (global)
content/marketplace         — listings (global)
content/profile/<peer_id>   — profile updates for a specific peer
dm/<sha256(sorted_peer_ids)>— direct messages (end-to-end encrypted)
```

---

### Layer 4: In-Game Terminal

A physical object in the 3D world. Walk up to it, press E, the game captures
input and renders a terminal session onto the object's surface.

#### Technical Stack

```
Game process                    Terminal process
──────────────                  ─────────────────
player presses E                
  → spawn PTY (portable-pty)  → PTY child process: mesh-browser binary
  → PTY stdout → vte parser   ← terminal output (ANSI escape codes)
  → char grid + color state
  → render to wgpu texture
  → texture on surface mesh
  → keyboard input → PTY stdin → mesh-browser receives input
```

**portable-pty**: creates a real PTY pair. Child process gets a terminal it can
draw to. Parent gets the stream. Standard Rust crate, cross-platform.

**vte**: VT100/ANSI escape code parser. Turns a byte stream into a grid of
characters with foreground/background colors, bold/italic state, cursor position.
Standard Rust crate used by alacritty.

**Texture generation**: character grid → bitmap → upload to wgpu as a texture.
Font rendering: bitmap font (fast, no layout engine needed) or ab_glyph (more
flexible). Resolution: ~80 columns × 24 rows is readable on a in-world surface.

**Input routing**: when a terminal surface is "active" (player is at it):
- Keyboard events bypass the game input handler → PTY stdin
- Mouse captured for terminal use (cursor + click)
- Escape or walk away → deactivates terminal, returns input to game

#### Terminal Types in the World

```
Public terminals:   Anyone can use. Read content. No key required.
                    Placed in the lobby, major hubs, starting areas.

Registered terminals: Require key to post. Read is free.
                      Standard terminals in the main world.

Private terminals:  Owned by a player/org. Custom software.
                    Access can be restricted by key type or group.

Admin terminals:    Server/Admin key only. Moderation tools, region admin.
```

#### The Mesh Browser Binary

A separate binary (or a mode of the main binary) that:
- Connects to the same P2P network as the game (same gossipsub/DHT)
- Browses content by type: forum, wiki, marketplace, profiles, world map
- Posts content (if key permits)
- Navigation: keyboard-driven, ncurses-style
- Content fetched via gossipsub topics / DHT lookups
- Optionally routes through onion circuit (toggleable)
- Internet passthrough: can spawn the OS browser for non-mesh URLs
  (that session is entirely outside the game process — user's own security)

---

### Layer 5: Large Content Distribution

Small content (forum posts, profile updates, voxel ops) travels via gossipsub.
Large content (images, video, file downloads) needs block-based distribution.

#### Block Model

```
Content C (e.g. a 4MB image)
  → split into 256KB blocks: B1, B2, B3, ... Bn
  → each block Bi has address: SHA-256(Bi)
  → manifest M = { content_id, block_hashes: [hash(B1), hash(B2), ...], 
                   total_size, content_type, created_at }
  → manifest itself has address: SHA-256(canonical_bytes(M))

Publish:
  → store each block in DHT: hash(Bi) → Bi
  → publish manifest as a SignedOperation (small, goes via gossipsub)
  → manifest op contains: manifest_hash (the address to retrieve it)

Retrieve:
  → receive SignedOperation with manifest_hash
  → DHT lookup: who has manifest_hash?
  → fetch manifest → get block_hashes list
  → parallel DHT fetch of all blocks (from multiple peers)
  → verify each block: SHA-256(received) must equal hash from manifest
  → reassemble content C
  → tamper-proof: any modified block fails hash check
```

No tracker. No central index. The manifest is the torrent file. The DHT is
the tracker. Content is verifiable without trusting any peer.

---

### Layer 6: Meshnet Services

Once block distribution exists, you can serve files. Services = collections
of files + an API endpoint.

#### Mesh-Hosted Static Sites

A "meshsite" is a signed manifest of files:
```
{
  site_id:    <peer_id of publisher>,
  name:       "paddyoh-news",
  files: [
    { path: "index.html", hash: "abc..." },
    { path: "style.css",  hash: "def..." },
    { path: "logo.png",   hash: "ghi..." },
  ],
  published_at: <timestamp>,
  sig: <publisher's signature>
}
```

Any node serving this site fetches the files from DHT and serves them.
The site has no canonical IP or domain — it's identified by its publisher's
peer ID and name. Resolution: DHT lookup for `<peer_id>/sites/<name>`.

In the mesh browser TUI: navigate to `mesh://paddyoh-news` → resolves to
site manifest → renders content in-terminal (or hands off to a renderer
for HTML/images if one is integrated).

#### Mesh Address System

Like DNS but DHT-based. No central registry.

```
"paddyoh-news" → hash("paddyoh-news") → DHT key → returns list of
                  (peer_id, manifest_hash) pairs for nodes claiming this name

Name conflicts: first publisher wins (timestamp), OR name is tied to peer_id
  (e.g. "paddyoh/news" where "paddyoh" is your peer_id prefix — globally unique)

Human-readable names:  "paddyoh/news"    (peer_id prefix + name)
Content-addressed:     "content/abc123"  (direct hash access)
```

#### Mesh Email

Store-and-forward over gossipsub. Defined in DECENTRALISED_PLATFORM.md Phase B.
Short version:
- Message encrypted with recipient's public key
- Published to gossipsub topic `dm/<pair_hash>`
- If recipient offline: server nodes hold it, deliver on next connection
- Retention: 30 days (configurable per server operator)

---

## 4. THE CONSTRUCT & VIRTUAL LOBBY

### 4.1 What the Construct Is

The **Construct** is a bundled layer that runs beneath and alongside the main
world. It is NOT a different game, NOT a loading screen, and NOT a menu system.
It is a real, populated 3D space — the first place every player arrives, and
one they can return to at any time.

Think of it as the space station you dock at before going planetside. It's real,
other people are there, things happen there. It just doesn't sit on the digital
earth's terrain — it has its own bundled scene data, loaded from the client
binary before any network chunk arrives.

```
Why the Construct, not a fixed GPS location in the world:

  PHYSICAL LOCATION             CONSTRUCT LAYER
  ──────────────────            ──────────────────────────────
  Subject to chunk lag          Loads from local data instantly
  Tied to world state           Isolated — world bugs can't reach it
  Hard to expand safely         Redesign freely, deploy to all clients
  No global kick mechanism      Security event → global kick here
  Growth limited by geography   Modules added without world changes
  Can be griefed/blocked        Fully admin-controlled, always clean
```

### 4.2 The Construct is the Backend. The Physical World is the Frontend.

Every real-world service analogue in the game has two parts:

1. **A physical building** in the digital earth — the immersive facade.
   Players walk past it, see it on the street, can enter for the experience.

2. **A construct module** — the actual functional space where the service runs.
   The physical building's special door **portals** here.

```
Physical World (digital earth)          Construct (bundled layer)
────────────────────────────            ──────────────────────────────
[Bank building]      ──door──────────→  [/construct/bank]
[Post Office]        ──door──────────→  [/construct/post]
[Police Station]     ──door──────────→  [/construct/emergency]
[Hospital]           ──door──────────→  [/construct/medical]
[Travel Agent]       ──door──────────→  [/construct/travel]
[News Agency]        ──door──────────→  [/construct/news]
[Supermarket]        ──door──────────→  [/construct/marketplace]
[Any building]       ──door──────────→  [/construct/<module>]
                     ──/lobby cmd──--→  [/construct/lobby]   ← entry hub
                     ──first run─────→  [/construct/signup]  ← new players
```

The door is a **portal trigger**: a collision zone on the door mesh that fires a
`PortalTo { destination: ConstructModule }` action when the player walks through.
Players without interest in immersion use chat commands (`/lobby`, `/bank`, etc.)
to jump directly.

Multiple physical buildings in different cities can all portal to the same
construct module — there is one bank in the construct, but a thousand bank
branches in the world.

### 4.3 Construct Modules (Planned)

| Module | Purpose |
|---|---|
| `lobby` | Entry hub. Where all players start. Signup terminals. World portals. |
| `signup` | First-run identity creation (Trial/Guest/User flows) |
| `bank` | Currency, transfers, escrow for commerce |
| `marketplace` | Browse/buy/sell items, land listings, blueprints |
| `post` | Async messaging between players (like postal mail, not DMs) |
| `forums` | Threaded public discussion, served from meshnet |
| `wiki` | Community knowledge base |
| `news` | News agencies, broadcast media, in-world journalism |
| `travel` | Book passage, teleport tokens, region transit |
| `emergency` | Police reports, incident filing, moderation appeals |
| `government` | Region governance, proposals, voting |
| `settings` | Identity management, key backup, preferences |

New modules are added to the construct without touching world geography.

### 4.4 Security: Global Kick

If a critical security event occurs (exploit, griefing wave, network attack),
the server sends a signed `GlobalKick` operation. Every connected client
immediately transitions to `/construct/lobby`. The world state is frozen,
investigated, and restored. Players wait in the lobby. When the world is safe,
a `WorldReopen` signal is broadcast and portals re-activate.

```
Security event timeline:
  T+0s   Server detects exploit / admin triggers GlobalKick
  T+1s   Signed GlobalKick broadcast via gossipsub
  T+2s   All clients receive it, verify server signature
  T+2s   Every client renders construct/lobby — world frozen
  T+Xm   World investigated and restored
  T+Xm   WorldReopen broadcast — portals re-activate
  T+Xm   Players choose when to re-enter
```

### 4.5 Lobby Scene — **✅ BUILT (v0.1.5)**

The lobby loads from geometry bundled in the client binary — no network needed. Implemented in `src/construct.rs` + `examples/metaworld_alpha.rs`.

```
What the lobby contains (current):
  60×60 flat collision floor — solid from frame 1
  Perimeter wall with gaps at module entrances
  Central plaza with pillars
  Signup terminal at (0, 0, -6) — active on first run
  World portal at (0, 0, +14) — teleports to terrain, triggers chunk load
  6 Meshsite module rooms: Login, Signup, Forums, Wiki, Marketplace, Post Office
  Debug HUD overlay (mode, position, proximity state)

Game mode separation:
  Construct mode:  terrain rendering OFF, terrain physics OFF, Construct geometry ON
  OpenWorld mode:  Construct geometry OFF, terrain/physics ON
  Portal transition: teleport + sync chunk generation + re-enter loading phase

What still needs building:
  E key interact → module UI overlay
  Screen wall content (rendered text/HTML per module)
  Other player avatars visible in lobby
```

---

## 5. FIRST RUN & SIGNUP

On first launch (no `~/.metaverse/identity.key` exists), the player appears in
the lobby. A signup overlay activates automatically. The world continues loading
in the background. The player stands on solid ground.

```
Identity tiers (from least to most commitment — see IDENTITY_SYSTEM.md):

  TRIAL   No registration. Walk around, observe, predefined chat only.
          Key regenerated every hour → returned to lobby for a new one.
          Zero abuse surface. No persistent identity at all.

  GUEST   Free account. Verified email + chosen nickname required.
          Home plot assigned. Public chat (no DMs). Moderatable.
          Distributed key (published to network). 30-day minimum before upgrade.

  USER    Full account. Earned: 30 days Guest in good standing, or invited.
          All features: DMs, commerce, contracts, full build rights.
          Additional ID verification. Address verification optional.

  BUSINESS  Created under a User account (like Facebook pages).
            Organisation identity. User creates it, adds admins/mods.
```

```
First run flow:

  1. Client starts → spawn chunk generated synchronously → floor exists
  2. Network connects in background
  3. Lobby renders — player is standing, can look around
  4. Signup overlay appears (egui panel on top of 3D world):

     ┌─ WELCOME ──────────────────────────────────────────────────┐
     │  [ Load My Key ]   Returning player — point to key file    │
     │  ─── New here? ──────────────────────────────────────────  │
     │  [ Try It Now  ]   Trial — no registration, hourly reset   │
     │  [ Free Account]   Guest — email + nickname required        │
     │  [ Full Account]   User  — requires Guest + 30 days        │
     └────────────────────────────────────────────────────────────┘

  5. Choice made → keypair generated locally → key saved to disk
  6. Key published to network (Guest/User only — Trial stays local)
  7. Overlay dismissed → player is in the lobby, free to move
  8. "Enter World" portal visible — step through to the digital earth
```

---

## 6. IMPLEMENTATION ROADMAP


Phases are sequenced by dependency. Each phase is independently useful.

---

### Phase 1 — Virtual Lobby + First Run — **✅ DONE (v0.1.5)**

**What:** A lobby scene. First-run signup. No more CLI key setup.
**Delivered:** Construct scene, GameMode separation, signup overlay, world portal trigger, module rooms, debug HUD.
**Remaining polish:** E key interact for module rooms, screen wall content per module.

---

### Phase 2 — Content Types

**What:** Extend `Action` enum with platform actions. Nodes store and serve them.
**Unlocks:** Something to browse. Forum posts, wiki pages, profiles exist.
**Dependencies:** Phase 0 (foundation — done)

Tasks:
- Add action variants to messages.rs (PostContent, UpdateProfile, CreateListing, etc.)
- Storage layer for content ops (per-topic SQLite tables on server nodes)
- Gossipsub topic routing for content topics
- Basic retrieval API (REST: GET /api/v1/content/:topic, GET /api/v1/content/:id)
- Permission enforcement per action (key type + verification tier checks)

---

### Phase 3 — In-Game Terminal (PTY + Texture Renderer)

**What:** Physical terminal objects in the 3D world running real processes.
**Unlocks:** Browsing content inside the world without leaving the game.
**Dependencies:** Phase 2 (content to browse), Phase 0 (rendering)

Tasks:
- TerminalObject world entity (surface + activation zone)
- portable-pty integration (spawn child process in PTY)
- vte integration (ANSI parser → character grid state)
- Character grid → wgpu texture pipeline
- Texture mapped onto terminal surface mesh in world
- Input routing when terminal active (keyboard → PTY stdin)
- Mesh Browser TUI binary (connects to P2P, navigates content types)
- Exit node for internet access (spawns OS browser, sandboxed)

---

### Phase 4 — Tor Transport

**What:** Add Tor as a transport alongside TCP.
**Unlocks:** Censorship resistance. Accessible behind strict NAT/firewalls.
**Dependencies:** Phase 0 (transport layer)

Tasks:
- Detect local Tor daemon or start embedded one (arti crate — pure Rust Tor)
- libp2p transport wrapper over Tor SOCKS5 proxy
- Relay nodes: generate .onion address, publish in KeyRecord alongside TCP addr
- Bootstrap list supports .onion addresses
- Client config: prefer-tor mode, fallback-to-tcp mode
- Test: connection to relay via .onion address only

---

### Phase 5 — Onion Routing (Peer-Routed Circuits)

**What:** 3-hop onion circuits through game peers. Novel architecture.
**Unlocks:** Anonymous content access. No single peer knows who is asking for what.
**Dependencies:** Phase 4 (stable multi-transport), Phase 2 (content to fetch anonymously)

Tasks:
- Circuit protocol definition (message types, layered encryption spec)
- Circuit construction: DHT-based relay selection, spatial correlation avoidance
- Per-hop encryption (X25519 ECDH for ephemeral shared key per hop)
- Circuit maintenance (detect broken hop, rebuild)
- Anonymous content request through circuit
- Relay routing implementation (Relay key holders: accept, peel, forward)
- Circuit reuse within session (don't rebuild for every request)
- Telemetry: circuit health, hop latency (anonymised — no source info)

This is the hardest phase. It is original research. No existing Rust crate
does exactly this. Budget significant time. Prototype the protocol spec first.

---

### Phase 6 — Large Content Distribution

**What:** Block-chunked DHT storage for images, video, downloads.
**Unlocks:** Image uploads, file sharing, eventually video streaming.
**Dependencies:** Phase 2 (content ops for manifests), Phase 0 (DHT)

Tasks:
- Block chunking: split file → 256KB blocks → hash each
- DHT block storage: store each block at its hash key
- Manifest op: new SignedOperation action carrying manifest_hash
- Parallel multi-peer block fetch
- Block integrity verification on receive (hash check)
- Progress reporting in terminal (% fetched)
- Garbage collection: blocks for expired/deleted content removed over time

---

### Phase 7 — Meshnet Services

**What:** Services traditionally on the internet, now on the mesh.
**Unlocks:** Mesh-hosted sites, email, arbitrary service hosting.
**Dependencies:** Phase 6 (block distribution), Phase 5 (onion routing for privacy)

Tasks:
- Mesh address system (DHT-based name resolution: "peer_id/name" → manifest)
- Meshsite manifest format and publishing protocol
- Mesh browser: navigate mesh:// URLs in terminal
- Mesh email: store-and-forward over gossipsub (see DECENTRALISED_PLATFORM.md Phase B)
- Mesh-hosted relay list (replace GitHub Gist dependency)

---

### Phase 8 — Screen Sharing (VNC-style in-world media)

**What:** Broadcast a screen/video onto an in-world surface. Others watch.
**Unlocks:** In-world cinemas, presentation screens, collaborative work.
**Dependencies:** Phase 6 (block distribution for video frames)

Tasks:
- Screen capture → encode as compressed frame stream (VP9 or AV1)
- Frames published as DHT blocks (short TTL, streaming)
- VideoSurface world entity: subscribes to stream, renders frames as texture
- VNC-compatible server for desktop broadcasting
- Bandwidth-adaptive quality: degrade gracefully on slow connections
- Access control: public stream vs invited-only

---

### Phase 9 — Verification Infrastructure

**What:** Email/phone/ID verification flows with sharded encrypted evidence.
**Unlocks:** Tier 1+ key upgrades. Trusted identities.
**Dependencies:** Phase 3 (terminal for in-game signup UI), Phase 7 (for email delivery over mesh)

Full detail in SIGNUP_AND_KEY_PROPAGATION.md.

Tasks:
- Email verification: SMTP or transactional email API
- Phone verification: SMS OTP
- VerificationRecord type (public proof, no personal data)
- Evidence encryption + Shamir sharding
- Shard distribution across multiple server nodes
- Operator review queue (in admin terminal)
- Right to erasure: DeleteVerificationData op

---

### Phase 10 — Alternative Transports

**What:** WebRTC, LoRa, Bluetooth — transport agnosticism fully realised.
**Unlocks:** Game accessible without an app (WebRTC). Game accessible where internet doesn't exist.
**Dependencies:** Phase 4 (transport abstraction proved with Tor)

Tasks:
- WebRTC transport (libp2p has crates: libp2p-webrtc)
- Browser-accessible relay nodes (serve WebRTC peers)
- Low-bandwidth mode: text-only (no 3D rendering) for LoRa/cellular
- LoRa transport (experimental): libp2p transport wrapper over serial LoRa module
- Bluetooth mesh: local device-to-device P2P, no internet

---

## 7. OPEN PROBLEMS (Research, Not Engineering)

These have no established solution. They require experimentation.

### 7.1 Sybil Resistance Without a Central Authority

A well-funded adversary can generate millions of fake peer IDs, flooding the
DHT with fake relay nodes and poisoning content routing.

Tor solves this with a directory authority (centralized — violates Invariant 1).
We can't do that. Current thinking:
- Proof-of-work on peer ID generation (expensive to create millions of IDs)
- Social proof (Relay key issuance is human-authorized, not automatable)
- DHT reputation (content-providing peers accumulate uptime/delivery score)

Neither alone is sufficient. All three together may be. This needs prototyping.

### 7.2 Circuit Maintenance Through Transient Peers

Player peers log on and off continuously. A circuit depending on a regular
player as a middle hop will break every time that player logs off.

Partially solved by tiered circuits (Relay key holders = stable backbone).
Open question: what's the minimum relay network size before circuits are
reliable enough to be useful? Too few relays = all circuits go through same
few nodes = trivial to deanonymize by compromising those nodes.

### 7.3 DHT Poisoning

A malicious peer can answer DHT queries with false "who has content X?" responses,
causing fetches to fail or route to honeypot nodes. Classic Kademlia attack.

Hash verification handles corrupted content (you detect it immediately).
But if ALL K closest peers for a key are adversarial, content is unreachable.
Mitigation: popular content over-replicated (hard to poison all copies),
plus DHT reputation (adversarial peers identified and avoided over time).

### 7.4 Bootstrap When All Bootstrap Nodes Are Offline

After first connection: peer exchange (PEX) provides a local peer cache.
Subsequent runs reconnect via cached peers without any bootstrap server.

Unsolved: truly first connection. Still needs one known address.
Long-term: DNS seeding (multiple operators each running a DNS seed server)
plus mDNS for local network discovery as fallback.

### 7.5 Bandwidth for 3D Terrain Over High-Latency Transports

Tor latency: 200–500ms. Current terrain streaming assumes low-latency connections.
3D world over Tor will be painful.

Solution: two modes.
- Full mode: 3D world + content layer. Requires reasonable bandwidth + latency.
- Terminal mode: text content only, no 3D rendering.
  Works over Tor, LoRa, cellular, anything with bytes.

A user on a high-latency connection can still access the full content layer
(forum, wiki, marketplace, DMs) via terminal mode. They just can't fly around
the world at the same time.

---

## 8. COMPARISON (Why This Is Different)

Nothing else combines all of these in one coherent system.

```
System         3D World  Meshnet  Onion Routing  Content Layer  Unified Identity
─────────────────────────────────────────────────────────────────────────────────
Tor            ✗         ✗        ✓ (dedicated)  ✗              ✗
IPFS/Filecoin  ✗         ✗        ✗              ✓              ✗
I2P            ✗         ✗        ✓ (dedicated)  partial        ✗
Freenet/Hyphanet ✗       ✗        ✓ (dedicated)  ✓              ✗
Second Life    ✓         ✗        ✗              ✗              centralised
Urbit          ✗         ✗        ✗              ✓              ✓
Matrix         ✗         ✗        ✗              ✓              federated
This           ✓         ✓        ✓ (peer-routed) ✓             ✓
```

The combination is the novel thing. Each individual component has prior art.
Peer-routed onion circuits (using players as relays) is genuinely new.
Putting the 3D world and the content layer into the same data model (unified
SignedOperation) so they're literally the same network is genuinely new.
The in-game terminal as first-class network access with no separate browser
is genuinely new.

Individual systems are engineering. The combination is research.

---

## 9. WHAT THE USER SEES (End-to-End Experience)

```
Day 1:
  → Downloads 50MB binary from a normal URL
  → Runs it
  → Appears in a lobby
  → Walks to a terminal, types "anon", gets a key, enters the world
  → Wanders around, meets other players
  → Finds a public terminal, reads the forum, sees what people are building
  → Builds something. It's saved. It's theirs.

Day 100:
  → Opens game → appears in lobby → enters world
  → Walks to their parcel, checks their builds
  → Opens a terminal, checks messages, sees someone offered to buy their shop
  → Accepts the offer (marketplace op, signed, CRDT-merged globally)
  → Goes to a cinema, watches a stream someone is broadcasting on a wall
  → Logs off → their node stays up as a relay (optional, earns reputation)

Day 1000 (adversarial):
  → Government orders all servers taken down
  → Servers go offline
  → Clients reconnect via cached peer list
  → Someone brings up a new server → discovers it via DHT
  → World continues. Content continues. Identities continue.
  → Nothing was lost. The network was the nodes.
```

---

*Status: Architecture document*
*Author: Copilot (structure/technical), PaddyOh (vision/direction)*
*Update this doc as phases move from planning to in-progress to done*
