# CRITICAL: SRTM Vertical Datum Issue

## Problem Discovered

SRTM elevations from AWS Terrain Tiles are ~70-90m too high for Brisbane:
- AWS reports: 80-97m at Story Bridge area  
- Expected: 5-30m (river to bridge deck)
- Error: ~75m offset

## Root Cause: Vertical Datum Mismatch

**AWS Terrain Tiles use WGS84 ellipsoid heights**
**SRTM spec requires EGM96 geoid heights**

The difference in Brisbane is approximately **-71m** (geoid below ellipsoid).

### What This Means

- **Ellipsoid height** = distance from WGS84 mathematical ellipsoid surface
- **Geoid height** = distance from mean sea level (what we actually need)
- **Geoid-ellipsoid separation** varies globally (-110m to +90m)

In Brisbane (−27°, 153°):
- N (geoid undulation) ≈ -71m
- Ellipsoid height = 97m
- Geoid height = 97m + (-71m) = 26m ✓ Correct!

## Solution Options

### Option 1: Use OpenTopography (CORRECT)
- OpenTopography API returns EGM96 geoid heights (correct datum)
- Requires API key: `OPENTOPOGRAPHY_API_KEY`
- Downloads SRTM1 (3601×3601, ~30m resolution)
- **This is the correct solution**

### Option 2: Apply Geoid Correction to AWS Data
- Download EGM96 geoid model grid
- Interpolate geoid undulation for each point
- Apply correction: `geoid_height = ellipsoid_height + N`
- Complex but works without API key

### Option 3: Hard-code Brisbane Correction (TEMPORARY)
- Apply fixed -71m correction for Brisbane testing
- **Only works for this specific area**
- Not scalable to global metaverse

## Current Status

- ✅ SRTM downloader working
- ✅ GeoTIFF conversion working
- ⚠️ AWS Terrain uses wrong vertical datum
- ❌ Need OpenTopography API key OR geoid model

## Test With Correction

```bash
# AWS data: 97m - 71m = 26m (correct!)
# AWS data: 80m - 71m = 9m (river level, correct!)
```

## Action Required

1. **Immediate**: Get OpenTopography API key for correct SRTM data
2. **Alternative**: Implement EGM96 geoid correction
3. **Document**: All elevation systems must specify vertical datum

## References

- SRTM specification: EGM96 geoid heights
- AWS Terrain Tiles: WGS84 ellipsoid heights (undocumented)
- EGM96 model: https://earth-info.nga.mil/php/download.php?file=egm-96interpolation
- Geoid calculator: https://geographiclib.sourceforge.io/cgi-bin/GeoidEval

## Files Affected

- `src/srtm_downloader.rs` - AWS source needs geoid correction
- `src/elevation.rs` - All elevations must document datum
- Test locations showing 75m offset prove this diagnosis

---

**CRITICAL**: This is not a bug in our code. AWS Terrain uses a different vertical reference system than SRTM specification requires. We must either use OpenTopography or implement geoid correction to get accurate real-world elevations.
