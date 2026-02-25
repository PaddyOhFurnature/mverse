# World Layer Architecture

## Three-Layer World Model

The metaverse world geometry is composed of **three distinct layers**, each with different mutability, authority, and network sync requirements.

---

## Layer 1: Base Terrain (Immutable, Authoritative)

### Purpose
The foundational Earth geometry that all clients share.

### Data Sources
- **SRTM elevation data** (NASA Shuttle Radar Topography Mission)
- **Satellite imagery** for surface textures
- **Real Earth coordinates** (GPS/ECEF)

### Properties
- ✅ **READ-ONLY** for all users
- ✅ **Authoritative** - server/dataset is source of truth
- ✅ **Deterministic** - everyone sees the same Earth
- ✅ **Cached locally** - download once, use forever
- ❌ **NOT synced via P2P** - too large, use canonical sources

### Storage Strategy
- Cache SRTM tiles locally in `elevation_cache/`
- Use OpenTopography API as fallback
- No need for CRDT - immutable data
- Hash tiles to verify integrity

### Implementation Status
- ✅ Already implemented in `TerrainGenerator`
- ✅ SRTM sampling working
- ✅ Local caching working

---

## Layer 2: Infrastructure (Immutable Once Generated, Authoritative)

### Purpose
Real-world infrastructure that modifies the base terrain but is still read-only.

### Data Sources
- **OpenStreetMap (OSM)** data
  - Roads (carved paths, flattened surfaces)
  - Rivers (carved channels)
  - Tunnels (carved through hills)
  - Bridges (placed structures spanning gaps)
  - Buildings (placed structures)

### Properties
- ✅ **READ-ONLY** for all users
- ✅ **Authoritative** - generated procedurally from OSM data
- ✅ **Modifies Layer 1** - carves, flattens, or places voxels
- ✅ **Deterministic** - same OSM data → same result
- ❌ **NOT editable** by users - part of the authoritative world
- ❌ **NOT synced via P2P** - regenerate from OSM data

### Operations
- **Carving**: Remove voxels (tunnels, river channels)
- **Flattening**: Modify terrain to level (roads)
- **Placement**: Add voxels (bridges, buildings)

### Storage Strategy
- Generate on-demand from OSM data
- Cache generated infrastructure locally
- Hash to verify consistency (OSM version + generation algorithm)
- No CRDT needed - deterministic generation

### Implementation Status
- ❌ **NOT YET IMPLEMENTED**
- **Next Phase**: OSM data integration
- Need to fetch OSM data for regions
- Need procedural generation algorithms (road carving, etc.)

---

## Layer 3: User Content (Mutable, Owned)

### Purpose
Player-created modifications to the world.

### Data Sources
- **Player actions** (dig, place, build)
- **Editable ONLY within owned parcels** or free-build zones

### Properties
- ✅ **MUTABLE** - players can edit
- ✅ **Owned** - tied to parcel ownership or permissions
- ✅ **Synced via P2P** - CRDT for conflict resolution
- ✅ **Logged** - operation history for replay/persistence
- ⚠️ **Permission-checked** - can't edit outside your parcel

### Ownership Model

#### Parcels (Private Land)
- Volumetric claims on top of Layers 1+2
- Only the **parcel owner** can edit voxels
- Coordinates define bounds: `(x1, y1, z1)` to `(x2, y2, z2)`
- Ownership stored in blockchain/distributed ledger
- **Network security**: Reject voxel ops outside owned parcels

#### Free-Build Zones (Public Land)
- Unclaimed areas (deserts, oceans, wilderness)
- **Anyone** can edit
- CRDT resolves conflicts between simultaneous edits
- May have rate limits to prevent griefing

### Storage Strategy
- **SVO (Sparse Voxel Octree)** for efficient storage
- **CRDT operation log** for conflict-free replication
- **Ed25519 signatures** on operations (already implemented)
- **Vector clocks** for causality tracking (TODO)
- **Merkle trees** for efficient sync (future)

### Network Sync
- ✅ Broadcast voxel operations via gossipsub (already working)
- ⚠️ **Verify ownership** before applying operation (TODO)
- ⚠️ **Apply CRDT merge** to resolve conflicts (TODO)
- ⚠️ **Replay operation log** for new peers (TODO)

### Implementation Status
- ✅ VoxelOperation message structure
- ✅ Ed25519 signing of operations
- ✅ Gossipsub broadcast
- ✅ Lamport clock for ordering
- ❌ **Ownership verification** (not implemented)
- ❌ **CRDT merge logic** (not implemented)
- ❌ **Vector clocks** (not implemented)

---

## Layer Composition

### Rendering Order
```
Final World = Layer 1 (terrain) 
            + Layer 2 (infrastructure modifications)
            + Layer 3 (user edits)
```

### Voxel Lookup Priority
When checking a voxel coordinate:
1. **Layer 3**: Check user content SVO first (most recent changes)
2. **Layer 2**: Check infrastructure (if voxel was carved/placed)
3. **Layer 1**: Check base terrain (SRTM elevation)

### Example: Tunnel Through Hill
1. **Layer 1**: Hill at elevation 100m (from SRTM)
2. **Layer 2**: OSM tunnel carves 5m×5m×50m void through hill
3. **Layer 3**: Player places decorative lights inside tunnel

Final result: Tunnel with lights through hill

---

## Network Security Implications

### Layer 1 Security
- **No risk** - immutable, no network operations
- Clients verify SRTM tile hashes to prevent tampering

### Layer 2 Security
- **Medium risk** - deterministic generation prevents most attacks
- Verify OSM data signatures if available
- Clients can regenerate and verify consistency

### Layer 3 Security ⚠️ **CRITICAL**
- **HIGH RISK** - players can send malicious operations
- **Required checks before applying voxel operation**:
  1. ✅ **Signature valid** (Ed25519) - already implemented
  2. ❌ **Parcel ownership check** - TODO
     - Query ownership of coordinate `(x, y, z)`
     - Verify operation signer owns the parcel
     - Reject if unauthorized
  3. ❌ **Free-build zone check** - TODO
     - If not in parcel, check if in free-build zone
     - Reject if in protected area (cities, monuments)
  4. ❌ **Rate limiting** - TODO
     - Prevent spam/griefing
     - Limit operations per second per player
  5. ❌ **CRDT merge** - TODO
     - Resolve conflicts if two players edit same voxel
     - Apply Last-Write-Wins or Add-Wins semantics

### Attack Vectors
- **Unauthorized edits**: Player tries to edit outside their parcel
  - Mitigation: Ownership verification before applying op
- **Replay attacks**: Re-send old operations to undo changes
  - Mitigation: Vector clocks + operation deduplication
- **Griefing**: Spam thousands of operations to lag peers
  - Mitigation: Rate limiting + reputation system
- **Forged operations**: Fake signatures from other players
  - Mitigation: Ed25519 verification (already implemented)

---

## Implementation Roadmap

### Phase 1: P2P Infrastructure ✅ COMPLETE
- Network topology (mDNS, gossipsub)
- Player position sync
- Voxel operation broadcast

### Phase 2: Layer 3 CRDT Sync (CURRENT)
- [ ] Apply received voxel operations to local octree
- [ ] Implement ownership/parcel checking
- [ ] CRDT merge logic for conflicts
- [ ] Vector clocks for causality
- [ ] Operation log and replay

### Phase 3: Layer 2 Infrastructure
- [ ] Fetch OSM data for regions
- [ ] Road carving algorithm
- [ ] River channel generation
- [ ] Tunnel/bridge placement
- [ ] Building generation from OSM footprints

### Phase 4: Parcel System
- [ ] Define parcel coordinate bounds
- [ ] Ownership ledger (blockchain or distributed DB)
- [ ] Free-build zone definitions
- [ ] Transfer/sale mechanics

---

## Data Flow Example: Player Digs in Their Parcel

1. **Player presses E** to dig voxel at `(100, 50, 200)`
2. **Client checks**:
   - Am I in my parcel? Yes ✅
   - Create VoxelOperation: `{ coord: (100,50,200), material: Air, timestamp, signature }`
3. **Broadcast** via gossipsub to all peers
4. **Remote peer receives** operation:
   - Verify signature ✅
   - Check ownership: Does sender own parcel at `(100,50,200)`? ✅
   - Check Layer 2: Is this coordinate protected infrastructure? No ✅
   - Apply to SVO: Set voxel to Air
   - Mark mesh dirty → regenerate
5. **All clients** now see the hole

---

## Questions to Resolve

### Parcel Ownership Storage
- Where is ownership data stored?
  - Option A: Blockchain (Ethereum/Solana)
  - Option B: Distributed hash table (IPFS/libp2p)
  - Option C: Central server (not decentralized)
- How do clients verify ownership without central authority?

### Free-Build Zone Definitions
- How are free-build zones defined?
  - Geographical (all land outside parcels)
  - Biome-based (deserts, oceans)
  - Altitude-based (underground, sky)
- Should free-build zones have size limits per player?

### Layer 2 Update Frequency
- OSM data changes over time (new roads built, etc.)
- How often do clients update infrastructure?
- How to handle divergence if clients have different OSM versions?

---

## References

- **SRTM Data**: https://www2.jpl.nasa.gov/srtm/
- **OpenStreetMap**: https://www.openstreetmap.org/
- **CRDT Theory**: "Conflict-free Replicated Data Types" (Shapiro et al.)
- **Ed25519**: https://ed25519.cr.yp.to/
- **libp2p**: https://libp2p.io/

---

**Status**: Layer 1 implemented, Layer 3 in progress, Layer 2 planned
**Author**: Architecture discussion with user on 2026-02-18
**Next**: Implement ownership verification for Layer 3 operations
