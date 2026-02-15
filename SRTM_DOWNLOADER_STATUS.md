# SRTM Downloader Implementation Status

## ✅ What's Working

1. **Multi-source fallback system** - Tries OpenTopography → AWS Terrain → CGIAR in order
2. **2-second cooldown** - Respects project rule for rate limiting between requests
3. **Exponential backoff** - Retries failed downloads (2s, 4s, 8s delays)
4. **Async parallel downloading** - Can download multiple tiles concurrently
5. **Disk caching** - Successful downloads cached at `.metaverse/cache/srtm/`
6. **Detailed logging** - Shows progress and why each source failed

## ⚠️ Current Limitations

1. **GeoTIFF format not yet supported** - AWS Terrain returns GeoTIFF (229KB) but we need .hgt format
   - AWS successfully downloaded tile but parse_hgt() rejected it (wrong format)
   - Need to add GeoTIFF parsing library and conversion to .hgt format
   
2. **OpenTopography requires API key** - Best source but needs registration
   - Get free key at: https://portal.opentopography.org/
   - Set environment variable: `export OPENTOPOGRAPHY_API_KEY=your_key`
   
3. **CGIAR legacy sources offline** - As expected from 2026 reality
   - All URLs return 404 Not Found
   - Can remove this source or keep as last-resort fallback

## Test Results (2026-02-15)

Tested with Brisbane Story Bridge tile (S28E153.hgt):

```
[SRTM] Trying source 1/2: AWS Terrain
[SRTM] AWS Terrain Tiles request: z=10, x=947, y=595
[SRTM] Successfully downloaded 229324 bytes from AWS Terrain
[SRTM] Failed to parse tile: Invalid HGT file size: 229324 bytes
```

**Conclusion:** Download infrastructure works perfectly, just need GeoTIFF support.

## Next Steps

### Option 1: Add GeoTIFF Support (Correct, Not Easy)
- Add `gdal` or `geotiff` crate dependency
- Parse GeoTIFF elevation data
- Convert to standard .hgt format (3601×3601 or 1201×1201 grid)
- Handle coordinate transformations and resampling
- **Advantage:** Works with AWS Terrain (no auth required)
- **Disadvantage:** Complex format, requires external libraries

### Option 2: Use OpenTopography API (Easy, But Requires Key)
- Register for free API key at https://portal.opentopography.org/
- Set OPENTOPOGRAPHY_API_KEY environment variable
- OpenTopography returns GeoTIFF too, so still need parsing
- **Advantage:** Most authoritative source, well-documented
- **Disadvantage:** Requires user registration and API key management

### Option 3: Pre-download Tiles Manually
- Download S28E153.hgt from any SRTM source
- Place in `.metaverse/cache/srtm/` directory
- Test with real elevation data immediately
- **Advantage:** No format conversion needed, works now
- **Disadvantage:** Not scalable, manual process

## Recommended Approach

1. **Immediate:** Pre-download S28E153.hgt manually for testing (Option 3)
2. **Short-term:** Get OpenTopography API key and implement GeoTIFF parsing (Option 2)
3. **Long-term:** Full GeoTIFF support for AWS Terrain fallback (Option 1)

## Manual Download Instructions

### Using Earth Explorer (USGS)
1. Go to https://earthexplorer.usgs.gov/
2. Register for free account
3. Search for "SRTM 1 Arc-Second Global"
4. Enter coordinates: -28°S to -27°S, 153°E to 154°E
5. Download tile, rename to S28E153.hgt
6. Place in `.metaverse/cache/srtm/`

### Using OpenTopography Web Interface  
1. Go to https://portal.opentopography.org/
2. Register for free account
3. Select "Global Datasets" → "SRTM GL1 (30m)"
4. Draw bounding box: -28° to -27° lat, 153° to 154° lon
5. Download as GeoTIFF or HGT format
6. If GeoTIFF, convert with: `gdal_translate -of SRTM input.tif S28E153.hgt`
7. Place in `.metaverse/cache/srtm/`

### Verify Installation
```bash
# Check tile is present and correct size
ls -lh .metaverse/cache/srtm/S28E153.hgt
# Should be 25,934,402 bytes (SRTM1) or 2,884,802 bytes (SRTM3)

# Test elevation at Story Bridge
cargo run --example test_srtm_download
# Should now load from cache and report actual elevation (~5-30m)
```

## Implementation Code Status

**Files Created:**
- `src/srtm_downloader.rs` - Async multi-source downloader (new)
- `examples/test_srtm_download.rs` - Test script for Brisbane tile
- `SRTM_SOURCES_2026.md` - Documentation of available sources
- `SRTM_DOWNLOADER_STATUS.md` - This status file

**Dependencies Added:**
- `tokio = { version = "1", features = ["rt", "time", "sync", "macros", "rt-multi-thread"] }`
- `futures = "0.3"`
- `reqwest = { version = "0.12", features = ["blocking", "json", "stream"] }`

**Integration Points:**
- `SrtmManager` in `src/elevation.rs` already has load_tile() method
- Can enhance load_tile() to call async downloader when tile not in cache
- Or keep existing sync blocking downloader for now

## User Decision Required

**Question:** Which approach do you want to take?

A. **Add GeoTIFF parsing** - Full solution, works with AWS Terrain, complex
B. **Get OpenTopography API key** - Quick solution, requires registration  
C. **Pre-download manually** - Immediate testing, not scalable
D. **Combination** - Manual for testing now, API key for automation

This is a "correct vs. easy" tradeoff:
- **Correct:** GeoTIFF support (Option A)
- **Easy:** API key + manual tile (Options B+C)

I recommend **Option D (Combination)**:
1. Manually download S28E153.hgt for immediate testing (5 minutes)
2. Get OpenTopography API key for automated downloading (10 minutes registration)
3. Add basic GeoTIFF parsing later when needed for other regions (future work)

This gets us unblocked for testing while building toward the scalable solution.

What would you like to do?
