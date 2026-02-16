# Continuous Query System - Implementation Log

**Phase:** 1 - Bounded Continuous World  
**Week:** 1  
**Date Range:** 2026-02-16 → 2026-03-02

---

## Day 1: Spatial Index Foundation (2026-02-16)

### Work Completed
- ✅ Added dependencies: `rstar` v0.12 (R*-tree), `lru` v0.12 (LRU cache)
- ✅ Created `src/spatial_index.rs` (360 lines)
- ✅ Implemented `AABB` struct for bounding boxes
- ✅ Implemented `VoxelBlock` struct (8m³ blocks, 512 voxels)
- ✅ Implemented `SpatialIndex` wrapper around R-tree
- ✅ Custom serde for large arrays (Box<[MaterialId; 512]>)
- ✅ 8 unit tests written and passing

### Key Decisions

**Block Size: 8m**
- Chosen as balance between index overhead and query granularity
- Test area (200m): 8,125 blocks total
- Each block: 1 KB storage
- Total memory if all loaded: ~8 MB (very reasonable)

**R-tree Choice:**
- `rstar` crate (v0.12) provides R*-tree (self-balancing variant)
- Native 3D spatial queries
- Mature, well-tested implementation
- Perfect fit for range queries

**Storage Granularity:**
- Blocks instead of individual voxels (512× reduction in index size)
- Trade-off: Must load entire 8m block even for single voxel query
- Acceptable for our use case (camera typically sees many voxels)

### Technical Challenges

**Challenge 1: Serde for large arrays**
- Problem: Rust serde doesn't derive for arrays >32 elements
- Solution: Custom serde module with serialize/deserialize
- Converts to/from Vec, then into [T; 512]

**Challenge 2: R-tree remove operation**
- Problem: rstar doesn't have remove-by-value API
- Solution: Rebuild tree without element (acceptable for prototype)
- Note: In production, might use different data structure or batch removals

**Challenge 3: PartialEq for VoxelBlock**
- Problem: Needed for R-tree remove operation
- Solution: Added PartialEq derive
- Compares all 512 voxels (acceptable for 1 KB blocks)

### Test Results

All 8 tests passing:
```
test spatial_index::tests::test_aabb_contains ... ok
test spatial_index::tests::test_aabb_intersects ... ok
test spatial_index::tests::test_block_alignment ... ok
test spatial_index::tests::test_spatial_index_nearest ... ok
test spatial_index::tests::test_spatial_index_insert_query ... ok
test spatial_index::tests::test_voxel_block_creation ... ok
test spatial_index::tests::test_voxel_block_get_set ... ok
test spatial_index::tests::test_voxel_block_sample ... ok
```

**Critical test: `test_block_alignment`**
- Verifies adjacent blocks touch exactly (no gaps)
- Block 1 max X == Block 2 min X
- Confirms blocks are continuous

### Code Quality
- 360 lines of well-documented code
- Every public function has rustdoc
- Comprehensive test coverage
- No unsafe code
- Zero clippy warnings (related to spatial_index)

### Performance Notes
- R-tree query: Expected <1ms (will benchmark in Phase 2)
- Block lookup: O(log n) where n = number of blocks
- Test area: log(8125) ≈ 13 comparisons max
- Memory overhead: ~0.5 MB for R-tree index

### Next Steps (Day 3-4: Caching)
- Implement `AdaptiveCache` (hot/warm/cold tiers)
- Implement `DiskCache` with compression
- LRU eviction policy for warm cache
- Cache hit rate tracking
- Benchmark cache performance

### Files Created
- `src/spatial_index.rs` - Core spatial indexing (360 lines)

### Files Modified
- `Cargo.toml` - Added rstar, lru dependencies
- `src/lib.rs` - Added spatial_index module

### Commits
- Will commit after completing Day 3-4 (caching)

---

## Lessons Learned (Day 1)

1. **R-tree is the right choice** - rstar API is clean, works out of box
2. **Block-based storage essential** - Individual voxels would be 512× overhead
3. **8m blocks seem right** - Test area fits in memory, fine granularity
4. **Serde complexity manageable** - Custom module works, not too complex
5. **Tests caught alignment issues** - Test-first approach validated
6. **PartialEq on 512 elements ok** - 1 KB comparison is fast enough

## Open Questions

1. **Block size optimization?** - Is 8m truly optimal? Test 4m, 16m?
2. **R-tree parameters?** - Default branching factor ok or tune?
3. **Remove performance?** - Rebuild tree acceptable or need better solution?

Will answer through measurement in Phase 2.

---

## Day 3-4: Caching System (2026-02-16)

### Work Completed
- ✅ Created `src/adaptive_cache.rs` (430 lines)
- ✅ Implemented three-tier cache (hot/warm/cold)
- ✅ Hot cache: HashMap for recent queries (O(1), no eviction order)
- ✅ Warm cache: LRU for frequent queries (O(1) with proper eviction)
- ✅ Cold cache: Disk storage with hierarchical directories
- ✅ BlockKey: Integer-based keys (mm precision) for deterministic hashing
- ✅ CacheStats: Hit rate tracking and monitoring
- ✅ 10 unit tests written and passing

### Key Decisions

**Three-Tier Design:**
- Hot (HashMap): Recent queries, fast access, simple random eviction
- Warm (LRU): Frequent queries, proper LRU eviction
- Cold (Disk): Unlimited storage, bincode serialization

**Block Key Design:**
- Uses millimeter-precision integers instead of floats
- Snaps to 8m grid for deterministic positioning
- Avoids floating-point comparison issues
- Hash-friendly (Eq + Hash traits)

**Disk Cache Structure:**
- Hierarchical directories: `x{km}/y{km}/z{km}/block_{mm}_{mm}_{mm}.bin`
- Prevents millions of files in single directory
- Groups by 1km buckets for filesystem efficiency
- Example: `/cache/x1/y2/z3/block_1000000_2000000_3000000.bin`

**Compression Decision:**
- Deferred compression for prototype
- Blocks are only 1 KB each
- Can add zstd later if disk space becomes issue
- Prioritized simplicity for Phase 1

### Technical Challenges

**Challenge 1: Borrow checker with LRU.get()**
- Problem: `LruCache::get()` takes `&mut self`, conflicts with using returned value
- Solution: Clone block before calling insert_hot
- Trade-off: Extra clone (acceptable for 1 KB blocks)

**Challenge 2: Hot cache eviction**
- Problem: HashMap doesn't track access order
- Solution: Random eviction (remove first entry)
- Rationale: Hot cache is small, warm cache has proper LRU

**Challenge 3: Block key rounding**
- Problem: Floating-point grid snapping could cause inconsistency
- Solution: Floor-based snapping, then convert to integer mm
- Verified by test: nearby positions hash to same key

### Test Results

All 10 tests passing:
```
test adaptive_cache::tests::test_block_key_from_ecef ... ok
test adaptive_cache::tests::test_block_key_roundtrip ... ok
test adaptive_cache::tests::test_cache_stats ... ok
test adaptive_cache::tests::test_adaptive_cache_hot ... ok
test adaptive_cache::tests::test_adaptive_cache_miss ... ok
test adaptive_cache::tests::test_adaptive_cache_warm_promotion ... ok
test adaptive_cache::tests::test_disk_cache_save_load ... ok
test adaptive_cache::tests::test_disk_cache_hierarchical_path ... ok
test adaptive_cache::tests::test_cache_clear ... ok
test adaptive_cache::tests::test_hot_cache_eviction ... ok
```

**Critical tests:**
- `test_block_key_from_ecef`: Verifies grid snapping works
- `test_warm_promotion`: Confirms promotion from warm → hot
- `test_disk_cache_save_load`: Validates serialization roundtrip
- `test_hot_cache_eviction`: Confirms capacity limits enforced

### Performance Analysis

**Memory footprint:**
- Hot cache (1000 blocks): 1 MB
- Warm cache (5000 blocks): 5 MB  
- BlockKey overhead: 24 bytes per entry (negligible)
- Total: ~6 MB for caches + ~0.5 MB for R-tree = **~7 MB total**

**Expected cache hit rates (estimated):**
- Hot cache: ~80% (recent frame's blocks)
- Warm cache: ~15% (nearby blocks from previous frames)
- Cold cache: ~4% (distant blocks, loaded from disk)
- Miss: ~1% (never generated blocks)
- **Overall hit rate: ~99%** (will benchmark in Phase 2)

**Access times (estimated):**
- Hot: <100ns (HashMap lookup)
- Warm: <100ns (LRU lookup)
- Cold: <5ms (disk I/O + deserialize)
- Miss: <100ms (generation from SRTM+OSM)

### Code Quality
- 430 lines of well-documented code
- Every public function has rustdoc comments
- Comprehensive test coverage (10 tests)
- No unsafe code
- Zero clippy warnings (related to adaptive_cache)

### Integration Notes

Cache integrates with spatial index:
```rust
// Query flow:
1. Check cache (hot → warm → cold)
2. If miss: Generate from source data
3. Insert into all cache tiers
4. Return blocks to spatial index
```

Next phase will implement the generation step.

### Next Steps (Day 5-7: Public API)
- Implement `ContinuousWorld` struct
- Integrate spatial index + cache
- Implement `query_range()`, `query_frustum()`, `sample_point()`
- Write integration tests
- Benchmark query performance

### Files Created
- `src/adaptive_cache.rs` - Three-tier caching system (430 lines)

### Files Modified
- `src/lib.rs` - Added adaptive_cache module

### Commits
- Will commit after completing Day 5-7 (public API)

---

## Lessons Learned (Day 3-4)

1. **LRU crate works well** - `lru` v0.12 API is clean, NonZeroUsize is fine
2. **Integer keys essential** - Float-based keys would cause hash collisions
3. **Three tiers justified** - Hot for speed, warm for intelligence, cold for capacity
4. **Hierarchical disk** layout necessary - flat would create millions of files in one dir
5. **Cloning 1 KB blocks** acceptable - not a performance bottleneck
6. **Statistics tracking valuable** - will help optimize cache sizes in Phase 2

## Open Questions

1. **Cache sizing optimal?** - 1000 hot, 5000 warm seems right for test area
2. **Compression needed?** - Blocks are small, may not be worth CPU cost
3. **LRU alone sufficient?** - Or need smarter eviction (access frequency, spatial locality)?

Will answer through measurement in Phase 2.

---

**Status:** Ahead of schedule. Day 1-4 complete in 1 day.  
**Next:** Day 5-7 - Public API and integration
