# Discovery Design Clarifications - Working Notes

## Issue 1: Content Discovery Approach ✅ RESOLVED

### USER CONFIRMED: Content-based DHT, No Geography

**The Correct Approach:**

```
Option B: Content-based DHT keys (no geography)

I need specific chunk_12345
Query DHT: "chunk:12345:providers"
Get list of peers who have that specific chunk
```

**Multi-path data flow:**
```
Query DHT: "Who has chunk_12345?"
  ↓
Response: [Peer_Alice, Peer_Bob, Node_X, Server_Y]
  ↓
Try in order:
  1. Peer_Alice (20ms) → Request chunk
  2. If fails → Peer_Bob (50ms)
  3. If fails → Node_X (100ms)
  4. If fails → Server_Y (200ms)
  
"Data must get through" - multi-path with fallbacks
```

**No geography involved:**
- ❌ NOT "chunks:near_tokyo"
- ❌ NOT "region:tile_12345"
- ✅ YES "chunk:12345:providers" (content-addressed only)

---

## Issue 2: Real-Time Streaming ✅ KEEP CURRENT SYSTEM

**Keep existing live data streaming via gossipsub:**

```
Client aware of chunk (in range, doesn't need to see it visually)
  ↓
Subscribe to updates for that chunk
  ↓
When chunk changes:
  - All aware clients get update immediately
  - Whether they can see it or not doesn't matter
  - "In range" = chunk loading range, not view frustum
```

**Key point from user:**
> "We still keep the live data streaming like we have now so whatever happens 
> in real time to whoever client is aware of the change in range, regardless 
> of if the client can see it or not."

**Example:**
```
You're walking toward a building
  ↓
ChunkStreamer loads chunks ahead (direction of travel)
  ↓
You're "aware" of those chunks (even behind you, not visible)
  ↓
Someone edits a chunk in that range
  ↓
You get the update immediately
  ↓
When you turn around, chunk already has latest state
```

---

## Issue 3: State = Current Only, No History ✅ CONFIRMED

**CRITICAL INSIGHT: No historical logs, only current state**

User said:
> "If you come to the chunk last, you only need to be aware of how that chunk 
> looks from now on. It doesn't matter what made it look that way. You're not 
> storing historical logs. You're only storing the most recent version of 
> what's currently happening."

```
You arrive at chunk that has been edited 1000 times
  ↓
You DON'T get:
  - History of changes
  - Who did what when
  - Edit logs
  
You DO get:
  - Current state of chunk (latest version)
  - That's it
```

**This is like:**
- Git: Clone gets full history ❌
- IPFS snapshot: Just latest state ✅
- Blockchain: Full transaction history ❌
- Database: Current row values only ✅

---

## Issue 4: Rolling Cache with Replication 🚧 WORKING OUT

**USER WORKING ON: Cache snapshot system**

User said:
> "If that means we have a rolling cache flush? A caching system that every X 
> seconds it creates a new one, and once the older one has been confirmed to 
> have self-replicated X amount of times, it deletes itself?? Still working it 
> out as we go..."

**Proposed mechanism:**

```
Every X seconds (maybe 60s):
  1. Create snapshot of current chunk state
  2. Announce snapshot to DHT
  3. Peers replicate snapshot
  4. Once replicated to N peers (maybe 3-5)
  5. Delete old snapshot
  
This creates rolling window of state:
  - Recent snapshots: Multiple copies
  - Old snapshots: Deleted (once replicated)
  - Very old: Gone (unless server keeps archive)
```

**Benefits:**
- ✅ Ensure redundancy (multiple copies exist)
- ✅ Garbage collection (old data pruned)
- ✅ Bandwidth efficient (don't keep everything forever)
- ✅ Recent state always available

**Example timeline:**
```
T=0s:   Snapshot_A created, announced to DHT
T=30s:  Snapshot_A replicated to 5 peers
T=60s:  Snapshot_B created, announced to DHT
        Snapshot_A confirmed replicated → can delete local copy
T=90s:  Snapshot_B replicated to 5 peers
T=120s: Snapshot_C created, announced to DHT
        Snapshot_B confirmed replicated → delete local copy
        
Keeps ~2-3 snapshots in flight, old ones pruned
```

**Questions to work out:**
- How to confirm replication? (DHT query for providers)
- What's X seconds? (60s? 5min? Based on chunk edit frequency?)
- How many replicas needed? (3? 5? 10?)
- Who keeps long-term archives? (Servers only?)

---

## Summary of Validated Design

### Content Discovery
- ✅ **Content-based DHT** (no geography)
  - Query: "chunk:12345:providers"
  - Multi-path: peers → nodes → servers
  - "Data must get through"

### Real-Time Updates
- ✅ **Keep current gossipsub streaming**
  - Aware clients get real-time updates
  - "In range" = loading range, not visual range
  - Immediate propagation of changes

### State Storage
- ✅ **Current state only, no history**
  - Latest version of chunk
  - Don't care about edit history
  - No logs, no audit trail

### Cache Management
- 🚧 **Rolling snapshots with replication**
  - Create snapshots periodically
  - Ensure N copies exist
  - Delete old once replicated
  - Still working out details

---

## Next Steps

1. Update main design doc with content-based DHT approach
2. Remove all geographic topic references
3. Design rolling cache/snapshot mechanism
4. Define replication confirmation protocol
5. Determine snapshot intervals and replica counts
