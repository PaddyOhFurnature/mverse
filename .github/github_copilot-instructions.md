# Copilot Instructions — Project Init

## WHAT THIS PROJECT IS

A 1:1 scale spherical Earth metaverse built in Rust. GTA V meets Minecraft on a real-world sphere.
P2P networked, fully interactive (build/destroy everything), AAA visual fidelity target.
Real-world data (OSM buildings, SRTM terrain) procedurally generates the world.

**No code exists yet. This is a brand new project. Phase 1 starts from `cargo init`.**

## CRITICAL: READ THESE FIRST

Before doing ANY work, read these files in order:
1. `docs/RULES.md` — Non-negotiable development constraints
2. `docs/TODO.md` — Prioritised task list (work top-to-bottom)
3. `docs/TECH_SPEC.md` — Architecture and design decisions
4. `docs/TESTING.md` — How to verify everything works
5. `docs/HANDOVER.md` — Full project context and current state
6. `docs/GLOSSARY.md` — Terminology definitions

## CURRENT STATE

**Phase 0 — Nothing exists. No code. No Cargo.toml. No tests. Empty project.**

The first task is Phase 1.1 in `docs/TODO.md`: run `cargo init --lib` and create the project skeleton.

## NEXT TASK

Always start at the first unchecked `[ ]` item in `docs/TODO.md`.

## COMMANDS

Once Phase 1.1 is complete:

```bash
# ALWAYS run before and after changes:
cargo test --lib -- --nocapture

# Performance benchmarks:
cargo test --release --lib -- --nocapture

# Single module tests:
cargo test --lib coordinate -- --nocapture
cargo test --lib chunk -- --nocapture
```

## HARD RULES (Summary — full version in docs/RULES.md)

1. Tests before code (TDD: red → green → refactor)
2. ALL tests must pass before ANY commit
3. Scale gate tests gate phase progression (see docs/TESTING.md)
4. Never modify coordinate math without extensive testing
5. Never optimise without measuring first
6. Priority order: Correctness → Performance → Readability → Simplicity
7. Document every public function with doc comments
8. 2-second cooldown between ALL external API calls
9. The world is a SPHERE — no planar approximations across chunks
10. All simulation must be deterministic (fixed timestep, no HashMap ordering for logic)
11. Descriptive commit messages: `type(scope): description`
12. No `unsafe` without documented justification and measured proof it is necessary

## PLANNED PROJECT STRUCTURE

Does not exist yet. Build incrementally as tasks are completed:

```
metaverse_core/
├── .github/
│   └── copilot-instructions.md  ← You are here
├── Cargo.toml
├── README.md
├── docs/
│   ├── HANDOVER.md
│   ├── TECH_SPEC.md
│   ├── TODO.md
│   ├── TESTING.md
│   ├── RULES.md
│   └── GLOSSARY.md
├── src/
│   ├── lib.rs
│   ├── coordinates.rs
│   ├── chunks.rs
│   ├── svo.rs
│   ├── osm.rs
│   ├── elevation.rs
│   ├── world.rs
│   ├── entity.rs
│   ├── physics.rs
│   ├── network.rs
│   ├── identity.rs
│   ├── renderer/
│   │   ├── mod.rs
│   │   ├── pipeline.rs
│   │   ├── camera.rs
│   │   ├── mesh.rs
│   │   ├── materials.rs
│   │   ├── lighting.rs
│   │   └── shaders/
│   └── tests/
│       ├── mod.rs
│       └── coordinate_tests.rs (etc)
├── examples/
└── benchmarks/
```

## KEY TECHNICAL DECISIONS

- **Language:** Rust (non-negotiable)
- **Renderer:** Custom wgpu/Vulkan (not Bevy, not Unreal)
- **Coordinate canonical frame:** ECEF (Earth-Centered Earth-Fixed), WGS84 ellipsoid
- **Chunk system:** Quad-sphere (cube projected onto sphere, quadtree per face)
- **Volumetric model:** Sparse Voxel Octree (SVO) for build/destroy
- **Physics:** Rapier (deterministic mode, fixed 60Hz timestep)
- **Networking:** libp2p (Kademlia DHT + Gossipsub), geo-sharded by chunk ID
- **State sync:** CRDT op logs, signed with Ed25519
- **Caching:** memory (LRU) → disk (~/.metaverse/cache/) → network (API/P2P)
- **Data sources:** OpenStreetMap (Overpass API + Geofabrik bulk), SRTM (NASA elevation)

## WHEN IN DOUBT

1. Re-read `docs/RULES.md`
2. Write a test first
3. Keep it simple
4. Ask the developer before making architectural decisions