# TECHNICAL SPECIFICATION

**Purpose:** Detailed architectural decisions, data structures, algorithms, and rationale.
**Last Updated:** 2026-02-13

---

## 1. COORDINATE SYSTEM

### 1.1 Why Spherical Coordinates Matter

The Earth is a sphere (oblate spheroid). A flat planar projection accumulates error with distance:
- At 1km: <0.1% error (acceptable)
- At 100km: ~0.5% error (marginal)
- At 1000km: ~5% error (unacceptable)
- At global scale: completely broken

**The world MUST be modelled as a sphere from the ground up.**

### 1.2 Coordinate Spaces

The system uses three coordinate spaces:

**ECEF (Earth-Centered, Earth-Fixed)** — Absolute reference
- Origin: centre of Earth
- X axis: through equator at 0° longitude (Greenwich)
- Y axis: through equator at 90° East
- Z axis: through North Pole
- Units: metres (f64)
- Used for: canonical storage, cross-chunk calculations, global positioning
- Conversion from GPS: standard geodetic → ECEF formulas (WGS84 ellipsoid)

**Chunk-Local Cartesian** — Per-chunk rendering/simulation
- Origin: centre of the chunk's surface patch
- Axes: tangent to sphere surface at chunk centre (East, Up, North or similar)
- Units: metres (f32 — sufficient for <500m chunks)
- Used for: rendering, physics, local entity positions
- WHY: float32 has ~7 decimal digits of precision. At global scale (10^7 metres), you lose sub-metre precision. By anchoring to chunk origin, all local values are small (<500m) and float32 is accurate to millimetres.

**GPS (Geodetic)** — Human-readable, data input
- Latitude, Longitude, Elevation (WGS84)
- Used for: OSM data, user input, teleportation, display
- Converted to ECEF on ingestion, never used for simulation

### 1.3 Conversion Pipeline

```
User Input / OSM Data
        │
        ▼
   GPS (lat, lon, elev)
        │
        ▼  [WGS84 geodetic → ECEF]
   ECEF (x, y, z) f64           ← canonical absolute position
        │
        ▼  [ECEF → chunk ID + local offset]
   (ChunkID, LocalPos f32)      ← runtime position for rendering/physics
```

### 1.4 Precision Guarantees

| Scale | Coordinate Space | Precision | Acceptable |
|-------|-----------------|-----------|------------|
| <500m (within chunk) | Chunk-Local f32 | <0.1mm | ✅ |
| <100km (city) | ECEF f64 | <1mm | ✅ |
| Global | ECEF f64 | <1m | ✅ |
| Cross-chunk (seams) | ECEF f64 → two Local f32 | <1mm | ✅ |

### 1.5 Floating Origin

The renderer uses a **floating origin** technique:
- Camera is always at or near (0, 0, 0) in render space
- The world is translated relative to the camera
- This prevents GPU float32 precision loss for distant geometry
- Implementation: subtract camera ECEF from all entity ECEF before converting to render coords

---

## 2. SPHERICAL CHUNKING SYSTEM

### 2.1 Quad-Sphere (Cube-to-Sphere Projection)

The chunk system uses a **quad-sphere**: a cube whose vertices are normalised onto a sphere.

**Why quad-sphere:**
- 6 cube faces → 6 independent quadtrees for LOD
- Near-uniform cell sizes (unlike lat/lon grids which distort at poles)
- Simple parent/child tile addressing (quadtree per face)
- GPU-friendly (rectangular patches map cleanly to texture tiles)
- Proven in planet renderers (Outerra, Google Earth, many research papers)

**Tile addressing:**
```
FaceID (0-5) + QuadTree Path (e.g., [0, 3, 1, 2])
```
- Face 0-5: top, bottom, front, back, left, right of cube
- Quadtree path: recursive subdivision index (0=TL, 1=TR, 2=BL, 3=BR)
- Depth determines LOD level and tile size

**Tile sizes at different depths:**

| Depth | Approx Tile Size | Use Case |
|-------|-----------------|----------|
| 0 | ~6,700 km (entire face) | Planet-scale LOD |
| 4 | ~420 km | Country/region view |
| 8 | ~26 km | City view |
| 12 | ~1.6 km | Neighbourhood view |
| 14 | ~400 m | Street-level detail |
| 16 | ~100 m | Building-level detail |
| 18 | ~25 m | Room-level detail |
| 20 | ~6 m | Object-level detail |

**The system dynamically subdivides based on camera distance.**

### 2.2 Chunk Data Structure

```rust
/// Unique identifier for a chunk tile on the quad-sphere
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChunkId {
    /// Which cube face (0-5)
    pub face: u8,
    /// Quadtree path from root to this tile
    /// Each element is 0-3 (TL, TR, BL, BR)
    pub path: Vec<u8>,
}

/// A loaded chunk containing world data
pub struct Chunk {
    pub id: ChunkId,
    /// Centre of this tile in ECEF
    pub center_ecef: DVec3,
    /// Bounding sphere radius (metres)
    pub bounding_radius: f64,
    /// LOD level (= path.len())
    pub lod: u8,
    /// Static geometry (buildings, roads, terrain mesh)
    pub static_mesh: Option<ChunkMesh>,
    /// Sparse voxel octree for mutable volumetric data
    pub svo: Option<SparseVoxelOctree>,
    /// Entities in this chunk (NPCs, objects, vehicles)
    pub entities: Vec<Entity>,
    /// Chunk manifest (content hash, signatures, provenance)
    pub manifest: ChunkManifest,
}
```

### 2.3 Seam Handling

Where quad-sphere faces or tiles meet, geometry must be stitched:
- Adjacent tiles share edge vertices
- LOD transitions use skirt geometry (downward-facing triangles at tile edges) to hide T-junction cracks
- Terrain heightmap samples overlap by 1 pixel at edges for smooth interpolation

---

## 3. VOLUMETRIC DATA MODEL (SVO)

### 3.1 Why Sparse Voxel Octrees

The world must be fully mutable (build/destroy). Options:
- **Dense voxel grid:** 1m³ voxels for Earth = 10²¹ voxels. Impossible.
- **Mesh-only:** Can't do destruction without re-meshing (expensive, complex).
- **SVO:** Stores only non-empty regions. Hierarchical LOD built in. Efficient for both sparse (mostly air) and dense (solid ground) regions.

### 3.2 SVO Structure

```rust
/// A node in the sparse voxel octree
pub enum SvoNode {
    /// Empty space — no children, no data
    Empty,
    /// Uniform solid — entire octant is one material
    Solid(MaterialId),
    /// Mixed — has 8 children (some may be Empty/Solid)
    Branch(Box<[SvoNode; 8]>),
}

/// Material identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialId(pub u16);
```

### 3.3 Operations on SVO

All operations are deterministic and produce an op log entry:

```rust
pub enum SvoOp {
    /// Set a voxel at (x, y, z) within chunk-local SVO space
    SetVoxel { x: u32, y: u32, z: u32, depth: u8, material: MaterialId },
    /// Clear a voxel (destruction)
    ClearVoxel { x: u32, y: u32, z: u32, depth: u8 },
    /// Fill a region (bulk placement)
    FillRegion { min: UVec3, max: UVec3, depth: u8, material: MaterialId },
}
```

### 3.4 Vertical Extent

Each chunk tile covers a vertical column:
- **Deep underground:** -6,371 km (core) to -200m — very low LOD, mostly solid rock material
- **Near underground:** -200m to 0m — basements, tunnels, parking, subways (from OSM `layer=-1`, `tunnel=yes`)
- **Surface:** 0m (terrain height) — terrain mesh + buildings + roads
- **Above ground:** 0m to 500m — buildings, bridges, overpasses
- **Atmosphere:** 500m to 100km — weather, clouds, aircraft
- **Space:** 100km to 35,786km (GEO) — satellites, space stations
- **Deep space:** 35,786km+ — Moon, celestial bodies (extremely low LOD, mostly skybox)

Most chunks will only have detailed SVO data near the surface. Deep rock and atmosphere are represented as uniform SVO nodes (extremely cheap).

---

## 4. DATA PIPELINES

### 4.1 OpenStreetMap (Buildings, Roads, Infrastructure)

- **Source:** Overpass API (rate-limited, 2-second cooldown per request)
- **Coverage:** Global, community-maintained, ODbL licensed
- **What we extract:**
  - Buildings: footprint polygons, height (levels), type (residential/commercial/industrial), addresses
  - Roads: centreline polylines, type (motorway/residential/path), width, surface, lanes
  - Water: polygons for rivers, lakes, coastlines
  - Parks: polygons for green spaces
  - Railways, power lines, bridges, tunnels
  - Underground features: `layer=-1`, `tunnel=yes`, `parking=underground`
- **Pipeline:** GPS query → Overpass JSON → parse into typed Rust structs → assign to chunk → cache to disk
- **Caching:** `~/.metaverse/cache/osm/<chunk_id>.bin` — binary serialised, 30-day expiry
- **Rate limiting:** 2-second minimum between API calls; exponential backoff on 429/timeout

### 4.2 SRTM Elevation (Terrain)

- **Source:** NASA Shuttle Radar Topography Mission
- **Format:** HGT files (binary, 1-arc-second or 3-arc-second resolution)
- **Coverage:** Global (between 60°N and 56°S)
- **Resolution:** ~30m (1-arc-second) or ~90m (3-arc-second)
- **Pipeline:** Download HGT tile → parse binary → generate heightmap per chunk → create terrain mesh → snap buildings to terrain height
- **Caching:** `~/.metaverse/cache/srtm/<tile>.hgt`

### 4.3 Satellite Imagery (Future — Textures)

- **Sources:** Mapbox, Sentinel-2, Bing Maps (licensing dependent)
- **Use:** Ground textures, building facade hints, vegetation classification
- **Pipeline:** Tile-based download → virtual texturing system → composite with procedural detail
- **Priority:** LOW — implement after core geometry pipeline is proven

### 4.4 Prefetch & Caching Strategy

All data sources follow the same pattern:

```
Request data for chunk
  ├─ Check memory cache (HashMap) → HIT → return immediately
  ├─ Check disk cache (~/.metaverse/cache/) → HIT → deserialise, populate memory, return
  └─ MISS → fetch from API/file (with rate limit + cooldown)
       → serialise to disk cache
       → populate memory cache
       → return
```

- Memory cache: LRU with configurable max size (default: 500 chunks)
- Disk cache: content-addressed files, 30-day expiry, optional shared P2P cache nodes
- Prefetch: when player moves, predict direction and preload chunks ahead of movement vector
- Cooldowns: 2-second minimum between external API calls; batch requests where possible

---

## 5. RENDERER ARCHITECTURE

### 5.1 Why Not Bevy / Why Not Unreal

**Bevy:** Excellent for prototyping. Cannot deliver AAA fidelity. No nanite-style mesh streaming, limited material system, no virtual texturing, no advanced GI. Useful for testing coordinate math, NOT for the final product.

**Unreal:** Can deliver AAA fidelity. However: C++ codebase, licensing constraints, massive binary size, difficult to integrate with Rust core, opinionated architecture that fights custom spherical world model. Also: solo dev can't maintain Unreal plugin + Rust core simultaneously.

**Decision: Custom wgpu/Vulkan renderer in Rust.**

Rationale:
- Full control over rendering pipeline (critical for spherical geometry, floating origin, LOD)
- Single language (Rust) for entire stack
- wgpu abstracts Vulkan/Metal/DX12/WebGPU — cross-platform by default
- Can implement exactly the features needed (deferred PBR, GPU-driven culling, virtual texturing) without engine overhead
- Trade-off: slower time-to-AAA-fidelity, but more sustainable for solo/small team

### 5.2 Rendering Pipeline (Target)

```
Visibility Determination
  ├─ Frustum cull (GPU-driven, per-chunk bounding spheres)
  ├─ Occlusion cull (hierarchical Z-buffer or software rasteriser)
  └─ LOD select (based on screen-space size / distance)
       │
       ▼
Geometry Pass (Deferred)
  ├─ G-Buffer: albedo, normal, roughness, metallic, depth, emissive
  ├─ Per-chunk instanced draw calls
  ├─ SVO → mesh (marching cubes or dual contouring for destructible surfaces)
  └─ Terrain mesh (heightmap-derived, per-chunk)
       │
       ▼
Lighting Pass
  ├─ Directional light (sun) with cascaded shadow maps
  ├─ Point/spot lights (streetlights, interior lights, vehicle lights)
  ├─ Ambient occlusion (SSAO or GTAO)
  └─ Global illumination (screen-space initially; SVO cone tracing later)
       │
       ▼
Post-Processing
  ├─ Tone mapping (ACES)
  ├─ Bloom
  ├─ TAA (temporal anti-aliasing)
  ├─ Atmospheric scattering (Bruneton model for sky/horizon)
  ├─ Depth of field, motion blur (optional, VR-off)
  └─ UI overlay
```

### 5.3 Target Framerates

| Platform | Target FPS | Minimum FPS |
|----------|-----------|-------------|
| Desktop (mid-range, e.g. RTX 3060) | 60 | 30 |
| Desktop (high-end) | 120+ | 60 |
| VR (future) | 90 | 72 |
| Mobile (future) | 30 | 20 |

### 5.4 Graphics Quality Tiers

Different hardware gets different feature sets:

| Tier | Shadow Maps | GI | LOD Bias | Draw Distance | Post-FX |
|------|------------|----|---------|--------------|---------| 
| Potato | 1 cascade, 1024px | None | +2 (less detail) | 2km | Minimal |
| Low | 2 cascade, 2048px | SSAO only | +1 | 5km | Basic |
| Medium | 3 cascade, 4096px | SSAO + SSR | 0 | 10km | Full |
| High | 4 cascade, 4096px | SSAO + SSR + GI | -1 (more detail) | 20km | Full + DOF |
| Ultra | 4 cascade, 8192px | Full SVO GI | -2 | 50km+ | Everything |

The client launcher detects hardware and selects tier automatically. User can override.

---

## 6. ENTITY SYSTEM

### 6.1 Entity-Component Architecture

All world objects are entities with components:

```rust
pub struct EntityId(pub u64);  // Globally unique, content-addressed

pub struct Transform {
    pub chunk: ChunkId,
    pub local_pos: Vec3,     // f32, relative to chunk origin
    pub rotation: Quat,
    pub scale: Vec3,
}

pub struct Renderable {
    pub mesh_id: AssetId,
    pub material_id: AssetId,
    pub lod_group: u8,
}

pub struct Interactable {
    pub interaction_type: InteractionType,
    pub state: InteractionState,
}

pub enum InteractionType {
    Door { open: bool },
    Screen { content_url: String, playing: bool },
    Switch { on: bool },
    Container { inventory: Vec<ItemId> },
    Vehicle { seats: u8, speed: f32 },
}
```

### 6.2 Shopfront TV Example (Concrete)

When a player walks past a shop and sees a TV:

1. **LOD far (>100m):** Shop is a simple box mesh with a facade texture. TV is a bright pixel on the texture.
2. **LOD medium (20-100m):** Shop has window geometry. TV is an emissive quad with an animated sprite sheet.
3. **LOD near (<20m):** Shop interior loads. TV is a mesh with a video texture (decoded from mp4 or streamed). Interior lights, shelves, products visible.

The TV entity:
```rust
Entity {
    transform: Transform { chunk: cbd_chunk, local_pos: vec3(12.3, 1.5, -4.2), .. },
    renderable: Renderable { mesh_id: tv_mesh, material_id: tv_screen_mat, .. },
    interactable: Interactable {
        interaction_type: InteractionType::Screen {
            content_url: "metaverse://media/news-channel-9",
            playing: true,
        },
        ..
    },
}
```

State changes (channel switch, on/off) are replicated via the op log to all nearby peers.

---

## 7. NETWORKING ARCHITECTURE

### 7.1 Hybrid P2P Model

**Primary: Peer-to-Peer**
- Users connect directly to nearby users (geographically)
- Discovery via geo-sharded DHT (libp2p Kademlia with geohash-tagged keys)
- State sync via CRDT op logs (Gossipsub pubsub per chunk region)

**Secondary: Helper Servers**
- Cache servers: serve pre-fetched chunk data (OSM/SRTM baked), reduce API pressure
- Bootstrap nodes: help new peers find the network
- Update servers: distribute client updates, patches, asset packs
- NOT authoritative: if servers go down, P2P still works (degraded but functional)

### 7.2 Geo-Sharded DHT

Peers are indexed by their current quad-sphere chunk ID in the DHT:
- When you move, you update your DHT record
- To find nearby peers, query DHT for your chunk ID and neighbours
- Chunk IDs are hierarchical — can query at different LOD levels for different scales of "nearby"

### 7.3 State Synchronisation (CRDT + Op Log)

Every mutable action produces a signed operation:

```rust
pub struct SignedOp {
    pub op: WorldOp,
    pub author: PublicKey,     // Ed25519
    pub signature: Signature,  // Signs (op + timestamp + chunk_id)
    pub timestamp: u64,        // Lamport clock or hybrid logical clock
    pub chunk_id: ChunkId,
}

pub enum WorldOp {
    SvoEdit(SvoOp),                    // Voxel modification
    EntitySpawn(EntityId, EntityData), // Place new entity
    EntityRemove(EntityId),            // Remove entity
    EntityUpdate(EntityId, ComponentDelta), // Modify entity component
    OwnershipClaim(VolumetricParcel, PublicKey), // Claim ownership
}
```

**Conflict resolution:**
- Additive ops (place block, spawn entity): merge freely (CRDT set union)
- Destructive ops (remove same block): deterministic ordering by (timestamp, author_pubkey) — last-writer-wins with tiebreak
- Ownership conflicts: first-valid-claim wins; disputes resolved by timestamp + stake (future governance)

### 7.4 Bandwidth Optimisation

- Only subscribe to chunks within interaction radius (configurable, default 2km)
- Send deltas only (not full state)
- Compress ops with binary encoding (bincode or CBOR)
- Batch ops per tick (default: 20 ticks/sec for state, 60 ticks/sec for position)
- Position updates use dead-reckoning: only send corrections when predicted position diverges

---

## 8. OWNERSHIP & IDENTITY

### 8.1 User Identity

- Each user has an Ed25519 keypair generated locally
- Public key = identity (no central account server)
- Display name, avatar, and profile are signed metadata attached to public key
- Key backup is user's responsibility (export as encrypted file)

### 8.2 Volumetric Ownership

Ownership is of 3D volumes, not 2D land plots:

```rust
pub struct VolumetricParcel {
    pub chunk_id: ChunkId,
    pub min_local: Vec3,   // Bottom corner of owned volume (chunk-local)
    pub max_local: Vec3,   // Top corner of owned volume (chunk-local)
    pub owner: PublicKey,
    pub claimed_at: u64,   // Timestamp
    pub signature: Signature,
}
```

- You own a 3D box within a chunk
- You can build/destroy within your parcel
- Others can traverse your parcel but not modify it (unless permissioned)
- Parcels cannot overlap (enforced by CRDT merge rules)

### 8.3 Provenance

Every chunk manifest includes:
- Content hash of all geometry + SVO data
- List of signed ops that produced the current state
- Source attribution (OSM data: ODbL; SRTM: public domain; user edits: signed)

---

## 9. PHYSICS

### 9.1 Deterministic Simulation

- Physics engine: Rapier (Rust-native, deterministic mode)
- Fixed timestep: 60 Hz (16.67ms per tick)
- Deterministic: same inputs → same outputs on all platforms
- Used for: gravity, collision, rigid body dynamics, character controller, vehicles
- NOT used for: cosmetic effects (particles, cloth — these are client-side only)

### 9.2 Collision Geometry

- Terrain: heightmap-derived trimesh collider per chunk
- Buildings: simplified convex hull colliders (auto-generated from mesh)
- SVO destructible regions: voxel collider (updated on SVO change)
- Entities: per-entity collider shapes (box, capsule, trimesh)

---

## 10. MULTI-PLATFORM STRATEGY

### 10.1 Client Tiers

Each platform gets a tailored launcher that selects features:

| Platform | Renderer | LOD Bias | Draw Distance | Physics | P2P |
|----------|---------|---------|--------------|---------|-----|
| Desktop (high) | Full wgpu pipeline | 0 | 20km | Full | Full |
| Desktop (low) | Reduced pipeline | +2 | 5km | Full | Full |
| VR | Full + reprojection | 0 | 10km | Full | Full |
| Mobile (future) | Minimal pipeline | +3 | 2km | Simplified | Relay-assisted |
| Console (future) | Platform-optimised | 0 | 15km | Full | Full |
| Web (future) | WebGPU subset | +2 | 5km | Simplified | WebRTC |

### 10.2 Shared Core

ALL platforms use the same Rust core library:
- Coordinate system
- Chunk system
- SVO engine
- World manager
- Data pipelines
- Op log / CRDT
- Ownership / identity

Only the renderer, input handling, and platform integration differ per client.