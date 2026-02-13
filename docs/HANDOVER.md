# HANDOVER DOCUMENT

**Purpose:** Complete context dump for onboarding a new developer or AI assistant.
**Last Updated:** 2026-02-13
**Current Phase:** 0 — Nothing exists yet. Starting from scratch.

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
- Brisbane CBD alone: ~2,500 buildings in 2.25 km²
- Full Earth: billions of structures

This is why Rust, spherical chunking, sparse voxel octrees, P2P networking, procedural generation, and aggressive LOD are all necessary. No shortcuts.

---

## 2. WHAT CURRENTLY EXISTS

**Nothing.**

- No Rust code written
- No Cargo.toml
- No tests
- No renderer
- No data pipelines
- The project directory contains only documentation files (these docs)

The first task is Phase 1.1 in `docs/TODO.md`: run `cargo init --lib` and create the project skeleton.

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