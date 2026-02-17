# Coordinate System Scale Evaluation

**Date:** 2026-02-17  
**Question:** Can `geoconv` (f64 ECEF) handle our scale and detail requirements?

---

## The Scale Challenge

**Horizontal scale:**
- Earth circumference: ~40,075 km = 40,075,000 meters
- Maximum distance between two points: ~20,037 km (half circumference)
- ECEF coordinates: Up to ±6,378,137 meters from origin

**Vertical scale:**
- Ocean depth: -11,034m (Mariana Trench)
- Land elevation: +8,849m (Mt. Everest)
- Atmosphere: Up to 100,000m (Karman line, edge of space)
- Total vertical range: ~111,000 meters

**Detail requirements:**
- Player height: 1.7m
- Player step: ~0.5m
- Small objects: 0.1m (10cm - rock, grass tuft)
- Collision precision: 0.01m (1cm minimum)
- Visual detail: 0.001m (1mm for close inspection)

---

## f64 (Double Precision) Capabilities

**IEEE 754 double precision:**
- 64 bits total
- 1 sign bit
- 11 exponent bits
- 52 mantissa bits (53 with implicit leading 1)

**Precision at different magnitudes:**
```
At 1 meter:      precision = 2^-52 ≈ 2.22 × 10^-16 m ≈ 0.000000000000000222 m
At 1,000 meters: precision ≈ 2.22 × 10^-13 m ≈ 0.000000000000222 m
At 1,000,000 m:  precision ≈ 2.22 × 10^-10 m ≈ 0.000000000222 m (0.222 nanometers!)
At 10,000,000 m: precision ≈ 2.22 × 10^-9 m ≈ 0.00000000222 m (2.22 nanometers!)
```

**At Earth's center (6.4 million meters from origin):**
```
Max ECEF value: ~6,400,000 meters
Precision: ~2^-52 × 6,400,000 ≈ 1.4 × 10^-9 meters ≈ 1.4 nanometers
```

**Conclusion:** f64 has INSANE precision - nanometers at Earth-scale distances!

---

## Our Requirements vs f64 Precision

| Requirement | Size (meters) | f64 Precision @ 6.4M meters | Ratio | ✓/✗ |
|-------------|---------------|----------------------------|-------|-----|
| Visual detail (1mm) | 0.001 | 1.4 × 10^-9 | 714,000× better | ✅ |
| Collision (1cm) | 0.01 | 1.4 × 10^-9 | 7,140,000× better | ✅ |
| Small objects (10cm) | 0.1 | 1.4 × 10^-9 | 71,400,000× better | ✅ |
| Player step (0.5m) | 0.5 | 1.4 × 10^-9 | 357,000,000× better | ✅ |
| Player height (1.7m) | 1.7 | 1.4 × 10^-9 | 1.2 billion× better | ✅ |

**f64 is MASSIVELY overkill for our needs. We could work at atomic scale if we wanted.**

---

## But Wait - What About Rendering?

**Problem:** GPU shaders use f32 (32-bit float), not f64

**f32 precision:**
- 32 bits total
- 1 sign bit
- 8 exponent bits  
- 23 mantissa bits (24 with implicit leading 1)

**Precision at Earth scale:**
```
At 6,400,000 meters:
Precision ≈ 2^-23 × 6,400,000 ≈ 0.76 meters
```

**That's a PROBLEM!**

If we send ECEF coordinates directly to GPU:
- At Earth scale, f32 only has ~0.76m precision
- We need 0.01m precision (1cm for collision)
- We're 76× too coarse!

---

## The Solution: Floating Origin (Already in TECH_SPEC.md!)

**From TECH_SPEC.md Section 1.5:**

> The renderer uses a **floating origin** technique:
> - Camera is always at or near (0, 0, 0) in render space
> - The world is translated relative to the camera
> - This prevents GPU float32 precision loss for distant geometry
> - Implementation: subtract camera ECEF from all entity ECEF before converting to render coords

**How it works:**

1. **Store positions in f64 ECEF** (absolute world coordinates)
2. **Camera position in f64 ECEF** (where player is looking from)
3. **For rendering:**
   ```rust
   // Convert to camera-relative coordinates (f64)
   relative_pos = entity_ecef - camera_ecef;
   
   // NOW relative_pos is SMALL (within a few km of camera)
   // At 1km distance: f32 precision = 0.00012m = 0.12mm ✅
   // At 10km distance: f32 precision = 0.0012m = 1.2mm ✅
   
   // Convert to f32 for GPU
   render_pos = relative_pos as f32;
   ```

4. **Result:** 
   - Close objects (<1km): sub-millimeter precision
   - Far objects (10km): millimeter precision
   - Distant objects (100km): centimeter precision (still acceptable for distant mountains)

---

## Scale Test Cases

### Test 1: Player Standing Still
- Camera ECEF: (−4,648,342.5, 2,560,198.3, −2,929,618.7) [Kangaroo Point]
- Rock ECEF: (−4,648,342.8, 2,560,198.5, −2,929,618.6) [0.5m away]
- Relative: (−0.3, 0.2, 0.1)
- f32 precision: < 0.00001m = 0.01mm ✅

### Test 2: Looking at Distant Mountain
- Camera ECEF: (−4,648,342, 2,560,198, −2,929,618) [Kangaroo Point]  
- Mountain ECEF: (−4,638,342, 2,570,198, −2,919,618) [~17km away]
- Relative: (10,000, 10,000, 10,000)
- f32 precision: ~0.001m = 1mm ✅ (mountain doesn't need sub-mm)

### Test 3: Two Players on Opposite Sides of Earth
- Player A ECEF: (6,378,137, 0, 0)
- Player B ECEF: (−6,378,137, 0, 0) [antipodal]
- Player A camera relative to B: (−12,756,274, 0, 0)
- f32 precision: ~1.5m ⚠️ (but Player B is 12,700km away - we don't render that far!)

**Rendering distance limits:**
- Realistic horizon: ~5km on ground (standing height)
- From airplane (10km up): ~357km horizon
- From space (100km up): ~1,130km horizon
- Atmospheric fade should hide anything >100km anyway

**At 100km distance:**
- f32 precision: ~0.012m = 1.2cm ✅ (acceptable for distant terrain)

---

## Chunk-Local Coordinates (Alternative)

**TECH_SPEC.md also mentions chunk-local coordinates:**

> **Chunk-Local Cartesian** — Per-chunk rendering/simulation
> - Origin: centre of the chunk's surface patch
> - Axes: tangent to sphere surface at chunk centre (East, Up, North)
> - Units: metres (f32 — sufficient for <500m chunks)

**How this works:**
- Each chunk (e.g., 256m × 256m) has its own local (0,0,0)
- All positions within chunk stored as f32 offsets from chunk origin
- At 256m scale: f32 precision = 0.00003m = 0.03mm ✅

**Conversion:**
```rust
// Entity absolute position (f64 ECEF)
entity_ecef: (f64, f64, f64)

// Find which chunk entity is in
chunk_id = ecef_to_chunk_id(entity_ecef);

// Get chunk origin (f64 ECEF)  
chunk_origin_ecef = chunk_id_to_ecef(chunk_id);

// Local position within chunk (f32)
local_pos = (entity_ecef - chunk_origin_ecef) as f32;
```

**Advantage:** Even simpler than floating origin, natural for chunk-based systems

---

## Coordinate Precision Hierarchy

```
Storage:   f64 ECEF          → ±6.4M meters, ~1 nanometer precision ✅
           (Canonical truth)

Rendering: f32 camera-relative → ±10km, ~1mm precision ✅
           or f32 chunk-local  → ±256m, ~0.03mm precision ✅
           (GPU shader input)

Physics:   f32 chunk-local    → ±256m, ~0.03mm precision ✅
           (Rapier simulation)
```

---

## Can geoconv Handle This?

**What geoconv provides:**
- WGS84 (lat/lon/alt) → ECEF (x, y, z) conversion
- ECEF → WGS84 conversion
- Uses f64 for all calculations
- Type-safe units (Degrees, Meters)

**What we need from it:**
1. ✅ Convert GPS → ECEF (for loading SRTM, OSM data)
2. ✅ Convert ECEF → GPS (for debugging, display, teleport)
3. ✅ f64 precision (1 nanometer at Earth scale)
4. ✅ Handles full Earth range (±6.4M meters)

**What we DON'T need from it:**
- ❌ Rendering coordinates (we compute those separately)
- ❌ f32 conversion (we do `as f32` ourselves)
- ❌ Chunk-local transforms (we implement that)

---

## Answer: YES, geoconv Can Handle Our Scale

**Evaluation:**

| Requirement | geoconv Support | Status |
|-------------|----------------|--------|
| f64 precision | ✅ Yes | ✅ PASS |
| WGS84 → ECEF | ✅ Yes | ✅ PASS |
| ECEF → WGS84 | ✅ Yes | ✅ PASS |
| Full Earth range | ✅ Yes | ✅ PASS |
| Sub-cm detail | ✅ Yes (via f64) | ✅ PASS |
| Handles vertical range | ✅ Yes | ✅ PASS |

**Precision verification:**
- f64 at Earth scale: ~1 nanometer precision
- Our requirement: 1cm minimum
- Safety margin: 10,000,000× (ten million times better than needed) ✅

**Rendering precision:**
- Floating origin technique (TECH_SPEC.md Section 1.5)
- f32 camera-relative at 10km: ~1mm precision ✅
- Chunk-local f32 at 256m: ~0.03mm precision ✅

**Conclusion:**

✅ **geoconv (f64 ECEF) can absolutely handle our scale and detail requirements**

The precision is vastly overkill - we have nanometer accuracy when we only need centimeter accuracy. The only consideration is rendering (f32 on GPU), but that's solved with floating origin or chunk-local coordinates, both of which are already in the TECH_SPEC.md design.

---

## Next Steps

**Question 1:** ✅ ANSWERED - Use `geoconv`

**Proceed to Question 2:** How do we access/read SRTM elevation data?

