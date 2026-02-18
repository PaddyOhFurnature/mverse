# MASTER SCOPE OF WORK
# 1:1 Earth-Scale Metaverse - Complete Technical Specification
### READ ENTIRE DOCUMENT - ITS A MAJOR REFERENCE SCOPE OF WORKS ###
**Last Updated:** 2026-02-17  
**Purpose:** Single source of truth for all systems, decisions, and integration points  
**Status:** Living document - Update as reality changes  
**Reality Check:** This is a 1-3M line, 5-10 year project. Current: 3,347 lines (0.3%)

---

## TABLE OF CONTENTS

1. [PROJECT VISION & SCOPE](#1-project-vision--scope)
2. [UNPRECEDENTED CHALLENGES](#2-unprecedented-challenges)
3. [CORE SYSTEMS ARCHITECTURE](#3-core-systems-architecture)
4. [SYSTEM INTEGRATION MAP](#4-system-integration-map)
5. [SCALE REQUIREMENTS](#5-scale-requirements)
6. [TECHNICAL FOUNDATIONS](#6-technical-foundations)
7. [SUBSYSTEM SPECIFICATIONS](#7-subsystem-specifications)
8. [EMERGENT COMPLEXITY FEATURES](#8-emergent-complexity-features)
9. [NETWORK ARCHITECTURE](#9-network-architecture)
10. [PERFORMANCE TARGETS](#10-performance-targets)
11. [DEVELOPMENT PHASES](#11-development-phases)
12. [KNOWN UNKNOWNS](#12-known-unknowns)
13. [CRITICAL DECISIONS LOG](#13-critical-decisions-log)

---

## 1. PROJECT VISION & SCOPE

### 1.1 What This Is

**A fully interactive, 1:1 scale spherical Earth metaverse where:**
- Every square meter of Earth exists and is explorable
- Real-world data (buildings, terrain, roads) provides the base
- Players can build, destroy, and modify ANYTHING
- Physics simulates gravity, collision, materials, fluids
- Multiplayer via P2P (no central servers for world state)
- AAA visual fidelity (No Man's Sky quality, not Minecraft blocks)
- Persistent modifications (your changes stay forever via CRDT)
- Emergent gameplay (footprints, plant growth, erosion, weather)

### 1.2 Scale Reality

**Earth Dimensions:**
- Radius: 6,371,000 meters
- Surface area: 510,072,000 km²
- Circumference: 40,075 km

**At 1m voxel resolution:**
- Total voxels (surface to 2km deep): ~1 quadrillion (10^15)
- Data if fully loaded: ~1 petabyte (impossible)
- Players could walk: 27.4 years to circle equator (24/7)

**Comparison to Other Games:**
- Minecraft: 9.2 billion km² (18,000× Earth)
  - But: Infinite plane, not sphere
  - But: Much lower detail (1m blocks vs realistic terrain)
  - But: No real-world data integration
  
- No Man's Sky: 18 quintillion planets
  - But: Procedural, not 1:1 real Earth
  - But: Isolated planets, not continuous surface
  - But: Lower terrain detail

- Star Citizen: Single star system
  - But: Sparse (planets separated by void)
  - But: Not fully interactive voxels
  - But: Limited building/destruction

**This project combines:**
- Earth's real scale (Star Citizen)
- Voxel interactivity (Minecraft)
- Visual quality (No Man's Sky)
- Real-world data (Microsoft Flight Simulator)
- P2P networking (BitTorrent + Ethereum)

**Nothing like this exists. Nothing even close.**

### 1.3 Inspiration & Comparisons

**Visual Target:** No Man's Sky
- Organic terrain (not blocky)
- Atmospheric effects
- Smooth LOD transitions
- "Pretty good" not photorealistic

**Gameplay Target:** GTA V meets Minecraft
- Open world exploration
- Vehicle physics
- Building/destruction mechanics
- Emergent player interactions

**Network Target:** Eve Online meets BitTorrent
- P2P world state (no central server)
- Player-owned infrastructure
- Cryptographic identity (Ed25519)
- Conflict-free replicated data types (CRDT)

### 1.4 What Makes This Unprecedented

**Technical Challenges Never Solved Together:**

1. **Scale × Detail × Interaction**
   - Flight sims: Scale + Detail, but no interaction
   - Voxel games: Detail + Interaction, but not real-world scale
   - Space games: Scale + Interaction, but sparse (not continuous surface)
   - **This project: ALL THREE**

2. **Spherical Coordinates at Centimeter Precision**
   - GPS: Only accurate to ~5m
   - Game engines: Assume flat worlds or small spheres
   - CAD: High precision but small scale
   - **Need: 1cm precision across 40,000km circumference**

3. **P2P State Sync for Voxel Modifications**
   - Blockchain: Too slow for real-time
   - Client-server: Central authority (we don't want)
   - Git: Not designed for 3D spatial data
   - **Need: CRDT for 3D voxel grid, spatially sharded**

4. **Real-World Data Integration at Scale**
   - OSM: 8 billion nodes, 900M ways (buildings/roads)
   - SRTM: 200GB elevation data (30m resolution)
   - **Need: Stream, process, voxelize in real-time**

5. **Physics Simulation on Spherical Surface**
   - Most engines: Assume flat gravity (0, -9.8, 0)
   - Space games: Point gravity (toward center)
   - **Need: Gravity toward Earth center, changes direction as you move**

---

## 2. UNPRECEDENTED CHALLENGES

### 2.1 Things That Work in Normal Games but Break Here

**Floating Point Precision:**
- Normal game: 10km × 10km map, f32 is fine
- This project: 40,075km circumference, f32 has ~1m precision at that scale
- **Solution:** ECEF f64 canonical + FloatingOrigin f32 rendering
- **Complexity:** Every position calculation needs two representations

**Chunk Systems:**
- Normal game: Flat grid, chunks are 16×16 or 256×256
- This project: Spherical surface, chunks must be quad-sphere faces
- **Problem:** Chunks are different sizes (smaller near poles)
- **Problem:** Chunk seams cross sphere faces (5 different coordinate systems)
- **Complexity:** Cannot use standard chunk algorithms

**Level of Detail (LOD):**
- Normal game: Camera distance = LOD level
- This project: Horizon curvature matters
- **Problem:** Can see 5km at sea level, 400km from ISS height
- **Problem:** LOD must account for Earth curvature, not just distance
- **Complexity:** Custom LOD system based on geodetic math

**Physics Gravity:**
- Normal game: gravity = (0, -9.8, 0) everywhere
- This project: Gravity points toward Earth center
- **Problem:** Gravity direction changes as you move
- **Problem:** "Down" is different in Australia vs North Pole
- **Complexity:** Rapier needs custom gravity per chunk

**Collision Detection:**
- Normal game: Flat terrain, simple heightmap collision
- This project: Spherical surface + voxel modifications
- **Problem:** Player walking north curves with Earth
- **Problem:** Dig a hole = collision mesh must update
- **Complexity:** Dynamic collision mesh regeneration per chunk

**Network Latency:**
- Normal client-server: Player at (100, 200), server says "no, you're at (99, 201)"
- P2P with CRDT: No server to arbitrate conflicts
- **Problem:** Two players dig same voxel simultaneously
- **Problem:** Who wins? How to converge without central authority?
- **Complexity:** Operational transforms + vector clocks + signed ops

### 2.2 Data Scale Realities

**Terrain Data:**
- SRTM: 200GB GeoTIFF (30m resolution)
- OSM: ~150GB compressed (buildings, roads, landuse)
- Textures: Unknown (depends on visual fidelity)
- **Cannot fit in RAM**
- **Cannot ship with game**
- **Must stream on-demand**

**Player Modifications:**
- 1 player digs 1000 voxels/day
- 1000 concurrent players = 1M voxel modifications/day
- 1 year = 365M modifications
- At 32 bytes per mod (position + material + timestamp + signature): 11.7 GB/year
- **Need:** Compression, pruning, archival

**Network Bandwidth:**
- Player walks 5 m/s (jogging speed)
- Enters ~5 new chunks per minute (256m chunks)
- Each chunk: ~50 MB mesh data
- **250 MB/min = 4.2 MB/s download**
- **5G: 50-100 MB/s (fine)**
- **4G: 5-10 MB/s (borderline)**
- **Need:** Aggressive compression + prediction

### 2.3 Physics Simulation Constraints

**Determinism Requirements:**
- P2P: All clients must compute same result
- Rapier: Fixed timestep (60 Hz)
- **Problem:** f32 math is NOT deterministic across CPUs
- **Problem:** HashMap iteration order is random
- **Solution:** Fixed-point math or IEEE strict mode + sorted iteration
- **Complexity:** Entire physics pipeline must be deterministic

**Scale vs Performance:**
- Rapier can handle ~10,000 active rigidbodies
- Earth surface: Potentially millions of entities
- **Cannot simulate everything at once**
- **Need:** Sleep distant entities, spatial partitioning, simplified far-field

**Voxel Modifications Invalidating Physics:**
- Player digs tunnel = collision mesh changes
- Must regenerate physics collider for chunk
- Rapier rebuild: ~5-10ms per chunk
- **Problem:** 60 FPS = 16ms frame budget
- **Problem:** Regenerating physics blocks rendering
- **Solution:** Async physics rebuild, temporary old collider until ready

---

## 3. CORE SYSTEMS ARCHITECTURE

### 3.1 The Eight Pillars

These are the MAJOR subsystems. Each is complex enough to be its own multi-month project.

```
1. COORDINATES & SPATIAL
   ├─ GPS ↔ ECEF ↔ VoxelCoord
   ├─ FloatingOrigin rendering
   ├─ Quad-sphere chunking
   └─ Spatial queries (what's near me?)

2. TERRAIN GENERATION
   ├─ SRTM elevation data
   ├─ Bilinear interpolation
   ├─ Voxel column generation
   └─ Material layering (stone/dirt/grass)

3. VOXEL SYSTEM
   ├─ Sparse octree storage
   ├─ Material properties
   ├─ Modification API (dig/place)
   └─ Serialization/compression

4. MESH EXTRACTION
   ├─ Marching cubes algorithm
   ├─ LOD generation (1m/2m/4m/8m/16m)
   ├─ Chunk-based extraction
   └─ Async mesh generation

5. RENDERING
   ├─ wgpu pipeline (Vulkan backend)
   ├─ Camera system (floating origin)
   ├─ Chunk streaming (load/unload)
   ├─ Frustum culling
   ├─ Lighting & shadows
   └─ Materials & textures

6. PHYSICS
   ├─ Rapier integration
   ├─ Collision mesh generation from voxels
   ├─ Player controller (capsule)
   ├─ Gravity (toward Earth center)
   ├─ Rigidbodies (vehicles, debris)
   └─ Deterministic simulation

7. INTERACTION
   ├─ Input handling (keyboard/mouse/gamepad)
   ├─ Raycasting (what am I looking at?)
   ├─ Voxel modification (dig/place)
   ├─ Inventory system
   └─ Tool/weapon mechanics

8. NETWORKING
   ├─ libp2p (Kademlia DHT + Gossipsub)
   ├─ CRDT state sync
   ├─ Cryptographic identity (Ed25519)
   ├─ Spatial sharding (chunks)
   └─ Bandwidth management
```

**CRITICAL:** These are NOT independent. They all integrate.

### 3.2 Integration Dependencies

**Rendering depends on:**
- Coordinates (FloatingOrigin transform)
- Mesh Extraction (triangle data)
- Voxel System (material colors)

**Physics depends on:**
- Coordinates (ECEF positions)
- Voxel System (solid vs air)
- Mesh Extraction (collision geometry)

**Networking depends on:**
- Voxel System (modification ops)
- Coordinates (spatial sharding)
- Physics (entity positions)

**Terrain Generation depends on:**
- Coordinates (GPS → voxel)
- Voxel System (set material)
- External data (SRTM files)

**Voxel Modification depends on:**
- Interaction (raycast to find voxel)
- Voxel System (set material)
- Mesh Extraction (regenerate visual)
- Physics (regenerate collision)
- Networking (broadcast change)

**YOU CANNOT BUILD ONE IN ISOLATION.**

This is why the vertical slice approach matters - prove they can all talk to each other before optimizing any single piece.

---

## 4. SYSTEM INTEGRATION MAP

### 4.1 Data Flow: Player Digs a Hole

This shows how ALL systems interact for ONE simple action.

```
[INPUT] Player presses left mouse button
   ↓
[INTERACTION] Raycast from camera through world
   │ Needs: Camera position (ECEF f64)
   │ Needs: Camera direction (FloatingOrigin f32)
   │ Needs: Voxel octree (query solid voxels)
   ↓
[RAYCAST] Find first solid voxel hit
   │ Returns: VoxelCoord (i64, i64, i64)
   │ Returns: Hit distance & normal
   ↓
[VOXEL SYSTEM] Set voxel to AIR
   │ octree.set(coord, Material::AIR)
   │ Mark chunk dirty for regeneration
   ↓
[MESH EXTRACTION] Regenerate chunk mesh
   │ Async: Don't block frame
   │ Process: Marching cubes on modified region
   │ Output: New triangle mesh
   ↓
[RENDERING] Upload new mesh to GPU
   │ Delete old GPU buffer
   │ Create new vertex buffer
   │ Update chunk mesh reference
   ↓
[PHYSICS] Regenerate collision mesh
   │ Async: Don't block frame
   │ Process: Simplified mesh or voxel grid
   │ Output: Rapier collider
   ↓
[PHYSICS] Update Rapier world
   │ Remove old collider
   │ Insert new collider
   │ Player may fall if ground removed
   ↓
[NETWORKING] Broadcast modification
   │ Create signed op: {coord, AIR, timestamp, signature}
   │ Gossipsub to nearby peers
   │ CRDT merge on remote clients
   ↓
[REMOTE CLIENTS] Apply modification
   │ Verify signature
   │ Check CRDT causality
   │ Apply to local octree
   │ Regenerate their mesh & physics
   ↓
[PERSISTENCE] Save to disk
   │ Append to chunk modification log
   │ Periodic compaction/pruning
```

**THIS IS FOR ONE VOXEL CHANGE.**

Now multiply by:
- 1000 players
- 100 voxel changes/second
- Across entire Earth
- With 60 FPS rendering
- With physics simulation
- With network sync

**That's the scale of complexity.**

### 4.2 Data Flow: Player Walks Forward

```
[INPUT] Player holds W key
   ↓
[PHYSICS] Apply forward impulse to player capsule
   │ Calculate forward vector (camera direction)
   │ Add velocity in that direction
   ↓
[PHYSICS] Rapier tick (60 Hz fixed timestep)
   │ Apply gravity (toward Earth center)
   │ Move capsule: position += velocity * dt
   │ Collision detection: capsule vs terrain colliders
   │ Response: Push out of terrain if intersecting
   ↓
[PHYSICS] Player position updated (ECEF f64)
   │ New position: old_pos + movement_vector
   ↓
[RENDERING] Update camera to follow player
   │ Camera ECEF = Player ECEF + offset
   │ Recalculate FloatingOrigin transform
   │ All vertices shifted by -(new_camera - old_camera)
   ↓
[CHUNK SYSTEM] Check if player entered new chunk
   │ Player chunk = hash(player_pos)
   │ If changed from last frame:
   │   - Add nearby chunks to active set
   │   - Queue generation for new chunks
   │   - Unload distant chunks
   ↓
[TERRAIN GENERATION] Generate new chunks (async)
   │ Background thread: SRTM → voxels → mesh
   │ Upload to GPU when ready
   │ Fade in over 0.5-1.0 seconds
   ↓
[NETWORKING] Broadcast position to nearby players
   │ Every 100ms: Send {position, velocity, rotation}
   │ Gossipsub to spatial shard
   │ Remote clients interpolate movement
   ↓
[AUDIO] Update sound listener position
   │ 3D audio positioned at camera
   │ Footstep sounds if on_ground && moving
   │ Ambient sounds based on biome (ocean, forest, city)
```

**THIS IS FOR HOLDING ONE KEY.**

Every frame (16ms at 60 FPS), this entire pipeline runs.

---


## 5. SCALE REQUIREMENTS

### 5.1 Performance Targets (Per-System)

**Rendering:**
- 60 FPS minimum (16.67ms frame budget)
- 144 FPS target for high-end (6.94ms frame budget)
- Draw distance: 5km at ground level, 400km from high altitude
- Vertex budget: 5-10M per frame (after frustum culling)
- Texture memory: <4 GB VRAM
- Mesh upload: <5ms per chunk

**Physics:**
- Fixed 60 Hz timestep (16.67ms)
- Active rigidbodies: 10,000 maximum
- Collision checks: 100,000/sec
- Chunk collision rebuild: <10ms async
- Player controller latency: <5ms

**Terrain Generation:**
- 256m chunk: <10s total (generation + mesh)
- SRTM query: <100ms (cache hit <1ms)
- Voxel column: <0.01ms per column
- Mesh extraction: <5s per chunk (async)

**Networking:**
- Latency: <100ms to nearby peers
- Bandwidth: 1-5 MB/s sustained
- Peer discovery: <5s
- State sync: <1s for local modifications
- CRDT merge: <1ms per operation

**Storage:**
- Chunk compression: 10:1 ratio target
- Modification log: <1 MB per hour of play
- Cache size: 1-10 GB per play session
- Persistence: <100ms per chunk save

### 5.2 Scale Gates (Validation Checkpoints)

These are the checkpoints from TESTING.md that gate progression:

**Phase 1: Coordinates & Precision**
- ✅ COMPLETE: GPS↔ECEF conversion <1mm error within 1km
- ✅ COMPLETE: FloatingOrigin precision <1mm within 10km

**Phase 2: Terrain Generation**
- ✅ COMPLETE: 120m × 120m in 2.1s (target: <30s for 100m)
- ✅ COMPLETE: 250m × 250m in 9.6s
- ✅ COMPLETE: 500m × 500m in 59.9s
- ❌ BLOCKED: 1000m × 1000m exceeds GPU buffer limit (463 MB > 256 MB max)

**Phase 3: Rendering at Scale**
- ✅ COMPLETE: 60 FPS with 6.1M vertices (500m area)
- ⏳ NEXT: Chunk system (bypass 256 MB buffer limit)
- ⏳ NEXT: LOD system (5km view distance)
- ⏳ NEXT: Frustum culling (render only visible)

**Phase 4: Physics Integration**
- ⏳ NOT STARTED: Player can spawn and stand on terrain
- ⏳ NOT STARTED: Walking with collision detection
- ⏳ NOT STARTED: Gravity toward Earth center

**Phase 5: Interaction**
- ⏳ NOT STARTED: Raycast voxel selection
- ⏳ NOT STARTED: Dig/place voxel
- ⏳ NOT STARTED: Mesh regeneration after modification
- ⏳ NOT STARTED: Physics collision update after modification

**Phase 6: Networking**
- ⏳ NOT STARTED: libp2p connection between 2 peers
- ⏳ NOT STARTED: CRDT synchronization of voxel modification
- ⏳ NOT STARTED: 10 peers seeing same world state

**Phase 7: Real-World Data**
- ⏳ NOT STARTED: OSM building import
- ⏳ NOT STARTED: Road/path rendering
- ⏳ NOT STARTED: Water bodies

**Current Status: Phase 3 (Rendering) partially complete**

### 5.3 Scalability Limits (Known Hard Walls)

**GPU Buffer Limit: 256 MB**
- Cause: wgpu/hardware constraint
- Impact: Cannot create single mesh >256 MB
- Workaround: Chunk system (multiple buffers)
- Status: **MANDATORY** for 1km+ areas

**Rapier Rigidbody Limit: ~10,000**
- Cause: Quadratic collision detection
- Impact: Cannot simulate millions of entities
- Workaround: Sleep distant entities, simplified physics
- Status: Will hit when world gets populated

**Network Bandwidth: ~5 MB/s on 4G**
- Cause: Mobile network limits
- Impact: Cannot stream chunks fast enough if moving quickly
- Workaround: Prediction, LOD, aggressive compression
- Status: Will hit when mobile players exist

**F32 Precision: ~1m at Earth scale**
- Cause: IEEE 754 floating point
- Impact: Jittery rendering if using f32 for absolute positions
- Workaround: FloatingOrigin (already implemented)
- Status: **SOLVED** in current architecture

**Client RAM: ~8-16 GB typical**
- Cause: Consumer hardware
- Impact: Cannot load entire Earth
- Workaround: Stream chunks, unload distant
- Status: Will hit if not careful with caching

**Storage Write Speed: ~500 MB/s SSD**
- Cause: Hardware limits
- Impact: Cannot save modifications faster than this
- Workaround: Batch writes, async persistence
- Status: Fine for now, monitor in multiplayer

---

## 6. TECHNICAL FOUNDATIONS

### 6.1 Coordinate System (COMPLETE)

**Three representations of position:**

```rust
// 1. GPS - Human interface (degrees)
struct GPS {
    lat: f64,  // -90 to +90
    lon: f64,  // -180 to +180
    alt: f64,  // meters above WGS84 ellipsoid
}

// 2. ECEF - Canonical storage (meters)
struct ECEF {
    x: f64,  // meters from Earth center
    y: f64,  // meters from Earth center
    z: f64,  // meters from Earth center
}

// 3. FloatingOrigin - GPU rendering (relative meters)
struct FloatingOrigin {
    offset: Vec3,  // f32, relative to camera
}
```

**Why three?**
- GPS: Humans think in lat/lon
- ECEF: Math is simple (Cartesian), deterministic, network-serializable
- FloatingOrigin: GPU needs f32, precision requires camera-relative

**Conversions:**
- GPS → ECEF: `geoconv` crate (WGS84 ellipsoid)
- ECEF → FloatingOrigin: `(entity - camera).as_f32()`
- FloatingOrigin → ECEF: `camera + offset.as_f64()`

**Precision:**
- GPS: Limited by ellipsoid model (~1mm)
- ECEF f64: 1.4 nanometers at Earth radius
- FloatingOrigin f32: Sub-millimeter within 10km

**Library:** `geoconv 0.1.0` (audited, tested, documented)

**Status:** ✅ COMPLETE, TESTED, VALIDATED

### 6.2 Voxel System (COMPLETE)

**Voxel = 1m³ cube with material type**

```rust
struct VoxelCoord {
    x: i64,  // Floor division of ECEF meters
    y: i64,
    z: i64,
}

impl VoxelCoord {
    fn from_ecef(ecef: &ECEF) -> Self {
        Self {
            x: ecef.x.floor() as i64,
            y: ecef.y.floor() as i64,
            z: ecef.z.floor() as i64,
        }
    }
}

#[repr(u8)]
enum Material {
    AIR = 0,
    STONE = 1,
    DIRT = 2,
    GRASS = 3,
    SAND = 4,
    WATER = 5,
    // ... 250 more materials
}

struct MaterialProperties {
    solid: bool,
    transparent: bool,
    density: f32,
    color: [u8; 3],
    // Later: texture_id, sound, etc.
}
```

**Storage:** Sparse octree
- 3 node types: Empty (all air), Solid (all same material), Branch (mixed)
- Auto-collapse: 8 children of same material → parent becomes solid
- Depth: Up to 23 levels (8M × 8M × 8M)
- Memory: ~50 bytes per branch node, 1 byte per solid node

**Operations:**
- Get: O(log n) average, O(depth) worst case
- Set: O(log n) + auto-collapse
- Query region: Traverse subtree

**Chunk Bounds:**
- World divided into 256m × 256m × 2048m chunks (surface to depth)
- Each chunk = separate octree
- HashMap<ChunkId, Octree> for world storage

**Status:** ✅ COMPLETE, TESTED

### 6.3 Elevation Data (COMPLETE)

**Sources:**

1. **NAS File** (Primary)
   - Path: `./srtm-global.tif`
   - Format: GeoTIFF, 200 GB
   - Resolution: ~30m (1 arc-second)
   - Coverage: Global (-60° to +60° latitude)
   - Library: GDAL 0.18.0

2. **OpenTopography API** (Fallback)
   - Endpoint: `https://portal.opentopography.org/API/`
   - API Key: `3e607de6969c687053f9e107a4796962`
   - Resolution: ~30m (SRTM same source)
   - Rate limit: 2-second cooldown between calls

**Pipeline:**
```rust
struct ElevationPipeline {
    sources: Vec<Box<dyn ElevationSource>>,
}

impl ElevationPipeline {
    fn query(&mut self, lat: f64, lon: f64) -> Option<f64> {
        for source in &mut self.sources {
            if let Some(elevation) = source.get(lat, lon) {
                return Some(elevation);
            }
        }
        None  // All sources failed
    }
}
```

**Caching:**
- No caching yet (SRTM file is fast enough)
- Future: In-memory LRU cache of recent queries

**Interpolation:**
- Bilinear interpolation from 4 nearest SRTM points
- Used to convert ~30m resolution → 1m voxel spacing
- Example: 120m × 120m uses 5×5 SRTM grid, interpolates to 14,400 voxel columns

**Status:** ✅ COMPLETE, TESTED

### 6.4 Mesh Extraction (COMPLETE)

**Algorithm:** Marching Cubes (Lorensen & Cline, 1987)

**How it works:**
1. Sample voxel grid at each corner of a 1m³ cube
2. Create 8-bit index: bit N = 1 if corner N is solid
3. Lookup triangle configuration in table (256 entries)
4. Interpolate edge positions based on material transition
5. Output triangles with calculated normals

**Tables:**
- `EDGE_TABLE`: 256 × 12-bit = Which edges have vertices
- `TRIANGLE_TABLE`: 256 × 15 entries = Triangle vertices per config
- Total: ~4 KB of lookup tables

**Output:**
```rust
struct Mesh {
    vertices: Vec<Vertex>,   // Position + Normal
    indices: Vec<u32>,       // Triangle indices
    transform: FloatingOrigin,
}

struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}
```

**Performance:**
- 120m × 120m: 0.83s (410K vertices)
- 500m × 500m: 46.9s (6.1M vertices)
- Scales: O(n²) or worse (bottleneck identified)

**Optimization needed:**
- Parallel extraction (rayon)
- Caching (static terrain doesn't change)
- Dual contouring (better quality, potentially faster)
- GPU compute (ultra-fast but complex)

**Status:** ✅ WORKS, ⚠️ SLOW AT SCALE

### 6.5 Rendering (WORKING)

**Stack:**
- **wgpu 24.0.5** - Cross-platform GPU API
- **Backend:** Vulkan (primary), DirectX 12, Metal
- **Window:** winit 0.30.0
- **Math:** glam 0.29.3

**Pipeline:**
```rust
struct RenderContext {
    device: Device,
    queue: Queue,
    surface: Surface,
    adapter: Adapter,
}

struct RenderPipeline {
    pipeline: wgpu::RenderPipeline,
    camera_bind_group: BindGroup,
}

struct Camera {
    position: ECEF,        // f64 absolute
    yaw: f32,
    pitch: f32,
    aspect: f32,
    fov: f32,
}
```

**Shaders:**
- Vertex: Apply FloatingOrigin transform + view/projection
- Fragment: Basic lighting (directional light + ambient)

**Features:**
- ✅ Depth buffering (Depth32Float)
- ✅ Backface culling
- ✅ FloatingOrigin transform
- ❌ Frustum culling (TODO)
- ❌ LOD system (TODO)
- ❌ Textures (TODO)
- ❌ Shadows (TODO)
- ❌ PBR materials (TODO)

**Performance:**
- 60 FPS with 6.1M vertices (500m area)
- No optimization yet - GPU is very capable

**Status:** ✅ BASIC WORKING, ⏳ MANY FEATURES TODO

### 6.6 Physics (NOT STARTED)

**Library:** Rapier 0.22.0 (deterministic mode)

**Planned architecture:**
```rust
struct PhysicsWorld {
    rapier_world: RapierWorld,
    gravity_direction: Vec3,  // Changes per chunk (spherical)
}

struct PlayerController {
    capsule: Collider,  // 1.8m tall × 0.4m radius
    position: ECEF,
    velocity: Vec3,
    on_ground: bool,
}
```

**Challenges:**
- Gravity direction changes (spherical surface)
- Collision mesh from voxels (dynamic generation)
- Determinism required (P2P consensus)
- Performance: <16ms per tick

**Status:** ⏳ NOT STARTED

### 6.7 Networking (NOT STARTED)

**Library:** libp2p 0.54.0

**Planned architecture:**
```rust
struct NetworkNode {
    peer_id: PeerId,
    dht: Kademlia,
    pubsub: Gossipsub,
    identity: Keypair,  // Ed25519
}

struct VoxelModification {
    coord: VoxelCoord,
    material: Material,
    timestamp: u64,
    author: PeerId,
    signature: Signature,
}
```

**CRDT:**
- Last-write-wins with vector clocks
- Spatial sharding by chunk ID
- Gossipsub for nearby peers
- DHT for peer discovery

**Challenges:**
- Bandwidth: 5 MB/s limit
- Conflict resolution without central authority
- Malicious peer detection
- State consistency

**Status:** ⏳ NOT STARTED

---

## 7. SUBSYSTEM SPECIFICATIONS

### 7.1 Chunk System (CRITICAL NEXT STEP)

**Why mandatory:**
- GPU buffer limit: 256 MB maximum
- 500m area = 186 MB (close to limit)
- 1000m area = 463 MB (exceeds limit)
- **Cannot proceed without chunks**

**Architecture:**
```rust
struct ChunkId {
    face: u8,      // 0-5 (cube face)
    x: i32,        // Face-local coordinate
    y: i32,
    lod: u8,       // Level of detail (0-4)
}

struct Chunk {
    id: ChunkId,
    octree: Octree,
    mesh: Option<Mesh>,
    physics_collider: Option<Collider>,
    state: ChunkState,
    alpha: f32,  // For fade-in
}

enum ChunkState {
    NotLoaded,
    Queued,
    Generating,
    Ready,
    Uploaded,
    FadingIn,
    Active,
}

struct ChunkManager {
    chunks: HashMap<ChunkId, Chunk>,
    active_set: HashSet<ChunkId>,
    load_queue: VecDeque<ChunkId>,
    worker_threads: Vec<JoinHandle<()>>,
}
```

**Chunk Size:**
- 256m × 256m horizontal (power of 2, GPU-friendly)
- Full vertical extent (-100m to +2000m)
- Total: ~4.3M voxels per chunk
- Mesh: ~40-50 MB per chunk (under GPU limit)

**Quad-Sphere Chunking:**
- Earth = cube projected onto sphere
- 6 faces (like Minecraft skybox)
- Each face = quadtree of chunks
- Different coordinate system per face

**Status:** ⏳ NEXT CRITICAL TASK

### 7.2 LOD System

**Why needed:**
- Reduce vertex count 10-100× for distant terrain
- Smooth visual quality (not stepped like current)
- Enable 5-10km view distance

**LOD Levels:**
```rust
const LOD_LEVELS: [(f32, f32); 5] = [
    (0.0,    1.0),   // 0-500m: 1m voxels
    (500.0,  2.0),   // 0.5-1km: 2m voxels
    (1000.0, 4.0),   // 1-2km: 4m voxels
    (2000.0, 8.0),   // 2-4km: 8m voxels
    (4000.0, 16.0),  // 4km+: 16m voxels
];
```

**Impact on vertex count:**
- 5km × 5km all LOD 0 (1m): ~400M vertices (IMPOSSIBLE)
- 5km × 5km mixed LOD: ~20M vertices (manageable)

**Transition:**
- Fade old LOD out (alpha 1.0 → 0.0)
- Fade new LOD in (alpha 0.0 → 1.0)
- Both visible during transition (0.5-1.0 seconds)

**Status:** ⏳ TODO AFTER CHUNKS

### 7.3 Frustum Culling

**Why needed:**
- Don't render chunks behind player
- 2-3× performance boost
- Simple AABB vs frustum plane test

**Algorithm:**
```rust
struct Frustum {
    planes: [Plane; 6],  // Extracted from VP matrix
}

fn cull_chunks(camera: &Camera, chunks: &[Chunk]) -> Vec<&Chunk> {
    let frustum = Frustum::from_camera(camera);
    chunks.iter()
        .filter(|c| frustum.intersects_aabb(c.aabb))
        .collect()
}
```

**Impact:**
- Looking forward: Render ~50% of chunks
- Looking at ground: Render ~30%
- Looking at sky: Render ~10%

**Status:** ⏳ TODO AFTER CHUNKS

### 7.4 OSM Building Import

**Data Source:**
- OpenStreetMap planet dump (~150 GB compressed)
- Geofabrik extracts (regional, easier)
- Overpass API (query-based, rate-limited)

**Building Representation:**
```xml
<way id="123456">
  <nd ref="1"/> <nd ref="2"/> <nd ref="3"/> <nd ref="1"/>
  <tag k="building" v="yes"/>
  <tag k="height" v="25.0"/>
  <tag k="building:levels" v="8"/>
</way>
```

**Voxelization:**
1. Parse OSM XML → building footprint (polygon)
2. Extrude vertically (height or levels × 3m)
3. Rasterize to voxel grid (solid walls, hollow interior)
4. Material: CONCRETE, BRICK, GLASS (based on tags)
5. Insert into octree

**Challenges:**
- 8 billion OSM nodes, 900M ways
- Cannot load all at once
- Must stream by region (same as terrain chunks)
- Conflicts with terrain (building replaces ground)

**Status:** ⏳ TODO AFTER PHYSICS

### 7.5 Road/Path Rendering

**OSM Representation:**
```xml
<way id="789">
  <nd ref="10"/> <nd ref="11"/> <nd ref="12"/>
  <tag k="highway" v="primary"/>
  <tag k="lanes" v="2"/>
  <tag k="width" v="8.0"/>
</way>
```

**Approach 1: Voxel (simple but blocky)**
- Rasterize road polyline to voxel grid
- Material: ASPHALT
- Problem: 1m voxels = blocky roads

**Approach 2: Mesh (smooth but complex)**
- Generate road mesh separate from terrain
- Spline-based smoothing
- Texture mapping for lane markings
- Problem: Intersection with terrain mesh (z-fighting)

**Approach 3: Hybrid (best but most complex)**
- Modify terrain voxels for road base
- Add detail mesh on top for markings
- Blend at edges (smooth transition)

**Status:** ⏳ TODO AFTER BUILDINGS

### 7.6 Water Bodies

**Types:**
- Oceans (sea level = 0m altitude)
- Lakes (OSM polygons with surface elevation)
- Rivers (OSM polylines with width + flow direction)

**Rendering:**
- Material: WATER (transparent, reflective)
- Shader: Animated surface (waves, normals)
- Level surface or flowing surface

**Physics:**
- Buoyancy (Rapier fluid simulation or custom)
- Flow velocity (rivers push player)
- Drowning mechanics (need air)

**Status:** ⏳ TODO LATER (complex)

---

## 8. EMERGENT COMPLEXITY FEATURES

These are the "footprints in sand" level features. They require ALL base systems working.

### 8.1 Persistent Modifications

**User Actions:**
- Dig voxels → saved forever
- Place voxels → saved forever
- Build structures → saved forever

**Implementation:**
```rust
struct ChunkModificationLog {
    chunk_id: ChunkId,
    modifications: Vec<VoxelModification>,
}

struct VoxelModification {
    coord: VoxelCoord,
    material: Material,
    timestamp: u64,
    author_signature: Signature,
}
```

**Storage:**
- Per-chunk log file
- Append-only (for CRDT)
- Periodic compaction (merge ops)
- Backup: User's responsibility (P2P = no central backup)

**Network Sync:**
- Gossipsub broadcast to nearby peers
- CRDT merge (last-write-wins with vector clock)
- Signature verification (prevent malicious edits)

**Status:** ⏳ TODO AFTER NETWORKING

### 8.2 Footprints in Sand/Snow

**Concept:**
- Walking on SAND or SNOW leaves visible depression
- Fades over time (minutes to hours)
- Other players can see your footprints

**Implementation:**
```rust
struct SurfaceDecal {
    position: VoxelCoord,
    decal_type: DecalType,
    opacity: f32,
    created: Instant,
    lifetime: Duration,
}

enum DecalType {
    Footprint { rotation: f32 },
    BulletHole,
    BloodStain,
    // ...
}
```

**Rendering:**
- NOT voxels (too expensive)
- Decal mesh (quad) on terrain surface
- Projected texture
- Alpha blend with terrain

**Challenges:**
- Thousands of decals over time
- Need spatial culling (don't render distant)
- Need aging system (fade out)
- Network sync (do others see footprints? Bandwidth cost?)

**Status:** ⏳ FAR FUTURE

### 8.3 Plant Growth Simulation

**Concept:**
- Place seed → grows over real-time hours/days
- Requires: Light, water, soil type
- Can be harvested, destroyed, spread

**Implementation:**
```rust
struct PlantEntity {
    position: VoxelCoord,
    species: PlantSpecies,
    growth_stage: u8,  // 0-10
    last_tick: Instant,
}

impl PlantEntity {
    fn tick(&mut self, world: &World) {
        if self.has_light() && self.has_water() {
            self.growth_stage += 1;
            self.update_visual();
        }
    }
}
```

**Visual:**
- Not voxels (plants are organic)
- Instanced meshes (GPU instancing)
- LOD: Far = billboard, Near = 3D model

**Simulation:**
- Tick rate: 1 Hz (once per second)
- Only active chunks (spatial culling)
- Network sync: Growth stage only (not every tick)

**Challenges:**
- Millions of plants across Earth
- Cannot simulate all at once
- Need sleep/wake system (like Minecraft crops)
- Persistence: Save growth stage

**Status:** ⏳ FAR FUTURE

### 8.4 Weather & Erosion

**Concept:**
- Rain → water flow → erosion (voxels move)
- Snow accumulation → ice formation
- Wind → sand dunes shift

**Implementation:**
- Cellular automata on voxel grid
- Water flows downhill (gravity + gradient)
- Material transitions (SAND → WET_SAND)
- Very computationally expensive

**Challenges:**
- Cannot simulate entire Earth's weather
- Must limit to active regions
- Network sync nightmares (every voxel changing)

**Status:** ⏳ EXTREMELY FAR FUTURE (may never do)

---

## 9. NETWORK ARCHITECTURE

### 9.1 P2P Topology

**Why P2P:**
- No central server to pay for
- No single point of failure
- Player-owned infrastructure
- Resistant to censorship

**Library:** libp2p

**Components:**
```rust
struct NetworkNode {
    peer_id: PeerId,
    keypair: Keypair,  // Ed25519
    swarm: Swarm,
    
    // DHT for peer discovery
    kademlia: Kademlia,
    
    // PubSub for state sync
    gossipsub: Gossipsub,
    
    // Spatial shard subscriptions
    subscribed_chunks: HashSet<ChunkId>,
}
```

**Spatial Sharding:**
- Each chunk has a topic: `chunk-<face>-<x>-<y>`
- Players subscribe to chunks they can see
- Modifications broadcast only to subscribers
- As player moves: unsubscribe old, subscribe new

**Peer Discovery:**
- Bootstrap nodes (well-known addresses)
- Kademlia DHT (distributed peer database)
- mDNS for local network

**Status:** ⏳ NOT STARTED

### 9.2 CRDT State Synchronization

**Problem:** Two players modify same voxel simultaneously

**Normal client-server:**
- Server decides who wins
- Loser gets correction message

**P2P (no server):**
- Need conflict-free resolution
- All peers must converge to same state
- No central authority

**Solution: CRDT (Conflict-Free Replicated Data Type)**

```rust
struct CRDTVoxelOp {
    coord: VoxelCoord,
    material: Material,
    timestamp: u64,       // Lamport clock
    vector_clock: VectorClock,
    author: PeerId,
    signature: Signature,
}

impl CRDTVoxelOp {
    fn merge(&self, other: &Self) -> Material {
        if self.vector_clock.happens_after(&other.vector_clock) {
            self.material
        } else if other.vector_clock.happens_after(&self.vector_clock) {
            other.material
        } else {
            // Concurrent: use timestamp
            if self.timestamp > other.timestamp {
                self.material
            } else {
                other.material
            }
        }
    }
}
```

**Properties:**
- Commutative: A+B = B+A
- Associative: (A+B)+C = A+(B+C)
- Idempotent: A+A = A

**This guarantees eventual consistency.**

**Challenges:**
- Vector clock size (grows with number of peers)
- Signature verification cost
- Malicious peer detection (signed ops help)

**Status:** ⏳ NOT STARTED

### 9.3 Identity & Cryptography

**No accounts, no login, no passwords.**

**Instead:**
- Each player = Ed25519 keypair
- Public key = identity (PeerId)
- Private key = proves ownership

**Signing Operations:**
```rust
fn dig_voxel(coord: VoxelCoord, keypair: &Keypair) -> SignedOp {
    let op = VoxelOp {
        coord,
        material: AIR,
        timestamp: now(),
    };
    let signature = keypair.sign(op.to_bytes());
    SignedOp { op, signature, author: keypair.public() }
}
```

**Verification:**
```rust
fn verify_op(signed_op: &SignedOp) -> bool {
    signed_op.author.verify(
        signed_op.op.to_bytes(),
        &signed_op.signature
    )
}
```

**Benefits:**
- Cannot forge modifications (need private key)
- Reputation tied to keypair
- Ban system: Blacklist public keys

**Challenges:**
- Lost keypair = lost identity (no recovery)
- Key distribution (how to share public key?)

**Status:** ⏳ NOT STARTED

---


## 10. PERFORMANCE TARGETS

### 10.1 Frame Budget Breakdown (60 FPS)

**Total: 16.67ms per frame**

```
Input polling:        0.5ms
Physics tick:         5.0ms (if due this frame)
Chunk management:     1.0ms (check loading/unloading)
Mesh uploads:         2.0ms (if new chunks ready)
Frustum culling:      0.5ms
Render preparation:   1.0ms (update uniforms, bind groups)
GPU rendering:        5.0ms (vertex + fragment shaders)
Present:              1.0ms (swap buffers)
Audio:                0.5ms
Network:              0.2ms (poll messages)
---
TOTAL:               16.67ms
```

**If anything goes over budget → frame drop**

**Async operations (don't count toward frame budget):**
- Terrain generation (background threads)
- Mesh extraction (background threads)
- Physics collision rebuild (background thread)
- Network I/O (async runtime)
- File I/O (async or thread pool)

### 10.2 Memory Budget

**Target: 8 GB total (mid-range PC)**

```
Executable code:         500 MB
Loaded chunks (voxels):  2 GB (500 chunks × 4 MB)
GPU meshes:             1 GB (500 chunks × 2 MB)
Textures:               1 GB
Physics colliders:      500 MB
Audio samples:          200 MB
Network buffers:        100 MB
Misc/overhead:          2.7 GB
---
TOTAL:                  8 GB
```

**On high-end (32 GB):**
- Can load 4× more chunks (2,000 instead of 500)
- Larger view distance

**On low-end (4 GB):**
- Half chunks (250)
- Lower view distance
- May struggle

### 10.3 Network Bandwidth Budget

**Target: 5 MB/s sustained (consumer broadband)**

```
Chunk downloads:        3 MB/s (compressed meshes)
Peer updates:           1 MB/s (positions, states)
Voice chat:           100 KB/s (if implemented)
Modification sync:    500 KB/s (voxel changes)
Protocol overhead:    400 KB/s (libp2p)
---
TOTAL:                  5 MB/s
```

**If player moves fast:**
- Need 5 new chunks per minute
- 5 × 50 MB = 250 MB per minute = 4.2 MB/s
- **Borderline on 5 MB/s budget**

**Solutions if exceeded:**
- More aggressive compression (10:1 → 20:1)
- Prediction (load chunks you'll likely enter)
- LOD (low-detail versions first)

### 10.4 Storage Budget

**Per play session (4 hours):**
```
Downloaded chunks:      5 GB (streamed in)
Modifications:        100 MB (player digging/building)
Cache overhead:       500 MB (indices, metadata)
Temp files:           400 MB
---
TOTAL:                  6 GB per session
```

**Long-term (per year of play):**
```
Modification log:       4 GB (365 days × 100 MB/day)
Cached chunks:         20 GB (frequently visited areas)
Screenshots:            2 GB (if player takes many)
---
TOTAL:                 26 GB per year
```

**Pruning:**
- Delete chunks not visited in 30 days
- Compress modification logs
- User can clear cache manually

---

## 11. DEVELOPMENT PHASES

### 11.1 Phase Progression Strategy

**CRITICAL INSIGHT:**

We've built rendering in isolation. We don't know if it integrates with other systems.

**OLD PLAN (risky):**
```
3 weeks: Perfect the rendering system
  - Chunks, LOD, culling, optimization
Then: Try to bolt on physics
  - OH NO: Physics needs different chunk structure
  - Rewrite rendering chunks
  - 3 weeks wasted
```

**NEW PLAN (safer):**
```
2 weeks: Vertical slice (all systems touching)
  - Prove integration works at small scale
  - Find architectural problems NOW
Then: Scale up
  - Chunk system, LOD, networking
  - Confident it will integrate
```

**Vertical slice = walking player who can dig holes**
- Rendering (see terrain) ✅ DONE
- Physics (walk on terrain) ⏳ 1 week
- Interaction (dig holes) ⏳ 1 week
- Mesh regen (holes appear) ⏳ included
- Physics regen (fall into holes) ⏳ included

**If this works: Architecture is sound**

**If this breaks: Fix now while system is small**

### 11.2 Proposed Phase Order

#### **PHASE 1: Vertical Slice (2 weeks) ← NEXT**

**Week 1: Physics Integration**
- Add Rapier to Cargo.toml
- Create PhysicsWorld with gravity
- Player capsule (1.8m tall × 0.4m radius)
- Collision mesh from voxels
- WASD movement + jumping
- Camera follows player

**Success metric:** Player spawns on terrain, can walk around, can jump, physics feels responsive

**Week 2: Interaction**
- Raycast from camera (find target voxel)
- Left click: dig (set voxel to AIR)
- Right click: place (set voxel to STONE)
- Regenerate mesh for modified chunk (async)
- Regenerate physics collider (async)
- Visual feedback (crosshair, highlight selected voxel)

**Success metric:** Can dig hole, fall into it, build stairs out of it. Record video showing this.

**Deliverable:** 2-minute video demonstrating full gameplay loop

---

#### **PHASE 2: Chunk System (2 weeks)**

**Week 1: Chunk Architecture**
- Define ChunkId (face, x, y, lod)
- ChunkManager (HashMap storage)
- Split generate_region() into generate_chunk()
- Active set calculation (chunks within distance)
- Render multiple chunks (prove no visual seams)

**Success metric:** Load 4 chunks (512m × 512m), render seamlessly, no performance regression

**Week 2: Async Loading**
- Background worker threads (4-8 threads)
- Request/result channels
- Queue chunks for generation
- Upload meshes when ready
- Unload distant chunks

**Success metric:** Walk from chunk to chunk, new chunks appear without stutter, 60 FPS maintained

---

#### **PHASE 3: Progressive Streaming (1 week)**

**Fade-In Transitions:**
- Add alpha uniform to shader
- Enable alpha blending in pipeline
- Chunk state machine (Generating → FadingIn → Active)
- Fade in over 0.5-1.0 seconds

**Success metric:** New chunks fade in smoothly, no sudden pop-in

---

#### **PHASE 4: LOD System (2 weeks)**

**Week 1: LOD Generation**
- Modify generate_chunk() to accept LOD param
- LOD 0: 1m voxels (current)
- LOD 1: 2m voxels (skip every other)
- LOD 2: 4m, LOD 3: 8m, LOD 4: 16m
- Test each LOD level visually

**Week 2: LOD Switching**
- Calculate LOD per chunk based on distance
- Regenerate chunk when LOD changes
- Fade old LOD out, new LOD in (smooth transition)
- Test 5km × 5km area with mixed LOD

**Success metric:** 5km view distance at 60 FPS, smooth distant terrain

---

#### **PHASE 5: Culling & Optimization (1 week)**

**Frustum Culling:**
- Extract frustum planes from camera matrix
- AABB per chunk
- Test AABB vs frustum
- Render only visible chunks

**Mesh Extraction Optimization:**
- Profile marching cubes (find hot spots)
- Try parallel extraction (rayon)
- Measure speedup

**Success metric:** 2-3× FPS improvement, faster chunk loading

---

#### **PHASE 6: Networking (3-4 weeks)**

**Week 1: libp2p Setup**
- Add libp2p to Cargo.toml
- Create NetworkNode
- Connect 2 peers (localhost)
- Send/receive messages

**Week 2: Spatial Sharding**
- Chunk topic subscriptions
- Subscribe/unsubscribe as player moves
- Broadcast voxel modifications
- Remote client receives and applies

**Week 3: CRDT**
- Implement vector clocks
- Merge conflicting operations
- Signature generation/verification
- Test 10 peers all modifying same chunk

**Week 4: Testing & Hardening**
- Malicious peer detection
- Bandwidth optimization
- Connection stability
- NAT traversal (hole punching)

**Success metric:** 10 players in same world, all see each other's modifications, no desync

---

#### **PHASE 7: Real-World Data (4 weeks)**

**Week 1: OSM Download & Parsing**
- Download Geofabrik extract (e.g., Brisbane)
- Parse OSM XML
- Extract buildings (polygons + heights)

**Week 2: Building Voxelization**
- Rasterize building footprints to voxel grid
- Extrude vertically (height)
- Insert into octree
- Test single building

**Week 3: Bulk Import**
- Process entire city (thousands of buildings)
- Async loading (background thread)
- Chunks with buildings + terrain
- Test performance

**Week 4: Roads & Refinement**
- Road voxelization or mesh overlay
- Material types (asphalt, concrete)
- Visual polish

**Success metric:** Spawn in Brisbane, see buildings and terrain together, 60 FPS

---

#### **PHASE 8: Audio (1 week)**

**Basic 3D Audio:**
- Add rodio or kira crate
- Footstep sounds (based on material)
- Ambient sounds (wind, ocean)
- 3D positioning (left/right speaker)

**Success metric:** Walk around, hear different surfaces, hear ocean nearby

---

#### **PHASE 9: Advanced Features (ongoing)**

**This is where we are for years:**
- Water rendering & physics
- Vehicle physics (cars, boats, planes)
- Vegetation system
- Weather & time of day
- Multiplayer voice chat
- UI/HUD
- Inventory system
- Tool durability
- Crafting
- ...hundreds more features

---

### 11.3 Timeline Estimates

**Realistic (full-time solo developer):**
- Phase 1: 2 weeks
- Phase 2: 2 weeks
- Phase 3: 1 week
- Phase 4: 2 weeks
- Phase 5: 1 week
- Phase 6: 4 weeks
- Phase 7: 4 weeks
- Phase 8: 1 week
- **Total to "playable multiplayer demo": 17 weeks (~4 months)**

**Then years of feature development for "complete game"**

**Optimistic (team of 3-5):**
- Parallel development (rendering + physics + networking)
- 17 weeks → 8-10 weeks for demo

**Pessimistic (part-time or many unknowns):**
- 2× time for debugging, research, iteration
- 17 weeks → 34 weeks (~8 months)

---

## 12. KNOWN UNKNOWNS

These are things we KNOW we don't know yet. They're risks.

### 12.1 Physics at Scale

**Question:** Can Rapier handle chunk-based collision meshes?
- What happens at chunk boundaries? Seams?
- How fast is collision mesh rebuild?
- Will determinism work across different CPUs?

**Risk:** May need custom physics or different approach

**Mitigation:** Prototype early (Phase 1)

### 12.2 Network Bandwidth

**Question:** Can we actually stay under 5 MB/s?
- Current chunk size: 50 MB
- Compression ratio unknown (depends on terrain)
- Fast movement may exceed budget

**Risk:** Game unplayable on slower connections

**Mitigation:** Aggressive LOD, compression, prediction

### 12.3 CRDT Convergence Time

**Question:** How long until all peers converge?
- 10 peers: probably fast
- 1000 peers: unknown
- Gossipsub latency: unknown

**Risk:** Players see different world states for too long

**Mitigation:** Test at scale, fallback to regional servers if P2P fails

### 12.4 OSM Data Quality

**Question:** Is OSM data accurate/complete enough?
- Building heights often missing (default to 3 floors?)
- Footprints sometimes wrong
- Some areas have no data

**Risk:** World looks sparse or wrong in some regions

**Mitigation:** Manual corrections, crowd-sourcing, procedural fill-in

### 12.5 Mobile Performance

**Question:** Can phones run this?
- GPU: Much weaker than PC
- RAM: 4-8 GB typical
- Battery: Constant 3D rendering drains fast

**Risk:** Mobile unplayable (PC only)

**Mitigation:** Aggressive LOD, lower settings, consider separate mobile build

### 12.6 Emergent Complexity Performance

**Question:** Can we actually simulate footprints, plants, weather?
- Each adds overhead
- Millions of entities

**Risk:** Features sound cool but impossible to implement

**Mitigation:** Don't promise until proven

---

## 13. CRITICAL DECISIONS LOG

### 13.1 Decisions Made (Locked)

**1. Language: Rust**
- Rationale: Performance, safety, ecosystem
- Status: ✅ FINAL

**2. Coordinates: ECEF f64 canonical**
- Rationale: Precision, determinism, simplicity
- Status: ✅ FINAL

**3. Rendering: wgpu + FloatingOrigin**
- Rationale: Cross-platform, proven technique
- Status: ✅ FINAL

**4. Voxels: 1m base resolution, sparse octree**
- Rationale: Balance detail vs performance
- Status: ✅ FINAL

**5. Mesh: Marching cubes**
- Rationale: Simple, fast, good enough
- Status: ✅ WORKS (can upgrade to dual contouring later)

**6. Elevation: SRTM via GDAL**
- Rationale: Best available free data
- Status: ✅ FINAL

**7. Network: libp2p + CRDT**
- Rationale: P2P vision, no central server
- Status: ✅ ARCHITECTURE DECIDED (implementation not started)

### 13.2 Decisions Pending (Need Input)

**1. Chunk Size**
- Options: 128m, 256m, 512m
- Tradeoff: Smaller = more chunks, Larger = bigger meshes
- **Current thinking:** 256m (balance)

**2. LOD Distances**
- Options: Various distance thresholds
- Tradeoff: More LOD levels = smoother but more complexity
- **Current thinking:** 5 levels (1m/2m/4m/8m/16m)

**3. Physics Collision Mesh**
- Options: Full voxel mesh, Simplified mesh, Voxel grid
- Tradeoff: Accuracy vs performance
- **Current thinking:** Simplified mesh (fewer triangles)

**4. Building Interior**
- Options: Hollow (can walk inside), Solid (just shell)
- Tradeoff: Hollow = more interesting but complex
- **Current thinking:** Start solid, add hollow later

**5. Road Rendering**
- Options: Voxels, Separate mesh, Hybrid
- Tradeoff: Quality vs simplicity
- **Current thinking:** Hybrid (voxels + detail mesh)

**6. Water Physics**
- Options: Rapier fluids, Custom, Fake (visual only)
- Tradeoff: Realism vs performance
- **Current thinking:** Start fake, upgrade later if needed

### 13.3 Mistakes Made (Learned From)

**1. Building rendering without validation**
- Mistake: Rushed through 3 phases in 1 day
- Consequence: Never saw screenshots until much later
- Lesson: TEST VISUALLY at every step
- Status: CORRECTED (now have screenshot tools)

**2. Not considering GPU buffer limits**
- Mistake: Assumed could scale monolithic mesh indefinitely
- Consequence: Hit 256 MB limit at 1000m
- Lesson: Research hardware limits BEFORE building
- Status: CORRECTED (chunk system planned)

**3. Optimizing rendering before proving integration**
- Mistake: Almost spent 3 weeks on LOD/chunks/culling
- Consequence: Would have built wrong architecture
- Lesson: Vertical slice FIRST, optimize SECOND
- Status: CORRECTED (physics integration next)

---

## 14. CURRENT STATUS SUMMARY

### 14.1 What Exists Now

**Code: 3,347 lines (0.3% of target 1M)**

**Working Systems:**
- ✅ Coordinates (GPS/ECEF/FloatingOrigin)
- ✅ Elevation pipeline (NAS + API)
- ✅ Voxel system (octree storage)
- ✅ Terrain generation (SRTM → voxels)
- ✅ Mesh extraction (marching cubes)
- ✅ Basic rendering (wgpu pipeline)
- ✅ Camera (FPS controller)
- ✅ Screenshot tools (multi-angle validation)

**Performance:**
- ✅ 120m: 2.1s load, 410K verts, 60 FPS
- ✅ 250m: 9.6s load, 1.5M verts, 60 FPS
- ✅ 500m: 59.9s load, 6.1M verts, 60 FPS
- ❌ 1000m: 5min load, 14.5M verts, GPU buffer limit

**Limits Found:**
- ✅ GPU buffer: 256 MB maximum
- ✅ Mesh extraction: O(n²) scaling (bottleneck)
- ✅ Rendering: Excellent (60 FPS with 6M verts)

### 14.2 What Doesn't Exist

**Missing Major Systems (0% code):**
- ❌ Physics (Rapier not added)
- ❌ Player controller (no entity)
- ❌ Collision detection
- ❌ Interaction (raycast, dig, place)
- ❌ Chunk system (mandatory for 1km+)
- ❌ LOD system
- ❌ Frustum culling
- ❌ Networking (libp2p not added)
- ❌ CRDT state sync
- ❌ OSM building import
- ❌ Road rendering
- ❌ Water
- ❌ Audio
- ❌ UI/HUD
- ❌ Inventory
- ❌ Tools/weapons
- ❌ Vehicles
- ❌ Weather
- ❌ Plants
- ❌ Animals
- ❌ NPCs
- ❌ Multiplayer features
- ❌ ...hundreds more

### 14.3 Next Immediate Steps

**Option A: Vertical Slice (RECOMMENDED)**
1. Add Rapier physics (1 day)
2. Player controller (2 days)
3. Dig/place voxels (2 days)
4. Prove integration (1 day)
- **Total: 1-2 weeks**
- **Risk: LOW** (proves architecture)
- **Value: HIGH** (validates entire stack)

**Option B: Rendering Optimization (RISKY)**
1. Chunk system (1 week)
2. Async loading (1 week)
3. LOD system (2 weeks)
- **Total: 3-4 weeks**
- **Risk: HIGH** (may not integrate with physics/networking)
- **Value: MEDIUM** (pretty but not playable)

**Option C: Networking First (WRONG)**
1. libp2p setup (1 week)
2. CRDT (2 weeks)
3. Multi-peer testing (1 week)
- **Total: 4 weeks**
- **Risk: EXTREME** (nothing to sync yet)
- **Value: LOW** (can't demo without gameplay)

**RECOMMENDATION: Option A (Vertical Slice)**

---

## 15. FINAL THOUGHTS

### 15.1 The Reality of This Project

**This is not a game you build in a year.**

This is a **PLATFORM** you build over 5-10 years, with a team, with funding.

**Comparable Projects:**
- Minecraft: 1 developer → 10 years → sold for $2.5B
- No Man's Sky: 15 developers → 4 years → £40M revenue
- Star Citizen: 500 developers → 12+ years → $700M funding → STILL not done

**You are attempting something harder than any of these.**

**Realistic expectations:**
- Year 1: Playable demo (walk, dig, build, multiplayer)
- Year 2: Real-world data integration (buildings, roads)
- Year 3: Advanced features (vehicles, weather, NPCs)
- Year 4-5: Polish, optimization, content
- Year 6+: Ongoing development, community, ecosystem

**This is a LIFETIME project.**

### 15.2 What Makes This Possible

**Despite the insane scope, this CAN work because:**

1. **Rust ecosystem is mature**
   - wgpu, Rapier, libp2p all exist and work
   - Don't have to build from scratch

2. **Real-world data is free**
   - OSM, SRTM both public domain
   - $0 data licensing cost

3. **P2P removes infrastructure cost**
   - No servers to pay for
   - Players host their own data
   - Scales infinitely (in theory)

4. **GPU hardware is powerful**
   - 60 FPS with 6M vertices proves this
   - Can handle the visual complexity

5. **Voxel approach is proven**
   - Minecraft validated the concept
   - We know it can work at scale

6. **You have time**
   - Not a startup rushing to market
   - Can iterate slowly, get it right

**But it requires:**
- Patience (5-10 years)
- Focus (don't try to do everything at once)
- Validation (test at every step)
- Realism (know when to cut scope)

### 15.3 How to Succeed

**1. Build in vertical slices**
- Prove integration before optimizing
- Small working demo > large broken system

**2. Validate constantly**
- Screenshots, videos, metrics
- If you can't see it, it's not real

**3. Don't over-optimize early**
- Make it work, THEN make it fast
- Premature optimization wastes time

**4. Document everything**
- Future you will forget why you made decisions
- This document is your lifeline

**5. Know when to pivot**
- If physics doesn't integrate → change architecture NOW
- If networking is too slow → consider hybrid P2P/server
- If footprints are impossible → cut the feature

**6. Celebrate small wins**
- First time player walks: HUGE
- First time multiplayer syncs: HUGE
- First time you see a real building: HUGE

**This is a marathon, not a sprint.**

---

## 16. CONCLUSION

**You asked for the full scope. This is it.**

- 1:1 Earth scale
- Voxel interaction
- AAA visuals
- P2P networking
- Real-world data
- Emergent complexity

**No one has built this. You're attempting the impossible.**

**But "impossible" just means "no one's done it YET."**

The technology exists. The data exists. The GPU power exists.

What's needed is **time, focus, and relentless validation.**

**Start with the vertical slice. Prove it can work. Then scale.**

---

**END OF MASTER SCOPE**

**Document Size: ~20,000 words**  
**Last Updated: 2026-02-17**  
**Version: 1.0**

This document will grow as we learn more. Treat it as a living guide.

