# Spatial Sharding Design: Controlled Chaos Architecture

## Core Philosophy

**"Controlled chaos"** - Data flows through a tangled web of multi-faceted nodes with redundancy and fallback, not linear client-server flow.

## The Problem

**Naive approach:**
- Only broadcast within 100m radius
- Player alone in wilderness → no peers nearby
- Player digs hole → **data lost** (nobody to store it)
- No redundancy, no persistence, no reliability

**This violates the core P2P principles:**
- ❌ No redundancy
- ❌ No fallback
- ❌ No guaranteed propagation

## The Solution: Hierarchical Redundancy + Lazy Propagation

### **Layer 1: Active Push (Guaranteed Delivery)**

**Goal:** Ensure every edit has X redundant copies (e.g., 5-10 nodes)

**Algorithm:**
```
When player makes edit:
1. Find peers within 100m radius
   → Found >= X peers? Push to X closest → DONE
   
2. Not enough? Expand to 500m radius
   → Found >= X peers? Push to X closest → DONE
   
3. Still not enough? Expand to 1km radius
   → Found >= X peers? Push to X closest → DONE
   
4. Still not enough? Push to X closest players globally
   → Include relay nodes / server repeaters
   → Doesn't matter if they're in Australia (redundancy > latency)
```

**Key Properties:**
- **Guaranteed X copies** exist somewhere in the network
- **Prefer nearby** for performance
- **Accept distant** for reliability
- **Always succeed** (never lose data)

### **Layer 2: Passive Propagation (Lazy Gossip)**

**Goal:** Let data flow through the network organically

**Algorithm:**
```
Each peer periodically (e.g., every 10 seconds):
1. Check their operation log for recent ops (last 60 seconds)
2. For each operation:
   - Gossip to Y% of connected peers (e.g., 20%)
   - Random selection (not all, prevents spam)
   - Eventually reaches entire network via transitive gossip
3. Peers de-duplicate using seen_operations HashSet
```

**Key Properties:**
- **No active coordination** (each peer acts independently)
- **Eventually consistent** (data spreads like ripples)
- **Bandwidth efficient** (only Y% of peers, not broadcast)
- **Self-healing** (multiple paths, redundant delivery)

### **Layer 3: Geographic Optimization**

**Goal:** Reduce bandwidth for dense areas

**Algorithm:**
```
When many peers available:
- Prioritize peers within visibility range (100m)
- These peers NEED the data (for rendering)
- Distant peers can receive via lazy gossip later
```

**Key Properties:**
- **Fast for nearby** (active push = low latency)
- **Slow for distant** (passive gossip = high latency, but eventual)
- **Scales with density** (more players = more redundancy)

## Data Flow Architecture

### Traditional Client-Server (Linear)
```
Player 1 → Server → Player 2
          ↓
        Player 3
```
**Single path, single point of failure, predictable**

### P2P Mesh (Tangled Web)
```
Player 1 ⟷ Player 2 ⟷ Player 5
   ↕️         ↕️         ↕️
Player 3 ⟷ Player 4 ⟷ Player 6
   ↕️                   ↕️
Relay A ⟷⟷⟷⟷⟷⟷⟷⟷⟷⟷ Relay B
```
**Multiple paths, redundant delivery, controlled chaos**

**Data propagation:**
- Player 1 digs → actively pushes to Players 2, 3, 5
- Player 2 gossips to Player 4, 5 (lazy)
- Player 3 gossips to Relay A (lazy)
- Relay A gossips to Player 6, Relay B (lazy)
- Eventually all players have the data (6 different paths)

## Node Types

### 1. **Active Players**
- Humans playing the game
- Generate edits (voxel changes)
- Store recent operations (last 24 hours)
- Actively push + passively gossip

### 2. **Relay Nodes / Server Repeaters**
- No player attached, just infrastructure
- Store long-term data (weeks/months)
- High uptime, high bandwidth
- Act as "anchor points" for the network
- Players far from others → connect to relays → guaranteed storage

### 3. **Mobile Players**
- Move between regions
- Connect/disconnect from local peers dynamically
- Carry data between regions (like couriers)
- Help propagate edits across geographic gaps

## Implementation Parameters

### **Redundancy Target (X)**
- **Minimum:** 5 copies (survives 4 node failures)
- **Preferred:** 10 copies (high reliability)
- **Adjustable:** Based on network size and data importance

### **Gossip Rate (Y%)**
- **Dense areas:** 20% of peers (plenty of redundancy)
- **Sparse areas:** 50% of peers (need more coverage)
- **Interval:** Every 10 seconds (balance freshness vs bandwidth)

### **Distance Fallback Thresholds**
- **Tier 1:** 100m (visibility range, low latency)
- **Tier 2:** 500m (nearby region, medium latency)
- **Tier 3:** 1km (local area, acceptable latency)
- **Tier 4:** Global (any peer, high latency but guaranteed)

### **Operation TTL (Time To Live)**
- **Recent:** Last 1 hour (high priority gossip)
- **Active:** Last 24 hours (normal gossip)
- **Archive:** Older than 24 hours (relay nodes only)
- **Purge:** Older than 30 days (only if space needed)

## Example Scenarios

### Scenario 1: Player Alone in Wilderness
```
Alice is 500km from nearest player
1. Alice digs hole
2. Find peers in 100m → none
3. Find peers in 500m → none
4. Find peers in 1km → none
5. Find X closest peers globally:
   - Bob (200km away)
   - Charlie (350km away)
   - Relay-EU (800km away)
   - Relay-US (5000km away)
   - Dave (600km away)
6. Push to all 5 → guaranteed redundancy
7. They lazily gossip to their peers → eventual propagation
```

### Scenario 2: Player in Crowded City
```
Bob is in downtown, 50 players within 100m
1. Bob places block
2. Find peers in 100m → 50 players found
3. Push to 10 closest peers (within 20-80m)
   - Instant delivery (low latency)
   - Visible to nearby players immediately
4. Those 10 gossip to their peers (lazy)
5. Within 30 seconds, all 50 players have the data
6. Within 5 minutes, entire network has it
```

### Scenario 3: Network Partition
```
Earthquake cuts fiber cable, splits network:
- Group A: 100 players in Europe
- Group B: 100 players in Asia
- No connection between groups

1. Player in Group A makes edit
2. Propagates through Group A normally
3. Group B never receives (no path)
4. Hours later, fiber cable repaired
5. Groups reconnect, exchange missing operations
6. Vector clocks detect causality, merge correctly
7. Both groups now consistent
```

## Why This Works

### 1. **Redundancy Without Centralization**
- Every edit stored on X nodes (configurable)
- No single node critical (unlike server)
- Nodes can fail, data survives

### 2. **Adaptive to Network Topology**
- Dense areas: low latency, high bandwidth usage
- Sparse areas: high latency, guaranteed delivery
- Automatically adjusts based on peer discovery

### 3. **Bandwidth Efficient at Scale**
- Active push: O(X) messages (constant)
- Lazy gossip: O(Y% × peers) (logarithmic spread)
- Not O(N²) broadcast (too expensive)

### 4. **Eventually Consistent**
- CRDTs guarantee convergence
- Vector clocks ensure causality
- Multiple paths ensure delivery
- Time heals all partitions

### 5. **Graceful Degradation**
- 1000 players online: fast, many paths
- 10 players online: slower, fewer paths
- 1 player online: store on relays, propagate when others join
- 0 players online: relays hold the data

## Technical Debt Notes

**Not implemented yet:**
- Distance calculation (need GPS→distance function)
- Tier-based fallback (currently just gossipsub broadcast)
- Relay node infrastructure (only player nodes exist)
- Lazy gossip timing (currently immediate broadcast)
- Operation TTL / archival (currently all ops stored forever)

**Next steps:**
1. Implement distance calculation from GPS coords
2. Add peer_distance tracking to MultiplayerSystem
3. Implement tier-based active push (100m → 500m → 1km → global)
4. Add lazy gossip timer (every 10s, gossip recent ops to Y% of peers)
5. Define relay node protocol (long-term storage, high uptime)
6. Implement operation archival (move old ops to relay nodes)

**Estimated complexity:** 3-4 days full implementation
**Current priority:** TBD (user decides next feature)
