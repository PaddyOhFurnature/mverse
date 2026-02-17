# World Data Inventory & Acquisition Strategy

**Last Updated:** 2026-02-17  
**Purpose:** Comprehensive inventory of available world data vs what we need to generate

This document categorizes ALL data required for a 1:1 Earth-scale metaverse, identifying what exists, what's accessible, quality/resolution, and what must be procedurally generated.

---

## 1. TERRAIN FOUNDATION (The Ground Truth)

The substrate everything sits on/in/through.

### 1.1 Elevation Data (Surface Topography)

| Dataset | Resolution | Coverage | Vertical Accuracy | Status | Access |
|---------|-----------|----------|-------------------|--------|--------|
| **SRTM/NASADEM** | 30m (1 arc-sec) | 60°N–56°S (80% of Earth) | ±16m absolute | ✅ Available | Free (NASA Earthdata) |
| **ALOS AW3D30** | 30m | 82°N–82°S (99% land) | ±5m (best for mountains) | ✅ Available | Free (JAXA) |
| **ASTER GDEM v3** | 30m | 83°N–83°S (99% land) | ±17m (artifacts in clouds) | ✅ Available | Free (NASA/METI) |
| **Copernicus DEM** | 30m, 90m (10m Europe) | Global | ±4m (best accuracy) | ✅ Available | Free (Copernicus) |
| **MERIT DEM** | 90m | Global | Error-minimized | ✅ Available | Free (research use) |
| **TanDEM-X** | 12m | Global | ±2m | ⚠️ Commercial | Paid only |
| **LiDAR (regional)** | 0.5–5m | Sparse (select cities/regions) | ±0.1m | ⚠️ Partial | Mixed (government sources) |

**GAPS:**
- ❌ **Sub-30m global coverage** — Most of world is 30m best case
- ❌ **Polar regions** — Above 83°N and below 83°S limited coverage
- ❌ **Vertical features** — Cliffs, overhangs, caves not captured (nadir view only)
- ❌ **Temporal changes** — Single snapshot, doesn't track erosion/landslides

**INFERENCE NEEDED:**
- Detail below 30m resolution (need fractal/procedural enhancement)
- Cliff face geometry (only top visible in DEM)
- Rock texture and composition
- Seasonal/weather effects on terrain

---

### 1.2 Bathymetry (Ocean Depth)

| Dataset | Resolution | Coverage | Depth Accuracy | Status | Access |
|---------|-----------|----------|----------------|--------|--------|
| **GEBCO 2025** | 15 arc-sec (~500m) | Global ocean + land | ±100m (varies) | ✅ Available | Free (GEBCO) |
| **Seabed 2030** | Variable | ~25% mapped (target 100% by 2030) | High (multibeam) | 🔄 In Progress | Free (GEBCO) |
| **ETOPO2022** | 15/30/60 arc-sec | Global relief | ±100m ocean | ✅ Available | Free (NOAA) |

**GAPS:**
- ❌ **75% of seafloor unmapped** — Only satellite gravity estimates (low accuracy)
- ❌ **Underwater features** — Seamounts, trenches, ridges poorly detailed
- ❌ **Coastal detail** — Shallow water, reefs, underwater caves minimal

**INFERENCE NEEDED:**
- Seafloor detail in unmapped areas (fractal terrain + oceanographic rules)
- Underwater cave systems
- Coral reef structures
- Shipwrecks, underwater infrastructure

---

### 1.3 Subsurface Geology

| Data Type | Coverage | Detail Level | Status | Access |
|-----------|----------|--------------|--------|--------|
| **Geological maps** | Regional (varies by country) | Surface layer only | ⚠️ Partial | Mixed (USGS, BGS, etc.) |
| **Macrostrat** | Global stratigraphic DB | Surface + inferred depth | ✅ Available | Free (API) |
| **OneGeology** | Multi-country collaboration | Surface geology | ✅ Available | Free (WMS) |
| **Cave databases** | Sparse (known caves only) | Individual records | ⚠️ Very Partial | Free (World Cave DB) |
| **Mine data** | Regulatory databases (varies) | Known mines only | ⚠️ Partial | Mixed |
| **Aquifer data** | Regional (hydro surveys) | Shallow aquifers | ⚠️ Partial | Mixed |

**GAPS:**
- ❌ **Volumetric subsurface** — Almost no 3D data below surface layer
- ❌ **Unknown caves** — Only mapped/explored caves in database
- ❌ **Abandoned mines** — Many undocumented
- ❌ **Deep geology** — Crust structure theoretical only
- ❌ **Natural voids** — Lava tubes, sinkholes, karst systems

**INFERENCE NEEDED:**
- Procedural cave generation (based on geology type, hydrology)
- Subsurface layer composition (sediment, bedrock, etc.)
- Natural void probability maps
- Rock strata and mineral distribution

---

## 2. SURFACE FEATURES (What Sits On The Ground)

### 2.1 Buildings & Structures

| Dataset | Coverage | Completeness | Detail Level | Status | Access |
|---------|----------|--------------|--------------|--------|--------|
| **OpenStreetMap** | Global | **21% average** (varies widely) | Footprint + metadata | ✅ Available | Free (ODbL) |
| **Microsoft Building Footprints** | Select regions | High (where available) | Footprint only | ✅ Available | Free (ODbL) |
| **Google Open Buildings** | Some countries | Medium | Footprint only | ✅ Available | Free (CC-BY) |
| **City-specific datasets** | Major cities | High | 3D models (some) | ⚠️ Partial | Mixed |

**COMPLETENESS BREAKDOWN:**
- 16% of global urban population: >80% OSM completeness (1,848 cities)
- 48% of global urban population: <20% OSM completeness (9,163 cities)
- Europe/North America: >50% average completeness
- Africa/Asia/South America: 7-17% average completeness

**GAPS:**
- ❌ **Building heights** — Mostly missing (some inferred from stories)
- ❌ **Interior structure** — None (except special cases)
- ❌ **Building materials** — Rarely tagged
- ❌ **Small structures** — Sheds, garages, outbuildings
- ❌ **Rural buildings** — Heavily underrepresented
- ❌ **Historical buildings** — Lost/demolished structures

**INFERENCE NEEDED:**
- Building height estimation (from surroundings, satellite shadows, typical patterns)
- Roof geometry (from satellite imagery or regional styles)
- Interior layouts (procedural based on building type/size)
- Construction materials (from region, age, building type)
- Missing buildings in low-completeness areas

---

### 2.2 Transportation Networks

| Dataset | Coverage | Completeness | Detail Level | Status | Access |
|---------|----------|--------------|--------------|--------|--------|
| **OSM Roads** | Global | **~80% overall** (varies by region) | Full network + metadata | ✅ Available | Free (ODbL) |
| **GRIP (Global Roads)** | 222 countries | 21M km integrated | Harmonized multi-source | ✅ Available | Free |
| **OSM Railways** | Global | High (major lines) | Network + stations | ✅ Available | Free (ODbL) |
| **OSM Paths/Trails** | Global | Medium (volunteer-mapped) | Hiking/bike paths | ✅ Available | Free (ODbL) |
| **OSM Waterways** | Global | Medium | Rivers, canals | ✅ Available | Free (ODbL) |

**GAPS:**
- ❌ **Minor roads** — Rural/unmaintained roads often missing
- ❌ **Road surface quality** — Rarely tagged
- ❌ **Lane markings** — Not captured
- ❌ **Traffic signs** — Not mapped
- ❌ **Informal paths** — Animal trails, desire paths

**INFERENCE NEEDED:**
- Road surface deterioration (from age, climate, traffic)
- Minor roads in sparse areas (connect known points)
- Path networks in parks/wilderness

---

### 2.3 Underground Infrastructure

| Dataset | Coverage | Completeness | Detail Level | Status | Access |
|---------|----------|--------------|--------------|--------|--------|
| **OSM Subway Systems** | Major cities | High (major systems) | Lines + stations | ✅ Available | Free (ODbL) |
| **OSM Underground Tags** | Sparse | Very Low | Tunnels, subways | ⚠️ Partial | Free (ODbL) |
| **Utility Infrastructure** | Proprietary (utilities) | Unknown | Water, sewer, electric, gas | ❌ Restricted | Not public |
| **City Infrastructure GIS** | Major cities only | Medium | Varies | ⚠️ Partial | Mixed (some open) |

**GAPS:**
- ❌ **Most underground infrastructure** — Utilities, sewers, service tunnels proprietary
- ❌ **Depth information** — Even when mapped, depth often missing
- ❌ **Connections** — Network topology incomplete
- ❌ **Abandoned systems** — Old tunnels, mines, sewers unmapped

**INFERENCE NEEDED:**
- Utility corridor placement (follow roads, building density)
- Sewer network (topology from terrain, drainage)
- Service tunnel networks (major cities)
- Subway tunnel geometry (between stations)

---

## 3. SURFACE CHARACTERISTICS

### 3.1 Land Cover & Vegetation

| Dataset | Resolution | Classes | Update Frequency | Status | Access |
|---------|-----------|---------|------------------|--------|--------|
| **ESA WorldCover** | 10m | 11 classes | Annual (2020+) | ✅ Available | Free (Copernicus) |
| **Copernicus Dynamic** | 10m, 100m | Multiple products | Annual | ✅ Available | Free (Copernicus) |
| **MODIS Land Cover** | 500m | 17 classes (IGBP) | Annual (2001–2024+) | ✅ Available | Free (NASA) |
| **ESA CCI Land Cover** | 300m | 22 classes | Annual (1992–present) | ✅ Available | Free (ESA) |
| **OSM Natural Features** | Varies | Tagged features | Continuous | ✅ Available | Free (ODbL) |

**LAND COVER CLASSES (WorldCover):**
- Tree cover, shrubland, grassland, cropland, built-up, bare/sparse vegetation, snow/ice, water bodies, herbaceous wetland, mangroves, moss/lichen

**GAPS:**
- ❌ **Individual trees** — Only forest/tree cover areas
- ❌ **Plant species** — No species identification
- ❌ **Seasonal variation** — Annual snapshots only
- ❌ **Undergrowth detail** — Only canopy visible
- ❌ **Agricultural detail** — Crop types rarely specified

**INFERENCE NEEDED:**
- Individual tree placement (from tree cover density + climate + terrain)
- Species distribution (from climate zones, altitude, soil)
- Grass/undergrowth detail (procedural based on biome)
- Crop field detail (from land use type)
- Seasonal foliage changes

---

### 3.2 Satellite Imagery

| Dataset | Resolution | Bands | Revisit Time | Status | Access |
|---------|-----------|-------|--------------|--------|--------|
| **Sentinel-2** | 10m (visible/NIR) | 13 bands | 5 days | ✅ Available | Free (Copernicus) |
| **Landsat 8/9** | 30m (15m pan) | 11 bands | 16 days | ✅ Available | Free (USGS/NASA) |
| **HLS (Harmonized)** | 30m | Combined S2+L8/9 | 2-3 days | ✅ Available | Free (NASA) |
| **MODIS** | 250-1000m | 36 bands | Daily | ✅ Available | Free (NASA) |
| **Commercial (Maxar, etc.)** | 0.3-1m | RGB + panchromatic | On-demand | ⚠️ Commercial | Paid only |

**GAPS:**
- ❌ **Sub-10m free imagery** — Need paid sources for higher resolution
- ❌ **Cloud coverage** — Persistent clouds block some regions
- ❌ **Historical imagery** — Limited archive depth
- ❌ **Real-time** — Days to weeks lag

**INFERENCE NEEDED:**
- Texture detail below 10m (procedural enhancement)
- Cloud-free composites (temporal merging)
- Ground-level detail (not visible from satellite)

---

## 4. POINT-OF-INTEREST & OBJECT DATA

### 4.1 Discrete Objects

| Data Type | Source | Coverage | Detail | Status |
|-----------|--------|----------|--------|--------|
| **Trees (individual)** | ❌ None | N/A | N/A | Must generate |
| **Vehicles** | ❌ None (realtime only) | N/A | N/A | Simulation only |
| **Street furniture** | OSM (sparse) | Very Low | Basic tags | ⚠️ Partial |
| **Utility poles** | ❌ Proprietary | Unknown | N/A | Not public |
| **Rocks/boulders** | ❌ None | N/A | N/A | Must generate |

**GAPS:**
- ❌ **All discrete natural objects** — Trees, rocks, plants
- ❌ **Most man-made objects** — Signs, poles, benches, etc.
- ❌ **Dynamic objects** — Vehicles, people (simulation only)

**INFERENCE NEEDED:**
- Tree placement from land cover + density maps
- Rock/boulder distribution from geology + terrain roughness
- Street furniture from urban density + road types
- Vegetation detail from biome rules

---

### 4.2 Points of Interest (POI)

| Dataset | Coverage | Types | Update Frequency | Status | Access |
|---------|----------|-------|------------------|--------|--------|
| **OSM POIs** | Global | Comprehensive tagging | Continuous (volunteer) | ✅ Available | Free (ODbL) |
| **Google Places** | Global | Extensive | Continuous | ⚠️ API | Paid (quota) |
| **Overture Maps** | Global | Integrated sources | Regular | ✅ Available | Free |

**POI CATEGORIES:**
- Amenities, shops, services, tourism, leisure, emergency, healthcare, education, government, etc.

**GAPS:**
- ❌ **Completeness varies** — Developed regions better than rural
- ❌ **Business churn** — Constant changes, hard to stay current
- ❌ **Hours/attributes** — Often missing or outdated

---

## 5. ATMOSPHERIC & ENVIRONMENTAL DATA

### 5.1 Climate & Weather

| Data Type | Resolution | Temporal Coverage | Status | Access |
|-----------|-----------|-------------------|--------|--------|
| **Historical climate** | ~10-50km | 1950–present | ✅ Available | Free (NOAA, etc.) |
| **Weather models** | ~10km | Real-time forecasts | ✅ Available | Free (NOAA, ECMWF) |
| **Temperature/precipitation** | Point data (stations) | Historical archive | ✅ Available | Free |

**GAPS:**
- ❌ **Micro-climate** — Local variations not captured
- ❌ **Real-time global** — Need API/service for current weather

**INFERENCE NEEDED:**
- Weather simulation (from climate data + physics)
- Micro-climate (from terrain, vegetation, water bodies)

---

### 5.2 Lighting & Sky

| Data Type | Source | Status |
|-----------|--------|--------|
| **Sun position** | Astronomical calculation | ✅ Computable |
| **Moon position** | Astronomical calculation | ✅ Computable |
| **Stars** | Astronomical catalogs | ✅ Available |
| **Atmospheric scattering** | Physics simulation | ✅ Computable |

---

## 6. WHAT WE HAVE vs WHAT WE NEED

### 6.1 Foundation Data (HAVE)

✅ **Surface elevation** — 30m global coverage (good)  
✅ **Land cover** — 10m classification (good)  
✅ **Satellite imagery** — 10m RGB (good)  
✅ **Major infrastructure** — Roads, buildings (partial but usable)  
✅ **Coastlines & water** — High quality  
✅ **Political boundaries** — Complete

### 6.2 Critical Gaps (NEED TO GENERATE)

❌ **Volumetric terrain** — Subsurface geology, caves, mines  
❌ **Detail below 30m** — Rock texture, cliff detail, micro-topography  
❌ **Individual objects** — Trees, rocks, vegetation detail  
❌ **Building interiors** — Almost none exist  
❌ **Underground infrastructure** — Utilities, sewers, service tunnels  
❌ **Dynamic elements** — Weather, vegetation growth, erosion  
❌ **Missing building data** — 79% of cities <20% complete  

---

## 7. GENERATION RULES FRAMEWORK

For data we DON'T have, we need **deterministic procedural generation** with these principles:

### 7.1 Rule Hierarchy

1. **REAL data always wins** — Never override known data
2. **Physically plausible** — Follow real-world constraints
3. **Deterministic** — Same seed → same result (for multiplayer sync)
4. **Context-aware** — Use surrounding known data to constrain generation
5. **Resolution-appropriate** — Only generate detail when viewed close enough

### 7.2 Generation Categories

#### A. **Terrain Detail Enhancement**
- Input: 30m elevation data
- Output: Sub-meter detail when player close
- Method: Fractal subdivision constrained by:
  - Geology type (from Macrostrat)
  - Slope/curvature (from DEM)
  - Climate/erosion patterns
  - Vegetation cover (erosion protection)

#### B. **Subsurface Volumetric**
- Input: Surface geology + elevation + hydrology
- Output: Layered subsurface (bedrock, soil, voids)
- Method: Geological deposition simulation
  - Sedimentary layers (from age/elevation history)
  - Cave probability (karst regions + water table)
  - Aquifers (from permeability + topology)

#### C. **Vegetation Distribution**
- Input: Land cover class + climate + elevation + soil
- Output: Individual plant placement
- Method: Poisson disk sampling with:
  - Species selection (climate zone + altitude)
  - Density from land cover data
  - Size/age distribution (realistic forests)
  - Constraints from infrastructure

#### D. **Building Reconstruction**
- Input: Footprint + region + imagery
- Output: 3D building with interior
- Method: Procedural architecture
  - Height from shadows/surroundings/typical
  - Style from region/age/function
  - Interior from building type + size
  - Materials from climate/culture

#### E. **Infrastructure Inference**
- Input: Known infrastructure + population density + terrain
- Output: Utility networks, minor roads
- Method: Network optimization
  - Utilities follow roads
  - Sewers follow drainage
  - Missing roads connect known points (Dijkstra with terrain cost)

---

## 8. DATA ACQUISITION STRATEGY

### Phase 0: Foundation (DO FIRST)
1. ✅ SRTM/ALOS elevation (30m global)
2. ✅ ESA WorldCover land cover (10m)
3. ✅ OSM base data (buildings, roads, POIs)
4. ✅ Sentinel-2 imagery (10m, cloud-free composite)
5. ✅ GEBCO bathymetry (ocean)

### Phase 1: Enhancement (NEXT)
1. ⏳ Copernicus DEM (better accuracy)
2. ⏳ Regional LiDAR (where available)
3. ⏳ Microsoft/Google building footprints (supplement OSM)
4. ⏳ Climate data (temperature, precipitation)

### Phase 2: Specialized (LATER)
1. ⏳ Geological maps (regional sources)
2. ⏳ Underground infrastructure (city datasets)
3. ⏳ Historical imagery (change detection)

---

## 9. TESTING & VALIDATION STRATEGY

**Critical Question:** How do we know generated data is "good enough"?

### Validation Approaches:

1. **Visual Plausibility** — Does it LOOK real to a human?
2. **Statistical Match** — Do distributions match real-world samples?
3. **Cross-validation** — Generate where we HAVE data, compare
4. **Expert Review** — Geologists/ecologists validate generation rules
5. **Player Testing** — Does it "feel" real during gameplay?

### Test Locations:
- **Well-mapped city** (NYC, London) — Test building generation
- **Forest region** (Amazon, Cascadia) — Test vegetation
- **Mountainous area** (Himalayas, Alps) — Test terrain detail
- **Cave region** (Kentucky karst) — Test subsurface
- **Mix of data quality** — High/medium/low OSM completeness

---

## 10. CRITICAL UNKNOWNS (RESEARCH NEEDED)

These questions determine if approach is viable:

1. **Can 30m elevation + fractals produce convincing <1m detail?**
   - Test: Generate terrain at Kangaroo Point Cliffs, compare to photos
   
2. **Can we infer building heights accurately enough?**
   - Test: Generate NYC buildings, compare to known heights
   
3. **Can procedural vegetation match real forest structure?**
   - Test: Generate forest, compare to LiDAR ground truth
   
4. **Can we sync procedural generation across clients deterministically?**
   - Test: Two clients, same seed, identical result?
   
5. **What's the performance reality?**
   - Test: Can hardware handle generation on-demand?

---

## SUMMARY

### We Have (Foundation):
- ✅ 30m global elevation
- ✅ 10m land cover
- ✅ 10m satellite imagery
- ✅ Partial building/road data (varies 7-90% complete)
- ✅ Ocean bathymetry (~500m)

### We Must Generate (Procedural):
- ❌ Sub-30m terrain detail (fractal)
- ❌ Subsurface geology (simulation)
- ❌ Individual vegetation (rules + density)
- ❌ Building details (procedural architecture)
- ❌ Underground infrastructure (network inference)
- ❌ Missing buildings (79% of cities <20% complete)
- ❌ Dynamic elements (weather, growth, erosion)

### Next Steps:
1. Acquire foundation datasets (Phase 0)
2. Implement one generation category as proof-of-concept
3. Validate against ground truth
4. Iterate on generation rules
5. Scale test

**The world substrate must exist FIRST, then we layer generation rules on top of real data.**

