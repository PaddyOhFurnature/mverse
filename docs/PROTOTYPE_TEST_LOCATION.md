# Continuous Query Prototype - Test Location

**Validated:** 2026-02-16  
**Status:** Ready for Phase 1 implementation

## Location Details

**Coordinates:** 27°28'47.17"S, 153°2'0.91"E  
**Decimal:** -27.47976944°, 153.03358611°  
**Elevation:** 258m above sea level  
**Area:** Brisbane, Queensland, Australia (1.5km northeast of city center)

**ECEF Position (center):**
- X: -5,047,081.96m
- Y: 2,567,891.19m
- Z: -2,925,600.68m

## Test Area Bounds

**Size:** 200m × 200m × 100m deep

**GPS Bounds:**
- Min: (-27.480669°, 153.032686°)
- Max: (-27.478869°, 153.034486°)
- Approximate: 0.0018° = 200m at this latitude

## Features Present (User Confirmed)

Within 100-200m radius:
- ✅ **Houses** - Residential structures (tests interior spaces - priority #1)
- ✅ **Carpark** - Flat paved surface (tests material variation)
- ✅ **Cliff face** - Vertical terrain feature (tests steep geometry)
- ✅ **Elevation changes** - Hills/slopes (tests terrain generation)
- ✅ **Roads** - Linear features (tests long thin geometry)
- ✅ **River** - Water feature (tests material boundaries)

**Perfect test location** - has every feature type we need to validate.

## Reference Screenshot

User provided: `screenshot/Screenshot from 2026-02-16 13-24-26.png`
- View from Google Earth
- Heading: -178° (nearly south)
- Tilt: 60° (looking down at angle)
- Shows terrain variety and structure placement

## Data Availability

### SRTM Elevation Data
- Tile: S28E153 (covers this location)
- Resolution: 1 arc-second (~30m)
- Coverage: ✅ Available from NASA/USGS

### OpenStreetMap Data
- Coverage: ✅ Brisbane well-mapped
- Expected data:
  - Building footprints (houses)
  - Road network (primary, residential)
  - Natural features (river, cliff)
  - Landuse areas (residential, parking)

## Test Scenarios

### 1. Exterior Terrain
- Query hillside surface (arbitrary bounds, not chunk-aligned)
- Verify seamless across any boundary
- Compare vs chunk system

### 2. Interior Spaces (Priority)
- Query single house interior (5m³ room)
- Verify only that room loads (not whole neighborhood)
- Test door transitions (inside ↔ outside)
- Compare memory efficiency vs chunks

### 3. Linear Features
- Query road segment (long thin bounds)
- Verify efficient (not loading wide area)
- Test road-building intersection

### 4. Vertical Features
- Query cliff face (vertical terrain)
- Test steep slopes and overhangs
- Verify voxel generation on vertical surfaces

### 5. Material Boundaries
- River edge (water vs ground)
- Road surface (asphalt vs dirt)
- Building walls (structure vs terrain)

## Phase 1 Implementation Plan

### Week 1-2 Tasks
1. ✅ Validate location and data (DONE)
2. Implement `ContinuousWorld` with R-tree spatial index
3. Download and cache SRTM + OSM data for test bounds
4. Implement procedural generation for arbitrary AABB
5. Unit tests: query various bounds, verify no chunk awareness
6. Visualize: render test area to validate correctness

### Success Criteria Phase 1
- Can query any AABB within 200m test area
- Returns correct voxels from SRTM + OSM
- No chunk boundaries visible in data
- Generation works continuously across any bounds
- Basic visualization confirms terrain matches reference

## Notes

- Location chosen by user with perfect feature variety
- Real-world data available (SRTM + OSM)
- Small enough to iterate quickly (40,000 m² surface)
- Large enough to test real scenarios
- Has priority feature: interior spaces (houses)
- Can validate against Google Earth reference screenshot

**Ready to start Phase 1 implementation.**
