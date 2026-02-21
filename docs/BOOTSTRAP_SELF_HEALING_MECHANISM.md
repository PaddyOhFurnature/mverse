# Bootstrap Self-Healing Mechanism Design
**How to Make a System Always Changing Yet Stable**

**Created:** 2026-02-21  
**Problem:** "How do you make a system that is always in a state of change, and make it stable and reliable?"

---

## 🎯 The Core Challenge

### What We Need:
- Bootstrap nodes that UPDATE themselves (like magnet updating peers)
- Peers can UPDATE where the magnet is (if host offline)
- Data must be VERIFIED (not like rolling cache)
- System must be STABLE despite constant change

### The Flow Chart You Described:
```
1. Does data exist?
   ├─ NO → Wait/retry
   └─ YES → Continue to 2

2. Is it the data I expect?
   ├─ NO → Reject, mark source as suspicious
   └─ YES → Continue to 3

3. Is it newer than what I have?
   ├─ NO → Ignore, keep my data
   └─ YES → Continue to 4

4. Is it valid/verified?
   ├─ NO → Reject, mark source as bad
   └─ YES → Update own data, propagate to others
```

**The Question:** How does this work in reality with P2P nodes coming/going?

---

## 🏗️ Real-World Examples (How Others Solved This)

### 1. **BitTorrent DHT** - Bootstrap via Routing Table
```
Problem: How to find trackers when they move/go offline?

Solution:
- Every node is part of DHT routing table
- Nodes announce themselves periodically (every 15 min)
- If node doesn't re-announce → expires from table (30 min TTL)
- Clients query DHT for "tracker:infohash" → get current peer list
- Self-healing: Dead nodes auto-expire, new nodes auto-join

Data verification:
- Infohash verifies content identity (SHA1 hash)
- No trust needed - data itself proves correctness
```

### 2. **IPFS Bootstrap** - Hardcoded + Gossip Discovery
```
Problem: How to join network when bootstrap nodes change?

Solution:
Layer 1: Hardcoded bootstrap list (5-10 well-known nodes)
- /dnsaddr/bootstrap.libp2p.io
- /ip4/104.131.131.82/tcp/4001/...
- Try all in parallel, connect to first 3

Layer 2: DHT crawling
- Once connected, query DHT for more peers
- Peers announce themselves: "I exist at <multiaddr>"
- Bootstrap list expands dynamically

Layer 3: Peer exchange (PEX)
- Connected peers share their peer lists
- "Here are 20 peers I know about"
- Network becomes self-sustaining

Data verification:
- CID (Content ID) = hash of content
- Merkle DAG ensures data integrity
- If hash doesn't match → reject data
```

### 3. **Ethereum Node Discovery (discv5)** - Gossip + ENR Records
```
Problem: How to find nodes when IP addresses change?

Solution:
ENR (Ethereum Node Record):
- Signed record with: IP, port, public key, metadata
- Includes sequence number (increments on update)
- Signature proves authenticity

Discovery process:
1. Start with hardcoded bootnodes
2. Query for random node IDs (fills routing table)
3. Nodes gossip ENRs to neighbors
4. If node's IP changes → publishes new ENR with higher seq number
5. Network propagates updated ENR

Data verification:
- ENR signature (must match public key)
- Sequence number (newer = valid, older = ignore)
- If signature invalid → reject + ban peer
```

### 4. **Matrix Federation** - Well-Known URIs
```
Problem: How to find homeserver when DNS changes?

Solution:
Layer 1: DNS lookup
- example.com → A record → IP

Layer 2: .well-known/matrix/server
- GET https://example.com/.well-known/matrix/server
- Returns: {"m.server": "matrix.example.org:8448"}
- Can redirect to different host/port

Layer 3: Server key verification
- Each server has signing key
- Requests include timestamp + signature
- Old signatures = reject (replay protection)

Data verification:
- TLS certificate (proves domain ownership)
- Server signature (proves message authenticity)
- Timestamp (prevents replay attacks)
```

---

## 🔧 Our Mechanism: Hybrid Self-Healing Bootstrap

### Design Principles:
1. **Multiple hardcoded seeds** (no single point of failure)
2. **DHT-based announcements** (dynamic discovery)
3. **Versioned records with signatures** (data verification)
4. **Gossip propagation** (self-sustaining network)
5. **Automatic expiry** (dead nodes removed)

---

## 📋 Concrete Implementation

### Data Structure: Relay Node Record (RNR)

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct RelayNodeRecord {
    // Identity
    pub peer_id: PeerId,
    pub public_key: PublicKey,
    
    // Network address
    pub multiaddr: Multiaddr,
    pub region: String,  // "us-east", "eu-west" (hint, not enforced)
    
    // Versioning
    pub sequence: u64,        // Increments on every update
    pub timestamp: u64,       // Unix timestamp (seconds)
    
    // Health metrics
    pub uptime_percent: f32,  // 0.0-1.0 (self-reported)
    pub capacity: u32,        // Max concurrent connections
    
    // Verification
    pub signature: Vec<u8>,   // Signs(peer_id + multiaddr + sequence + timestamp)
}

impl RelayNodeRecord {
    /// Verify signature is valid
    pub fn verify(&self) -> Result<bool> {
        let message = format!("{:?}{}{}{}",
            self.peer_id,
            self.multiaddr,
            self.sequence,
            self.timestamp
        );
        self.public_key.verify(message.as_bytes(), &self.signature)
    }
    
    /// Check if record is expired (> 10 minutes old)
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        now - self.timestamp > 600  // 10 minute TTL
    }
    
    /// Check if other record is newer (higher sequence number)
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.sequence > other.sequence
    }
}
```

---

### Bootstrap Process (The Flow You Described)

```rust
pub struct BootstrapManager {
    // Hardcoded seeds (embedded in binary)
    hardcoded_relays: Vec<Multiaddr>,
    
    // Known relays (from DHT, persisted to disk)
    known_relays: HashMap<PeerId, RelayNodeRecord>,
    
    // Active connections
    connected_relays: HashSet<PeerId>,
    
    // DHT reference
    dht: Kademlia,
}

impl BootstrapManager {
    /// Bootstrap flow: Try cached → hardcoded → DHT → gossip
    pub async fn bootstrap(&mut self) -> Result<()> {
        // STEP 1: Try cached relays from disk (fastest)
        info!("Trying cached relays...");
        if self.try_cached_relays().await? >= 2 {
            info!("Connected to 2+ cached relays, bootstrap complete");
            return Ok(());
        }
        
        // STEP 2: Try hardcoded relays in parallel
        info!("Trying hardcoded relays...");
        if self.try_hardcoded_relays().await? >= 2 {
            info!("Connected to 2+ hardcoded relays");
        }
        
        // STEP 3: Query DHT for more relays
        info!("Querying DHT for relay list...");
        self.refresh_relay_list_from_dht().await?;
        
        // STEP 4: Start gossip (get relays from connected peers)
        info!("Starting peer exchange...");
        self.request_peer_lists().await?;
        
        Ok(())
    }
    
    /// Query DHT for "bootstrap:relays" key
    async fn refresh_relay_list_from_dht(&mut self) -> Result<()> {
        let key = "bootstrap:relays".as_bytes();
        
        // Get all providers for this key
        let records = self.dht.get_record(key).await?;
        
        for record in records {
            // Deserialize record
            let rnr: RelayNodeRecord = bincode::deserialize(&record.value)?;
            
            // YOUR FLOW CHART:
            // 1. Does data exist? YES (we got record)
            
            // 2. Is it the data I expect?
            if !self.is_valid_record(&rnr) {
                warn!("Invalid record from DHT: {:?}", rnr.peer_id);
                continue;  // Reject
            }
            
            // 3. Is it newer than what I have?
            if let Some(existing) = self.known_relays.get(&rnr.peer_id) {
                if !rnr.is_newer_than(existing) {
                    debug!("Ignoring older record for {:?}", rnr.peer_id);
                    continue;  // Ignore, keep my data
                }
            }
            
            // 4. Is it valid/verified?
            if !rnr.verify()? {
                warn!("Signature verification failed for {:?}", rnr.peer_id);
                // Mark source as suspicious
                self.mark_peer_suspicious(record.publisher);
                continue;  // Reject
            }
            
            // 5. Is it expired?
            if rnr.is_expired() {
                debug!("Record expired for {:?}", rnr.peer_id);
                continue;  // Ignore
            }
            
            // ALL CHECKS PASSED → Update own data
            info!("Accepted new relay record: {:?}", rnr.peer_id);
            self.known_relays.insert(rnr.peer_id, rnr.clone());
            
            // Try to connect
            self.try_connect_relay(&rnr).await?;
        }
        
        // Persist updated list to disk
        self.save_relay_cache()?;
        
        Ok(())
    }
    
    fn is_valid_record(&self, rnr: &RelayNodeRecord) -> bool {
        // Basic sanity checks
        if rnr.sequence == 0 {
            return false;  // Sequence must be positive
        }
        
        if rnr.timestamp == 0 {
            return false;  // Must have timestamp
        }
        
        // Timestamp can't be in future (allow 5 min clock skew)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if rnr.timestamp > now + 300 {
            return false;
        }
        
        // Uptime must be 0.0-1.0
        if rnr.uptime_percent < 0.0 || rnr.uptime_percent > 1.0 {
            return false;
        }
        
        true
    }
}
```

---

### Relay Self-Announcement (The Other Side)

```rust
pub struct RelayServer {
    peer_id: PeerId,
    keypair: Keypair,
    public_addr: Multiaddr,
    region: String,
    sequence: u64,  // Increments on update
    dht: Kademlia,
}

impl RelayServer {
    /// Announce ourselves to DHT every 5 minutes
    pub async fn announce_loop(&mut self) {
        let mut interval = tokio::time::interval(Duration::from_secs(300));
        
        loop {
            interval.tick().await;
            
            // Increment sequence number
            self.sequence += 1;
            
            // Create record
            let rnr = RelayNodeRecord {
                peer_id: self.peer_id,
                public_key: self.keypair.public(),
                multiaddr: self.public_addr.clone(),
                region: self.region.clone(),
                sequence: self.sequence,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                uptime_percent: self.calculate_uptime(),
                capacity: self.max_connections(),
                signature: Vec::new(),  // Filled below
            };
            
            // Sign record
            let message = format!("{:?}{}{}{}",
                rnr.peer_id,
                rnr.multiaddr,
                rnr.sequence,
                rnr.timestamp
            );
            let signature = self.keypair.sign(message.as_bytes())?;
            let mut rnr = rnr;
            rnr.signature = signature;
            
            // Publish to DHT
            let key = "bootstrap:relays".as_bytes();
            let value = bincode::serialize(&rnr)?;
            
            match self.dht.put_record(key.to_vec(), value, Quorum::One).await {
                Ok(_) => info!("Announced to DHT (seq: {})", self.sequence),
                Err(e) => warn!("Failed to announce to DHT: {}", e),
            }
        }
    }
    
    /// If our address changes (NAT rebind, new IP), increment sequence
    pub fn update_address(&mut self, new_addr: Multiaddr) {
        self.public_addr = new_addr;
        self.sequence += 1;  // Force update propagation
        // Next announce_loop iteration will publish new record
    }
}
```

---

## 🛡️ Stability Mechanisms

### 1. **Multiple Hardcoded Seeds (Redundancy)**
```rust
// Embedded in binary (never changes)
const HARDCODED_RELAYS: &[&str] = &[
    "/ip4/49.182.84.9/tcp/4001/p2p/12D3Koo...",      // Primary
    "/ip4/104.131.131.82/tcp/4001/p2p/12D3Koo...",   // Backup 1
    "/ip4/178.62.123.45/tcp/4001/p2p/12D3Koo...",    // Backup 2
    "/dns4/relay1.metaverse.org/tcp/4001/p2p/...",   // DNS fallback
    "/dns4/relay2.metaverse.org/tcp/4001/p2p/...",   // DNS fallback 2
];

// Try all in parallel, succeed with 2+
async fn try_hardcoded_relays(&mut self) -> Result<usize> {
    let futures: Vec<_> = HARDCODED_RELAYS
        .iter()
        .map(|addr| self.try_connect(addr))
        .collect();
    
    let results = futures::future::join_all(futures).await;
    let connected = results.iter().filter(|r| r.is_ok()).count();
    
    Ok(connected)
}
```

### 2. **Sequence Numbers (Version Vector)**
```rust
// Prevents old data overwriting new data
if new_record.sequence <= existing_record.sequence {
    // Ignore stale update
    return;
}

// Example scenario:
// Relay changes IP: 1.2.3.4 (seq 5) → 5.6.7.8 (seq 6)
// Some peer has cached old record (seq 5)
// DHT has new record (seq 6)
// Peer queries DHT → gets seq 6 → updates cache
// Old seq 5 records ignored everywhere
```

### 3. **Signatures (Authenticity)**
```rust
// Only the relay owner can create valid records
// Prevents:
// - Impersonation (can't fake another relay)
// - Injection attacks (can't poison DHT with fake relays)
// - Man-in-middle (signature proves source)

if !record.verify() {
    // Signature invalid → reject + ban peer
    self.blacklist.insert(publisher_peer_id);
    return;
}
```

### 4. **Timestamps + TTL (Expiry)**
```rust
// Records auto-expire if not refreshed
const TTL: u64 = 600;  // 10 minutes

if now - record.timestamp > TTL {
    // Record expired → remove from cache
    self.known_relays.remove(&record.peer_id);
}

// Why this works:
// - Live relays re-announce every 5 min → stay fresh
// - Dead relays don't re-announce → expire after 10 min
// - Network self-cleans without manual intervention
```

### 5. **Gossip Propagation (Redundancy)**
```rust
// Don't rely on DHT alone - peers share relay lists
async fn peer_exchange(&mut self, peer: PeerId) {
    // Request: "Send me your relay list"
    let request = PeerExchangeRequest {
        max_relays: 20,
    };
    
    let response = self.send_request(peer, request).await?;
    
    // Merge their list into ours (apply same verification)
    for rnr in response.relays {
        self.process_relay_record(rnr).await?;
    }
}

// Every connected peer shares lists
// Network becomes self-sustaining mesh
// Even if DHT fails, gossip keeps it alive
```

---

## 🔄 Example Lifecycle

### Scenario: Relay Changes IP Address

**Initial state:**
```
Relay "R1":
  - IP: 1.2.3.4
  - Sequence: 10
  - Announced to DHT
  
Clients have cached:
  - R1 at 1.2.3.4 (seq 10)
```

**Event: Relay's IP changes (NAT rebind, server migration, etc.)**

**Step 1: Relay detects address change**
```rust
// Relay's network stack detects new external address
relay.update_address("/ip4/5.6.7.8/tcp/4001".parse()?);
// Sequence: 10 → 11 (incremented)
```

**Step 2: Relay announces new record**
```rust
// Next announce cycle (within 5 minutes)
let new_record = RelayNodeRecord {
    peer_id: R1,
    multiaddr: "/ip4/5.6.7.8/tcp/4001",
    sequence: 11,  // Incremented!
    timestamp: now(),
    signature: sign(...),
};

dht.put_record("bootstrap:relays", new_record);
```

**Step 3: Clients refresh from DHT**
```rust
// Client queries DHT (every 5-10 minutes)
let records = dht.get_record("bootstrap:relays").await?;

for record in records {
    // Found R1 with seq 11 (vs our cached seq 10)
    if record.sequence > our_cache.sequence {
        // Update cache: 1.2.3.4 (seq 10) → 5.6.7.8 (seq 11)
        known_relays.insert(R1, record);
    }
}
```

**Step 4: Clients reconnect**
```rust
// Old connection to 1.2.3.4 fails
// Try new address from cache: 5.6.7.8
// Success! Connection restored
```

**Result:**
- Network adapted to IP change
- No manual intervention needed
- Stale records automatically ignored (sequence number)
- System remains stable throughout

---

## 🎯 Summary: How It Actually Works

### Your Flow Chart in Code:

```rust
async fn process_relay_record(&mut self, rnr: RelayNodeRecord) -> Result<()> {
    // 1. Does data exist?
    // (Yes - we received it)
    
    // 2. Is it the data I expect?
    if !self.is_valid_record(&rnr) {
        warn!("Invalid record format");
        return Err(Error::InvalidRecord);
    }
    
    // 3. Is it newer than what I have?
    if let Some(existing) = self.known_relays.get(&rnr.peer_id) {
        if rnr.sequence <= existing.sequence {
            // NO → Ignore and keep my data
            debug!("Ignoring older record (seq {} vs {})", 
                   rnr.sequence, existing.sequence);
            return Ok(());
        }
    }
    
    // 4. Is it valid/verified?
    if !rnr.verify()? {
        // NO → Reject, mark source as bad
        warn!("Signature verification failed");
        self.mark_peer_suspicious(rnr.peer_id);
        return Err(Error::InvalidSignature);
    }
    
    // YES → Update own data
    info!("Accepting relay record: {:?} (seq {})", rnr.peer_id, rnr.sequence);
    self.known_relays.insert(rnr.peer_id, rnr.clone());
    
    // Propagate to others (optional gossip)
    self.propagate_to_neighbors(&rnr).await?;
    
    // Persist to disk
    self.save_cache()?;
    
    Ok(())
}
```

### The Key Mechanisms:

1. **Sequence numbers** → Prevent old data overwriting new
2. **Signatures** → Verify data authenticity (like signed git commits)
3. **TTL + Timestamps** → Auto-remove dead nodes
4. **Multiple hardcoded seeds** → Redundancy, no single point of failure
5. **DHT announcements** → Dynamic discovery, self-updating list
6. **Gossip propagation** → Self-sustaining mesh, survives DHT failure
7. **Cached peer list** → Instant reconnect, survives total network failure

### How It's Stable Despite Constant Change:

- **No single source of truth** → Multiple relays, DHT, gossip, hardcoded
- **Graceful degradation** → If DHT fails, use gossip. If gossip fails, use hardcoded.
- **Verification at every step** → Bad data rejected immediately
- **Automatic cleanup** → Dead nodes expire without manual intervention
- **Version vectors** → Conflicts resolved automatically (highest sequence wins)

**This is NOT like rolling cache because:**
- Rolling cache = eventual consistency, no verification
- This = immediate verification, signed records, version control
- Rolling cache = "trust the latest"
- This = "verify then trust"

---

## 🚀 Next Steps

**To implement this:**
1. Define `RelayNodeRecord` struct with signature support
2. Add sequence number tracking to relay server
3. Implement DHT announcement loop (every 5 min)
4. Implement client refresh loop (every 5-10 min)
5. Add peer exchange protocol (gossip backup)
6. Add relay cache persistence (JSON/bincode to disk)

**Open questions:**
- How many relays in hardcoded list? (5-10 seems reasonable)
- DHT announcement interval? (5 min = good balance)
- Client refresh interval? (5-10 min = responsive enough)
- Gossip fanout? (share with how many peers?)
- Blacklist policy? (how long to ban suspicious peers?)

**Does this address your question about "how do we actually do it"?**
