# SRTM Data Pipeline - Redundant Multi-Source Design

**Last Updated:** 2026-02-17  
**Purpose:** Resilient elevation data access with local caching and multiple fallbacks

---

## REQUIREMENTS

**From user:**
- ✅ Redundancy (multiple sources work in parallel or as fallbacks)
- ✅ Local disk cache (stored in project folder for backups)
- ✅ Can use all sources, some sources, or single source
- ✅ Resilient to individual source failures

---

## DATA SOURCES (Priority Order)

### Source 1: LOCAL CACHE (Highest Priority)
**Path:** `./elevation_cache/`  
**Format:** Individual GeoTIFF tiles (organized by lat/lon)  
**Purpose:** Fast access, no network, survives backups

**Structure:**
```
metaverse_core/
  elevation_cache/
    tiles/
      N27/E153/srtm_n27_e153.tif    # Kangaroo Point
      N40/W075/srtm_n40_w075.tif    # Philadelphia example
      S90/E000/srtm_s90_e000.tif    # South pole
      ...
    metadata.json                     # Track what's cached
    README.md                         # Explain cache structure
```

**Advantages:**
- Instant access (no network delay)
- Works offline
- Survives in project backups (user can copy folder)
- No API rate limits

**Cache strategy:**
- Cache tiles as they're requested
- Keep most recently used (LRU eviction if too large)
- Optional: Pre-download tiles for test areas (Kangaroo Point)

---

### Source 2: NAS FILE (Second Priority)
**Path:** `/mnt/nas/srtm-v3-1s.tif`  
**Format:** Single global GeoTIFF  
**Purpose:** Complete dataset, no API limits

**Advantages:**
- No rate limits
- Complete global coverage
- Single file (no tile management)

**Disadvantages:**
- Requires NAS mount
- Large file (~15-20GB)
- Random access performance unknown (need to test)

**Strategy:**
- Query tile from global file
- Extract and cache to local cache
- If NAS unavailable, fall back to API sources

---

### Source 3: OPENTOPOGRAPHY API (Third Priority)
**API Key:** `3e607de6969c687053f9e107a4796962`  
**Endpoint:** TBD (need to research API docs)  
**Purpose:** On-demand tile download

**Advantages:**
- No file storage needed
- Can get higher resolution (if needed)
- Good for gaps or testing

**Disadvantages:**
- Rate limits (need to check)
- Network dependency
- May have coverage gaps

**Strategy:**
- Request individual tiles by lat/lon bounds
- Cache response to local cache
- Respect rate limits (2-second cooldown per RULES.md)

---

### Source 4: NASA EARTHDATA (Fallback)
**Account:** User has credentials  
**Purpose:** Authoritative reference, last resort

**Advantages:**
- Official source
- Highest quality metadata
- Complete coverage

**Disadvantages:**
- Requires authentication
- Slower (FTP/HTTP download)
- Not meant for real-time queries

**Strategy:**
- Use for bulk tile downloads
- Validate cached data against official source
- Manual fallback (not automated in pipeline)

---

## PIPELINE ARCHITECTURE

### Multi-Source Waterfall (Priority Order)

```rust
async fn get_elevation(lat: f64, lon: f64) -> Result<f32> {
    // 1. CHECK LOCAL CACHE (instant)
    if let Some(elevation) = cache.get(lat, lon)? {
        return Ok(elevation);
    }
    
    // 2. TRY NAS FILE (fast, no rate limit)
    if let Ok(elevation) = nas_source.query(lat, lon).await {
        cache.store(lat, lon, elevation)?;  // Cache for next time
        return Ok(elevation);
    }
    
    // 3. TRY OPENTOPOGRAPHY API (network, rate limited)
    if let Ok(elevation) = opentopo_source.query(lat, lon).await {
        cache.store(lat, lon, elevation)?;
        return Ok(elevation);
    }
    
    // 4. FAIL (couldn't get data)
    Err(Error::NoElevationData { lat, lon })
}
```

**Advantages:**
- Fast common case (cache hit)
- Resilient (tries 3 sources)
- Auto-populates cache
- No wasted API calls

---

### Parallel Fetch (Fastest Response)

**Alternative strategy for critical paths:**

```rust
async fn get_elevation_parallel(lat: f64, lon: f64) -> Result<f32> {
    // 1. CHECK CACHE FIRST (synchronous, instant)
    if let Some(elevation) = cache.get(lat, lon)? {
        return Ok(elevation);
    }
    
    // 2. QUERY ALL SOURCES IN PARALLEL
    let (nas_result, api_result) = tokio::join!(
        nas_source.query(lat, lon),
        opentopo_source.query(lat, lon),
    );
    
    // 3. RETURN FIRST SUCCESS
    let elevation = nas_result
        .or(api_result)
        .ok_or(Error::NoElevationData { lat, lon })?;
    
    // 4. CACHE FOR FUTURE
    cache.store(lat, lon, elevation)?;
    
    Ok(elevation)
}
```

**Advantages:**
- Fastest possible response (race condition)
- Validates consistency (both should return same value)
- Auto-discovers fastest source

**Disadvantages:**
- Wastes API calls (hits both sources even if NAS works)
- More network traffic
- Rate limit concerns

**When to use:**
- Initial terrain generation (need many tiles fast)
- Pre-warming cache for test area
- Not for real-time queries (use waterfall)

---

## LOCAL CACHE DESIGN

### Tile Organization

**Strategy: Store as individual GeoTIFF tiles**

```
elevation_cache/
  tiles/
    {LAT_BAND}/{LON_BAND}/srtm_{lat}_{lon}.tif
    
Example:
  N27/E153/srtm_n27_e153.tif       # 1° x 1° tile
  N27/E154/srtm_n27_e154.tif
  S28/E153/srtm_s28_e153.tif
```

**Tile size:** 1° x 1° (SRTM standard)
- At equator: ~111km x 111km
- At 30° latitude: ~96km x 111km
- Contains ~3600 x 3600 pixels at 1 arc-second resolution

**Why individual tiles:**
- ✅ Fast lookup (know which file from lat/lon)
- ✅ Partial caching (don't need whole world)
- ✅ Easy deletion (remove old tiles)
- ✅ Standard format (other tools can read)

---

### Cache Metadata

**File: `elevation_cache/metadata.json`**

```json
{
  "version": "1.0",
  "cache_created": "2026-02-17T05:00:00Z",
  "tiles": [
    {
      "lat": -27,
      "lon": 153,
      "file": "tiles/S27/E153/srtm_s27_e153.tif",
      "source": "nas",
      "cached_at": "2026-02-17T05:15:00Z",
      "file_size_bytes": 51840000,
      "checksum_sha256": "abc123...",
      "access_count": 47,
      "last_accessed": "2026-02-17T05:18:00Z"
    },
    ...
  ],
  "stats": {
    "total_tiles": 15,
    "total_size_mb": 780,
    "cache_hits": 1547,
    "cache_misses": 23
  }
}
```

**Purpose:**
- Track what's cached
- Know data sources
- LRU eviction (if cache too large)
- Validate integrity (checksums)
- Statistics (hit rate, size)

---

### Cache Management

**Max cache size:** Configurable (default 10GB)

```rust
struct CacheConfig {
    max_size_bytes: u64,        // Default: 10GB
    max_tiles: usize,            // Default: unlimited
    eviction_policy: EvictionPolicy,  // LRU, LFU, or None
}

enum EvictionPolicy {
    LRU,   // Least Recently Used
    LFU,   // Least Frequently Used
    None,  // Never evict (unbounded)
}
```

**Eviction strategy (when cache > max_size):**
1. Sort tiles by last access time (LRU)
2. Delete oldest tiles until under threshold
3. Update metadata.json

**Pre-warming (optional):**
```rust
async fn prewarm_cache(bounds: GeoBounds) {
    // Download all tiles in bounds
    for tile in tiles_in_bounds(bounds) {
        get_elevation_parallel(tile.center_lat, tile.center_lon).await?;
    }
}

// Example: Pre-download Kangaroo Point area
prewarm_cache(GeoBounds {
    min_lat: -28.0, max_lat: -27.0,
    min_lon: 153.0, max_lon: 154.0,
}).await?;
```

---

## SOURCE IMPLEMENTATIONS

### Source 1: Local Cache

```rust
struct LocalCache {
    cache_dir: PathBuf,  // ./elevation_cache/
    metadata: Metadata,
    config: CacheConfig,
}

impl LocalCache {
    fn get(&self, lat: f64, lon: f64) -> Result<Option<f32>> {
        let tile_path = self.tile_path(lat, lon);
        if !tile_path.exists() {
            return Ok(None);  // Cache miss
        }
        
        // Open tile, query elevation
        let tile = GeoTiffTile::open(tile_path)?;
        let elevation = tile.query_bilinear(lat, lon)?;
        
        // Update access time
        self.metadata.record_access(lat, lon);
        
        Ok(Some(elevation))
    }
    
    fn store(&mut self, tile: &GeoTiffTile) -> Result<()> {
        let path = self.tile_path(tile.lat, tile.lon);
        fs::create_dir_all(path.parent().unwrap())?;
        
        // Write tile to cache
        tile.save(path)?;
        
        // Update metadata
        self.metadata.add_tile(tile)?;
        
        // Check cache size, evict if needed
        if self.total_size() > self.config.max_size_bytes {
            self.evict_lru()?;
        }
        
        Ok(())
    }
    
    fn tile_path(&self, lat: f64, lon: f64) -> PathBuf {
        let lat_band = format!("{}{:02}", if lat >= 0.0 { "N" } else { "S" }, lat.abs() as i32);
        let lon_band = format!("{}{:03}", if lon >= 0.0 { "E" } else { "W" }, lon.abs() as i32);
        self.cache_dir
            .join("tiles")
            .join(lat_band)
            .join(lon_band)
            .join(format!("srtm_{}_{}.tif", lat as i32, lon as i32))
    }
}
```

---

### Source 2: NAS Global File

```rust
struct NasSource {
    file_path: PathBuf,  // /mnt/nas/srtm-v3-1s.tif
    dataset: OnceCell<GdalDataset>,  // Lazy load
}

impl NasSource {
    async fn query(&self, lat: f64, lon: f64) -> Result<f32> {
        // Check NAS mounted
        if !self.file_path.exists() {
            return Err(Error::NasNotMounted);
        }
        
        // Open dataset (cached after first open)
        let dataset = self.dataset.get_or_try_init(|| {
            GdalDataset::open(&self.file_path)
        })?;
        
        // Convert lat/lon to pixel coordinates
        let (px, py) = dataset.geo_to_pixel(lat, lon)?;
        
        // Read elevation value
        let elevation = dataset.read_pixel(px, py)?;
        
        Ok(elevation)
    }
    
    async fn extract_tile(&self, lat: i32, lon: i32) -> Result<GeoTiffTile> {
        // Extract 1° x 1° tile from global file
        let dataset = self.dataset.get_or_try_init(|| {
            GdalDataset::open(&self.file_path)
        })?;
        
        let tile = dataset.extract_region(
            lat as f64, lon as f64,
            1.0, 1.0  // 1° x 1°
        )?;
        
        Ok(tile)
    }
}
```

---

### Source 3: OpenTopography API

```rust
struct OpenTopoSource {
    api_key: String,
    rate_limiter: RateLimiter,  // 2-second cooldown
    client: reqwest::Client,
}

impl OpenTopoSource {
    async fn query(&self, lat: f64, lon: f64) -> Result<f32> {
        // Rate limit (2-second cooldown per RULES.md)
        self.rate_limiter.wait().await;
        
        // Download tile containing this point
        let tile = self.download_tile(lat as i32, lon as i32).await?;
        
        // Query elevation from tile
        let elevation = tile.query_bilinear(lat, lon)?;
        
        Ok(elevation)
    }
    
    async fn download_tile(&self, lat: i32, lon: i32) -> Result<GeoTiffTile> {
        // API endpoint (TODO: research actual OpenTopography API)
        let url = format!(
            "https://portal.opentopography.org/API/...\
             ?apiKey={}&north={}&south={}&east={}&west={}",
            self.api_key,
            lat + 1, lat,
            lon + 1, lon
        );
        
        // Download
        let response = self.client.get(&url).send().await?;
        let bytes = response.bytes().await?;
        
        // Parse GeoTIFF
        let tile = GeoTiffTile::from_bytes(&bytes)?;
        
        Ok(tile)
    }
}
```

**TODO:** Research actual OpenTopography API endpoint and format

---

## REDUNDANCY STRATEGIES

### Strategy A: Waterfall (Recommended)

**Use case:** Normal operation

1. Check cache (instant)
2. If miss → try NAS (fast, no limit)
3. If NAS fail → try OpenTopo API (slow, limited)
4. Cache result for future

**Advantages:**
- Fast common case
- No wasted API calls
- Resilient to single source failure

**Code:** See "Multi-Source Waterfall" above

---

### Strategy B: Parallel Race

**Use case:** Initial terrain generation, cache pre-warming

1. Check cache (instant)
2. If miss → query NAS AND API simultaneously
3. Return first response
4. Validate both responses match (if both succeed)
5. Cache result

**Advantages:**
- Fastest possible response
- Validates data consistency
- Auto-discovers which source is faster

**Disadvantages:**
- Wastes API calls
- May hit rate limits faster

**Code:** See "Parallel Fetch" above

---

### Strategy C: Validation Mode

**Use case:** Testing, data integrity checking

1. Query ALL sources (cache, NAS, API)
2. Compare all results
3. Flag if they don't match
4. Return consensus value (or error if disagreement)

**Purpose:**
- Detect corrupted cache
- Verify NAS file integrity
- Find API data bugs

**Code:**
```rust
async fn get_elevation_validated(lat: f64, lon: f64) -> Result<f32> {
    let cache_result = cache.get(lat, lon)?;
    let nas_result = nas_source.query(lat, lon).await.ok();
    let api_result = opentopo_source.query(lat, lon).await.ok();
    
    // Collect all results
    let mut results = Vec::new();
    if let Some(v) = cache_result { results.push(("cache", v)); }
    if let Some(v) = nas_result { results.push(("nas", v)); }
    if let Some(v) = api_result { results.push(("api", v)); }
    
    // Check agreement (within 1m tolerance - SRTM accuracy)
    if results.len() >= 2 {
        let max_diff = results.iter()
            .map(|(_, v)| v)
            .max()
            .zip(results.iter().map(|(_, v)| v).min())
            .map(|(max, min)| max - min)
            .unwrap_or(0.0);
        
        if max_diff > 1.0 {
            warn!("Elevation mismatch at ({}, {}): {:?}", lat, lon, results);
        }
    }
    
    // Return first result (or average?)
    results.first()
        .map(|(_, v)| *v)
        .ok_or(Error::NoElevationData { lat, lon })
}
```

---

## CONFIGURATION

**File: `elevation_config.toml`**

```toml
[cache]
enabled = true
path = "./elevation_cache"
max_size_gb = 10.0
eviction_policy = "LRU"

[sources.nas]
enabled = true
path = "/mnt/nas/srtm-v3-1s.tif"
priority = 1  # Try first (after cache)

[sources.opentopography]
enabled = true
api_key_env = "OPENTOPOGRAPHY_API_KEY"  # Read from environment
rate_limit_seconds = 2.0
priority = 2  # Try second

[sources.earthdata]
enabled = false  # Manual fallback only
username_env = "EARTHDATA_USERNAME"
password_env = "EARTHDATA_PASSWORD"

[strategy]
mode = "waterfall"  # waterfall, parallel, or validation
parallel_validation_threshold = 10.0  # Warn if sources differ by >10m
```

---

## ERROR HANDLING

**Graceful degradation:**

```rust
async fn get_elevation(lat: f64, lon: f64) -> Result<f32> {
    // Try cache
    match cache.get(lat, lon) {
        Ok(Some(elev)) => return Ok(elev),
        Ok(None) => {},  // Cache miss, continue
        Err(e) => warn!("Cache error: {}, continuing", e),
    }
    
    // Try NAS
    match nas_source.query(lat, lon).await {
        Ok(elev) => {
            let _ = cache.store(lat, lon, elev);  // Best effort cache
            return Ok(elev);
        }
        Err(e) => warn!("NAS error: {}, trying API", e),
    }
    
    // Try API
    match opentopo_source.query(lat, lon).await {
        Ok(elev) => {
            let _ = cache.store(lat, lon, elev);
            return Ok(elev);
        }
        Err(e) => error!("API error: {}", e),
    }
    
    // All sources failed
    Err(Error::NoElevationData { lat, lon })
}
```

**Log all failures, degrade gracefully, return clear errors**

---

## TESTING STRATEGY

### Test 1: Cache Hit
```rust
#[test]
async fn test_cache_hit() {
    let mut cache = LocalCache::new("./test_cache");
    cache.store_tile(test_tile()).await?;
    
    let elevation = get_elevation(-27.4775, 153.0355).await?;
    
    // Should come from cache (no network calls)
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(cache.stats().misses, 0);
}
```

### Test 2: NAS Fallback
```rust
#[test]
async fn test_nas_fallback() {
    let cache = LocalCache::empty();  // Empty cache
    
    let elevation = get_elevation(-27.4775, 153.0355).await?;
    
    // Should query NAS, then cache
    assert_eq!(cache.stats().misses, 1);
    assert!(cache.has_tile(-27, 153));
}
```

### Test 3: API Fallback
```rust
#[test]
async fn test_api_fallback() {
    let cache = LocalCache::empty();
    let nas = NasSource::unavailable();  // Simulate NAS failure
    
    let elevation = get_elevation(-27.4775, 153.0355).await?;
    
    // Should fall back to API
    assert_eq!(api.stats().requests, 1);
}
```

### Test 4: All Sources Fail
```rust
#[test]
async fn test_all_sources_fail() {
    let cache = LocalCache::empty();
    let nas = NasSource::unavailable();
    let api = OpenTopoSource::unavailable();
    
    let result = get_elevation(-27.4775, 153.0355).await;
    
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), Error::NoElevationData);
}
```

---

## IMPLEMENTATION PHASES

### Phase 2.1: Local Cache (FIRST)
- [ ] Design cache directory structure
- [ ] Implement tile storage/retrieval
- [ ] Implement metadata tracking
- [ ] Test cache hit/miss
- [ ] LRU eviction

### Phase 2.2: NAS Source (WHEN FILE AVAILABLE)
- [ ] Test GeoTIFF library with actual file
- [ ] Implement global file reader
- [ ] Implement tile extraction
- [ ] Test performance (random access)
- [ ] Cache tiles from NAS

### Phase 2.3: API Source
- [ ] Research OpenTopography API docs
- [ ] Implement API client
- [ ] Implement rate limiting
- [ ] Test tile download
- [ ] Cache tiles from API

### Phase 2.4: Integration
- [ ] Implement waterfall strategy
- [ ] Implement parallel strategy
- [ ] Configuration file loading
- [ ] Error handling and logging
- [ ] All integration tests pass

---

## BACKUP STRATEGY

**User requirement:** Cache stored in project folder for backups

**Backup includes:**
```
metaverse_core/
  elevation_cache/        ← This whole folder
    tiles/                ← All cached tiles
    metadata.json         ← Cache metadata
    README.md             ← Documentation
```

**To restore from backup:**
1. Copy `elevation_cache/` folder to project
2. Application auto-detects cached tiles
3. No re-download needed

**Cache is portable:**
- Can share between machines
- Can distribute with project
- Can pre-populate for offline use

---

## SUMMARY

**Redundant multi-source SRTM pipeline:**
- ✅ Local cache (instant, survives backups)
- ✅ NAS file (fast, no limits, when available)
- ✅ OpenTopography API (fallback, rate limited)
- ✅ NASA Earthdata (manual/bulk, last resort)

**Strategies:**
- Waterfall (normal): cache → NAS → API
- Parallel (fast): cache → (NAS || API) race
- Validation (testing): compare all sources

**Resilient:**
- Works with any subset of sources
- Caches all queries for future
- Graceful degradation
- Clear error messages

**Backup-friendly:**
- Cache in project folder
- Portable between machines
- Pre-warming for offline use

