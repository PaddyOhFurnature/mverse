# Quick Deployment Guide

## On Dev Machine (after making changes)

```bash
# Build and copy latest binaries to deploy/
./deploy/update_binaries.sh
```

This will:
- Build `metaworld_alpha` (client) in release mode
- Build `metaverse-relay` (relay server) in release mode  
- Copy both to `deploy/bin/`
- Show binary sizes and next steps

## Copy to Remote Machine

```bash
# Copy entire deploy folder
scp -r deploy/ user@remote:/path/to/metaverse/

# Or use existing ssh alias
scp -r deploy/ portable@192.168.1.209:/home/portable/metaverse/
```

## On Remote Machine

### Run Client

```bash
cd /path/to/metaverse/deploy

# Run as bob (default)
./run_client.sh

# Run as alice
./run_client.sh alice

# Run as charlie
./run_client.sh charlie
```

### Run Relay Server

```bash
cd /path/to/metaverse/deploy

# Basic (auto-detect IP, port 4001)
./run_relay.sh

# Custom port
./run_relay.sh 8080

# With external IP (for NAT/firewall)
./run_relay.sh 4001 49.182.84.9
```

## Testing Mesh Relay

### Localhost (3 terminals)

```bash
# Terminal 1
./run_client.sh alice

# Terminal 2
./run_client.sh bob

# Terminal 3
./run_client.sh charlie
```

**Expected:** All three discover each other via mDNS. Watch for:
```
✅ [RELAY SERVER] Reservation accepted for peer: 12D3Koo...
🔄 [RELAY SERVER] Circuit: 12D3Koo... → 12D3Koo...
```

This proves clients are relaying for each other!

### LAN (2 machines)

**Machine A:**
```bash
./run_client.sh alice
./run_client.sh bob
```

**Machine B:**
```bash
./run_client.sh charlie
```

**Expected:** All three connect. Each can relay for the others.

### With Dedicated Relay

**VPS/Server:**
```bash
./run_relay.sh 4001 YOUR.PUBLIC.IP
# Note the peer ID shown on startup
```

**Update metaworld_alpha.rs line 178 with relay address:**
```rust
let relay_addr = "/ip4/YOUR.PUBLIC.IP/tcp/4001/p2p/PEER_ID";
```

**Rebuild and deploy:**
```bash
./deploy/update_binaries.sh
scp -r deploy/ user@remote:/path/
```

**Clients:**
```bash
./run_client.sh alice
./run_client.sh bob
```

**Expected:** 
- Clients connect to dedicated relay
- Clients ALSO relay for each other
- Full mesh: dedicated relay + client relays

## Directory Structure

```
deploy/
├── README.txt              # Quick reference
├── update_binaries.sh      # Build script (run on dev machine)
├── run_client.sh           # Client launcher
├── run_relay.sh            # Relay server launcher
├── bin/
│   ├── metaworld_alpha     # Client binary (31M)
│   └── metaverse-relay     # Relay server binary (7.4M)
└── identities/
    ├── alice.key
    ├── bob.key
    └── charlie.key
```

## What Changed (Mesh Relay)

Every client now has **both**:
- `relay_client` - Can USE other peers as relays ✅
- `relay_server` - Can BE a relay for other peers ✅ **NEW**

This creates a true P2P mesh where:
- Any peer can help any other peer connect
- Dedicated relays are just "always-on peers"
- Network is resilient (no single point of failure)

## Verification

### Client is Relaying

Watch for `[RELAY SERVER]` messages:
```
✅ [RELAY SERVER] Reservation accepted for peer: ...
🔄 [RELAY SERVER] Circuit: ... → ...
🔚 [RELAY SERVER] Circuit closed: ... → ...
```

### Client is Using Relays

Watch for `[RELAY]` messages:
```
✅ [RELAY] Reservation accepted by relay: ...
🔄 [RELAY] Circuit established via ...
```

### Full Mesh Working

With 3 clients (A, B, C):
- A should see relay server messages from B and C
- B should see relay server messages from A and C
- C should see relay server messages from A and B

This proves everyone is relaying for everyone = true mesh!

## Troubleshooting

### Binary won't run

```bash
chmod +x deploy/bin/metaworld_alpha
chmod +x deploy/bin/metaverse-relay
```

### Identity not found

```bash
ls deploy/identities/
# Make sure alice.key, bob.key, charlie.key exist
```

### Can't connect between machines

1. Check firewall (allow port 4001 and ephemeral ports)
2. Check both on same network (WiFi)
3. Try with dedicated relay on public IP
4. Check mDNS works (`ping hostname.local`)

### Relay messages not appearing

This is normal if using direct P2P (mDNS on LAN).
Relay is only used for NAT traversal across networks.

Try connecting from different networks to see relay in action.
