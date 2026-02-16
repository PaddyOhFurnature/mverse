# Chunk Size Analysis - Trade-offs and Constraints

## Current Configuration
- **Chunk depth**: 14 (quadtree depth on cube face)
- **Chunk size**: ~677m × 547m (non-square due to projection)
- **SVO depth**: 7 (128³ voxels)
- **Voxel size**: ~5.3m per voxel
- **Vertices**: 238k at LOD 0 per chunk

## Chunk Size Options

### 1m Chunks (Depth ~24)
**Pros:**
- Extremely fine-grained streaming
- Minimal sphere projection distortion
- Very fast generation per chunk (<1ms)
- Perfect for build/destroy granularity

**Cons:**
- MASSIVE chunk count (10¹⁴ chunks for Earth!)
- Network: Billions of chunk IDs to track
- Memory: Chunk metadata overhead dominates
- CPU: Chunk management overhead >> rendering
- P2P: DHT routing table explosion
- **IMPRACTICAL**

### 10m Chunks (Depth ~20)
**Pros:**
- Fine-grained streaming
- Minimal distortion
- Fast generation (~10ms per chunk)
- Good for dense urban areas

**Cons:**
- Still too many chunks (10¹² for Earth)
- Overhead still dominates
- Network chatter intense
- **TOO SMALL**

### 100m Chunks (Depth ~16)
**Pros:**
- Fast generation (~100-200ms per chunk)
- Low distortion
- Manageable chunk count (10¹⁰ for Earth)
- Good balance for urban detail
- Each chunk fits 1-2 city blocks

**Cons:**
- Still high chunk count for global scale
- More chunks in view = more draw calls
- Network: ~100-200 chunks loaded per player
- **VIABLE but high overhead**

### 400-700m Chunks (Depth 14, CURRENT)
**Pros:**
- Medium chunk count (10⁸ for Earth)
- Reasonable generation time (~30s at LOD 0)
- Good network efficiency (~9-25 chunks in view)
- Matches typical render distance (1-2km)
- Single chunk covers small neighborhood

**Cons:**
- Sphere distortion creates 128m gaps
- Large chunks = high generation time
- High vertex count (238k) per chunk
- **CURRENT CHOICE**

### 1-2km Chunks (Depth 12)
**Pros:**
- Low chunk count (10⁶ for Earth)
- Very efficient network (1-9 chunks in view)
- Minimal metadata overhead
- Good for rural/ocean areas

**Cons:**
- HUGE generation time (minutes per chunk)
- Massive vertex count (millions)
- Increased sphere distortion
- Coarse streaming granularity
- **TOO LARGE**

## Key Constraints

### 1. Sphere Projection Distortion
**Smaller chunks = less distortion**

Distortion formula for quad-sphere:
```
distortion ≈ (chunk_angle / 90°)²
```

At depth 14: chunk ≈ 0.0055° → distortion ≈ 0.0004%
At depth 16: chunk ≈ 0.0014° → distortion ≈ 0.00002%

**Smaller chunks reduce north/south gap issues!**

### 2. Generation Time

With current 128³ SVO:
- 100m chunk: ~15k vertices, ~5-10s generation
- 400m chunk: ~240k vertices, ~30s generation  
- 700m chunk: ~500k vertices, ~60s generation
- 1km chunk: ~1M vertices, ~2min generation

**Smaller chunks = faster per-chunk generation**
**But need more chunks → total time similar**

### 3. Memory Budget

Per-chunk memory:
- SVO: 128³ = 2MB (sparse storage ~200KB typical)
- Mesh vertices: 240k × 32 bytes = 7.6MB
- Indices: 240k × 4 bytes = 960KB
- Metadata: ~10KB

**Total: ~8-10MB per chunk at LOD 0**

If we load 9 chunks (3×3 grid):
- Current (depth 14): 9 × 10MB = 90MB ✓
- Depth 16 (100m): 81 × 10MB = 810MB ⚠️
- Depth 12 (1km): 4 × 40MB = 160MB ✓

**Smaller chunks = more chunks in view = more memory**

### 4. Network Efficiency

Chunk size affects:
- **Update frequency**: Smaller = more frequent chunk loads
- **Payload size**: Smaller = smaller per-chunk, but more total
- **P2P routing**: Smaller = more DHT lookups

Optimal: 10-100 chunks in render distance
- Depth 16 (100m): ~200 chunks in 2km radius ⚠️
- Depth 14 (400m): ~25 chunks in 2km radius ✓
- Depth 12 (1km): ~9 chunks in 2km radius ✓

**Current depth 14 is in sweet spot**

### 5. Draw Call Overhead

GPU rendering:
- **Batch size matters**: Want 100k+ triangles per draw call
- **More chunks = more draw calls** (unless batched)

Current: 240k tris/chunk, 9 chunks = 9 draw calls ✓
Depth 16: 15k tris/chunk, 81 chunks = 81 draw calls ⚠️

**Smaller chunks hurt rendering performance**

### 6. Build/Destroy Granularity

Player modifies world:
- Depth 16 (100m): Modify affects 1 chunk → fast ✓
- Depth 14 (400m): Modify affects 1 chunk → medium ✓
- Depth 12 (1km): Modify affects 1 chunk → slow ⚠️

**Smaller chunks better for interactivity**

## Sphere Distortion Fix

The 128m gap comes from **quad-sphere projection math**, not chunk size.

**Key insight**: Smaller chunks reduce but don't eliminate distortion.

At depth 16 (100m chunks):
- Distortion: ~0.00002% (100× better)
- North/south gap: ~1.3m (100× smaller)

At depth 18 (25m chunks):
- Distortion: ~0.000001%
- North/south gap: ~0.3m (400× smaller)

**So yes, smaller chunks help! But at cost of chunk count.**

## Recommendations

### Option A: Increase Depth to 16 (100m chunks)
**Best balance of accuracy vs performance**

Benefits:
- Reduces north/south gap to ~1.3m (acceptable!)
- Fast generation per chunk (~5-10s)
- Still manageable chunk count
- Better build/destroy granularity

Costs:
- 16× more chunks than current
- More memory (810MB for 3×3 grid)
- More draw calls (need better batching)
- More network traffic

### Option B: Keep Depth 14, Fix Math
**Fix quad-sphere neighbor calculation**

Benefits:
- No performance cost
- Keep current memory/network profile
- Surgically fix the gap issue

Costs:
- Complex math
- Need to account for sphere geometry
- May introduce subtle bugs

### Option C: Hybrid - Depth 15 (200m chunks)
**Middle ground**

Benefits:
- Reduces gap to ~6m (much better)
- Only 4× more chunks
- Reasonable memory (360MB for 3×3 grid)
- Faster generation per chunk

Costs:
- Still some gap (but < 1 voxel!)
- More chunks than current

## My Recommendation

**Option C: Depth 15 (200m chunks)**

Reasoning:
1. **Gap reduced to ~6m** → smaller than 1 voxel (5.3m)!
2. **4× more chunks is manageable** (was 9, now 36 in view)
3. **Generation time cut to ~1/4** per chunk (~7-8s each)
4. **Memory acceptable** (~360MB for loaded chunks)
5. **Distortion minimal** (0.0001%)

With depth 15:
- Each chunk: ~200m × 164m
- 36 chunks in 2km view distance
- Per-chunk generation: ~7-8s
- Total memory: ~360MB
- Gap: ~6m (within 1 voxel tolerance!)

### Implementation Plan

1. Change chunk depth from 14 → 15
2. Test gap size (should be ~6m, acceptable)
3. Adjust SVO depth to maintain voxel size:
   - 200m chunk / 128 voxels = 1.56m/voxel (good!)
   - Or use depth 8 (256³) = 0.78m/voxel (better detail!)
4. Optimize multi-chunk rendering (batching)
5. Test performance with 36 chunks loaded

## Constraints Summary

| Metric | Depth 12 | Depth 14 | Depth 15 | Depth 16 | Depth 18 |
|--------|----------|----------|----------|----------|----------|
| Chunk size | ~1.6km | ~700m | ~200m | ~100m | ~25m |
| Distortion gap | ~512m | ~128m | ~6m | ~1.3m | ~0.3m |
| Chunks in 2km | 9 | 25 | 36 | 100 | 400 |
| Memory (3×3) | 160MB | 90MB | 360MB | 810MB | 3.2GB |
| Gen time/chunk | 2min | 30s | 8s | 5s | 1s |
| Total Earth chunks | 250k | 4M | 16M | 64M | 1B |

**Sweet spot: Depth 15**
- Gap small enough to ignore (< 1 voxel)
- Memory reasonable
- Generation fast
- Chunk count manageable
