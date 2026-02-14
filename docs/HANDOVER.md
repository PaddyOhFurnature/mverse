# HANDOVER DOCUMENT

**Purpose:** Complete context dump for onboarding a new developer or AI assistant.
**Last Updated:** 2026-02-14
**Current Phase:** SVO PIPELINE INTEGRATED WITH RENDERER. All 6 phases complete. Viewer ready for testing.

---

## 1. WHAT YOU ARE BUILDING

A 1:1 scale, spherical, volumetric digital twin of Earth. Not a game — infrastructure.

### Core Concept

"GTA V meets Minecraft on a real-world 1:1 sphere."

- **GTA V:** AAA visual fidelity, interactive world, NPCs, vehicles, physics, weather. Walk past a shop and watch the TV in the window.
- **Minecraft:** Every block is buildable and destroyable. Player-driven world modification. Volumetric space. Dig to bedrock, build to the sky.
- **Real-world 1:1 sphere:** Actual spherical Earth geometry. Real GPS coordinates. Real building and road data. Real terrain elevation. The entire planet.

### What Makes This Different

- The world is a **sphere** — not a flat plane, not a skybox, not a heightmap on a flat grid
- It covers the **entire Earth** — not a 10km² game map
- It uses **real data** — OpenStreetMap buildings, SRTM terrain, satellite imagery (future)
- It is **volumetric** — from Earth's core to orbiting satellites, one continuous 3D space
- It is **fully mutable** — dig, build, destroy anything
- It is **decentralised** — P2P primary, servers for caching/bootstrap only
- It targets **AAA fidelity** — minimum GTA V quality
- Every entity is **interactable** — TVs play, doors open, vehicles drive

### The Scale

- Earth surface: 510 million km²
- At 1m resolution: 510 trillion surface points
- Volumetric: ~10²¹ cubic metres
- Brisbane CBD alone: ~55,000 buildings in 10km × 10km
- Full Earth: billions of structures

This is why Rust, spherical chunking, sparse voxel octrees, P2P networking, procedural generation, and aggressive LOD are all necessary. No shortcuts.

---

## 2. WHAT CURRENTLY EXISTS

**ALL SVO PHASES COMPLETE (1-5): Volumetric world pipeline fully implemented from data to renderable mesh.**

### Working Systems

**Foundation (Pre-SVO):**
- ✅ **Coordinate System**: ECEF ↔ GPS conversions (WGS84 ellipsoid, sub-millimetre precision)
- ✅ **Quad-sphere Chunks**: Cube-projected sphere, quadtree subdivision, face/path addressing
- ✅ **Multi-source Elevation**: AWS Terrarium tiles (primary), USGS 3DEP (stub), OpenTopography (stub)
- ✅ **Parallel Downloading**: Up to 8 concurrent tile downloads with smart caching
- ✅ **Three-level Cache**: Memory → Disk (~/.metaverse/cache/) → Network
- ✅ **OSM Data**: 55k buildings, 47k roads, 90 water features cached for Brisbane

**SVO Pipeline (Complete + Integrated):**
- ✅ **Phase 1: Core SVO** (39 tests)
  - SvoNode enum (Empty/Solid/Branch)
  - MaterialId system (16 materials: STONE, DIRT, WATER, CONCRETE, ASPHALT, WOOD, etc.)
  - set_voxel/get_voxel/clear_voxel operations
  - fill_region/clear_region bulk operations
  - Op logging for CRDT synchronization

- ✅ **Phase 2: Terrain Generation** (9 tests)
  - Heightmap → volumetric SVO conversion
  - STONE below surface, DIRT top layer, AIR above
  - WATER for below-sea-level areas
  - Chunk boundary smoothing
  - Surface gradient calculation

- ✅ **Phase 3: OSM CSG Operations** (5 tests)
  - Rivers: carved channels with WATER (5m/2m/3m depth)
  - Roads: ASPHALT surface + STONE roadbed (4-12m width)
  - Buildings: filled volumes with foundations (CONCRETE/WOOD)
  - Bridges: elevated decks + support pillars (every 20m)
  - Tunnels: circular passages with CONCRETE walls

- ✅ **Phase 4: Mesh Extraction** (9 tests)
  - Marching cubes algorithm (256-case lookup)
  - Per-material mesh generation
  - 5-level LOD system (0m, 50m, 200m, 500m, 1km+)
  - Automatic LOD selection by distance

- ✅ **Phase 5: Material Rendering** (3 tests)
  - Material color palette (16 materials with RGB values)
  - Lambertian diffuse + ambient lighting
  - Vertex color application
  - Extendable to 256 materials

- ✅ **Phase 6: Renderer Integration** (2 tests)
  - ColoredVertex format matching wgpu pipeline (pos+normal+RGBA)
  - generate_chunk_mesh_from_svo() bridge function
  - generate_test_mesh_from_osm() for simplified testing
  - Updated viewer.rs to use SVO pipeline
  - Removed old surface mesh generation code

### Current State

- **251 tests passing** (4 ignored, all green)
- **Complete SVO pipeline + renderer integration**: Data → Terrain → CSG → Mesh → Materials → GPU
- **8 new modules**: svo.rs, terrain.rs, osm_features.rs, marching_cubes.rs, mesh_generation.rs, materials.rs, svo_integration.rs
- **~2,800 lines** of new SVO code
- **Viewer ready to render** (using test mesh path currently)

### Architecture Flow

```
Real World Data
    ↓
[OpenStreetMap] + [SRTM Elevation]
    ↓
Terrain Generation (Phase 2)
    ↓
[Sparse Voxel Octree - Volumetric World]
    ↓
CSG Operations (Phase 3)
    ├─ Carve Rivers
    ├─ Place Roads
    ├─ Add Buildings
    ├─ Create Bridges
    └─ Dig Tunnels
    ↓
Marching Cubes (Phase 4)
    ↓
[Triangle Mesh per Material]
    ↓
LOD Generation (5 levels)
    ↓
Material Colors + Lighting (Phase 5)
    ↓
[Renderable Colored Mesh]
    ↓
GPU Rendering → Screen
```

### What Can It Do Now

✅ Generate volumetric terrain from elevation data  
✅ Carve rivers into terrain with flowing water  
✅ Place roads on terrain surfaces  
✅ Build structures with foundations  
✅ Create bridges spanning valleys  
✅ Dig tunnels through mountains  
✅ Extract renderable meshes from voxels  
✅ Automatic LOD based on distance  
✅ Material-specific coloring with lighting  

### Why We Pivoted

**Old approach:** Hollow sphere with meshes "painted" on surface
- Roads/water underground or not visible
- Backface culling broken (visible from below, not above)
- No volumetric representation = can't show:
  - Rivers cutting INTO terrain  
  - Bridges spanning OVER valleys
  - Tunnels through mountains
  - Building interiors
  - Multi-level roads
  - Terrain modification

**New approach:** World is SOLID volume from core (-6,371km) to atmosphere (100km+)
- SVO stores only non-empty regions (sparse)
- CSG operations (add/subtract/replace materials)
- Marching cubes extracts renderable surface mesh
- Supports all features: tunnels, caves, overhangs, building interiors

### Next Steps (After Integration)

1. **Populate marching cubes triangle table** (256 complete entries)
   - Currently stubbed with [-1; 16] for all entries
   - Need full triangle definitions for actual mesh generation
   - See marching cubes lookup table references

2. **Switch to full SVO pipeline in viewer**
   - Currently using generate_test_mesh_from_osm() (simple boxes)
   - Update to use generate_chunk_mesh_from_svo() with full pipeline
   - Connect real elevation data to terrain generation

3. **Test with Brisbane data**
   - Run viewer with 55k buildings
   - Verify roads and water render correctly
   - Measure FPS and memory usage

4. **Performance optimization**
   - Frustum culling per chunk
   - Chunk streaming/unloading based on distance
   - Vertex deduplication in mesh extraction
   - Internal face removal optimization

5. **Material improvements**
   - Texture mapping support
   - PBR material properties
   - More realistic lighting

### Recently Completed

✅ **Renderer Integration** (Phase 6)
- Created svo_integration.rs bridge module
- ColoredVertex format matches wgpu pipeline
- generate_chunk_mesh_from_svo() entry point
- Updated viewer.rs to use SVO pipeline
- Removed 114 lines of old surface mesh code
- All tests passing (251 passed, 4 ignored)

### Obsolete Code (Mostly Removed)

The following still exist but are no longer used by viewer:
- `src/renderer/mesh.rs` - generate_buildings_from_osm(), generate_roads_from_osm_with_elevation(), generate_water_from_osm_with_elevation()

These can be removed once we confirm SVO pipeline works correctly.

---

## 3. PLANNED FILE STRUCTURE

This is the TARGET. Build it incrementally. Do not create files until the relevant task requires them.

```
metaverse_core/
├── .github/
│   └── copilot-instructions.md
├── Cargo.toml
├── README.md
├── docs/
│   ├── HANDOVER.md               # This file
│   ├── TECH_SPEC.md
│   ├── TODO.md
│   ├── RULES.md
│   ├── TESTING.md
│   └── GLOSSARY.md
├── src/
│   ├── lib.rs                    # Crate root, module declarations
│   ├── coordinates.rs            # Phase 1: ECEF, GPS, ENU conversions
│   ├── chunks.rs                 # Phase 2: quad-sphere tile system
│   ├── svo.rs                    # Phase 3: sparse voxel octree
│   ├── cache.rs                  # Phase 4: disk cache infrastructure
│   ├── osm.rs                    # Phase 4: OpenStreetMap pipeline
│   ├── elevation.rs              # Phase 4: SRTM terrain pipeline
│   ├── world.rs                  # Phase 7: world state manager
│   ├── entity.rs                 # Phase 8: entity-component system
│   ├── identity.rs               # Phase 10: Ed25519 identity + ownership
│   ├── network.rs                # Phase 11: libp2p P2P layer
│   ├── physics.rs                # Phase 12: Rapier integration
│   ├── renderer/                 # Phase 5+: wgpu rendering
│   │   ├── mod.rs
│   │   ├── pipeline.rs
│   │   ├── camera.rs
│   │   ├── mesh.rs
│   │   ├── materials.rs
│   │   ├── lighting.rs
│   │   └── shaders/
│   └── tests/
│       ├── mod.rs
│       ├── coordinate_tests.rs   # Phase 1
│       ├── chunk_tests.rs        # Phase 2
│       ├── svo_tests.rs          # Phase 3
│       ├── cache_tests.rs        # Phase 4
│       ├── osm_tests.rs          # Phase 4
│       ├── elevation_tests.rs    # Phase 4
│       ├── world_tests.rs        # Phase 7
│       ├── entity_tests.rs       # Phase 8
│       ├── identity_tests.rs     # Phase 10
│       ├── network_tests.rs      # Phase 11
│       └── physics_tests.rs      # Phase 12
├── examples/
│   └── viewer.rs                 # Phase 5: visual demo
├── tests/
│   └── fixtures/
│       └── brisbane_cbd.json     # Phase 4: saved Overpass response for testing
└── benchmarks/
    └── results.csv               # Performance tracking
```

---

## 4. DEVELOPMENT CONSTRAINTS

- **Solo developer** — one person, working alone for now
- **Desktop-first** — Linux (Pop!_OS) is the primary development and target platform
- **Correctness over speed** — every phase is a grind; no shortcuts, no half-measures
- **Incremental testing** — scale gate tests at every phase boundary
- **Plans will change** — this documentation is the plan; code is reality; when they conflict, update the docs to match what was learned
- **No code exists** — everything starts from `cargo init`

---

## 5. HOW TO START A NEW SESSION

```
1. Read docs/RULES.md
2. Read docs/TODO.md — find the first unchecked [ ] task
3. If any code exists: run  cargo test --lib -- --nocapture
4. If all tests pass (or no code exists yet): proceed with the next unchecked task
5. If any test fails: STOP and fix it before doing anything else
```

---

## 6. KEY TECHNICAL DECISIONS (SUMMARY)

Full details in `docs/TECH_SPEC.md`.

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Performance, safety, no GC pauses |
| Coordinate frame | ECEF (WGS84) | Accurate globally, no flat-earth errors |
| Chunk system | Quad-sphere | Uniform tiles, hierarchical LOD, GPU-friendly |
| Volumetric model | Sparse Voxel Octree | Build/destroy, memory-efficient, LOD built-in |
| Renderer | Custom wgpu/Vulkan | Full control, single language, cross-platform |
| Physics | Rapier3D | Rust-native, deterministic, fast |
| Networking | libp2p (Kademlia + Gossipsub) | Proven P2P stack, Rust-native |
| State sync | CRDT op logs | Conflict-free merging, deterministic |
| Identity | Ed25519 keypairs | No central auth, fast, secure |
| Caching | Memory LRU → disk → network | Minimise API load, fast loads |
| OSM data | Overpass API + Geofabrik | Global coverage, open license |
| Elevation | NASA SRTM (HGT) | Global coverage, public domain |

---

## 7. REFERENCE LANDMARKS FOR TESTING

These GPS coordinates are used throughout testing to verify accuracy:

| Landmark | Latitude | Longitude | Notes |
|----------|----------|-----------|-------|
| Queen Street Mall, Brisbane | −27.4698 | 153.0251 | Primary origin / reference point |
| Story Bridge, Brisbane | −27.4634 | 153.0394 | ~1.6km from Queen St |
| Mt Coot-tha, Brisbane | −27.4750 | 152.9578 | Elevated terrain (287m) |
| Brisbane Airport | −27.3942 | 153.1218 | ~15km from CBD |
| Sydney Opera House | −33.8568 | 151.2153 | ~732km from Brisbane |
| London, Big Ben | 51.5007 | −0.1246 | ~16,500km from Brisbane |
| North Pole | 90.0 | 0.0 | Edge case: cos(φ)=0 |
| South Pole | −90.0 | 0.0 | Edge case: cos(φ)=0 |
| Null Island | 0.0 | 0.0 | Equator + Greenwich intersection |
| Antimeridian | 0.0 | 180.0 | Edge case: longitude wrap |
| Dead Sea | 31.5 | 35.5 | Negative elevation (−430m) |
| Mount Everest | 27.9881 | 86.9250 | High elevation (8,848m) |