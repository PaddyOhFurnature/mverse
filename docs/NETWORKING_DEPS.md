# Networking Dependencies Added (Phase 1.5)

**Date:** 2026-02-18  
**Purpose:** P2P networking foundation for local-first architecture

---

## Dependencies Added to Cargo.toml

### Core libp2p (v0.56.0)
Main peer-to-peer networking library with essential protocols.

**Features enabled:**
- `tcp` — TCP transport (primary connection method)
- `noise` — Noise protocol for connection encryption
- `yamux` — Yamux multiplexing (stream muxer for multiple logical streams over one connection)
- `gossipsub` — Publish/subscribe messaging (for state sync: player positions, voxel operations)
- `kad` — Kademlia DHT (distributed hash table for peer discovery)
- `mdns` — Multicast DNS (automatic peer discovery on local networks)
- `request-response` — Request/response protocol (for chunk data requests)
- `identify` — Peer identification protocol (exchange peer info)
- `ping` — Keepalive and latency measurement
- `macros` — Convenience macros for Swarm building
- `tokio` — Tokio async runtime integration
- `serde` — Serialization support for network messages

**Why these features:**
- **tcp + noise + yamux**: Standard secure transport stack
- **gossipsub**: Core of our bandwidth-budgeted state sync (Priority 2)
- **kad**: Distributed peer discovery (no central server)
- **mdns**: Zero-config local network multiplayer
- **request-response**: Future chunk/geometry sync (Priority 4)

### Cryptographic Identity (ed25519-dalek v2.2.0)
Ed25519 elliptic curve cryptography for:
- Peer ID generation (deterministic from public key)
- Signing voxel operations (CRDT authenticity)
- Verifying remote operations (anti-cheat foundation)

**Features enabled:**
- `serde` — Serialize keypairs and signatures
- `rand_core` — Secure random number generation for keypair creation

**Why Ed25519:**
- Fast (sign/verify ~70k ops/sec)
- Small signatures (64 bytes)
- Deterministic peer IDs (public key hash = PeerId)
- Industry standard (used by libp2p, SSH, cryptocurrencies)

### NAT Traversal (libp2p-relay v0.18.0, libp2p-dcutr v0.12.0)
Enable connections between peers behind NATs/firewalls.

**libp2p-relay:**
- Relay protocol for indirect connections
- Allows peers to communicate via intermediary relay nodes
- Fallback when direct connection impossible

**libp2p-dcutr:**
- "Direct Connection Upgrade through Relay"
- Hole-punching protocol to upgrade relayed connections to direct
- Reduces latency and bandwidth costs after initial connection

**Why needed:**
- Most home users behind NAT routers
- Direct P2P connections require hole-punching
- Relay provides fallback when hole-punching fails
- Critical for "works anywhere" goal

### Random Number Generation (rand v0.8)
Secure randomness for:
- Keypair generation
- Nonces in cryptographic operations
- Testing and simulation

---

## Locked Versions (from Cargo.lock)

```toml
libp2p = "0.56.0"
ed25519-dalek = "2.2.0"
libp2p-relay = "0.18.0"
libp2p-dcutr = "0.12.0"
rand = "0.8.5"
```

**Notable transitive dependencies added:**
- `curve25519-dalek` — Elliptic curve operations for Ed25519
- `noise-protocol` — Noise handshake implementation
- `yamux` — Multiplexing implementation
- `hickory-resolver` — Async DNS resolver (for bootstrap nodes)
- `chacha20poly1305` — Authenticated encryption
- `blake2` — Hash function for peer IDs

**Total new dependencies:** ~143 crates  
**Compilation time:** ~19 seconds on first build  
**Impact on binary size:** ~2-3 MB (networking stack)

---

## Integration Points

### Existing Code Compatibility
All networking code will be **new modules** — no changes to existing systems:
- `src/physics.rs` — No changes (yet) — will add `get_network_state()` later
- `src/voxel.rs` — No changes (yet) — will add operation logging later
- `src/terrain.rs` — No changes
- `src/coordinates.rs` — No changes

**Why no breaking changes:**
- Phase 1 code remains fully functional in single-player mode
- Networking is **additive**, not replacement
- Gradual integration strategy reduces risk

### Future Files (to be created)
1. `src/network.rs` — NetworkNode, Swarm management, protocol handlers
2. `src/identity.rs` — Keypair management, signing, verification
3. `src/sync.rs` — CRDT, vector clocks, operation merging
4. `src/entity.rs` — Networked player management
5. `examples/two_peers.rs` — Minimal connection test
6. `examples/two_player_demo.rs` — Full interactive P2P demo

---

## Bandwidth Budget Priorities (Mapping to Dependencies)

**Priority 1:** Local state (no network deps)  
**Priority 2:** State sync → `gossipsub` (player positions, voxel ops)  
**Priority 3:** Voice → Future (Opus codec, separate crate)  
**Priority 4:** Geometry → `request-response` (chunk meshes)  
**Priority 5:** Textures → `request-response` (texture streaming)  
**Priority 6:** Media → Future (video streaming protocol)

Currently implementing **Priority 2** foundations (libp2p + state sync).

---

## Testing Strategy

### Dependency Verification
```bash
# Verify all dependencies compiled
cargo check --lib

# Expected: 6 warnings (existing unused variables)
# Expected: "Finished `dev` profile" success message
```

### Next Steps
1. Create `src/identity.rs` with Ed25519 keypair management
2. Create `src/network.rs` with basic NetworkNode
3. Create `examples/two_peers.rs` to test connection
4. Verify: Two processes can connect and exchange messages

---

## Architecture Philosophy Reminder

These dependencies enable **local-first architecture:**
- Network is sync layer, not requirement
- Works offline (Priority 1)
- Gracefully degrades with bandwidth
- P2P = no central servers
- CRDT = eventual consistency without authority

**Core Principle:** Your machine is the metaverse; network is a mirror.

---

## Troubleshooting

### Compilation Issues
If `cargo check` fails:
1. Verify Rust version: `rustc --version` (need 1.83.0+)
2. Update toolchain: `rustup update`
3. Clean build: `cargo clean && cargo check`

### Feature Conflicts
If feature resolution fails:
- Check `Cargo.lock` for version conflicts
- Ensure `tokio` features compatible (already using v1 with full features)
- libp2p requires specific versions of transitive deps (Cargo handles automatically)

### Runtime Issues (Future)
When implementing NetworkNode:
- Ensure tokio runtime is running (`tokio::runtime::Runtime::new()`)
- Use `pollster::block_on()` for single-threaded contexts (like our current main loop)
- Consider switching to full async main loop (major refactor, defer to later phase)

---

## References

- **libp2p Documentation:** https://docs.libp2p.io/
- **Rust libp2p Book:** https://github.com/libp2p/rust-libp2p
- **Ed25519 Spec:** https://ed25519.cr.yp.to/
- **Noise Protocol:** https://noiseprotocol.org/

---

**Status:** ✅ Dependencies added and verified  
**Next Todo:** `identity-module` (create src/identity.rs)
