# Continuous Query System - Technical Specification

**Version:** 1.0  
**Date:** 2026-02-16  
**Status:** Design Phase  
**Branch:** `feature/continuous-queries-prototype`

---

## 1. Overview

### 1.1 Purpose

Design and implement a continuous query system for voxel-based world representation that eliminates chunk boundaries entirely from the game logic, rendering, and generation APIs. Chunks (if used) become pure storage/networking optimization, invisible to all other systems.

### 1.2 Goals

**Primary Goals:**
1. **Perfect alignment** - No visible seams or boundaries anywhere in the world
2. **Context-aware querying** - Query exactly what's needed (single room vs entire hillside)
3. **Interior space efficiency** - Load 5m³ room without loading whole building (user priority #1)
4. **Arbitrary bounds** - Query any AABB, not constrained to chunk grid
5. **Scalable prototype** - Validate architecture on 200m test area before global commitment

**Success Criteria:**
- Zero visible boundaries in test area
- Interior queries more efficient than chunk system
- 60 FPS rendering performance
- Memory usage reasonable (<2GB for 200m test area)
- Clear path to global scale (40,075km Earth)

### 1.3 Scope

**Prototype Phase (7 weeks):**
- Test area: Kangaroo Point, Brisbane (-27.479769°, 153.033586°)
- Bounds: 200m × 200m × 100m deep
- Features: Houses, cliff face, river, roads, carpark
- Real data: SRTM elevation + OSM buildings
- Compare against current chunk system

**NOT in scope for prototype:**
- P2P networking/replication
- Global scale implementation
- Destructible world modifications (Phase 4 only)
- Multi-player synchronization

---

## 2. Architecture

### 2.1 Core Components

```
┌────────────────────────────────────────────────┐
│         Continuous World API (Public)          │
│  query_range(), query_frustum(), sample_point()│
└────────────────────────────────────────────────┘
                      ▼
┌────────────────────────────────────────────────┐
│            Spatial Index (R-tree)              │
│      Fast lookup of voxels by position         │
└────────────────────────────────────────────────┘
                      ▼
┌────────────────────────────────────────────────┐
│        Adaptive Cache (Hot/Warm/Cold)          │
│   Memory → Disk → Generate on miss             │
└────────────────────────────────────────────────┘
                      ▼
┌────────────────────────────────────────────────┐
│      Procedural Generator (On-Demand)          │
│    SRTM + OSM → Voxels (arbitrary bounds)      │
└────────────────────────────────────────────────┘
```

### 2.2 Data Flow

**Read Path (Query):**
1. User requests voxels in AABB: `world.query_range(bounds)`
2. Check cache (hot → warm → cold)
3. If miss: Generate from source data (SRTM + OSM)
4. Insert into cache with LRU eviction
5. Return voxels to caller

**Write Path (Modification - Phase 4):**
1. User modifies voxel: `world.set_voxel(ecef, material)`
2. Insert into modification index (separate R-tree)
3. Invalidate affected cache entries
4. Serialize modification to disk
5. Future queries merge base + modifications

### 2.3 Coordinate System

**Global Frame:** ECEF (Earth-Centered, Earth-Fixed)
- All voxels positioned in ECEF coordinates
- Consistent with existing coordinate system
- No chunk-local coordinates

**Test Area Bounds (ECEF):**
- Center: (-5,047,081.96, 2,567,891.19, -2,925,600.68)
- Extent: ±100m in each axis
- Total volume: 200m × 200m × 100m = 4,000,000 m³

---

## 3. Spatial Index Design

### 3.1 R-tree Selection

**Chosen: `rstar` crate (R*-tree implementation)**

Why R-tree:
- **Range queries** - Primary operation: "give me all voxels in this AABB"
- **3D support** - Native 3D bounding box queries
- **Balanced** - R*-tree variant self-balances for query performance
- **Production-ready** - `rstar` crate is mature (v0.12.2), widely used
- **Insert/delete** - Supports dynamic modifications (Phase 4)

Alternatives considered:
- **Octree** - Good for hierarchical LOD, but sphere/cube mismatch, power-of-2 constraint
- **KD-tree** - Fast point queries, slower range queries, harder to balance
- **Grid/hash** - Simple but poor for arbitrary range queries

### 3.2 R-tree Configuration

```rust
use rstar::{RTree, AABB as RTreeAABB};

/// Voxel point in R-tree
#[derive(Debug, Clone)]
struct VoxelPoint {
    /// Position in ECEF coordinates
    ecef: [f64; 3],
    /// Material at this position
    material: MaterialId,
    /// Optional metadata (for buildings, roads, etc.)
    metadata: Option<VoxelMetadata>,
}

impl rstar::RTreeObject for VoxelPoint {
    type Envelope = RTreeAABB<[f64; 3]>;
    
    fn envelope(&self) -> Self::Envelope {
        RTreeAABB::from_point(self.ecef)
    }
}

/// Spatial index for voxels
struct SpatialIndex {
    tree: RTree<VoxelPoint>,
    bounds: AABB,  // Test area bounds
}
```

### 3.3 Storage Granularity

**Problem:** Individual voxels in R-tree = massive overhead

At 1m³ voxels:
- Test area: 200 × 200 × 100 = 4,000,000 voxels
- Each R-tree entry: ~64 bytes (position + material + metadata + tree overhead)
- Total: ~256 MB just for index (before any actual data)

**Solution: Block-based storage**

Store voxels in small blocks (8³ = 512 voxels per block):
- Test area: 4M voxels ÷ 512 = 7,812 blocks
- Each block: 8m × 8m × 8m volume
- R-tree entries: 7,812 (reasonable)
- Memory: ~500 KB for index (acceptable)

```rust
/// Block of voxels (8³ = 512 voxels)
#[derive(Debug, Clone)]
struct VoxelBlock {
    /// Block position (minimum corner in ECEF)
    ecef_min: [f64; 3],
    /// Block size (8m in each axis)
    size: f64,
    /// Voxel data (8×8×8 = 512 materials)
    voxels: Box<[MaterialId; 512]>,
}

impl rstar::RTreeObject for VoxelBlock {
    type Envelope = RTreeAABB<[f64; 3]>;
    
    fn envelope(&self) -> Self::Envelope {
        let min = self.ecef_min;
        let max = [
            min[0] + self.size,
            min[1] + self.size,
            min[2] + self.size,
        ];
        RTreeAABB::from_corners(min, max)
    }
}
```

**Trade-offs:**
- ✅ Reduces R-tree size by 512×
- ✅ Cache-friendly (load 512 voxels at once)
- ✅ Reasonable memory overhead
- ⚠️ Granularity: must load entire 8m block even if only querying 1 voxel
- ⚠️ 8m chosen as balance (smaller = more overhead, larger = less granular)

### 3.4 Query Operations

```rust
impl SpatialIndex {
    /// Query all blocks intersecting AABB
    pub fn query_range(&self, bounds: AABB) -> Vec<VoxelBlock> {
        let rtree_bounds = self.aabb_to_rtree(bounds);
        self.tree
            .locate_in_envelope_intersecting(&rtree_bounds)
            .cloned()
            .collect()
    }
    
    /// Query blocks visible in camera frustum
    pub fn query_frustum(&self, frustum: &Frustum) -> Vec<VoxelBlock> {
        // Approximate frustum with bounding AABB for now
        // TODO Phase 3: True frustum culling
        let bounds = frustum.bounding_aabb();
        self.query_range(bounds)
    }
    
    /// Sample single point (nearest neighbor)
    pub fn sample_point(&self, ecef: [f64; 3]) -> Option<MaterialId> {
        self.tree
            .nearest_neighbor(&ecef)
            .and_then(|block| block.sample_voxel(ecef))
    }
    
    /// Insert block into index
    pub fn insert(&mut self, block: VoxelBlock) {
        self.tree.insert(block);
    }
    
    /// Remove block from index
    pub fn remove(&mut self, block: VoxelBlock) {
        self.tree.remove(&block);
    }
}
```

---

## 4. Procedural Generation

### 4.1 Decoupled from Chunks

**Current system (chunk-based):**
```rust
// Generation tied to chunk boundaries
fn generate_chunk_svo(chunk_id: ChunkId) -> SVO {
    let bounds = chunk_bounds_gps(chunk_id);  // Fixed chunk size
    let elevation = query_srtm(bounds);
    voxelize(elevation)
}
```

**New system (continuous):**
```rust
// Generation for arbitrary bounds
fn generate_voxels(bounds: AABB) -> Vec<VoxelBlock> {
    let elevation = query_srtm(bounds);
    let buildings = query_osm(bounds);
    voxelize_continuous(elevation, buildings, bounds)
}
```

**Key difference:** Bounds are arbitrary, not aligned to any grid.

### 4.2 Source Data Queries

**SRTM Elevation:**
```rust
/// Query elevation data for arbitrary bounds
fn query_srtm(bounds: AABB) -> ElevationData {
    // Expand bounds slightly to ensure coverage
    let expanded = bounds.expand(10.0);  // 10m buffer
    
    // Find all SRTM tiles intersecting bounds
    let tiles = srtm_tiles_for_bounds(expanded);
    
    // Load and merge tiles
    let merged = merge_srtm_tiles(tiles);
    
    // Sample at voxel resolution (1m)
    sample_elevation_grid(merged, bounds, 1.0)
}
```

**OSM Buildings/Roads:**
```rust
/// Query OSM features for arbitrary bounds
fn query_osm(bounds: AABB) -> OSMFeatures {
    // Convert ECEF bounds to GPS
    let gps_bounds = ecef_aabb_to_gps(bounds);
    
    // Query Overpass API (or local cache)
    let features = overpass_query(gps_bounds, vec![
        "building",
        "highway",
        "natural=water",
        "amenity=parking",
    ]);
    
    // Convert to ECEF for voxelization
    features_to_ecef(features)
}
```

### 4.3 Voxelization Strategy

```rust
/// Generate voxel blocks for arbitrary bounds
fn voxelize_continuous(
    elevation: ElevationData,
    osm: OSMFeatures,
    bounds: AABB,
) -> Vec<VoxelBlock> {
    // Divide bounds into 8m blocks
    let blocks = subdivide_into_blocks(bounds, 8.0);
    
    blocks.par_iter().map(|block_bounds| {
        // Generate voxels for this block
        let mut voxels = [AIR; 512];
        
        // Sample elevation for terrain
        fill_terrain(&mut voxels, &elevation, block_bounds);
        
        // Add OSM features (buildings, roads)
        fill_buildings(&mut voxels, &osm, block_bounds);
        fill_roads(&mut voxels, &osm, block_bounds);
        
        VoxelBlock {
            ecef_min: block_bounds.min,
            size: 8.0,
            voxels: Box::new(voxels),
        }
    }).collect()
}
```

**Parallelization:** Use `rayon` for parallel block generation (already in Cargo.toml).

### 4.4 Boundary Continuity

**Critical:** Adjacent blocks must align perfectly.

**Problem:** Floating-point rounding could cause gaps.

**Solution:** Deterministic block positioning
```rust
/// Snap ECEF coordinate to block grid
fn snap_to_block_grid(ecef: f64, block_size: f64) -> f64 {
    (ecef / block_size).floor() * block_size
}

/// Get block containing point
fn block_containing_point(ecef: [f64; 3], block_size: f64) -> [f64; 3] {
    [
        snap_to_block_grid(ecef[0], block_size),
        snap_to_block_grid(ecef[1], block_size),
        snap_to_block_grid(ecef[2], block_size),
    ]
}
```

This ensures:
- Blocks always align on 8m grid
- No floating-point gaps between blocks
- Reproducible positioning

---

## 5. Caching Strategy

### 5.1 Three-Tier Cache

```rust
struct AdaptiveCache {
    /// Hot cache - most recent queries (in-memory, fast)
    hot: HashMap<BlockKey, VoxelBlock>,
    hot_capacity: usize,  // e.g., 1000 blocks = 256MB
    
    /// Warm cache - frequently accessed (in-memory, LRU)
    warm: LruCache<BlockKey, VoxelBlock>,
    warm_capacity: usize,  // e.g., 5000 blocks = 1.3GB
    
    /// Cold cache - long-term storage (disk)
    cold: DiskCache,
    cold_path: PathBuf,  // e.g., ~/.metaverse/cache/blocks/
}

#[derive(Hash, Eq, PartialEq)]
struct BlockKey {
    /// Block position (snapped to grid)
    ecef_min: [i64; 3],  // Integer (mm precision)
}
```

### 5.2 Cache Access Pattern

```rust
impl AdaptiveCache {
    fn get_or_generate(&mut self, key: BlockKey) -> VoxelBlock {
        // 1. Check hot cache (most recent)
        if let Some(block) = self.hot.get(&key) {
            return block.clone();
        }
        
        // 2. Check warm cache (LRU)
        if let Some(block) = self.warm.get(&key) {
            // Promote to hot
            self.hot.insert(key, block.clone());
            self.maybe_evict_hot();
            return block.clone();
        }
        
        // 3. Check cold cache (disk)
        if let Some(block) = self.cold.load(&key) {
            // Promote to warm
            self.warm.put(key, block.clone());
            return block;
        }
        
        // 4. Cache miss - generate from source data
        let bounds = block_key_to_aabb(key);
        let block = generate_block(bounds);
        
        // Insert into caches
        self.cold.save(&key, &block);
        self.warm.put(key, block.clone());
        
        block
    }
    
    fn maybe_evict_hot(&mut self) {
        if self.hot.len() > self.hot_capacity {
            // Evict least recently used from hot to warm
            // (HashMap doesn't track LRU, so evict random)
            if let Some((key, block)) = self.hot.iter().next() {
                let key = key.clone();
                let block = block.clone();
                self.hot.remove(&key);
                self.warm.put(key, block);
            }
        }
    }
}
```

### 5.3 Cache Sizing (Test Area)

**Test area:** 200m × 200m × 100m = 4M m³

At 8m block size:
- Blocks: (200÷8) × (200÷8) × (100÷8) = 25 × 25 × 13 = 8,125 blocks
- Block size: 512 voxels × 2 bytes (MaterialId) = 1 KB per block
- Total if all loaded: 8,125 KB ≈ 8 MB (very reasonable!)

**Cache configuration for test area:**
- Hot: 1,000 blocks = 1 MB (recent queries)
- Warm: 5,000 blocks = 5 MB (frequently accessed)
- Cold: Unlimited (disk, compressed)

**This means entire test area fits in warm cache!** Memory is not limiting factor for prototype.

### 5.4 Disk Cache Format

```rust
struct DiskCache {
    base_path: PathBuf,
}

impl DiskCache {
    fn save(&self, key: &BlockKey, block: &VoxelBlock) {
        let path = self.block_path(key);
        
        // Compress with bincode + zstd
        let serialized = bincode::serialize(block).unwrap();
        let compressed = zstd::compress(&serialized);
        
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, compressed).unwrap();
    }
    
    fn load(&self, key: &BlockKey) -> Option<VoxelBlock> {
        let path = self.block_path(key);
        
        if !path.exists() {
            return None;
        }
        
        let compressed = std::fs::read(path).ok()?;
        let serialized = zstd::decompress(&compressed).ok()?;
        bincode::deserialize(&serialized).ok()
    }
    
    fn block_path(&self, key: &BlockKey) -> PathBuf {
        // Hierarchical directory structure to avoid single dir with millions of files
        // e.g., ~/.metaverse/cache/blocks/-5047/2567/-2925/block.bin
        self.base_path
            .join(format!("{}", key.ecef_min[0] / 1000))
            .join(format!("{}", key.ecef_min[1] / 1000))
            .join(format!("{}", key.ecef_min[2] / 1000))
            .join("block.bin")
    }
}
```

---

## 6. Public API Design

### 6.1 Core Interface

```rust
pub struct ContinuousWorld {
    index: SpatialIndex,
    cache: AdaptiveCache,
    generator: ProceduralGenerator,
    bounds: AABB,  // Test area bounds (for prototype)
}

impl ContinuousWorld {
    /// Create new continuous world for test area
    pub fn new(center_ecef: [f64; 3], extent: f64) -> Self {
        let bounds = AABB {
            min: [
                center_ecef[0] - extent,
                center_ecef[1] - extent,
                center_ecef[2] - extent,
            ],
            max: [
                center_ecef[0] + extent,
                center_ecef[1] + extent,
                center_ecef[2] + extent,
            ],
        };
        
        Self {
            index: SpatialIndex::new(bounds),
            cache: AdaptiveCache::new(),
            generator: ProceduralGenerator::new(),
            bounds,
        }
    }
    
    /// Query voxels in arbitrary AABB
    pub fn query_range(&mut self, bounds: AABB) -> Vec<VoxelBlock> {
        // Find all block keys intersecting bounds
        let keys = self.block_keys_in_bounds(bounds);
        
        // Get or generate each block
        keys.into_iter()
            .map(|key| self.cache.get_or_generate(key))
            .collect()
    }
    
    /// Query voxels visible in camera frustum
    pub fn query_frustum(&mut self, frustum: &Frustum) -> Vec<VoxelBlock> {
        let bounds = frustum.bounding_aabb();
        self.query_range(bounds)
    }
    
    /// Sample material at single point
    pub fn sample_point(&mut self, ecef: [f64; 3]) -> MaterialId {
        let key = BlockKey::from_ecef(ecef);
        let block = self.cache.get_or_generate(key);
        block.sample_voxel(ecef).unwrap_or(AIR)
    }
    
    /// Raycast from origin in direction
    pub fn raycast(&mut self, origin: [f64; 3], direction: [f64; 3], max_dist: f64) -> Option<Hit> {
        // DDA raycasting through blocks
        // TODO: Optimize with R-tree traversal
        unimplemented!()
    }
}
```

### 6.2 Usage Examples

**Example 1: Render camera view**
```rust
let camera_pos = [-5047081.96, 2567891.19, -2925600.68];
let camera_dir = [0.0, 0.0, -1.0];  // Looking down
let frustum = Frustum::from_camera(camera_pos, camera_dir, fov, aspect);

// Query visible voxels
let blocks = world.query_frustum(&frustum);

// Extract mesh for rendering
let mesh = marching_cubes(&blocks);
render(mesh);
```

**Example 2: Query building interior (5m room)**
```rust
let room_center = [-5047081.0, 2567891.0, -2925595.0];
let room_size = 5.0;  // 5m × 5m × 5m room

let room_bounds = AABB {
    min: [room_center[0] - 2.5, room_center[1] - 2.5, room_center[2] - 2.5],
    max: [room_center[0] + 2.5, room_center[1] + 2.5, room_center[2] + 2.5],
};

// Only loads blocks intersecting this room (~1-2 blocks)
let room_voxels = world.query_range(room_bounds);
```

**Example 3: Sample ground elevation**
```rust
let surface_pos = [-5047081.96, 2567891.19, -2925500.0];  // High altitude
let mut z = surface_pos[2];

// Walk down until hit non-air
loop {
    let material = world.sample_point([surface_pos[0], surface_pos[1], z]);
    if material != AIR {
        println!("Ground at elevation: {}", z);
        break;
    }
    z -= 1.0;
}
```

---

## 7. Integration with Existing Code

### 7.1 Viewer Integration

**Current:** `examples/viewer.rs` uses `WorldManager` (chunk-based)

**New:** Swap in `ContinuousWorld`

```rust
// OLD (viewer.rs, lines 298-302):
let world_manager = Arc::new(Mutex::new(
    WorldManager::new(14, 1500.0, 7)
));

// NEW:
let world = Arc::new(Mutex::new(
    ContinuousWorld::new(
        camera_ecef,  // Center on camera
        100.0,        // 100m extent for test area
    )
));
```

**Rendering loop:** Same marching cubes, just different query API
```rust
// OLD:
let chunks = world_manager.lock().unwrap().find_chunks_in_range(&camera_ecef);
for chunk in chunks {
    let mesh = extract_mesh(chunk.svo());
    render(mesh);
}

// NEW:
let frustum = camera.frustum();
let blocks = world.lock().unwrap().query_frustum(&frustum);
let mesh = marching_cubes(&blocks);
render(mesh);
```

### 7.2 Screenshot Tool Integration

**Current:** `examples/screenshot_capture.rs` generates chunk-based screenshots

**New:** Same screenshot positions, continuous queries
```rust
// Reference position (Brisbane test point)
let camera_ecef = [-5047081.96, 2567891.19, -2925600.68];

// Query visible region (no chunks)
let visible_bounds = calculate_visible_bounds(camera_ecef, camera_dir);
let blocks = world.query_range(visible_bounds);

// Same mesh extraction
let mesh = marching_cubes(&blocks);
render_to_png(mesh, "test_continuous.png");
```

### 7.3 Coordinate System Compatibility

**No changes needed** - already using ECEF globally.

Existing transforms still work:
- GPS ↔ ECEF (`coordinates.rs`)
- ECEF ↔ ENU (`coordinates.rs`)
- ENU ↔ Floating origin (`renderer/camera.rs`)

---

## 8. Performance Considerations

### 8.1 Query Performance

**Target:** <16ms per frame @ 60 FPS

**Breakdown:**
- R-tree query: <1ms (proven fast for thousands of objects)
- Cache lookup: <1ms (hash table O(1))
- Decompression (if cold): <5ms (zstd is fast)
- Generation (if miss): <100ms (acceptable for occasional misses)

**Expected cache hit rate:** >95% during normal movement
- Camera moves slowly relative to test area
- Most blocks remain visible across frames
- Hot cache holds recent frame's blocks

### 8.2 Memory Estimates

**Test area (200m):**
- Total blocks: 8,125 (if fully generated)
- Block size: 1 KB
- Total: 8 MB

**Cache:** 
- Hot + Warm: 6 MB (1000 + 5000 blocks)
- R-tree index: ~0.5 MB
- **Total: ~7 MB** (entire test area fits in RAM!)

### 8.3 Generation Performance

**Bottleneck:** SRTM/OSM queries

**Optimization:** Pre-cache source data
```rust
struct ProceduralGenerator {
    srtm_cache: SRTMCache,  // Pre-load test area tiles
    osm_cache: OSMCache,    // Pre-load test area features
}

impl ProceduralGenerator {
    fn new() -> Self {
        // Load entire test area source data on startup
        let srtm_cache = SRTMCache::load_tiles(test_area_bounds);
        let osm_cache = OSMCache::load_features(test_area_bounds);
        
        Self { srtm_cache, osm_cache }
    }
}
```

With pre-cached source data:
- Block generation: ~1ms (just voxelization, no network)
- Can generate 1000 blocks/second
- Test area (8125 blocks): ~8 seconds to fully generate

### 8.4 Comparison vs Chunks

**Chunk system (current):**
- Load granularity: 547m × 677m chunk (370,000 m³)
- Memory per chunk: ~50 MB (SVO depth 7)
- Loading: All-or-nothing (entire chunk)
- Interior query: Must load entire chunk containing building

**Continuous system (proposed):**
- Load granularity: 8m × 8m × 8m block (512 m³)
- Memory per block: 1 KB
- Loading: Exactly what's visible
- Interior query: Only blocks in room (~2 KB)

**Memory efficiency for interior:** 50 MB → 2 KB = **25,000× improvement**

---

## 9. Testing Strategy

### 9.1 Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_block_alignment() {
        // Verify adjacent blocks have no gaps
        let block1 = VoxelBlock { ecef_min: [0.0, 0.0, 0.0], size: 8.0, .. };
        let block2 = VoxelBlock { ecef_min: [8.0, 0.0, 0.0], size: 8.0, .. };
        
        // Block 1 max == Block 2 min (no gap)
        assert_eq!(block1.ecef_min[0] + block1.size, block2.ecef_min[0]);
    }
    
    #[test]
    fn test_query_single_block() {
        let mut world = ContinuousWorld::new([0.0, 0.0, 0.0], 100.0);
        
        // Query small region (should return 1 block)
        let bounds = AABB { min: [0.0, 0.0, 0.0], max: [5.0, 5.0, 5.0] };
        let blocks = world.query_range(bounds);
        
        assert_eq!(blocks.len(), 1);
    }
    
    #[test]
    fn test_query_spanning_blocks() {
        let mut world = ContinuousWorld::new([0.0, 0.0, 0.0], 100.0);
        
        // Query across block boundary (should return 2 blocks)
        let bounds = AABB { min: [0.0, 0.0, 0.0], max: [12.0, 5.0, 5.0] };
        let blocks = world.query_range(bounds);
        
        assert!(blocks.len() >= 2);  // At least 2 blocks
    }
    
    #[test]
    fn test_cache_hit() {
        let mut world = ContinuousWorld::new([0.0, 0.0, 0.0], 100.0);
        let bounds = AABB { min: [0.0, 0.0, 0.0], max: [5.0, 5.0, 5.0] };
        
        // First query (cache miss, generates)
        let blocks1 = world.query_range(bounds);
        
        // Second query (cache hit)
        let blocks2 = world.query_range(bounds);
        
        // Should return same blocks
        assert_eq!(blocks1.len(), blocks2.len());
    }
}
```

### 9.2 Integration Tests

```rust
#[test]
fn test_real_location_generation() {
    // Test Kangaroo Point location
    let center_ecef = [-5047081.96, 2567891.19, -2925600.68];
    let mut world = ContinuousWorld::new(center_ecef, 100.0);
    
    // Query terrain at known elevation
    let ground_pos = [center_ecef[0], center_ecef[1], -2925600.0];
    let material = world.sample_point(ground_pos);
    
    // Should be terrain (not air)
    assert_ne!(material, AIR);
}

#[test]
fn test_interior_query_efficiency() {
    let center_ecef = [-5047081.96, 2567891.19, -2925600.68];
    let mut world = ContinuousWorld::new(center_ecef, 100.0);
    
    // Query small room (5m³)
    let room_bounds = AABB {
        min: [center_ecef[0] - 2.5, center_ecef[1] - 2.5, center_ecef[2] - 2.5],
        max: [center_ecef[0] + 2.5, center_ecef[1] + 2.5, center_ecef[2] + 2.5],
    };
    
    let blocks = world.query_range(room_bounds);
    
    // Should return very few blocks (~1-2)
    assert!(blocks.len() <= 2, "Interior query loaded {} blocks (expected ≤2)", blocks.len());
    
    // Total memory should be tiny
    let memory = blocks.len() * 1024;  // 1 KB per block
    assert!(memory < 5000, "Interior query used {} bytes (expected <5KB)", memory);
}
```

### 9.3 Benchmark Tests

```rust
#[bench]
fn bench_query_performance(b: &mut Bencher) {
    let center_ecef = [-5047081.96, 2567891.19, -2925600.68];
    let mut world = ContinuousWorld::new(center_ecef, 100.0);
    
    let bounds = AABB {
        min: [center_ecef[0] - 50.0, center_ecef[1] - 50.0, center_ecef[2] - 50.0],
        max: [center_ecef[0] + 50.0, center_ecef[1] + 50.0, center_ecef[2] + 50.0],
    };
    
    b.iter(|| {
        world.query_range(bounds)
    });
}

#[bench]
fn bench_cache_hit_rate(b: &mut Bencher) {
    let center_ecef = [-5047081.96, 2567891.19, -2925600.68];
    let mut world = ContinuousWorld::new(center_ecef, 100.0);
    
    // Simulate camera movement
    let positions = generate_camera_path(center_ecef, 100);
    
    b.iter(|| {
        for pos in &positions {
            let frustum = Frustum::from_camera(*pos, [0.0, 0.0, -1.0], 90.0, 16.0/9.0);
            world.query_frustum(&frustum);
        }
    });
}
```

---

## 10. Comparison Methodology

### 10.1 Metrics to Collect

**Quantitative:**
1. **Boundary quality** - Visual inspection + pixel difference
2. **Memory usage** - Total RAM for visible area
3. **Query performance** - Time per query (avg, p50, p95, p99)
4. **Cache hit rate** - % of queries served from cache
5. **Generation cost** - Time to generate new block/chunk
6. **Interior efficiency** - Memory to query single room

**Qualitative:**
1. **Code complexity** - Lines of code, cyclomatic complexity
2. **API ergonomics** - Ease of use
3. **Debuggability** - How easy to understand what's happening
4. **Maintainability** - How easy to modify/extend

### 10.2 Test Scenarios

**Scenario 1: Ground-level walk (slow movement)**
- Camera: 2m above ground
- Speed: 2 m/s (walking)
- FOV: 90°, view distance: 50m
- Duration: 60 seconds
- Measure: Cache hit rate, memory, FPS

**Scenario 2: Fast vehicle (moderate movement)**
- Camera: 2m above road
- Speed: 20 m/s (72 km/h)
- FOV: 90°, view distance: 200m
- Duration: 30 seconds
- Measure: Cache hit rate, memory, FPS

**Scenario 3: Interior exploration (fine detail)**
- Camera: Inside building
- Movement: Room to room
- FOV: 90°, view distance: 20m
- Duration: 60 seconds
- Measure: Memory per room, boundary quality, FPS

**Scenario 4: Aerial view (large area)**
- Camera: 100m altitude
- Speed: 10 m/s
- FOV: 90°, view distance: 500m
- Duration: 30 seconds
- Measure: Memory, generation cost, FPS

### 10.3 Comparison Implementation

```rust
// Implement both systems for test area
struct ComparisonTest {
    continuous: ContinuousWorld,
    chunks: WorldManager,
    metrics: Metrics,
}

impl ComparisonTest {
    fn run_scenario(&mut self, scenario: Scenario) {
        // Run with continuous system
        let continuous_metrics = self.run_with_continuous(&scenario);
        
        // Run with chunk system
        let chunk_metrics = self.run_with_chunks(&scenario);
        
        // Compare
        self.metrics.record(continuous_metrics, chunk_metrics);
    }
}
```

---

## 11. Phase 1 Implementation Plan

### Week 1: Core Infrastructure

**Day 1-2: Spatial index**
- Add `rstar` crate to Cargo.toml
- Implement `VoxelBlock` with R-tree traits
- Implement `SpatialIndex` wrapper
- Unit tests for insertion/query

**Day 3-4: Caching**
- Implement `AdaptiveCache` (hot/warm/cold)
- Implement `DiskCache` with compression
- Unit tests for cache behavior
- Benchmark cache hit rates

**Day 5-7: Public API**
- Implement `ContinuousWorld` struct
- Implement `query_range()`, `query_frustum()`, `sample_point()`
- Integration tests with test location
- Documentation

### Week 2: Procedural Generation

**Day 8-9: Source data caching**
- Pre-load SRTM tiles for test area
- Pre-load OSM features for test area
- Implement `ProceduralGenerator` struct

**Day 10-12: Voxelization**
- Implement `voxelize_continuous()` for arbitrary bounds
- Ensure block alignment (no gaps)
- Add terrain generation (elevation)
- Add building generation (OSM)

**Day 13-14: Validation**
- Generate entire test area
- Visual inspection vs screenshot
- Verify boundary continuity
- Performance profiling

**Deliverable:** Working continuous query system for 200m test area with real data.

---

## 12. Risk Analysis

### 12.1 Technical Risks

**Risk 1: R-tree performance insufficient**
- **Probability:** Low (R-trees are proven for spatial queries)
- **Impact:** High (core architecture depends on it)
- **Mitigation:** Benchmark early (Day 1-2), have fallback to simpler grid

**Risk 2: Cache hit rate too low**
- **Probability:** Medium (depends on access patterns)
- **Impact:** High (generation cost per miss)
- **Mitigation:** Pre-generate test area, optimize cache eviction policy

**Risk 3: Memory usage too high**
- **Probability:** Low (test area is small)
- **Impact:** Medium (could limit scalability)
- **Mitigation:** Profile early, reduce block size if needed

**Risk 4: Floating-point precision causes gaps**
- **Probability:** Medium (coordinate transforms accumulate error)
- **Impact:** Critical (defeats purpose of continuous system)
- **Mitigation:** Use deterministic snapping, extensive boundary tests

### 12.2 Scaling Risks

**Risk 5: Doesn't scale to global (200m → 40,000km)**
- **Probability:** High (200,000× scale factor)
- **Impact:** High (invalidates architecture)
- **Mitigation:** This is expected - prototype is to learn if it's viable

**Risk 6: P2P distribution hard**
- **Probability:** Medium (blocks need indexing for DHT)
- **Impact:** High (conflicts with P2P goal)
- **Mitigation:** Defer to Phase 6, may need hybrid approach

### 12.3 Schedule Risks

**Risk 7: Generation complexity underestimated**
- **Probability:** Medium (OSM voxelization is complex)
- **Impact:** Medium (delays Week 2)
- **Mitigation:** Start simple (terrain only), add buildings incrementally

**Risk 8: Debugging takes longer than expected**
- **Probability:** High (new architecture, many unknowns)
- **Impact:** Medium (extends timeline)
- **Mitigation:** Time-boxed investigation (max 3 days per issue)

---

## 13. Success Criteria

### Phase 1 Success (End of Week 2)

**Must Have:**
- ✅ Can query arbitrary AABB in test area
- ✅ Returns correct voxels from real data (SRTM + OSM)
- ✅ No visible boundaries (verified visually)
- ✅ Cache hit rate >90% for typical movement
- ✅ Performance: 60 FPS rendering
- ✅ Memory: <2GB for test area

**Nice to Have:**
- Interior spaces more efficient than chunks (Phase 3 focus)
- Frustum culling optimized (Phase 2 focus)
- Full OSM feature support (buildings + roads + water)

### Prototype Success (End of Week 7)

**Decision Criteria:**
1. **Boundary quality:** Perfect (zero visible seams) ✓ or ✗
2. **Interior efficiency:** Better than chunks ✓ or ✗
3. **Performance:** 60 FPS sustained ✓ or ✗
4. **Memory:** Reasonable (<2GB test area) ✓ or ✗
5. **Complexity:** Manageable code ✓ or ✗
6. **Scalability:** Clear path to global ✓ or ✗

**Decision Matrix:**
- **6/6 ✓** → Commit to continuous queries globally
- **4-5/6 ✓** → Hybrid approach (continuous for interiors, chunks for terrain)
- **≤3/6 ✓** → Return to hidden chunks approach

---

## 14. Dependencies

### 14.1 New Crates

```toml
[dependencies]
# Spatial indexing
rstar = "0.12"  # R*-tree for spatial queries

# LRU cache
lru = "0.12"  # LRU cache implementation

# Compression (already have zstd via deps)
# (no changes needed)
```

### 14.2 Existing Code Reuse

**Reuse (no changes):**
- `src/coordinates.rs` - GPS ↔ ECEF transforms
- `src/srtm.rs` - SRTM elevation loading
- `src/osm.rs` - OSM feature loading
- `src/svo.rs` - SVO voxel storage (for block internal storage)
- `src/renderer/` - Rendering pipeline (just swap query API)

**Modify:**
- `examples/viewer.rs` - Swap `WorldManager` → `ContinuousWorld`
- `examples/screenshot_capture.rs` - Add continuous query mode

**New files:**
- `src/continuous_world.rs` - Core continuous query implementation
- `src/spatial_index.rs` - R-tree wrapper
- `src/adaptive_cache.rs` - Hot/warm/cold caching
- `tests/continuous_tests.rs` - Integration tests

---

## 15. Documentation Requirements

### Per Phase:

1. **Technical Spec** - This document (done)
2. **Implementation Notes** - Daily log of decisions, problems, solutions
3. **API Documentation** - Rustdoc for all public functions
4. **Test Results** - Metrics, benchmarks, screenshots
5. **Lessons Learned** - What worked, what didn't, why

### Deliverables:

- `docs/CONTINUOUS_QUERIES_SPEC.md` - This document
- `docs/CONTINUOUS_QUERIES_IMPL_LOG.md` - Daily implementation notes
- `docs/CONTINUOUS_QUERIES_RESULTS.md` - Final comparison and decision
- Inline rustdoc for all code

---

## 16. Open Questions

1. **Block size:** 8m chosen as balance - too large/small?
2. **R-tree parameters:** Default branching factor ok or tune?
3. **Cache eviction:** LRU sufficient or need smarter policy?
4. **Compression:** Worth CPU cost for disk cache?
5. **Parallelization:** Generate blocks in parallel or sequential?
6. **Frustum culling:** Bounding AABB sufficient or need true frustum?
7. **OSM detail:** How much OSM data to voxelize (roads, trees, signs)?
8. **Vertical extent:** 100m depth sufficient for test area?

**Resolve during implementation based on measurements.**

---

## 17. Next Steps

1. **User approval** of this spec
2. **Start Phase 1, Week 1, Day 1:** Add `rstar` crate, implement `VoxelBlock`
3. **Daily commit** with implementation notes
4. **Weekly check-in** after each phase

**Estimated start:** 2026-02-16 (today)  
**Estimated Phase 1 complete:** 2026-03-02 (2 weeks)  
**Estimated prototype complete:** 2026-04-06 (7 weeks)

---

**Ready to proceed?**
