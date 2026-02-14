# SRTM Data Sources in 2026

## Problem
Most legacy SRTM mirrors from 2020s are now offline or require authentication:
- ❌ viewfinderpanoramas.org - 404
- ❌ srtm.csi.cgiar.org - 404
- ❌ srtm.kurviger.de - 404
- ❌ USGS version2_1 - 404

## Solutions

### Option 1: OpenTopography API (Best, but requires free account)
- URL: https://portal.opentopography.org/API
- Free tier: 1000 requests/day
- Coverage: Global SRTM 30m and 90m
- Setup: Register → get API key → set OPENTOPOGRAPHY_API_KEY env var

### Option 2: AWS Open Data (Works, but different format)
- Registry: https://registry.opendata.aws/terrain-tiles/
- Bucket: s3://elevation-tiles-prod/
- Format: GeoTIFF (not HGT), requires conversion
- Example: https://s3.amazonaws.com/elevation-tiles-prod/geotiff/10/553/384.tif

### Option 3: Mapzen/Nextzen Terrain Tiles
- URL: https://tile.nextzen.org/tilezen/terrain/v1/{tilesize}/{z}/{x}/{y}.json
- Requires API key (free)
- JSON format with elevation array

### Option 4: JAXA AW3D30 (30m global, requires registration)
- URL: https://www.eorc.jaxa.jp/ALOS/en/dataset/aw3d30/aw3d30_e.htm
- Free but requires registration
- Better quality than SRTM in some regions

### Option 5: Generate procedurally (Fallback)
- Use noise functions (simplex/perlin) for terrain generation
- Not real-world accurate but looks good
- Good for testing before real data

## Recommendation
Implement multi-tier system:
1. Try OpenTopography if API key available
2. Fallback to AWS Terrain Tiles (with format conversion)
3. Fallback to procedural generation (noise-based)
4. Never block - always return *something*
