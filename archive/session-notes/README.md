# METAVERSE CORE

**GTA V meets Minecraft on a 1:1 spherical Earth — built in Rust, powered by real-world data, decentralised by design.**

---

## What Is This?

A Rust-based runtime and renderer for a planet-scale, fully interactive, volumetric digital twin of Earth.

- **Spherical geometry** — the world is a sphere. Chunks are quad-sphere wedge tiles, not flat grids.
- **1:1 scale** — every GPS coordinate maps to an exact 3D position on the sphere.
- **Volumetric** — from Earth's core to orbiting satellites. Underground, surface, and sky are one continuous space.
- **Fully interactive** — build, destroy, modify anything. Watch a TV through a shop window as you walk past.
- **Real-world data** — procedurally generated from OpenStreetMap and SRTM elevation data.
- **Peer-to-peer** — users ARE the infrastructure. Servers exist for caching and bootstrapping only.
- **AAA fidelity target** — minimum GTA V quality with scalable settings for all hardware.
- **Rust-first** — custom wgpu/Vulkan renderer. No game engine dependency.

---

## Current Status

**No code exists yet. This project is starting from scratch.**

See `docs/TODO.md` for the full task list starting from Phase 1.

---

## Project Documents

| Document | Purpose |
|----------|---------|
| `docs/HANDOVER.md` | Full context for developer/AI onboarding |
| `docs/TECH_SPEC.md` | Technical architecture and design decisions |
| `docs/TODO.md` | Prioritised task list with acceptance criteria |
| `docs/RULES.md` | Non-negotiable development rules |
| `docs/TESTING.md` | Testing strategy, scale gates, verification |
| `docs/GLOSSARY.md` | Terminology definitions |

---

## Vision

Read Neal Stephenson's *Snow Crash*. Now imagine that Metaverse, but mapped 1:1 onto the real spherical Earth, fully volumetric from core to orbit, procedurally generated from real data, every object interactable and destructible, running on a decentralised P2P network, with AAA visual fidelity.

That is what this project is building.

---

## License

TBD — currently private development.