# Mesh Relay Testing Guide

**Implementation:** Complete ✅  
**Commit:** `c64cd2c`  
**Tag:** `mesh-relay-implemented-20260221-0445`

## What Was Implemented

Every client now has **both** relay capabilities:

1. **Relay Client** (`relay::client::Behaviour`)
   - Can USE other peers as relays
   - Connects through relays for NAT traversal
   - Uses DCUtR for hole punching

2. **Relay Server** (`relay::Behaviour`) **← NEW**
   - Can BE a relay for other peers
   - Accepts reservation requests
   - Establishes circuits between peers
   - Conservative limits for client nodes:
     * 128 max reservations
     * 16 max simultaneous circuits
     * 4 circuits per peer
     * 2 minute circuit duration
     * 1 MB per circuit

## Architecture

**Before (Hub-and-Spoke):**
```
    Dedicated Relay
      /    |    \
     /     |     \
Client A  B   C
```
- Only dedicated relay could BE a relay
- Clients could only USE relays
- Single point of failure

**Now (Mesh):**
```
  A ←→ B
   ↘ ↗ ↕
    C ←→ D
```
- Every client is ALSO a relay
- Any client can help any other connect
- Dedicated relays = just "always-on peers"
- True P2P mesh: "all clients-servers-nodes-relays ARE the network"

## Testing

### Test 1: Localhost - 3 Clients Relay for Each Other

```bash
# Terminal 1: Alice
METAVERSE_IDENTITY_FILE=alice.key cargo run --release --example metaworld_alpha

# Terminal 2: Bob  
METAVERSE_IDENTITY_FILE=bob.key cargo run --release --example metaworld_alpha

# Terminal 3: Charlie
METAVERSE_IDENTITY_FILE=charlie.key cargo run --release --example metaworld_alpha
```

**Expected behavior:**
- All three discover each other via mDNS
- Each can see RELAY SERVER messages when others reserve through them
- All three connected in full mesh

**Look for:**
```
✅ [RELAY SERVER] Reservation accepted for peer: 12D3Koo...
🔄 [RELAY SERVER] Circuit: 12D3Koo... → 12D3Koo...
```

### Test 2: LAN - 2 Computers, Clients Relay for Each Other

**Computer A:**
```bash
# Run Alice and Bob
METAVERSE_IDENTITY_FILE=alice.key cargo run --release --example metaworld_alpha
METAVERSE_IDENTITY_FILE=bob.key cargo run --release --example metaworld_alpha
```

**Computer B:**
```bash
# Run Charlie
METAVERSE_IDENTITY_FILE=charlie.key cargo run --release --example metaworld_alpha
```

**Expected:**
- All three discover via mDNS
- Charlie can relay through Alice OR Bob
- Alice and Bob can relay for each other

### Test 3: NAT - With Dedicated Relay as Bootstrap

**VPS/Relay Server:**
```bash
cargo run --release --bin metaverse-relay -- --port 4001
# Note the peer ID
```

**Behind NAT (multiple clients):**
```bash
# Each client connects to dedicated relay on startup (line 178 in metaworld_alpha.rs)
# But they can ALSO relay for each other once connected
cargo run --release --example metaworld_alpha
```

**Expected:**
- All clients connect to dedicated relay
- Clients make reservations on both the dedicated relay AND each other
- Circuits can go through any peer, not just dedicated relay
- Dedicated relay is just one node in the mesh

## Verification

### Check Relay Server is Active

Look for these log messages:
```
✅ [RELAY SERVER] Reservation accepted for peer: ...
🔄 [RELAY SERVER] Circuit: ... → ...
🔚 [RELAY SERVER] Circuit closed: ... → ...
```

If you see these, your client is successfully acting as a relay for others!

### Check Relay Client Still Works

Look for these log messages:
```
✅ [RELAY] Reservation accepted by relay: ...
🔄 [RELAY] Circuit established via ...
📞 [RELAY] Inbound circuit from ...
```

### Check Full Mesh

With 3 clients (A, B, C):
- A should see relay server messages from B and C
- B should see relay server messages from A and C  
- C should see relay server messages from A and B

This proves true mesh: everyone relays for everyone.

## Configuration

Relay server limits are in `src/network.rs` line ~369:

```rust
let relay_server_config = relay::Config {
    max_reservations: 128,           // How many peers can reserve
    max_reservations_per_peer: 4,    // Slots per peer
    max_circuits: 16,                // Simultaneous circuits
    max_circuits_per_peer: 4,        // Circuits per peer through us
    max_circuit_duration: Duration::from_secs(120),  // 2 minutes
    max_circuit_bytes: 1024 * 1024,  // 1 MB
    ..Default::default()
};
```

These are **conservative** for client nodes. Dedicated relays can use higher limits.

## Rollback

If mesh relay causes issues:

```bash
git checkout pre-mesh-relay-20260221-0435
cargo clean
cargo build --release --example metaworld_alpha
```

This restores hub-and-spoke relay (clients can only USE relays, not BE relays).

## Success Criteria

✅ Code compiles and builds  
✅ Existing features still work (chunks, persistence, P2P)  
⏳ Clients can relay for each other on localhost (test pending)  
⏳ mDNS still discovers peers on LAN (test pending)  
⏳ Dedicated relay still works as bootstrap (test pending)  
⏳ No regressions in chunk streaming/persistence (test pending)  

## Next Steps

1. **Test localhost mesh** (3 clients, all relay for each other)
2. **Test LAN mesh** (2 computers, clients relay across network)
3. **Test with dedicated relay** (ensure it works as bootstrap + mesh node)
4. **Profile resource usage** (check if relay server uses too much CPU/memory)
5. **Tune limits** if needed (may need different configs for different scenarios)

## Notes

- Relay is for **NAT traversal coordination**, not data routing
- Once DCUtR succeeds, direct P2P connection is used
- Relay circuits are temporary (2 minutes max)
- State sync happens over gossipsub (direct P2P), not relay circuits
- This matches our "data must get through" philosophy - use ALL available paths
