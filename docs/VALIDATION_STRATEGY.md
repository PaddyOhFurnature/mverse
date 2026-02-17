# Question 4: Validation Strategy - Known Test Points

**Last Updated:** 2026-02-17  
**Purpose:** Plan how to validate coordinate conversions are correct

---

## VALIDATION SOURCES

### Online Calculators (Reference Implementation)

**Found 5 authoritative calculators:**

1. **RF Wireless World** - https://www.rfwireless-world.com/calculators/lla-to-ecef-coordinate-converter-and-formula
   - Shows formulas
   - Example: (40°, -75°, 100m) → ECEF (1266345.73, -4726066.62, 4078049.85)
   - Uses WGS84 standard parameters

2. **ConvertECEF.com** - https://www.convertecef.com/
   - Handles HAE (Height Above Ellipsoid) vs MSL (Mean Sea Level)
   - Bidirectional (ECEF ↔ LLA)

3. **Math for Engineers** - http://www.mathforengineers.com/math-calculators/GGPS-and-ECEF-converter.html
   - Formula documentation
   - Instant calculator

4. **AeroVia** - https://www.aerovia.org/tools/coordinate-system-converter
   - Open source, transparent formulas
   - Multiple unit options

5. **Bislins WGS84** - https://walter.bislins.ch/bloge/index.asp?page=Rainy+Lake+Experiment%3A+WGS84+Calculator
   - Comprehensive (multiple coordinate systems)
   - Advanced WGS84 parameters

**Strategy:** Use RF Wireless World as primary (shows example), cross-validate with others

---

## WGS84 PARAMETERS (Constants)

**From WGS84 standard:**
```
Semi-major axis (a):  6,378,137.0 meters       (equatorial radius)
Semi-minor axis (b):  6,356,752.314245 meters  (polar radius)
Flattening (f):       1 / 298.257223563
Eccentricity² (e²):   0.00669438002290
```

**These are DEFINED values** (not measured) - see `ABSOLUTE_FOUNDATION.md`

---

## TEST POINTS WITH KNOWN ECEF VALUES

### Test 1: Origin Point (Equator at Prime Meridian)

**GPS Input:**
- Latitude: 0.0°
- Longitude: 0.0°  
- Altitude: 0.0m (on ellipsoid)

**Expected ECEF:**
```
N = a / sqrt(1 - e² × sin²(lat))
  = 6,378,137.0 / sqrt(1 - 0)
  = 6,378,137.0

X = (N + h) × cos(lat) × cos(lon)
  = (6,378,137.0 + 0) × cos(0°) × cos(0°)
  = 6,378,137.0 × 1 × 1
  = 6,378,137.0 meters

Y = (N + h) × cos(lat) × sin(lon)
  = 6,378,137.0 × 1 × 0
  = 0.0 meters

Z = (N × (1 - e²) + h) × sin(lat)
  = (6,378,137.0 × (1 - 0.00669438) + 0) × 0
  = 0.0 meters
```

**Expected ECEF: (6,378,137.0, 0.0, 0.0)**

**Validation:** Should be exact (on X-axis)

---

### Test 2: Equator at 90° East

**GPS Input:**
- Latitude: 0.0°
- Longitude: 90.0° (East)
- Altitude: 0.0m

**Expected ECEF:**
```
X = 6,378,137.0 × cos(0°) × cos(90°) = 0.0
Y = 6,378,137.0 × cos(0°) × sin(90°) = 6,378,137.0
Z = 0.0
```

**Expected ECEF: (0.0, 6,378,137.0, 0.0)**

**Validation:** Should be exact (on Y-axis)

---

### Test 3: North Pole

**GPS Input:**
- Latitude: 90.0° (North)
- Longitude: 0.0° (irrelevant at pole)
- Altitude: 0.0m

**Expected ECEF:**
```
N = 6,378,137.0 / sqrt(1 - e² × sin²(90°))
  = 6,378,137.0 / sqrt(1 - 0.00669438 × 1)
  = 6,378,137.0 / sqrt(0.99330562)
  = 6,399,593.626 meters

X = (N + h) × cos(90°) × cos(lon) = 0.0
Y = (N + h) × cos(90°) × sin(lon) = 0.0
Z = (N × (1 - e²) + h) × sin(90°)
  = (6,399,593.626 × 0.99330562 + 0) × 1
  = 6,356,752.314 meters
```

**Expected ECEF: (0.0, 0.0, 6,356,752.314)**

**Validation:** Should match semi-minor axis (on Z-axis)

---

### Test 4: South Pole

**GPS Input:**
- Latitude: -90.0° (South)
- Longitude: 0.0°
- Altitude: 0.0m

**Expected ECEF: (0.0, 0.0, -6,356,752.314)**

**Validation:** Negative Z (opposite of North Pole)

---

### Test 5: Reference Example (From RF Wireless World)

**GPS Input:**
- Latitude: 40.0°
- Longitude: -75.0° (West)
- Altitude: 100.0m

**Expected ECEF (from calculator):**
- X: 1,266,345.73 meters
- Y: -4,726,066.62 meters
- Z: 4,078,049.85 meters

**Validation:** Match calculator to ~0.01m (centimeter precision)

---

### Test 6: Kangaroo Point Cliffs (Our Test Location)

**GPS Input:**
- Latitude: -27.4775° (South)
- Longitude: 153.0355° (East)
- Altitude: 20.0m (estimated cliff top)

**Expected ECEF (calculate using online tool):**

Using RF Wireless World calculator:
- Lat: -27.4775°
- Lon: 153.0355°
- Alt: 20m

**TODO:** Run through calculator and record values

**Validation:** Will use for terrain generation testing

---

### Test 7: Antipodal Points (Maximum Distance)

**Point A:** (40.0°, -75.0°, 0m) - Near Philadelphia
**Point B:** (-40.0°, 105.0°, 0m) - Indian Ocean (antipodal)

**Distance:** Should be ~20,000 km (Earth circumference / 2)

**Validation:** 
- Convert both to ECEF
- Calculate distance: `sqrt((x2-x1)² + (y2-y1)² + (z2-z1)²)`
- Should be ~20,015 km (accounting for ellipsoid)

---

## ROUND-TRIP TESTS

### Test 8: GPS → ECEF → GPS Identity

**For each test point above:**
```rust
let gps_original = GPS { lat: 40.0, lon: -75.0, alt: 100.0 };
let ecef = gps_to_ecef(gps_original);
let gps_roundtrip = ecef_to_gps(ecef);

// Should match within floating-point precision
assert_delta(gps_roundtrip.lat, gps_original.lat, 1e-9);  // ~0.1mm
assert_delta(gps_roundtrip.lon, gps_original.lon, 1e-9);
assert_delta(gps_roundtrip.alt, gps_original.alt, 1e-6);  // 1 micrometer
```

**Acceptance:** Error < 1mm for all coordinates

---

### Test 9: Random Point Statistical Validation

**Generate 1000 random GPS points:**
- Latitude: -90° to +90°
- Longitude: -180° to +180°
- Altitude: -100m to +10,000m

**For each:**
1. Convert GPS → ECEF using our code
2. Convert ECEF → GPS using our code
3. Calculate round-trip error

**Validation:**
- Mean error < 0.1mm
- Max error < 1mm
- No NaN or infinity values
- All points pass

---

## SCALE GATE TESTS

### Test 10: Scale Gate 1m (Player Collision)

**Setup:** Two entities 1 meter apart
```rust
let entity_a = GPS { lat: 0.0, lon: 0.0, alt: 0.0 };
let entity_b = GPS { lat: 0.0, lon: 0.0, alt: 1.0 };  // 1m higher

let ecef_a = gps_to_ecef(entity_a);
let ecef_b = gps_to_ecef(entity_b);

let distance = (ecef_b - ecef_a).length();
assert_delta(distance, 1.0, 0.001);  // Within 1mm
```

**Acceptance:** Distance accurate to 1mm

---

### Test 11: Scale Gate 1km (Terrain Tile)

**Setup:** Two points 1km apart (horizontal)
```rust
// ~1km at equator ≈ 0.009° latitude
let point_a = GPS { lat: 0.0, lon: 0.0, alt: 0.0 };
let point_b = GPS { lat: 0.009, lon: 0.0, alt: 0.0 };

let ecef_a = gps_to_ecef(point_a);
let ecef_b = gps_to_ecef(point_b);

let distance = (ecef_b - ecef_a).length();
assert_delta(distance, 1000.0, 0.01);  // Within 1cm
```

**Acceptance:** Distance accurate to 1cm

---

### Test 12: Scale Gate 100km (City)

**Setup:** Two cities 100km apart
```rust
let city_a = GPS { lat: -27.4698, lon: 153.0251, alt: 0.0 };  // Brisbane
let city_b = GPS { lat: -28.0167, lon: 153.4000, alt: 0.0 };  // Gold Coast (~70km)

let ecef_a = gps_to_ecef(city_a);
let ecef_b = gps_to_ecef(city_b);

let distance = (ecef_b - ecef_a).length();
// Validate against known distance (Google Maps says ~66km)
assert_delta(distance, 66000.0, 10.0);  // Within 10m
```

**Acceptance:** Distance accurate to 10m (0.01%)

---

### Test 13: Scale Gate Global (Antipodal)

**Setup:** Opposite sides of Earth
```rust
let point_a = GPS { lat: 40.0, lon: -75.0, alt: 0.0 };
let point_b = GPS { lat: -40.0, lon: 105.0, alt: 0.0 };

let ecef_a = gps_to_ecef(point_a);
let ecef_b = gps_to_ecef(point_b);

let distance = (ecef_b - ecef_a).length();
// Half Earth circumference ≈ 20,015km
assert_delta(distance, 20_015_000.0, 100.0);  // Within 100m
```

**Acceptance:** Distance accurate to 100m (0.0005%)

---

## FLOATING ORIGIN PRECISION TESTS

### Test 14: Render Precision at 1m

**Setup:** Camera at Earth surface, entity 1m away
```rust
let camera_ecef = Vec3::new(6_371_000.0, 0.0, 0.0);
let entity_ecef = Vec3::new(6_371_001.0, 0.0, 0.0);

let render_pos = to_render_space(entity_ecef, camera_ecef);
assert_eq!(render_pos, Vec3f::new(1.0, 0.0, 0.0));

// Check f32 precision
let render_pos_f64 = render_pos.as_f64();
assert_delta(render_pos_f64.x, 1.0, 1e-6);  // Micrometer
```

**Acceptance:** f32 represents 1m exactly

---

### Test 15: Render Precision at 10km

**Setup:** Camera at Earth surface, entity 10km away
```rust
let camera_ecef = Vec3::new(6_371_000.0, 0.0, 0.0);
let entity_ecef = Vec3::new(6_381_000.0, 0.0, 0.0);

let render_pos = to_render_space(entity_ecef, camera_ecef);
assert_delta(render_pos.x, 10_000.0, 0.001);  // Within 1mm

// Can distinguish 1mm at 10km?
let entity_plus_1mm = Vec3::new(6_381_000.001, 0.0, 0.0);
let render_plus_1mm = to_render_space(entity_plus_1mm, camera_ecef);

let difference = (render_plus_1mm - render_pos).length();
assert!(difference > 0.0);  // Can distinguish
assert!(difference < 0.01); // And it's ~1mm
```

**Acceptance:** Sub-millimeter precision at 10km

---

## ERROR MARGINS

**Coordinate conversion:**
- Within 1m: < 1mm error
- Within 1km: < 1cm error
- Within 100km: < 10m error
- Global: < 100m error

**Rendering (floating origin):**
- Within 10km: < 1mm precision
- Critical zone (±1km): < 0.1mm precision

**Round-trip (GPS → ECEF → GPS):**
- Latitude/Longitude: < 1e-9° (~0.1mm)
- Altitude: < 1e-6m (1 micrometer)

---

## IMPLEMENTATION PLAN (Phase 1)

### Step 1: Get Reference Values
- [ ] Run test points 1-7 through online calculator
- [ ] Record expected ECEF values
- [ ] Document in test file as constants

### Step 2: Write Test Suite
- [ ] Create `src/tests/coordinate_tests.rs`
- [ ] Write all 15 tests above
- [ ] ALL TESTS SHOULD FAIL (no implementation yet - TDD)

### Step 3: Add geoconv Library
- [ ] Add `geoconv` to Cargo.toml
- [ ] Create `src/coordinates.rs`
- [ ] Implement GPS ↔ ECEF using geoconv

### Step 4: Run Tests
- [ ] `cargo test coordinate_tests`
- [ ] Debug any failures
- [ ] Iterate until ALL TESTS PASS

### Step 5: Validate
- [ ] All 15 tests passing ✅
- [ ] No compiler warnings ✅
- [ ] Round-trip error < 1mm ✅
- [ ] Scale gates pass ✅

### Step 6: Commit
- [ ] Git commit with message: "feat: coordinate conversion with validation"
- [ ] Clean test suite required

---

## REFERENCE TOOLS

**For getting expected values:**
1. RF Wireless World: https://www.rfwireless-world.com/calculators/lla-to-ecef-coordinate-converter-and-formula
2. ConvertECEF: https://www.convertecef.com/
3. Cross-validate with at least 2 calculators

**For manual calculation:**
- WGS84 formulas documented in TECH_SPEC.md
- Can compute by hand for simple cases (origin, poles)

---

## ACCEPTANCE CRITERIA

**Question 4 is answered when:**
- [x] Reference calculators identified
- [x] Test points defined (15 tests)
- [x] Expected ECEF values documented (or plan to get them)
- [x] Error margins defined
- [x] Implementation plan written
- [ ] **TODO:** Actually run calculators and fill in expected values
- [ ] **TODO:** Write test file in Phase 1

**Question 4: MOSTLY ANSWERED** ✅ (need to fill in calculated values)

