# GLOSSARY

**Purpose:** Definitions of every term, acronym, and concept used in this project.
**Last Updated:** 2026-02-13

---

## Coordinate Systems

**GPS / Geodetic** — Latitude (degrees, −90 to +90), Longitude (degrees, −180 to +180), Elevation (metres above WGS84 ellipsoid). Human-readable. Used for data input, display, and teleportation. Never used directly for simulation or rendering.

**ECEF (Earth-Centered, Earth-Fixed)** — Cartesian coordinate system with origin at Earth's centre. X axis through equator at 0° longitude, Y axis through equator at 90°E, Z axis through North Pole. Units: metres. Stored as f64. The canonical absolute reference frame for all positions in this project.

**ENU (East-North-Up)** — Local tangent plane coordinate system anchored at a specific point on the Earth's surface. East = +X, North = +Y, Up = +Z. Used for chunk-local positions. Accurate within ~50km of the anchor point. Stored as f32 for rendering (sufficient precision at chunk scale < 500m).

**WGS84** — World Geodetic System 1984. The reference ellipsoid used by GPS. Semi-major axis: 6,378,137.0m. Flattening: 1/298.257223563. All geodetic calculations in this project use WGS84.

**Floating Origin** — Rendering technique where the camera is always at or near world-space origin (0,0,0) and all geometry is translated by (−camera_position) before GPU submission. Prevents float32 precision loss at large coordinate values. Without this, geometry at Brisbane ECEF coordinates (~6.37 million metres from origin) would have metre-level jitter.

---

## Chunking & Spatial

**Quad-Sphere** — A sphere created by projecting a subdivided cube onto a sphere surface using an equal-area projection. Each of the 6 cube faces becomes an independent quadtree. The primary spatial indexing and LOD system for this project.

**ChunkId** — Unique identifier for a tile on the quad-sphere. Composed of a face index (0-5) and a quadtree path (sequence of 0-3 indices). Example: face=2, path=[0,3,1,2] means "face 2, subdivided top-left, then bottom-right, then top-right, then bottom-left." Depth = path length.

**LOD (Level of Detail)** — Rendering technique where distant objects use simpler geometry and textures to maintain performance. In this project, LOD is driven by quadtree depth: deeper subdivision = more detail = closer to camera.

**Tile Depth** — Number of quadtree subdivisions from the root face. Depth 0 = entire cube face (~6,700km). Depth 14 = ~400m tile. Depth 20 = ~6m tile.

**Scale Gate** — Mandatory test checkpoint at a specific geographic radius. Development phases cannot advance until all required scale gates pass at the current level.

**Seam** — Boundary between two adjacent quad-sphere tiles. Must be handled carefully to avoid visible gaps, cracks, or overlapping geometry. Cross-face seams (where two cube faces meet) require special neighbour lookup logic.

**Frustum Culling** — Not rendering objects outside the camera's view pyramid. Essential for performance at planet scale where millions of tiles exist but only dozens are visible.

**Occlusion Culling** — Not rendering objects hidden behind other objects. Important for dense city scenes and underground areas where most geometry is occluded.

---

## Volumetric & Voxels

**SVO (Sparse Voxel Octree)** — Hierarchical 3D data structure where each node has up to 8 children (octants). "Sparse" means only occupied regions are stored; empty space costs almost nothing. Used for mutable/destructible world geometry (build and destroy).

**Voxel** — Volumetric pixel. A cubic unit of 3D space with a material type. The fundamental unit of buildable/destroyable world geometry in this project.

**Octant** — One of 8 child regions of an octree node. Each octant is half the parent's size in each dimension (x, y, z).

**MaterialId** — Identifier for the substance a voxel is made of (stone, wood, metal, glass, etc.). Stored as u16 (65,536 possible materials). Determines visual appearance, physical properties, and destruction behaviour.

**Marching Cubes** — Algorithm to convert a voxel field into a triangle mesh for rendering. Produces smooth surfaces from discrete voxel data. One of the options for SVO-to-mesh conversion.

**Surface Nets / Dual Contouring** — Alternative mesh extraction algorithms. Dual contouring preserves sharp edges better than marching cubes. Surface nets are simpler and produce decent results.

**Op Log** — Ordered list of operations (set voxel, clear voxel, fill region, etc.) applied to an SVO. Every mutation produces an op log entry. Ops are signed, timestamped, and content-addressed for P2P replication and determinism verification.

---

## Rendering

**wgpu** — Rust-native graphics API abstracting over Vulkan (Linux/Windows), Metal (macOS), DX12 (Windows), and WebGPU (browser). The rendering backend for this project. Cross-platform by design.

**WGSL** — WebGPU Shading Language. The shader language used with wgpu. Vertex shaders, fragment shaders, and compute shaders are written in WGSL.

**PBR (Physically Based Rendering)** — Lighting model where materials are defined by albedo (base colour), roughness (matte to glossy), and metallic (dielectric to metal) properties. Produces realistic shading under any lighting condition. Industry standard.

**Deferred Rendering** — Technique where geometry is first rendered to a G-Buffer (multiple textures storing per-pixel data), then lighting is computed in a separate fullscreen pass reading the G-Buffer. Efficient for many light sources. Used in GTA V, most modern AAA games.

**G-Buffer** — Set of textures storing per-pixel geometry information: albedo, normal, depth, roughness, metallic, emissive. Written during the geometry pass, read during the lighting pass.

**CSM (Cascaded Shadow Maps)** — Shadow technique using multiple shadow maps at different resolutions for different distance ranges. Near shadows are high resolution; far shadows are lower resolution. Standard for open-world games.

**SSAO (Screen-Space Ambient Occlusion)** — Post-processing effect that darkens creases and corners to simulate ambient light being blocked by nearby geometry. Cheap approximation of global illumination.

**GTAO (Ground Truth Ambient Occlusion)** — Improved SSAO variant with better accuracy and fewer artefacts. Slightly more expensive.

**SSR (Screen-Space Reflections)** — Reflections computed by ray-marching the depth buffer. Works for on-screen geometry; falls back to environment maps for off-screen reflections. Used for water, glass, wet surfaces.

**Virtual Texturing** — Technique where only visible portions of very large textures are resident in GPU memory. Essential for planet-scale terrain texturing where the full texture would be terabytes.

**Instancing** — GPU technique drawing the same mesh many times with different transforms in a single draw call. Critical for rendering thousands of similar buildings efficiently.

**Billboard** — Flat quad that always faces the camera. Used for distant trees, signs, and other objects where full 3D is unnecessary at that distance.

---

## Networking

**P2P (Peer-to-Peer)** — Network architecture where clients connect directly to each other without a central authoritative server. In this project, the P2P network is the primary infrastructure; servers are helpers only.

**DHT (Distributed Hash Table)** — Decentralised key-value store spread across all peers. Used for discovering other peers and locating chunk data. This project uses Kademlia (via libp2p).

**Kademlia** — Specific DHT algorithm using XOR-distance metric for routing. Well-studied, efficient, widely deployed (BitTorrent, IPFS, Ethereum). Available in libp2p.

**Geo-Sharding** — Partitioning the DHT keyspace by geographic location using quad-sphere tile IDs as keys. Peers only need detailed knowledge of other peers in nearby geographic regions, reducing lookup overhead.

**CRDT (Conflict-free Replicated Data Type)** — Data structure that can be modified independently on multiple peers and merged deterministically without coordination. Guarantees eventual consistency. Used for shared world state (op logs).

**Gossipsub** — libp2p pub-sub protocol where messages propagate through the network via gossip. Peers subscribe to topics (e.g., chunk region IDs) and receive messages published to those topics. Used for broadcasting ops and position updates.

**Dead Reckoning** — Predicting an entity's future position based on its last known position and velocity. Reduces bandwidth by sending corrections only when prediction diverges from reality by more than a threshold.

**Bootstrap Node** — A well-known, publicly reachable server that helps new peers discover the P2P network. It runs a libp2p node and participates in the DHT. Not authoritative over world state.

**Cache Server** — A server that stores pre-fetched and pre-processed chunk data. Reduces load on external APIs and speeds up chunk loading for new peers. Not authoritative. The world works without it (just slower initial loads).

**HLC (Hybrid Logical Clock)** — Timestamp mechanism combining physical wall-clock time with a logical counter to establish causal ordering of events across distributed peers without requiring perfectly synchronised clocks.

---

## Data Sources

**OSM (OpenStreetMap)** — Community-maintained global geographic database. Licensed under ODbL (Open Database License — requires attribution and share-alike). Source for buildings, roads, parks, water, railways, underground features.

**Overpass API** — Query interface for OpenStreetMap data. Accepts Overpass QL queries, returns JSON or XML. Rate-limited (max ~2 concurrent queries, ~1-2 requests per minute per IP). Must be used respectfully with cooldowns and backoff.

**Geofabrik** — Provider of bulk OSM data extracts. Pre-processed PBF files for continents, countries, cities. Use for large-area imports instead of Overpass API.

**SRTM (Shuttle Radar Topography Mission)** — NASA elevation dataset from 2000 Space Shuttle mission. Provides terrain height at ~30m (SRTM1) or ~90m (SRTM3) resolution globally (60°N to 56°S). File format: HGT (raw binary, 16-bit big-endian signed integers). Public domain.

**HGT** — File format for SRTM elevation data. Each file covers 1° × 1° of latitude/longitude. Named by south-west corner: `N37W122.hgt`. SRTM1 files: 3601 × 3601 samples × 2 bytes = 25,934,402 bytes. SRTM3 files: 1201 × 1201 × 2 = 2,884,802 bytes.

**ODbL (Open Database License)** — License governing OpenStreetMap data. Requires: attribution ("© OpenStreetMap contributors"), share-alike (derivative databases must use same license). Does NOT restrict rendering/display; DOES restrict redistributing raw data without license compliance.

---

## Identity & Ownership

**Ed25519** — Elliptic curve digital signature algorithm. Used for user identity keypairs. Fast signing and verification. 32-byte public keys, 64-byte signatures. Used by Signal, SSH, many blockchain projects.

**Keypair** — Public key (shareable, acts as identity) + private key (secret, signs operations). Generated locally on first run. No central authority. User is responsible for backup.

**Volumetric Parcel** — 3D bounding box within a chunk that is owned by a user. Defined by minimum and maximum corners (chunk-local coordinates) plus owner's public key and a cryptographic signature. You own a volume of space, not just a flat land plot.

**Signed Op** — A world operation (build, destroy, entity change, ownership claim) cryptographically signed by the author's Ed25519 private key. Peers verify signatures before applying ops. Forgery is computationally infeasible.

**Manifest** — Metadata record for a chunk: content hash (SHA-256) of all geometry and SVO state, ordered list of signed ops, data source attribution, ownership records. Used for verification, provenance, and P2P data integrity.

---

## Physics

**Rapier3D** — Rust-native physics engine by Dimforge. Supports rigid bodies, colliders, joints, character controllers. Offers near-deterministic simulation with fixed timestep on the same platform.

**Fixed Timestep** — Running physics at a constant rate (60 Hz = 16.67ms per tick) regardless of rendering frame rate. Essential for deterministic simulation and stable behaviour.

**Collider** — Invisible geometry shape used for physics collision detection. Types: box, sphere, capsule, convex hull, triangle mesh, heightfield (terrain).

**Character Controller** — Physics system for player movement: walk on surfaces, gravity, jumping, slope handling, stair stepping, wall collision. Not a simple rigid body — has special rules for smooth human-like movement.

---

## Project-Specific Terms

**Phase** — Major development milestone. Phases are sequential. Each phase has multiple tasks with acceptance criteria. Scale gate tests must pass before advancing to the next phase.

**Scale Gate** — See Chunking & Spatial section above.

**PoC (Proof of Concept)** — First working demo: one street block with buildings, roads, terrain, and interactive entities. Target for demonstrating viability and attracting collaborators.

**MVP (Minimum Viable Product)** — Phase 15 completion: walk anywhere on Earth, see accurate buildings/roads/terrain, build/destroy, meet other players via P2P, all at playable FPS.

**AAA Fidelity** — Visual quality comparable to modern high-budget games (GTA V, Red Dead Redemption 2). PBR materials, realistic lighting, shadows, atmospheric effects, detailed geometry. The target, not the starting point.

**Graphics Tier** — Quality preset (Potato, Low, Medium, High, Ultra) that enables/disables rendering features to match hardware capability. Auto-detected, user-overridable.

**Floating-Point Determinism** — Ensuring floating-point arithmetic produces identical bit-exact results across different runs on the same platform. Required for P2P state consistency. Achieved through fixed timestep, careful operation ordering, and avoiding non-deterministic CPU features.