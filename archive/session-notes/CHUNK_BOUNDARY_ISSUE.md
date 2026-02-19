# Chunk Boundary Artifacts - Recessed Edges

## Issue Description

User-reported: "recessed edge on all 4 sides of every chunk" visible in interactive viewer.

## Visual Evidence

See screenshots:
- `screenshot/user_test_horizontal.png` - Jagged sawtooth pattern at top edge of terrain
- `screenshot/user_test_angle.png` - Vertical cliff-like wall at chunk boundary

## Root Cause

**Marching cubes edge artifact** - When extracting mesh from SVO, marching cubes doesn't have neighbor chunk data at boundaries, so it creates faces where terrain continues into adjacent chunks.

This creates:
- Vertical walls/cliffs at chunk edges (artificial boundaries)
- Recessed stepped patterns (missing neighbor voxel data)
- Discontinuous terrain surfaces

## Why It Happens

Current implementation:
1. Each chunk has isolated 128³ SVO
2. Marching cubes operates on single SVO only
3. At X=0, X=127, Z=0, Z=127 boundaries, no neighbor data available
4. Algorithm assumes AIR beyond boundary → creates boundary face
5. Result: artificial walls/cliffs at chunk edges

## Solutions

### Option 1: Neighbor Voxel Padding (BEST)
Add 1-voxel border around each chunk SVO by querying neighbor chunks:
```rust
// When generating mesh for chunk, query neighbors
let neighbors = get_neighbor_chunks(chunk_id);
// Pad SVO with neighbor data at boundaries
// Marching cubes now has full context → seamless edges
```

**Pros**: Seamless terrain, mathematically correct
**Cons**: Requires neighbor chunks loaded, 3-5% more memory

### Option 2: Post-Process Mesh Stitching
Generate meshes independently, then stitch boundary vertices:
```rust
// After extracting all meshes
stitch_chunk_boundaries(chunk_meshes);
// Match vertices at boundaries, remove duplicate faces
```

**Pros**: Works with isolated chunks
**Cons**: Complex geometry processing, potential gaps

### Option 3: Overlap Chunks
Make chunks overlap by 1-2 voxels at boundaries:
```rust
// Chunk A: X=0..128 (includes boundary)
// Chunk B: X=127..255 (1 voxel overlap)
// Deduplicate overlapping geometry
```

**Pros**: Simple, no neighbor queries needed
**Cons**: Redundant voxel storage/processing

### Option 4: Multi-Chunk SVO (COMPLEX)
Use single large SVO spanning multiple chunks:
```rust
// 3x3 chunk region = 384³ unified SVO
// Marching cubes operates on entire region
```

**Pros**: Perfect seamless terrain
**Cons**: Memory intensive, breaks per-chunk streaming

## Recommendation

**Option 1 (Neighbor Padding)** is best for this architecture:
1. Already have chunk neighbor finding (quad-sphere math)
2. Chunk streaming supports loading neighbors
3. 1-voxel padding is minimal memory cost (~3%)
4. Produces mathematically correct seamless terrain

Implementation:
1. Load 3x3 chunk grid (camera chunk + 8 neighbors)
2. When extracting mesh for center chunk, pad SVO edges with neighbor voxel data
3. Marching cubes operates on padded SVO → seamless boundaries

## Priority

**MEDIUM** - Terrain is visible and functional, but artifacts are visually distracting.

Should fix after:
- Multi-chunk loading (required for neighbor data)
- Frustum culling (to manage 9-chunk memory cost)

Can defer until:
- OSM buildings implemented (more important for gameplay)
- Visual feedback from more testing

## Related Issues

- Multi-chunk loading currently disabled (single chunk only)
- FOV-based chunk selection not implemented
- Neighbor chunk finding needs testing at boundaries
