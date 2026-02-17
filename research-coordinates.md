# Research: Rust Geodetic Coordinate Libraries

**Date:** 2026-02-17  
**Question:** What Rust library handles WGS84 ↔ ECEF conversion?

## Candidates Found

### 1. geoconv
- **Status:** Active, well-tested
- **Features:**
  - WGS84 geodetic (lat/lon/elevation) ↔ ECEF (x/y/z)
  - Also supports ENU (East-North-Up) and AER (Azimuth-Elevation-Range)
  - Uses `f64` for precision
- **API Example:**
  ```rust
  use geoconv::{Lle, Wgs84, Degrees, Meters};
  let lle = Lle::<Wgs84>::new(Degrees::new(42.0), Degrees::new(-71.0), Meters::new(10.0));
  let ecef = lle.to_xyz();
  ```
- **Docs:** https://docs.rs/geoconv
- **Pros:**
  - Simple API
  - Type-safe units (Degrees, Meters)
  - Well-documented
- **Cons:**
  - Explicitly not suitable for "mission-critical navigation" (per docs)
  - But fine for our use case (terrain generation)

### 2. map_3d
- **Status:** Active, inspired by Python's pymap3d
- **Features:**
  - Geodetic ↔ ECEF
  - ECEF ↔ ENU, NED, AER
  - Multiple ellipsoid support (WGS84 default)
  - No external dependencies
  - Uses `f64`
- **GitHub:** https://github.com/gberrante/map_3d
- **Pros:**
  - No dependencies (lean)
  - Multiple ellipsoids (if we need alternatives to WGS84)
  - Proven Python API design
- **Cons:**
  - Less mature than geoconv?
  - Need to check crates.io publish status

### 3. nav_types
- **Status:** Active
- **Features:**
  - WGS84, ECEF, ENU, NED conversions
  - Built on `nalgebra` (vector math library)
  - Type-safe coordinate system structs
- **API Example:**
  ```rust
  use nav_types::{ECEF, WGS84};
  let geo = WGS84::from_degrees_and_meters(59.95, 10.75, 0.0);
  let ecef: ECEF<_> = geo.into();
  ```
- **Docs:** https://docs.rs/nav-types
- **Pros:**
  - nalgebra integration (if we use nalgebra elsewhere)
  - Type-safe conversions (compile-time safety)
  - Active development
- **Cons:**
  - Dependency on nalgebra (heavier)
  - May be overkill if we just need coordinate conversion

### 4. geoconvert
- **Status:** Active, port of C++ GeographicLib
- **Features:**
  - LatLon (WGS84) to UTM, UPS, MGRS
  - **NO ECEF support** ❌
- **Verdict:** Not suitable for our needs

### 5. coord_transforms
- **Status:** ARCHIVED (read-only, no longer maintained)
- **Features:**
  - Geodetic ↔ ECEF
  - Uses nalgebra
- **Verdict:** Avoid (unmaintained)

## Evaluation Criteria

| Criterion | geoconv | map_3d | nav_types |
|-----------|---------|---------|-----------|
| **Active maintenance** | ✅ Yes | ✅ Yes | ✅ Yes |
| **WGS84 → ECEF** | ✅ Yes | ✅ Yes | ✅ Yes |
| **ECEF → WGS84** | ✅ Yes | ✅ Yes | ✅ Yes |
| **f64 precision** | ✅ Yes | ✅ Yes | ✅ Yes |
| **Type safety** | ✅ Strong (Units) | ⚠️ Standard | ✅ Strong (Types) |
| **Dependencies** | Few | None | nalgebra (heavy) |
| **Documentation** | ✅ Good | ⚠️ GitHub only | ✅ Good |
| **Simplicity** | ✅ Simple | ✅ Simple | ⚠️ More complex |

## Decision Criteria

**What we need:**
1. WGS84 ↔ ECEF conversion (both directions)
2. High precision (f64)
3. Reliable (well-tested)
4. Simple API (don't need complex features)
5. Maintained (not abandoned)

**What we DON'T need:**
- Mission-critical navigation accuracy (geoconv disclaimer is OK)
- Multiple ellipsoid support (WGS84 is sufficient)
- Complex coordinate systems (just WGS84 and ECEF)

## Recommendation

**Choice: `geoconv`**

**Rationale:**
1. ✅ Meets all requirements (WGS84 ↔ ECEF, f64, maintained)
2. ✅ Simple API with type-safe units
3. ✅ Well-documented on docs.rs
4. ✅ Active on crates.io
5. ✅ Lightweight (minimal dependencies)
6. ⚠️ "Not mission-critical" disclaimer is acceptable
   - We're generating terrain, not launching missiles
   - Accuracy to within meters is fine
   - Can validate with test suite

**Alternative if geoconv fails testing: `map_3d`**
- No dependencies (even cleaner)
- More feature-complete (if we need ENU/NED later)

## Next Steps

1. Add `geoconv` to Cargo.toml
2. Write test: (0°, 0°, 0m) → ECEF → GPS
3. Write test: Known points with reference ECEF values
4. Validate round-trip accuracy
5. If accuracy insufficient, try map_3d

## Validation Test Points

**Test 1: Origin (Equator at Prime Meridian)**
- Input: (0° lat, 0° lon, 0m elevation)
- Expected ECEF: (6,378,137m, 0m, 0m) [semi-major axis of WGS84]

**Test 2: North Pole**
- Input: (90° lat, any lon, 0m)
- Expected ECEF: (0m, 0m, 6,356,752m) [semi-minor axis]

**Test 3: Equator at 90°E**
- Input: (0° lat, 90° lon, 0m)
- Expected ECEF: (0m, 6,378,137m, 0m)

**Test 4: Kangaroo Point Cliffs**
- Input: (-27.4775° lat, 153.0355° lon, 30m)
- Expected ECEF: Calculate and validate round-trip

**Test 5: Antipodal Points**
- Two points on opposite sides of Earth
- Distance should equal Earth diameter

**Acceptance:**
- Round-trip error < 1mm (per TECH_SPEC.md precision requirements)
- All test points pass
- Conversion time acceptable (< 1μs per point)

