# Bootstrap Node List Schema Design

**Purpose:** Define JSON schema for dynamic bootstrap node discovery
**Version:** 1.0
**Date:** 2026-02-21

---

## Design Goals

1. **Updateable without client recompilation** - IP changes don't break network
2. **Multiple fallback sources** - GitHub Gist, Pages, IPFS, etc.
3. **Geographic distribution** - Prefer nodes by region/latency
4. **Capability advertising** - Nodes declare what they support
5. **Version compatibility** - Prevent old clients from breaking
6. **Community extensible** - Anyone can run a node
7. **Self-healing** - Dead nodes removed, new nodes added
8. **Lightweight** - Keep file small for fast downloads

---

## Schema v1.0

### Complete Example

```json
{
  "schema_version": "1.0",
  "network": "mainnet",
  "updated_at": "2026-02-21T12:30:00Z",
  "min_client_version": "0.1.0",
  "ttl_seconds": 3600,
  
  "bootstrap_nodes": [
    {
      "id": "phone-relay-primary",
      "name": "Primary Phone Relay",
      "description": "Main relay running on Android phone",
      "multiaddr": "/ip4/49.182.84.9/tcp/4001/p2p/12D3KooWEzai1nEViFuX6JmLWDLU61db7T1A3hyd4xpmGs4W59ge",
      "capabilities": ["relay", "bootstrap", "dht"],
      "region": "AU",
      "priority": 100,
      "verified": true,
      "uptime_percent": 95.5,
      "last_seen": "2026-02-21T12:25:00Z",
      "added_at": "2026-01-15T00:00:00Z"
    },
    {
      "id": "home-server-backup",
      "name": "Home Server Backup",
      "description": "24/7 home server relay",
      "multiaddr": "/dns4/relay.myserver.com/tcp/4001/p2p/12D3KooWBackupNode123456789ABCDEF",
      "capabilities": ["relay", "bootstrap"],
      "region": "AU",
      "priority": 80,
      "verified": true,
      "uptime_percent": 99.2,
      "last_seen": "2026-02-21T12:29:00Z",
      "added_at": "2026-02-01T00:00:00Z"
    },
    {
      "id": "community-us-1",
      "name": "Community Relay US-East",
      "description": "Community contributed relay",
      "multiaddr": "/ip4/203.0.113.42/tcp/4001/p2p/12D3KooWCommunityUS123456789ABCDEF",
      "capabilities": ["relay"],
      "region": "US",
      "priority": 60,
      "verified": false,
      "uptime_percent": 87.3,
      "last_seen": "2026-02-21T12:20:00Z",
      "added_at": "2026-02-10T00:00:00Z"
    },
    {
      "id": "ipfs-bootstrap-1",
      "name": "IPFS Bootstrap Node",
      "description": "Public IPFS bootstrap for testing",
      "multiaddr": "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
      "capabilities": ["bootstrap", "dht"],
      "region": "GLOBAL",
      "priority": 40,
      "verified": true,
      "uptime_percent": 99.9,
      "last_seen": "2026-02-21T12:30:00Z",
      "added_at": "2026-01-15T00:00:00Z"
    }
  ],
  
  "fallback_discovery": {
    "dns_seeds": [
      "bootstrap.metaverse.community",
      "nodes.metaverse.org"
    ],
    "http_rendezvous": [
      "https://rendezvous.metaverse.org/api/v1/peers"
    ],
    "ipfs_gateways": [
      "https://ipfs.io/ipfs/QmBootstrapListHash123",
      "https://dweb.link/ipfs/QmBootstrapListHash123"
    ]
  },
  
  "health_check": {
    "endpoint": "https://health.metaverse.org/api/v1/status",
    "interval_seconds": 300,
    "report_failures": true
  },
  
  "metadata": {
    "maintainer": "Metaverse Core Team",
    "source_repo": "https://github.com/yourproject/bootstrap-nodes",
    "report_issues": "https://github.com/yourproject/bootstrap-nodes/issues",
    "community_submit": "https://github.com/yourproject/bootstrap-nodes/pulls"
  }
}
```

---

## Field Specifications

### Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `schema_version` | string | ✅ | Schema version (semantic versioning). Client checks compatibility. |
| `network` | string | ✅ | Network identifier: "mainnet", "testnet", "devnet" |
| `updated_at` | ISO8601 | ✅ | Last update timestamp. Client caches based on this. |
| `min_client_version` | string | ✅ | Minimum client version required. Old clients refuse to parse. |
| `ttl_seconds` | integer | ❌ | How long to cache (default: 3600). Client re-fetches after TTL. |
| `bootstrap_nodes` | array | ✅ | List of bootstrap nodes (see below) |
| `fallback_discovery` | object | ❌ | Alternative discovery mechanisms |
| `health_check` | object | ❌ | Health monitoring configuration |
| `metadata` | object | ❌ | Human-readable metadata |

### Bootstrap Node Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | ✅ | Unique identifier (kebab-case). Used for deduplication. |
| `name` | string | ✅ | Human-readable name |
| `description` | string | ❌ | Short description of node purpose |
| `multiaddr` | string | ✅ | libp2p multiaddr (includes peer ID) |
| `capabilities` | array | ✅ | What node supports: "relay", "bootstrap", "dht", "storage" |
| `region` | string | ✅ | Geographic region: "AU", "US", "EU", "AS", "GLOBAL" |
| `priority` | integer | ✅ | Connection priority (0-100). Higher = try first. |
| `verified` | boolean | ✅ | Is this an official/trusted node? |
| `uptime_percent` | float | ❌ | 30-day uptime percentage |
| `last_seen` | ISO8601 | ❌ | Last successful health check |
| `added_at` | ISO8601 | ❌ | When node was added to list |

### Capabilities

Standardized capability strings:

- **`relay`** - Acts as relay for NAT traversal (libp2p relay protocol)
- **`bootstrap`** - Serves as DHT bootstrap node
- **`dht`** - Participates in Kademlia DHT
- **`storage`** - Provides content storage/pinning
- **`rendezvous`** - Implements rendezvous protocol
- **`mdns`** - Supports mDNS local discovery

### Regions

ISO 3166-1 alpha-2 codes + special:

- **Country codes:** `US`, `AU`, `GB`, `JP`, `CN`, etc.
- **Continents:** `NA`, `SA`, `EU`, `AS`, `AF`, `OC`
- **Special:** `GLOBAL` (anycast/CDN), `LAN` (local network only)

### Fallback Discovery Object

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `dns_seeds` | array | ❌ | DNS names that return TXT records with multiaddrrs |
| `http_rendezvous` | array | ❌ | HTTP APIs returning peer lists |
| `ipfs_gateways` | array | ❌ | IPFS gateway URLs for bootstrap list |

---

## Client Behavior Specification

### Startup Bootstrap Flow

```
1. Load cached bootstrap list from ~/.metaverse/bootstrap_cache.json
   ├─ If cache exists AND fresh (< ttl_seconds):
   │  └─ Use cached nodes ✅
   └─ If cache stale or missing:
      └─ Fetch from remote sources ↓

2. Fetch from remote sources (try in parallel, use first success):
   ├─ Primary: https://gist.githubusercontent.com/.../bootstrap.json
   ├─ Backup:  https://yourproject.github.io/bootstrap.json
   ├─ IPFS:    https://ipfs.io/ipfs/Qm.../bootstrap.json
   └─ Archive: https://archive.org/download/.../bootstrap.json
   
   On success:
   ├─ Validate schema_version compatibility
   ├─ Validate min_client_version
   ├─ Save to cache
   └─ Use nodes ✅
   
   On all failures:
   └─ Use hardcoded emergency fallback ⚠️

3. Process bootstrap nodes:
   ├─ Filter by capabilities (need "relay" or "bootstrap")
   ├─ Sort by priority (high → low)
   ├─ Sort by region preference (local region first)
   ├─ Filter out unverified if verified nodes exist
   └─ Connect to top N nodes (N = 3-5)

4. Add to DHT:
   ├─ For each connected node:
   │  └─ kademlia.add_address(&peer_id, multiaddr)
   └─ kademlia.bootstrap()?

5. Background refresh:
   └─ Every ttl_seconds / 2:
      ├─ Re-fetch bootstrap list
      ├─ Update cache
      └─ Add any new nodes to DHT
```

### Validation Rules

```rust
fn validate_bootstrap_list(list: &BootstrapList) -> Result<()> {
    // Schema version must match major version
    if list.schema_version.split('.').next() != Some("1") {
        return Err("Incompatible schema version");
    }
    
    // Client version must meet minimum
    if current_version() < list.min_client_version {
        return Err("Client too old, please upgrade");
    }
    
    // Must have at least one node
    if list.bootstrap_nodes.is_empty() {
        return Err("No bootstrap nodes in list");
    }
    
    // Each node must have valid multiaddr
    for node in &list.bootstrap_nodes {
        node.multiaddr.parse::<Multiaddr>()?;
    }
    
    Ok(())
}
```

---

## Use Cases & Examples

### Use Case 1: IP Address Changes

**Scenario:** Your phone relay gets new IP from ISP

**Solution:**
1. Update GitHub Gist with new IP
2. Change one line:
   ```diff
   - "multiaddr": "/ip4/49.182.84.9/tcp/4001/p2p/12D3KooW...",
   + "multiaddr": "/ip4/203.0.113.50/tcp/4001/p2p/12D3KooW...",
   ```
3. All clients pick up change within 1 hour (TTL)
4. No recompilation needed!

### Use Case 2: Add DNS-Based Node

**Scenario:** Set up permanent server with domain name

**Solution:**
```json
{
  "id": "permanent-relay",
  "name": "Permanent Relay Server",
  "multiaddr": "/dns4/relay.yourserver.com/tcp/4001/p2p/12D3KooW...",
  "capabilities": ["relay", "bootstrap", "dht"],
  "region": "US",
  "priority": 90,
  "verified": true
}
```

DNS changes = no bootstrap file update needed!

### Use Case 3: Community Relay

**Scenario:** Community member wants to run relay

**Solution:**
1. They submit PR to bootstrap repo
2. Add their node with `verified: false` and `priority: 50`
3. After testing period → upgrade to `verified: true`
4. Community governance decides priority

### Use Case 4: Emergency Fallback

**Scenario:** All your nodes are down

**Solution:**
Client falls back to IPFS bootstrap nodes:
```json
{
  "id": "ipfs-emergency",
  "multiaddr": "/dnsaddr/bootstrap.libp2p.io/p2p/QmNnooDu7...",
  "capabilities": ["bootstrap", "dht"],
  "priority": 10,
  "verified": true
}
```

### Use Case 5: Geographic Optimization

**Scenario:** User in Australia connecting

**Client logic:**
1. Fetch bootstrap list
2. Sort by region:
   - `AU` priority: 100 (same country)
   - `OC` priority: 90 (same continent)
   - `AS` priority: 80 (nearby continent)
   - Others: 70
3. Connect to highest priority nodes
4. Measure latency, keep best connections

---

## Evolution & Migration

### Schema Versioning

**Minor version bump (1.0 → 1.1):**
- Add optional fields only
- Clients ignore unknown fields
- Backward compatible

**Major version bump (1.x → 2.0):**
- Breaking changes allowed
- Old clients refuse to parse
- `min_client_version` forces upgrades

### Adding New Capabilities

**Future capabilities (examples):**
- `"webrtc"` - Supports WebRTC transport
- `"quic-v2"` - Supports QUIC v2
- `"storage-public"` - Public IPFS pinning
- `"rendezvous-v2"` - Enhanced rendezvous protocol

**Backward compatibility:**
Clients ignore unknown capabilities, only filter for what they understand.

### Deprecating Old Nodes

**Process:**
1. Add `"deprecated": true` field (schema 1.1+)
2. Lower priority to 10
3. Add `"deprecation_date": "2026-03-01T00:00:00Z"`
4. After date, remove from list

---

## Security Considerations

### Multiaddr Validation

**Client MUST validate:**
1. Multiaddr syntax is valid
2. Peer ID in multiaddr matches expected format
3. Port is reasonable (1-65535)
4. Protocol is supported (tcp, quic, dns4, dns6, dnsaddr)

**Client SHOULD warn:**
- Unverified nodes (display warning in UI)
- Nodes with low uptime (<80%)
- Nodes not seen recently (>7 days)

### Trust Model

**Verified nodes (`verified: true`):**
- Operated by core team
- Long uptime history
- Trusted infrastructure

**Community nodes (`verified: false`):**
- Community contributed
- Less trusted
- May have lower priority
- Client shows warning on first connect

### Poisoning Attacks

**Attack:** Malicious bootstrap list with attacker nodes

**Mitigations:**
1. **Hardcoded fallback** - Emergency nodes in source code
2. **Multiple sources** - Fetch from Gist + Pages + IPFS
3. **Content hash verification** - IPFS content addressing
4. **Signature verification** - Sign bootstrap.json with GPG (future)
5. **Peer validation** - Kademlia DHT validates peer behavior

---

## Hosting Recommendations

### Primary: GitHub Gist

**URL:** `https://gist.githubusercontent.com/youruser/abc123/raw/bootstrap.json`

**Pros:**
- Free
- Global CDN (Fastly)
- Version history
- Easy to update (web UI or API)
- Raw URL that never changes

**Update process:**
```bash
# Update via API
curl -X PATCH \
  -H "Authorization: token YOUR_TOKEN" \
  https://api.github.com/gists/abc123 \
  -d '{"files":{"bootstrap.json":{"content":"..."}}}'
```

### Backup: GitHub Pages

**URL:** `https://yourproject.github.io/bootstrap.json`

**Setup:**
1. Create repo: `yourproject/yourproject.github.io`
2. Push `bootstrap.json` to root
3. Enable GitHub Pages
4. Automatic deployment on push

### Tertiary: IPFS

**URL:** `https://ipfs.io/ipfs/QmHashOfBootstrapFile`

**With DNSLink:**
```
_dnslink.bootstrap.yoursite.com = /ipfs/QmHash...
```

**Access:** `https://ipfs.io/ipns/bootstrap.yoursite.com/bootstrap.json`

---

## Example Implementations

### Minimal Bootstrap (Single Node)

```json
{
  "schema_version": "1.0",
  "network": "mainnet",
  "updated_at": "2026-02-21T12:30:00Z",
  "min_client_version": "0.1.0",
  "bootstrap_nodes": [
    {
      "id": "primary",
      "name": "Primary Relay",
      "multiaddr": "/ip4/49.182.84.9/tcp/4001/p2p/12D3KooWEzai...",
      "capabilities": ["relay", "bootstrap"],
      "region": "AU",
      "priority": 100,
      "verified": true
    }
  ]
}
```

### Full Production (Multiple Regions)

```json
{
  "schema_version": "1.0",
  "network": "mainnet",
  "updated_at": "2026-02-21T12:30:00Z",
  "min_client_version": "0.1.0",
  "ttl_seconds": 1800,
  "bootstrap_nodes": [
    {
      "id": "au-primary",
      "multiaddr": "/dns4/au.relay.metaverse.org/tcp/4001/p2p/12D3KooW...",
      "capabilities": ["relay", "bootstrap", "dht"],
      "region": "AU",
      "priority": 100,
      "verified": true
    },
    {
      "id": "us-primary",
      "multiaddr": "/dns4/us.relay.metaverse.org/tcp/4001/p2p/12D3KooW...",
      "capabilities": ["relay", "bootstrap", "dht"],
      "region": "US",
      "priority": 100,
      "verified": true
    },
    {
      "id": "eu-primary",
      "multiaddr": "/dns4/eu.relay.metaverse.org/tcp/4001/p2p/12D3KooW...",
      "capabilities": ["relay", "bootstrap", "dht"],
      "region": "EU",
      "priority": 100,
      "verified": true
    },
    {
      "id": "global-cdn",
      "multiaddr": "/dns4/relay.metaverse.org/tcp/4001/p2p/12D3KooW...",
      "capabilities": ["relay", "bootstrap"],
      "region": "GLOBAL",
      "priority": 80,
      "verified": true
    }
  ],
  "fallback_discovery": {
    "dns_seeds": ["bootstrap.metaverse.org"],
    "http_rendezvous": ["https://api.metaverse.org/peers"]
  }
}
```

---

## Summary

**Key Features:**
- ✅ IP changes don't require recompilation
- ✅ Multiple fallback sources
- ✅ Geographic optimization
- ✅ Community extensible
- ✅ Version compatibility
- ✅ Caching for offline support
- ✅ Security through verification flags
- ✅ Future-proof with capabilities

**Next Steps:**
1. Review schema design
2. Create initial bootstrap.json
3. Upload to GitHub Gist
4. Implement Rust fetcher
5. Test with real network

**Questions for User:**
- Schema looks good or needs changes?
- Any additional fields needed?
- Ready to implement fetcher?
