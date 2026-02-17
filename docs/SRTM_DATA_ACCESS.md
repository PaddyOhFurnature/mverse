# SRTM Data Access Research

**Last Updated:** 2026-02-17  
**Status:** Question 2 - ANSWERED

---

## ANSWER: We have THREE data sources available

### 1. **Local NAS Copy (PRIMARY - No rate limits)**

**Source:** Stanford Natural Capital Alliance  
**URL:** https://data.naturalcapitalalliance.stanford.edu/download/global/nasa-srtm-v3-1s/srtm-v3-1s.tif  
**Location:** User's NAS (download in progress)  
**Format:** GeoTIFF (single global file)  
**Resolution:** 1 arc-second (~30m at equator)  
**Coverage:** Global (60°N to 56°S)  

**Advantages:**
- No API rate limits
- No network dependency during development
- Entire dataset locally available
- Fast random access
- Single file (no tile management)

**Path:** `/mnt/nas/srtm-v3-1s.tif` (when download completes)

---

### 2. **OpenTopography API (FALLBACK - On-demand)**

**API Key:** `3e607de6969c687053f9e107a4796962`  
**Usage:** On-demand elevation queries when local data unavailable  
**Rate limits:** Unknown - need to check documentation  

**Export for testing:**
```bash
export OPENTOPOGRAPHY_API_KEY=3e607de6969c687053f9e107a4796962
```

**Use cases:**
- Fill gaps outside SRTM coverage (>60°N, <56°S)
- Higher resolution data for specific areas
- Cross-validation against local copy
- Development before NAS mount available

---

### 3. **NASA Earthdata (AUTHORITATIVE SOURCE)**

**Account:** User has Earthdata login  
**Usage:** Official source for SRTM, reference validation  
**Access:** Requires authentication  

**Use cases:**
- Validate local copy integrity
- Download specific tiles for testing
- Access metadata and accuracy specs
- Official documentation reference

---

## FILE FORMAT: GeoTIFF

**What is it:**
- Standard geospatial raster format
- TIFF image with geographic metadata
- Supports geographic projections
- Self-describing (contains coordinate system info)

**Structure:**
- Pixel values = elevation in meters
- Georeferenced (lat/lon → pixel coordinate mapping)
- Compressed or uncompressed
- May contain multiple bands (SRTM has 1: elevation)

**Coordinate system:**
- WGS84 geographic (lat/lon)
- Pixels aligned to arc-seconds
- Need to query: (lat, lon) → pixel (x, y) → elevation value

---

## RUST LIBRARIES FOR GEOTIFF

### Option 1: `gdal` (GDAL bindings)

**Crates.io:** https://crates.io/crates/gdal  
**Pros:**
- Industry standard (GDAL is THE geospatial library)
- Comprehensive format support
- Well-tested
- Python GDAL equivalent in Rust

**Cons:**
- Requires system GDAL installation (apt install libgdal-dev)
- Native dependency (complicates deployment)
- Large dependency tree
- Rust bindings may lag C++ GDAL releases

**Usage:**
```rust
use gdal::Dataset;
let dataset = Dataset::open("srtm.tif")?;
let rasterband = dataset.rasterband(1)?;
let elevation = rasterband.read_as::<f32>((x, y), (1, 1))?;
```

---

### Option 2: `geotiff` (Pure Rust)

**Crates.io:** https://crates.io/crates/geotiff  
**Pros:**
- Pure Rust (no system dependencies)
- Lightweight
- Cross-platform (easier deployment)

**Cons:**
- Less mature than GDAL
- May not support all GeoTIFF variants
- Smaller ecosystem
- Need to verify supports our specific file

**Need to research:**
- Can it read Stanford SRTM file?
- Does it handle geographic → pixel coordinate conversion?
- Performance compared to GDAL?

---

### Option 3: `tiff` + manual georeferencing

**Crates.io:** https://crates.io/crates/tiff  
**Approach:**
- Use generic TIFF reader
- Manually parse GeoTIFF tags
- Implement lat/lon → pixel conversion ourselves

**Pros:**
- Pure Rust
- Full control over implementation
- Minimal dependencies

**Cons:**
- More work (need to implement projection math)
- Risk of bugs in georeferencing
- Reinventing wheel (GDAL/geotiff already do this)

**Verdict:** Probably not worth it unless other options fail

---

## TEST LOCATIONS FOR SRTM VALIDATION

### Kangaroo Point Cliffs (Primary test case)

**GPS:** -27.4775°S, 153.0355°E  
**Pixel calculation (1 arc-second):**
- Latitude:  -27.4775° × 3600 = -98,919 arc-seconds
- Longitude: 153.0355° × 3600 = 550,927.8 arc-seconds

**Expected elevation:**
- River level: ~0-2m (Brisbane River mean sea level)
- Cliff top: ~20-30m (need to verify from map)

**Validation:**
- Query SRTM at GPS coordinates
- Compare to Google Earth elevation
- Visual inspection: cliff should show slope change

---

## NEXT STEPS

### Immediate (Phase 0 research):

1. ✅ Identify data source (Stanford NAS copy)
2. ✅ Identify format (GeoTIFF)
3. ⏳ Choose Rust library (GDAL vs geotiff)
4. ⏳ Wait for NAS download to complete
5. ⏳ Test file accessibility from project

### Testing (Phase 2):

1. Mount NAS or copy test tile locally
2. Add chosen library to Cargo.toml
3. Write test: Open file successfully
4. Write test: Query elevation at known point
5. Write test: Validate against reference data
6. Implement bilinear interpolation (query between pixels)

---

## LIBRARY DECISION CRITERIA

**Evaluate:**
1. Can it read the Stanford SRTM file? (test with actual file)
2. API complexity (simple query: lat/lon → elevation)
3. Dependencies (prefer pure Rust)
4. Maintenance (recent updates?)
5. Performance (can query thousands of points/second?)

**Decision method:**
- Try both `gdal` and `geotiff` with actual file
- Compare code complexity
- Measure query performance
- Choose based on results, not assumptions

---

## NOTES

- Stanford file is SINGLE global GeoTIFF (unusual - most SRTM is tiled)
- This simplifies code (no tile management needed)
- But file is HUGE (~15-20GB for global 1-arc-second)
- Need to verify random access is efficient (GeoTIFF supports this)
- May need to extract regional tiles for development work

**Open questions:**
- File size of srtm-v3-1s.tif?
- Is it compressed (LZW, deflate)?
- Internal tiling structure (GDAL block size)?
- Memory mapping possible?

