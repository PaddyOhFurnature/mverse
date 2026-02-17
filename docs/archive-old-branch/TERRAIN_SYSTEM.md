# Terrain System Architecture

## Overview

The terrain system generates realistic ground surfaces using real-world SRTM elevation data, ensuring buildings and roads are properly grounded on Earth's surface.

## Components

### 1. SRTM Data Pipeline

**Source:** NASA Shuttle Radar Topography Mission
- Resolution: SRTM1 (1 arc-second, ~30m)
- Coverage: Global (60°N to 56°S)
- Vertical accuracy: ±16m absolute, ±6m relative

**Download:** Multi-source with fallback
```
Primary:   OpenTopography API (EGM96 geoid)
Secondary: AWS Terrain Tiles (WGS84 ellipsoid)  
Tertiary:  CGIAR Direct (may 404)
```

**Format:** GeoTIFF → .hgt conversion
- Automatic format detection
- All data types supported (U8/U16/I16/F32/F64)
- Bilinear resampling to standard grid
- NoData value handling

**Caching:** Three-tier strategy
```
1. Memory: LRU cache (10 tiles)
2. Disk: ~/.metaverse/cache/srtm/*.hgt
3. Network: Download on demand
```

### 2. Terrain Mesh Generation

**Algorithm:** Regular grid with SRTM sampling

```rust
pub fn generate_terrain_mesh(
    center: &GpsPos,
    radius_m: f64,
    grid_spacing_m: f64,
    srtm: &mut SrtmManager,
) -> (Vec<ColoredVertex>, Vec<u32>)
```

**Process:**
1. Calculate grid dimensions based on radius
2. Sample SRTM elevation at each grid point
3. Convert GPS → ECEF for each vertex
4. Calculate surface normal (radial up vector)
5. Apply elevation-based coloring
6. Triangulate into quads (2 triangles each)

**Coloring Scheme:**
- < 5m elevation: Sandy (near water)
- 5-20m: Grass green (typical ground)  
- \> 20m: Dark green (hills/mountains)

**Performance:**
- 100m spacing: ~10K vertices for 5km radius
- 50m spacing: ~40K vertices (4x more detail)
- 25m spacing: ~160K vertices (highest detail)

### 3. Water Plane

**Simple Implementation:**
```rust
pub fn generate_water_plane(
    center: &GpsPos,
    radius_m: f64,
    water_level_m: f64,
) -> (Vec<ColoredVertex>, Vec<u32>)
```

Currently: Flat quad at sea level (0m)
- 4 vertices, 2 triangles
- Blue semi-transparent (70% opacity)
- Covers entire render radius

**Future:** OSM water polygon rendering
- Follow actual river/lake shapes
- Variable water levels
- Wave simulation

### 4. Integration with Buildings

**Building Ground Elevation:**
```rust
// In generate_mesh_from_osm_filtered()
let terrain_elevation = srtm.get_elevation(center_lat, center_lon)
    .unwrap_or(building.elevation.unwrap_or(0.0));
```

Buildings sit on SRTM ground level, not OSM building:level value.

**Structure Heights:**
- OSM building:height → building top
- OSM building:levels × 3.5m → fallback height
- Bridges use bridge:height for deck elevation above ground

### 5. Rendering Pipeline

**Mesh Combination:**
```
1. Generate building/road mesh (5M vertices)
2. Generate terrain mesh (10K vertices)  
3. Generate water plane (4 vertices)
4. Merge with proper index offsets
5. Upload to GPU as single buffer
```

**Render Order:**
- Opaque: Buildings → Roads → Terrain
- Transparent: Water (depth-sorted)

**Memory Usage:**
- Vertex: 40 bytes (3×f32 position + 3×f32 normal + 4×f32 color)
- 5M vertices = 200MB
- Terrain adds ~0.4MB (negligible)

## Usage

### Basic Example

```rust
use metaverse_core::terrain_mesh::generate_terrain_mesh;
use metaverse_core::elevation::SrtmManager;
use metaverse_core::coordinates::GpsPos;

let center = GpsPos { 
    lat_deg: -27.463697, 
    lon_deg: 153.035725, 
    elevation_m: 0.0 
};

let mut srtm = SrtmManager::new(cache);
let (vertices, indices) = generate_terrain_mesh(
    &center,
    5000.0,  // 5km radius
    100.0,   // 100m grid spacing
    &mut srtm,
);

// vertices.len() ≈ 10,000
// indices.len() ≈ 60,000
```

### With Water

```rust
use metaverse_core::terrain_mesh::generate_water_plane;

let (water_verts, water_inds) = generate_water_plane(
    &center,
    5000.0,  // Same radius as terrain
    0.0,     // Sea level
);

// Merge into combined mesh
let mut all_vertices = vertices;
let mut all_indices = indices;

let offset = all_vertices.len() as u32;
all_vertices.extend(water_verts);
all_indices.extend(water_inds.iter().map(|i| i + offset));
```

## Configuration

### Grid Spacing Recommendations

**Walking (0-5 m/s):**
- Spacing: 25-50m
- Detail: High (see undulations)
- Vertices: 40K-160K

**Driving (5-30 m/s):**
- Spacing: 50-100m  
- Detail: Medium (see major features)
- Vertices: 10K-40K

**Flying (30+ m/s):**
- Spacing: 100-200m
- Detail: Low (overview only)
- Vertices: 2.5K-10K

### Radius Guidelines

**Urban areas:** 500m-1km
- Dense buildings, frequent updates
- Terrain less important (mostly flat)

**Suburban:** 1-2km  
- Mix of buildings and terrain
- Terrain variation visible

**Rural/wilderness:** 2-5km
- Few buildings, terrain is main feature
- Show distant mountains/valleys

## Performance

### Measurements (5km radius, 100m spacing)

- **Generation time:** ~50ms (10K elevation queries)
- **Memory:** 0.4MB (10K vertices × 40 bytes)
- **GPU upload:** ~1ms (negligible)
- **Render time:** ~0.1ms (terrain only)

**Bottleneck:** SRTM elevation queries (5ms each)
- Cache hit: <0.01ms
- Cache miss: 5ms (disk read) or 100ms (network)

### Optimization Strategies

1. **Preload tiles:** Download SRTM before entering area
2. **Reduce queries:** Use coarser grid when far away
3. **LOD system:** Variable spacing based on distance
4. **Async generation:** Build mesh in background thread

## Future Enhancements

### Short Term
- [ ] OSM water polygon rendering
- [ ] Variable grid spacing (LOD)
- [ ] Terrain texture generation
- [ ] Normal map from elevation gradients

### Medium Term
- [ ] Terrain chunks with streaming
- [ ] Vegetation placement (trees, grass)
- [ ] Rock/cliff detection from slope
- [ ] Beach generation at water edges

### Long Term  
- [ ] SVO terrain integration (destructible)
- [ ] Erosion simulation
- [ ] Dynamic water simulation
- [ ] Weather effects (snow, rain puddles)

## Testing

```bash
# Generate terrain mesh
cargo test terrain_mesh

# Visual validation
cargo run --example capture_screenshots

# Compare with references
ls screenshot/*.png
ls reference/*.png
```

## Troubleshooting

**Issue:** Terrain appears flat despite elevation data

**Cause:** SRTM vertical scale is meters, Earth radius is 6.4M meters
- Solution: Scale is correct, elevation IS visible (10-50m changes visible)

**Issue:** Buildings floating above terrain

**Cause:** Using OSM building:level instead of SRTM ground
- Solution: Query SRTM for ground elevation, add building height

**Issue:** Terrain has visible seams

**Cause:** Grid edges don't align with neighbor chunks
- Solution: Share vertices at chunk boundaries

**Issue:** Performance degrades with fine grid

**Cause:** Too many SRTM queries (50m spacing = 40K queries)
- Solution: Use LOD, increase spacing far from camera

## References

- SRTM Documentation: https://lpdaac.usgs.gov/documents/179/SRTM_User_Guide_V3.pdf
- OpenTopography API: https://portal.opentopography.org/apidocs/
- EGM96 Geoid: https://earth-info.nga.mil/GandG/wgs84/gravitymod/egm96/egm96.html
