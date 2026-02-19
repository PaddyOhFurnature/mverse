# SRTM Data Sources (2026)

## Overview

SRTM (Shuttle Radar Topography Mission) provides near-global elevation data from 60°N to 56°S.
This document lists verified data sources for tile-based `.hgt` file downloads as of February 2026.

## Current Status of Legacy Sources

Most SRTM mirrors from early 2020s are offline:
- ❌ viewfinderpanoramas.org - 404
- ❌ srtm.csi.cgiar.org - 404 (main site)
- ❌ srtm.kurviger.de - 404
- ❌ USGS version2_1 FTP - deprecated

## Active Sources (2026)

### 1. OpenTopography API (PRIMARY - Best Quality)
- **URL:** https://portal.opentopography.org/API
- **API Endpoint:** https://portal.opentopography.org/API/globaldem
- **Resolution:** 30m (SRTMGL1) and 90m (SRTMGL3)
- **Format:** GeoTIFF, can request specific bounding box
- **Coverage:** Near-global (60°N to 56°S)
- **Authentication:** Requires free API key
- **Rate Limits:** ~1000 requests/day (free tier), ~5 requests/minute
- **Setup:** Register at portal.opentopography.org → get API key → set OPENTOPOGRAPHY_API_KEY env var
- **Notes:**
  - Most authoritative source still available
  - Can request specific lat/lon bounding boxes
  - Returns GeoTIFF (requires conversion to .hgt format)
  - Good error handling and status codes
  - **Project cooldown:** 2 seconds between requests

**Example API Request:**
```
https://portal.opentopography.org/API/globaldem?
  demtype=SRTMGL1&
  south=-28&north=-27&west=153&east=154&
  outputFormat=GTiff&
  API_Key=YOUR_KEY_HERE
```

### 2. AWS Terrain Tiles (SECONDARY - No Registration)
- **Registry:** https://registry.opendata.aws/terrain-tiles/
- **Bucket:** s3://elevation-tiles-prod/
- **Format:** GeoTIFF tiles (not .hgt format)
- **Tile scheme:** Slippy map tiles (z/x/y)
- **Coverage:** Global
- **Authentication:** None (public bucket)
- **Rate Limits:** AWS S3 limits (very high)
- **Notes:**
  - Uses web mercator tile system (not SRTM lat/lon grid)
  - Requires coordinate conversion: lat/lon → tile z/x/y
  - GeoTIFF format requires parsing (not simple binary .hgt)
  - Good for areas where OpenTopography unavailable
  - **Project cooldown:** 2 seconds between requests

**Example URL:**
```
https://s3.amazonaws.com/elevation-tiles-prod/geotiff/10/553/384.tif
```

### 3. Mapzen/Nextzen Terrain Tiles (TERTIARY - Alternative)
- **URL:** https://tile.nextzen.org/tilezen/terrain/v1/
- **Format:** JSON with elevation array or GeoTIFF
- **Tile scheme:** Slippy map tiles (z/x/y)
- **Coverage:** Global
- **Authentication:** Requires free API key
- **Rate Limits:** Moderate (free tier)
- **Setup:** Register at developers.nextzen.org → get API key
- **Notes:**
  - JSON format easier to parse than GeoTIFF
  - Uses terrarium encoding (R,G,B → elevation)
  - Good for real-time lookups
  - **Project cooldown:** 2 seconds between requests

**Example URL:**
```
https://tile.nextzen.org/tilezen/terrain/v1/512/10/553/384.json?api_key=YOUR_KEY
```

### 4. CGIAR-CSI Archive (BACKUP - Limited Availability)
- **Direct tiles:** https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/
- **Resolution:** 90m (3 arc-second)
- **Format:** GeoTIFF tiles (5°×5° blocks)
- **Coverage:** Near-global, hole-filled version
- **Authentication:** None
- **Notes:**
  - Main site offline, but direct file links may still work
  - Try accessing specific tile URLs directly
  - Tiles organized in 5°×5° blocks (not 1°×1° like standard SRTM)
  - Use only as last resort before procedural fallback
  - **Project cooldown:** 2 seconds between requests

**Example (may be 404):**
```
https://srtm.csi.cgiar.org/wp-content/uploads/files/srtm_5x5/SRTM_v4_1/srtm_66_19.zip
```

### 5. Procedural Generation (FALLBACK - Always Available)
- **Method:** Perlin/Simplex noise with realistic parameters
- **Resolution:** Arbitrary (we use 30m equivalent)
- **Coverage:** Unlimited
- **Advantages:**
  - Never fails (always returns data)
  - No network required
  - Fast generation
  - Looks plausible from distance
- **Disadvantages:**
  - Not real-world accurate
  - Doesn't match actual topography
  - Unsuitable for navigation/measurement
- **Use case:** Testing, offline mode, when all sources fail

## Tile Naming Convention

**Standard SRTM:** `[N|S]YY[E|W]XXX.hgt`
- Examples: S28E153.hgt, N37W122.hgt
- Each tile covers 1°×1° (latitude × longitude)
- Named by southwest corner

**Web Tiles (AWS/Nextzen):** `{z}/{x}/{y}`
- Uses slippy map tile coordinates
- Requires lat/lon → tile coordinate conversion
- Variable zoom levels (typically z=10-14 for SRTM-equivalent)

## Data Format Details

**.hgt file structure:**
- Binary 16-bit signed integers (big-endian)
- Row-major order, north-to-south, west-to-east
- Void values: -32768 (0x8000)
- Vertical units: meters above WGS84 ellipsoid
- No header (raw grid data)

**File sizes:**
- SRTM1 (30m): 3601×3601 samples = 25,934,402 bytes
- SRTM3 (90m): 1201×1201 samples = 2,884,802 bytes

## Implementation Strategy

### Priority Order (with fallback):
1. **Local disk cache** (~/.metaverse/cache/srtm/)
2. **OpenTopography API** (if API key available)
3. **AWS Terrain Tiles** (no auth, but requires format conversion)
4. **Nextzen Tiles** (if API key available)
5. **CGIAR direct URLs** (try specific tiles, may 404)
6. **Procedural fallback** (Perlin noise, always works)

### Parallel Download Design:
- Download multiple tiles concurrently using tokio async
- Prioritize tiles by distance from camera (download closest first)
- Respect 2-second cooldown **per provider** (can query providers in parallel)
- Maximum 3 concurrent downloads per provider
- Timeout: 15 seconds per tile request
- Retry failed downloads with exponential backoff (2s, 4s, 8s, max 3 retries)
- Skip to next provider on repeated failures

### Caching Strategy:
- **Memory cache:** LRU cache for 50 most recent tiles (~125MB for SRTM1)
- **Disk cache:** ~/.metaverse/cache/srtm/ (persistent between runs)
- **Pre-loading:** Download adjacent tiles in background when camera moves
- **Validation:** Check file size and elevation ranges before caching

## Brisbane Test Case

**Location:** Story Bridge (-27.463697°, 153.035725°)

**Required tile:** S28E153.hgt (covers -28° to -27° latitude, 153° to 154° longitude)

**Expected elevations (from reference images):**
- River level: ~5m above sea level
- Bridge deck: ~30m above sea level  
- Kangaroo Point cliffs: ~40-60m above sea level
- Flat parklands: ~10-20m above sea level

**Current status:** Using procedural fallback (~236m, completely wrong)

## API Authentication Setup

### OpenTopography (Recommended):
1. Visit https://portal.opentopography.org/
2. Register for free account
3. Navigate to "MyOpenTopo" → "API Key"
4. Copy API key
5. Set environment variable: `export OPENTOPOGRAPHY_API_KEY="your_key_here"`

### Nextzen (Optional):
1. Visit https://developers.nextzen.org/
2. Register for free account
3. Create new API key
4. Set environment variable: `export NEXTZEN_API_KEY="your_key_here"`

## Rate Limits & Project Rules

| Source | Registration | Rate Limit | Project Cooldown |
|--------|--------------|------------|-----------------|
| OpenTopography | Required | ~1000/day, ~5/min | 2 seconds |
| AWS Terrain | None | Very high | 2 seconds |
| Nextzen | Required | Moderate | 2 seconds |
| CGIAR | None | Unknown | 2 seconds |

**Project Rule (RULES.md):** 2-second cooldown between ALL external API calls

## Validation

After downloading, verify tile integrity:
1. File size matches expected (25.9MB for SRTM1, 2.8MB for SRTM3)
2. Not all void values (check for non-32768 data)
3. Elevation range reasonable (-500m to +9000m)
4. Bilinear interpolation produces smooth gradients

## References

- OpenTopography Portal: https://portal.opentopography.org/
- AWS Terrain Tiles: https://registry.opendata.aws/terrain-tiles/
- Nextzen Tiles: https://developers.nextzen.org/
- NASA SRTM Mission: https://www.earthdata.nasa.gov/data/instruments/srtm

## Status

**Last Updated:** 2026-02-15
**Test Case:** Brisbane Story Bridge  
**Current:** Using procedural fallback (need to implement OpenTopography integration)
**Next:** Implement async multi-source downloader with OpenTopography primary
