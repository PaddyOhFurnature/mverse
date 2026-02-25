# NAT Traversal & Relay Server Implementation Plan

## Overview

Enable metaverse to work across the internet, not just local LAN. Users behind NATs, firewalls, and mobile networks can connect and play together.

## Current State

**What Works:**
- ✅ Local LAN discovery via mDNS
- ✅ Direct TCP connections on same network
- ✅ P2P messaging with gossipsub
- ✅ Dependencies already added (libp2p-relay, libp2p-dcutr)

**What Doesn't Work:**
- ❌ Connecting across internet (blocked by NATs)
- ❌ Mobile/restrictive networks
- ❌ Firewall traversal
- ❌ Peer discovery beyond local network

## Solution Architecture - P2P First, Server Aware

**Core Principle:** Players communicate P2P. Relay server is ONLY for NAT traversal bootstrap and fallback.

### Connection Priority (Waterfall):

```
1. DIRECT P2P (Local LAN)
   ↓ (if not on same network)
2. HOLE PUNCHING (NAT traversal via DCUtR)
   ↓ (if hole punch fails - strict NAT)
3. RELAY FALLBACK (temporary, limited)
   ↓ (relay helps establish direct connection)
4. DIRECT P2P (upgraded from relay)
```

### Architecture Diagram:

```
┌─────────────┐                    ┌─────────────┐
│   Player A  │                    │   Player B  │
│  (Behind    │                    │  (Behind    │
│   NAT 1)    │                    │   NAT 2)    │
└──────┬──────┘                    └──────┬──────┘
       │                                  │
       │ PRIMARY: Direct P2P              │
       │◄────────────────────────────────►│
       │  - All game state                │
       │  - Voxel operations              │
       │  - Player positions              │
       │  - Chat messages                 │
       │                                  │
       │ BOOTSTRAP: Discover via relay    │
       ├──────┐                    ┌──────┤
       │      ▼                    ▼      │
       │  ┌─────────────────────────┐     │
       │  │   Relay (Optional)      │     │
       │  │   - NAT coordination    │     │
       │  │   - Hole punch assist   │     │
       │  │   - Fallback only       │     │
       │  └─────────────────────────┘     │
       │                                  │
       │ GOAL: Upgrade to direct P2P      │
       │◄────────────────────────────────►│
       
SERVER GOES OFFLINE → P2P continues working (LAN/direct)
```

### Key Differences from Client-Server:

**✅ P2P-First:**
- Players connect directly to each other
- All game state shared P2P (gossipsub)
- No server required for gameplay
- LAN works without internet

**✅ Server-Aware:**
- Use relay for NAT traversal coordination
- Optional cache/heartbeat features
- Degrade gracefully if server offline

**❌ NOT Client-Server:**
- Server does NOT store game state
- Server does NOT validate operations
- Server does NOT route all traffic
- Server is NOT required for core gameplay

## Implementation Phases - P2P First!

### Phase 1: NAT Traversal (Client-Side) - HIGHEST PRIORITY

**Goal:** Players behind NATs can establish DIRECT P2P connections

**This is the critical path - enables internet play!**

**Files to Modify:**
- `src/network.rs` - Add relay client + DCUtR
- `src/multiplayer.rs` - Connection upgrade logic
- `examples/metaworld_alpha.rs` - Bootstrap relay addresses

**Features:**

1. **Hole Punching (DCUtR - Direct Connection Upgrade through Relay)**
   - Attempt direct connection through NAT
   - Use relay to coordinate simultaneous connects
   - UDP hole punching for symmetric NATs
   - TCP hole punching for cone NATs
   - Success rate: 70-90% depending on NAT type

2. **Connection Modes**
   ```
   Priority 1: Direct (both have public IPs or same LAN)
   Priority 2: Hole-punched (both behind NAT, punch successful)
   Priority 3: Relayed (strict NAT, temporary until upgrade)
   Priority 4: Direct (upgraded from relay after coordination)
   ```

3. **Bootstrap Relay Discovery**
   - Connect to known relay on startup (for coordination only)
   - Get relay address from config/CLI arg
   - Fall back to LAN-only if no relay available
   - Disconnect from relay once direct P2P established

4. **Graceful Degradation**
   - LAN works without relay (mDNS)
   - Direct connections work without relay
   - Only use relay for NAT coordination
   - Gameplay continues if relay goes offline

**Implementation:**
```rust
// Add to network behavior
use libp2p::{relay, dcutr};

// Relay client (for hole punch coordination)
let relay_client = relay::client::Behaviour::new(local_peer_id);

// Hole punching
let dcutr = dcutr::Behaviour::new(local_peer_id);

// Connection flow:
// 1. Try direct connection (no relay needed)
// 2. If fails, connect via relay for coordination
// 3. DCUtR negotiates hole punch
// 4. Upgrade to direct P2P
// 5. Disconnect from relay (no longer needed)
```

**Success Metrics:**
- ✅ Players on different networks can connect
- ✅ Hole punching succeeds >70% of time
- ✅ Direct P2P established within 5 seconds
- ✅ Relay used only for coordination, not data
- ✅ Works on mobile/home/corporate networks

### Phase 2: Minimal Relay Server (SECONDARY PRIORITY)

**Goal:** Lightweight coordination server for hole punching only

**Important:** This is a helper service, NOT the primary communication channel!

**Files to Create:**
- `examples/relay_server.rs` - Standalone relay program
- `src/relay_config.rs` - Relay node configuration

**Features:**
1. **Bootstrap Node**
   - Known public address (relay.metaverse.network:4001)
   - Helps peers discover each other
   - Provides relay address to clients

2. **Circuit Relay v2**
   - Limited relay (prevent abuse)
   - Max 2 minutes per connection
   - Max 1MB data transfer per relay
   - Auto-cleanup stale circuits

3. **Monitoring**
   - Active connections count
   - Bandwidth usage
   - Relay circuit count
   - Uptime stats

**Server Requirements:**
- Minimal: 1 CPU core, 512MB RAM
- Public IP address (static or dynamic DNS)
- Ports: 4001 (TCP), 4001 (UDP for QUIC)
- Bandwidth: ~100KB/s per active relay circuit

**Deployment Options:**
- Self-hosted (VPS, home server, NAS)
- Free tier cloud (Oracle, AWS free tier)
- Docker container (easy deployment)
- systemd service (auto-restart)

### Phase 2: Client NAT Traversal (HIGH PRIORITY)

**Goal:** Clients auto-discover and use relay, attempt hole punching

**Files to Modify:**
- `src/network.rs` - Add relay client support
- `src/multiplayer.rs` - Add DCUtR behavior
- `examples/metaworld_alpha.rs` - Connect to bootstrap relay

**Features:**
1. **Auto Relay Discovery**
   - Connect to known bootstrap relays on startup
   - Discover relay nodes via DHT
   - Fall back to relay if direct connection fails

2. **Hole Punching (DCUtR)**
   - Attempt direct connection through NAT
   - Use relay to coordinate hole punch
   - Upgrade to direct P2P if successful
   - Fall back to relay if hole punch fails

3. **Connection Modes**
   - **Direct:** Both peers have public IPs (best)
   - **Hole-punched:** Both behind NAT, hole punch works (good)
   - **Relayed:** At least one behind strict NAT (acceptable)

**Configuration:**
```rust
// Default relay bootstrap nodes
const BOOTSTRAP_RELAYS: &[&str] = &[
    "/ip4/relay.metaverse.network/tcp/4001/p2p/12D3Koo...",
    "/ip4/backup-relay.metaverse.network/tcp/4001/p2p/12D3Koo...",
];
```

### Phase 3: Enhanced Server Features (MEDIUM PRIORITY)

**Goal:** Make relay servers do more than just relay

**Features:**

1. **Heartbeat Service**
   - Track online players (PeerId → last_seen)
   - Broadcast presence to interested peers
   - Detect disconnections (30s timeout)
   - Publish online player count

2. **Data Propagation Hub**
   - Cache recent voxel operations
   - Replay operations to new joiners
   - Reduce bandwidth for late joiners
   - Max 1000 operations in memory

3. **Chunk Data Cache**
   - Cache generated terrain chunks
   - Share cached chunks with peers
   - Reduce redundant terrain generation
   - LRU eviction (max 10GB cache)

4. **Software Update Distribution**
   - Serve latest client binaries
   - Publish version metadata
   - Auto-update notifications
   - Integrity verification (SHA-256)

### Phase 4: Advanced Features (LOW PRIORITY)

1. **Geographic Relay Selection**
   - Multiple relay servers worldwide
   - Clients connect to nearest relay
   - Latency-based selection
   - Failover to backup relays

2. **Relay Federation**
   - Relays discover each other
   - Share peer routing info
   - Cross-relay connections
   - Distributed load balancing

3. **Metrics & Analytics**
   - Prometheus metrics endpoint
   - Grafana dashboards
   - Connection success rates
   - Bandwidth usage graphs

## Relay Server Specification

### Command Line Interface

```bash
# Run relay server
metaverse-relay --port 4001 --external-ip 1.2.3.4

# With custom limits
metaverse-relay \
  --max-circuits 100 \
  --max-circuit-duration 120 \
  --max-circuit-bytes 1048576

# With cache enabled
metaverse-relay \
  --enable-chunk-cache \
  --cache-dir /var/lib/metaverse/cache \
  --cache-max-size 10GB

# With heartbeat service
metaverse-relay --enable-heartbeat --heartbeat-interval 30
```

### Configuration File (relay.toml)

```toml
[network]
port = 4001
external_ip = "1.2.3.4"
max_connections = 1000

[relay]
enabled = true
max_circuits = 100
max_circuit_duration_secs = 120
max_circuit_bytes = 1048576

[heartbeat]
enabled = true
interval_secs = 30
max_tracked_peers = 10000

[cache]
enabled = true
directory = "/var/lib/metaverse/cache"
max_size_bytes = 10737418240  # 10GB
chunk_cache = true
operation_cache = true
max_operations = 1000

[metrics]
enabled = true
prometheus_port = 9090
```

### API Endpoints (Optional HTTP server)

```
GET  /health              - Health check
GET  /metrics             - Prometheus metrics
GET  /peers               - Online peer count
GET  /version             - Server version
POST /announce            - Manual peer announcement
```

## libp2p Integration

### Relay Server Behavior Setup

```rust
// In relay_server.rs
use libp2p::{
    relay,
    swarm::SwarmBuilder,
    PeerId,
};

// Configure relay behavior
let relay_config = relay::Config {
    max_circuits: 100,
    max_circuit_duration: Duration::from_secs(120),
    max_circuit_bytes: 1024 * 1024, // 1MB
    ..Default::default()
};

let relay_behaviour = relay::Behaviour::new(local_peer_id, relay_config);
```

### Client Relay Setup

```rust
// In network.rs
use libp2p::{
    relay::client,
    dcutr,
};

// Connect to relay
let relay_client = client::Behaviour::new(local_peer_id);

// Enable hole punching
let dcutr = dcutr::Behaviour::new(local_peer_id);

// Combine behaviors
let behaviour = MyBehaviour {
    gossipsub,
    kademlia,
    identify,
    relay_client,
    dcutr,
    ..
};
```

### Connection Flow

```rust
// 1. Connect to bootstrap relay
swarm.dial("/ip4/relay.metaverse.network/tcp/4001/p2p/12D3Koo...")?;

// 2. Listen via relay
let relay_addr = format!("/p2p/{}/p2p-circuit", relay_peer_id);
swarm.listen_on(relay_addr.parse()?)?;

// 3. On new peer discovered via relay, attempt hole punch
match event {
    SwarmEvent::Behaviour(MyEvent::RelayClient(event)) => {
        // Trigger DCUtR hole punch
        dcutr.attempt_hole_punch(peer_id);
    }
    SwarmEvent::Behaviour(MyEvent::Dcutr(event)) => {
        match event {
            dcutr::Event::DirectConnectionUpgradeSucceeded { .. } => {
                // Direct connection established! Use this instead of relay
            }
            dcutr::Event::DirectConnectionUpgradeFailed { .. } => {
                // Hole punch failed, continue using relay
            }
        }
    }
}
```

## Testing Strategy

### Local Testing (Development)

```bash
# Terminal 1: Start local relay
cargo run --release --example relay_server -- --port 4001

# Terminal 2: Alice (behind simulated NAT)
METAVERSE_RELAY=/ip4/127.0.0.1/tcp/4001 \
  cargo run --release --example metaworld_alpha

# Terminal 3: Bob (behind simulated NAT)
METAVERSE_RELAY=/ip4/127.0.0.1/tcp/4001 \
  cargo run --release --example metaworld_alpha
```

### Real NAT Testing (Staging)

1. Deploy relay to VPS with public IP
2. Test from home network (behind NAT)
3. Test from mobile hotspot (carrier NAT)
4. Test from restrictive corporate network

### Performance Testing

- **Latency:** Direct vs relayed (expect 2x-3x latency for relay)
- **Throughput:** Direct vs relayed (expect 50% throughput for relay)
- **Connection success rate:** Hole punch success vs relay fallback
- **Server load:** CPU/memory/bandwidth usage under load

## Deployment Guide

### Docker Deployment (Recommended)

```dockerfile
FROM rust:1.80 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --example relay_server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/examples/relay_server /usr/local/bin/
EXPOSE 4001
CMD ["relay_server", "--port", "4001"]
```

```bash
# Build and run
docker build -t metaverse-relay .
docker run -d -p 4001:4001 --name relay metaverse-relay
```

### Systemd Service

```ini
[Unit]
Description=Metaverse Relay Server
After=network.target

[Service]
Type=simple
User=metaverse
ExecStart=/usr/local/bin/relay_server --port 4001
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

### Free Hosting Options

1. **Oracle Cloud Free Tier**
   - 2 AMD VMs (1 core, 1GB RAM each)
   - Permanent free tier
   - Public IP included

2. **AWS Free Tier**
   - t2.micro (1 core, 1GB RAM)
   - 750 hours/month for 12 months
   - Elastic IP included

3. **Fly.io**
   - 3 shared VMs free
   - Auto-scaling
   - Global deployment

4. **Home NAS (Synology/QNAP)**
   - Docker support
   - Always-on
   - Dynamic DNS recommended

## Security Considerations

1. **Relay Abuse Prevention**
   - Rate limiting per peer
   - Max circuit duration (2 min)
   - Max data transfer per circuit (1MB)
   - Peer reputation system (future)

2. **DDoS Protection**
   - Connection rate limiting
   - IP-based throttling
   - Cloudflare proxy (optional)

3. **Data Privacy**
   - Relay sees encrypted traffic only
   - No plaintext data accessible
   - Noise protocol encryption end-to-end

4. **Authentication**
   - Ed25519 signatures on all messages
   - Relay doesn't verify (peers verify)
   - No central authentication needed

## Success Metrics

**Phase 1 Complete When:**
- ✅ Relay server runs standalone
- ✅ Clients connect to relay
- ✅ Messages forward through relay
- ✅ Works across different networks

**Phase 2 Complete When:**
- ✅ Hole punching succeeds >50% of time
- ✅ Direct connections upgrade from relay
- ✅ Relay fallback works when hole punch fails
- ✅ Connection latency acceptable (<200ms)

**Phase 3 Complete When:**
- ✅ Heartbeat tracks online players
- ✅ Chunk cache reduces load times
- ✅ Data propagation helps late joiners
- ✅ Server handles 100+ concurrent connections

## Timeline Estimate

**Phase 1 (Relay Server):** 4-6 hours
- 2h: relay_server.rs basic implementation
- 1h: Configuration and CLI
- 1h: Testing and debugging
- 1h: Docker containerization

**Phase 2 (Client NAT Traversal):** 4-6 hours
- 2h: Integrate relay client behavior
- 2h: Add DCUtR hole punching
- 2h: Testing across NATs

**Phase 3 (Enhanced Features):** 8-12 hours
- 3h: Heartbeat service
- 3h: Chunk data cache
- 3h: Data propagation
- 3h: Testing and optimization

**Total:** 16-24 hours of focused development

## Next Actions

1. **Create relay_server.rs** - Start with basic relay functionality
2. **Test locally** - Two clients behind simulated NAT
3. **Deploy to VPS** - Get public relay running
4. **Update clients** - Add relay support to metaworld_alpha
5. **Real-world test** - Test from home network + mobile
6. **Iterate** - Fix issues, add features, optimize
