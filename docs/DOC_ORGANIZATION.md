# Documentation Organization - What's Current vs Old

**Last Updated:** 2026-02-17  

---

## CURRENT FOUNDATION RESEARCH (Created Today - VALID)

These are NEW research docs for the fresh start:

1. **ABSOLUTE_FOUNDATION.md** - How coordinate origins are defined (WGS84 standards)
2. **COORDINATE_SCALE_EVALUATION.md** - f64 precision analysis (nanometer at Earth scale)
3. **FOUNDATION_COORDINATE_SYSTEM.md** - Layer-by-layer coordinate foundation
4. **FOUNDATION_IMPLEMENTATION_PLAN.md** - Phase plan from research to cliff generation
5. **SRTM_DATA_ACCESS.md** - Data sources (NAS, OpenTopography, Earthdata)
6. **WORLD_DEPTH_BOUNDARIES.md** - What we simulate vs define (core to space)
7. **TERRAIN_RENDERING_COMPARISON.md** - Voxels+Smooth vs SDF analysis
8. **FOUNDATION_WORK_PLAN.md** - Work plan while awaiting SRTM download

**Status:** These are the ONLY current design docs. Everything else is from old branch.

---

## KEEP (Still Relevant Reference)

These exist from before but contain valid reference info:

- **RULES.md** - Development constraints (TDD, tests must pass, scale gates)
- **TESTING.md** - How to verify everything works
- **GLOSSARY.md** - Terminology definitions
- **HANDOVER.md** - Project context (might need updating)

**Status:** Reference only, may need updates as we progress

---

## OLD BRANCH DOCS (Archive or Review Later)

Everything else is from the old implementation attempt:

**Implementation logs:**
- ASYNC_IMPLEMENTATION_COMPLETE.md
- ASYNC_TERRAIN_SOLUTION.md
- CODE_QUALITY_FIXES.md
- CONTINUOUS_QUERIES_IMPL_LOG.md
- LOD_HYSTERESIS_COMPLETE.md

**Architecture from old code:**
- ADAPTIVE_CHUNK_SYSTEM.md
- ARCHITECTURE_VIOLATION.md
- BRIDGE_TUNNEL_SYSTEM.md
- CONTINUOUS_QUERIES_SPEC.md
- CONTINUOUS_QUERIES_VOLUMETRIC_FIX.md
- CONTINUOUS_WORLD_ARCHITECTURE.md
- MULTI_RESOLUTION_ARCHITECTURE.md
- VOLUMETRIC_ARCHITECTURE_REQUIRED.md
- WORLD_ARCHITECTURE_OPTIONS.md

**Old research:**
- ORGANIC_TERRAIN_RESEARCH.md (has voxel discussion, may reference old code)
- CHUNK_SIZE_ANALYSIS.md
- HOW_REAL_GAMES_HIDE_CHUNKS.md
- LOD_STRATEGY.md
- WHAT_I_ACTUALLY_LEARNED.md

**Old planning:**
- DATA_DEPENDENCY_TREE.md
- DATA_INVENTORY.md
- DATA_REQUIREMENTS.md
- CLIFF_GENERATION_SCHEMATIC.md (premature detail, built without foundation)
- RESEARCH_QUESTIONS.md

**Old specs (CONTAINS OLD CODE REFERENCES):**
- TECH_SPEC.md - **WARNING: References old implementation, not clean slate**
- TERRAIN_SYSTEM.md
- TODO-OLD-REFERENCE.md (already renamed)

**Status:** Archive these. Don't reference them as current truth.

---

## ACTION NEEDED

**Move old docs to archive:**
```bash
mv docs/ASYNC*.md docs/archive-old-branch/
mv docs/CONTINUOUS*.md docs/archive-old-branch/
mv docs/ARCHITECTURE*.md docs/archive-old-branch/
mv docs/ADAPTIVE*.md docs/archive-old-branch/
mv docs/BRIDGE*.md docs/archive-old-branch/
mv docs/CHUNK*.md docs/archive-old-branch/
mv docs/CODE_QUALITY*.md docs/archive-old-branch/
mv docs/DATA_*.md docs/archive-old-branch/
mv docs/HOW_REAL*.md docs/archive-old-branch/
mv docs/LOD*.md docs/archive-old-branch/
mv docs/MULTI*.md docs/archive-old-branch/
mv docs/ORGANIC*.md docs/archive-old-branch/
mv docs/PROTOTYPE*.md docs/archive-old-branch/
mv docs/RESEARCH_QUESTIONS.md docs/archive-old-branch/
mv docs/TERRAIN_SYSTEM.md docs/archive-old-branch/
mv docs/TERRAIN_REPRESENTATION_CLARIFICATION.md docs/archive-old-branch/
mv docs/VOLUMETRIC*.md docs/archive-old-branch/
mv docs/WHAT_I*.md docs/archive-old-branch/
mv docs/WORLD_ARCHITECTURE*.md docs/archive-old-branch/
mv docs/CLIFF_GENERATION*.md docs/archive-old-branch/
mv docs/IMMEDIATE*.md docs/archive-old-branch/
```

**Create NEW TECH_SPEC.md from scratch** - Based on foundation research, not old code

**Result:** Clean docs/ with only current foundation work

