# Question 6: Voxel Data Structure Design

**Last Updated:** 2026-02-17  
**Purpose:** Define how we represent Earth's volumetric structure  
**Target:** No Man's Sky level detail, Earth-scale efficiency

---

## THE CHALLENGE

**Naive approach fails:**
- 1m³ voxels for entire Earth volume
- Earth radius: 6,371,000 meters
- Volume: 4/3 × π × r³ ≈ 1.08 × 10²¹ cubic meters
- **1.08 × 10²¹ voxels = 1 exabyte just for material IDs** ❌

**We need sparse representation:**
- Most of Earth is uniform (solid rock, empty air)
- Only surface ±200m needs detail (see WORLD_DEPTH_BOUNDARIES.md)
- Octree compression: uniform regions = one node

---

## VOXEL SIZE DECISION

### Base Resolution: 1 meter

**Rationale:**
- ✅ Matches human scale (doorway width, step height)
- ✅ SRTM elevation ~30m → can interpolate to 1m
- ✅ Building features distinguishable (walls, windows)
- ✅ Cave detail reasonable (tunnel width, stalactites)
- ✅ f64 ECEF precision: nanometers (way better than 1m)

**What 1m allows:**
- Player height: ~2 voxels
- Door: 1m wide × 2m tall (1×2 voxels)
- Room: 5m × 5m × 3m (125 voxels)
- Tree: 1m trunk, 5m tall (5 voxels vertical)
- Rock: 0.5-2m features visible

**What 1m doesn't allow:**
- Sub-meter detail (pebbles, cracks, small plants)
- Smooth curves at small scale (blockiness <1m)

**Solution for finer detail:** Texture maps, normal maps, procedural variation (not voxels)

---

### Variable LOD (Level of Detail)

**Key insight:** Don't need 1m everywhere

```
Distance from player:
  0-100m:     1m voxels    (high detail, interactive)
  100m-1km:   2m voxels    (visible detail)
  1km-10km:   4m voxels    (distant terrain)
  10km-100km: 8m voxels    (background)
  >100km:     16m+ voxels  (skybox/horizon)
```

**Implementation:** Octree naturally supports variable resolution
- Deep nodes (level 8) = 1m voxels
- Mid nodes (level 7) = 2m voxels  
- Shallow nodes = coarser

**LOD transition:** No pop-in (smooth mesh extraction handles resolution changes)

---

## MATERIAL REPRESENTATION

### Material ID: u8 (256 materials max)

**Why u8:**
- ✅ Fits in single byte (memory efficient)
- ✅ 256 materials more than enough for Earth
- ✅ Fast comparison, hashing, copying
- ❌ Can't have 1000s of unique materials (don't need them)

**Material palette:**

```rust
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Material {
    // Air and void
    AIR = 0,           // Empty space (most common - optimize for this)
    VOID = 1,          // Ungenerated/unknown
    
    // Natural terrain (2-49)
    STONE = 2,         // Generic rock
    GRANITE = 3,
    BASALT = 4,
    LIMESTONE = 5,
    SANDSTONE = 6,
    DIRT = 7,
    CLAY = 8,
    SAND = 9,
    GRAVEL = 10,
    SOIL = 11,         // Topsoil, grass layer
    SNOW = 12,
    ICE = 13,
    
    // Liquids (50-59)
    WATER = 50,
    SALT_WATER = 51,
    LAVA = 52,
    
    // Vegetation (60-79)
    WOOD = 60,         // Tree trunks, logs
    LEAVES = 61,
    GRASS = 62,        // Grass blocks
    
    // Manufactured (80-149)
    CONCRETE = 80,
    BRICK = 81,
    ASPHALT = 82,
    GLASS = 83,
    STEEL = 84,
    WOOD_PLANK = 85,   // Processed wood
    TILE = 86,
    CARPET = 87,
    
    // Underground (150-199)
    COAL = 150,
    IRON_ORE = 151,
    BEDROCK = 152,     // Indestructible deep layer
    
    // Special (200-255)
    LAVA_ROCK = 200,   // Volcanic
    CORAL = 201,
    // ... reserve for future
}
```

**Properties stored separately:**
```rust
struct MaterialProperties {
    solid: bool,           // Blocks movement?
    transparent: bool,     // See through?
    density: f32,          // For physics
    color: [u8; 3],        // Base color (RGB)
    texture_id: u16,       // Index into texture atlas
}

const MATERIAL_PROPERTIES: [MaterialProperties; 256] = [
    // AIR
    MaterialProperties {
        solid: false,
        transparent: true,
        density: 0.0,
        color: [0, 0, 0],
        texture_id: 0,
    },
    // STONE
    MaterialProperties {
        solid: true,
        transparent: false,
        density: 2500.0,  // kg/m³
        color: [128, 128, 128],
        texture_id: 1,
    },
    // ... etc
];
```

**Why separate properties:**
- Material ID: 1 byte (stored in octree)
- Properties: lookup table (not stored per-voxel)
- Change properties globally (update table, not every voxel)

---

## SPARSE VOXEL OCTREE STRUCTURE

### Node Types

```rust
#[derive(Debug, Clone)]
pub enum OctreeNode {
    // Uniform region (entire subtree is same material)
    Solid(Material),
    
    // Empty region (entire subtree is AIR)
    // Special case of Solid(AIR) for optimization
    Empty,
    
    // Mixed region (has children)
    Branch {
        children: Box<[OctreeNode; 8]>,  // Heap allocation
        // Optional metadata
        bounds: AABB,           // Bounding box in ECEF
        cached_mesh: Option<Mesh>,  // Pre-computed mesh
    },
}
```

**Why these node types:**
- `Empty`: Most common (atmosphere, deep space)
- `Solid(material)`: Second most common (deep underground rock, water)
- `Branch`: Only where detail needed (surface ±200m, buildings, caves)

**Memory per node:**
```
Empty:  1 byte (enum discriminant)
Solid:  2 bytes (discriminant + material u8)
Branch: ~64 bytes (8 pointers + metadata)
```

**Compression ratio:**
- Uniform 1km³ of rock: 1 Solid node = 2 bytes
- Naive storage: 1 billion voxels × 1 byte = 1 GB
- **Compression: 500 million ×** ✅

---

### Octree Depth

**Depth 0 (root):** Entire Earth
- Size: ~12,742 km (diameter)
- Each octant: ~6,371 km

**Depth 10:** ~12.4 km per octant
**Depth 15:** ~388 m per octant
**Depth 20:** ~12 m per octant
**Depth 23:** ~1.5 m per octant (base voxel size)

**Maximum depth: 23 levels**
- Leaf node: ~1.5m (close enough to 1m target)
- Path from root to leaf: 23 nodes
- Maximum tree depth handles all of Earth

**Actual depth varies by region:**
- Deep underground: depth ~5 (large uniform nodes)
- Surface terrain: depth ~20-23 (detailed)
- Atmosphere: depth ~3 (mostly empty)

---

### Coordinate Mapping

**ECEF (f64) → Voxel Coordinate (i64)**

```rust
// World bounds (centered on Earth center)
const WORLD_MIN: Vec3<f64> = Vec3::new(-6_400_000.0, -6_400_000.0, -6_400_000.0);
const WORLD_SIZE: f64 = 12_800_000.0;  // 12.8M meters (contains Earth)
const VOXEL_SIZE: f64 = 1.0;  // 1 meter base resolution

fn ecef_to_voxel(ecef: Vec3<f64>) -> Vec3<i64> {
    // Translate from ECEF origin to world corner
    let relative = ecef - WORLD_MIN;
    
    // Divide by voxel size
    let voxel_x = (relative.x / VOXEL_SIZE).floor() as i64;
    let voxel_y = (relative.y / VOXEL_SIZE).floor() as i64;
    let voxel_z = (relative.z / VOXEL_SIZE).floor() as i64;
    
    Vec3::new(voxel_x, voxel_y, voxel_z)
}

fn voxel_to_ecef(voxel: Vec3<i64>) -> Vec3<f64> {
    // Voxel center position
    let relative = Vec3::new(
        voxel.x as f64 + 0.5,
        voxel.y as f64 + 0.5,
        voxel.z as f64 + 0.5,
    ) * VOXEL_SIZE;
    
    // Translate back to ECEF
    relative + WORLD_MIN
}
```

**Voxel coordinate range:**
- Minimum: (0, 0, 0)
- Maximum: (12,800,000, 12,800,000, 12,800,000)
- Total possible voxels: 2.1 × 10²¹ (but sparse octree only stores occupied)

---

### Octree Traversal

**Get voxel at position:**

```rust
impl Octree {
    pub fn get_voxel(&self, voxel_pos: Vec3<i64>) -> Material {
        self.get_voxel_recursive(&self.root, voxel_pos, 0, WORLD_SIZE as i64)
    }
    
    fn get_voxel_recursive(
        &self,
        node: &OctreeNode,
        voxel_pos: Vec3<i64>,
        node_min: Vec3<i64>,
        node_size: i64,
    ) -> Material {
        match node {
            OctreeNode::Empty => Material::AIR,
            OctreeNode::Solid(material) => *material,
            OctreeNode::Branch { children, .. } => {
                // Determine which child octant contains voxel
                let half_size = node_size / 2;
                let child_index = 
                    ((voxel_pos.x >= node_min.x + half_size) as usize) << 0 |
                    ((voxel_pos.y >= node_min.y + half_size) as usize) << 1 |
                    ((voxel_pos.z >= node_min.z + half_size) as usize) << 2;
                
                // Recurse into child
                let child_min = Vec3::new(
                    node_min.x + if child_index & 1 != 0 { half_size } else { 0 },
                    node_min.y + if child_index & 2 != 0 { half_size } else { 0 },
                    node_min.z + if child_index & 4 != 0 { half_size } else { 0 },
                );
                
                self.get_voxel_recursive(
                    &children[child_index],
                    voxel_pos,
                    child_min,
                    half_size,
                )
            }
        }
    }
}
```

**Set voxel at position:**

```rust
impl Octree {
    pub fn set_voxel(&mut self, voxel_pos: Vec3<i64>, material: Material) {
        self.set_voxel_recursive(&mut self.root, voxel_pos, 0, WORLD_SIZE as i64, material);
    }
    
    fn set_voxel_recursive(
        &mut self,
        node: &mut OctreeNode,
        voxel_pos: Vec3<i64>,
        node_min: Vec3<i64>,
        node_size: i64,
        material: Material,
    ) {
        // Base case: node is small enough (1m voxel)
        if node_size <= 1 {
            *node = if material == Material::AIR {
                OctreeNode::Empty
            } else {
                OctreeNode::Solid(material)
            };
            return;
        }
        
        // If uniform node, need to subdivide
        if matches!(node, OctreeNode::Empty | OctreeNode::Solid(_)) {
            let old_material = match node {
                OctreeNode::Empty => Material::AIR,
                OctreeNode::Solid(m) => *m,
                _ => unreachable!(),
            };
            
            // Create 8 children (all same as parent initially)
            let children = Box::new([
                if old_material == Material::AIR { OctreeNode::Empty } else { OctreeNode::Solid(old_material) };
                8
            ]);
            
            *node = OctreeNode::Branch {
                children,
                bounds: AABB { min: node_min, size: node_size },
                cached_mesh: None,
            };
        }
        
        // Now node is Branch, recurse into appropriate child
        if let OctreeNode::Branch { children, .. } = node {
            let half_size = node_size / 2;
            let child_index = /* calculate as in get_voxel */ ;
            let child_min = /* calculate as in get_voxel */ ;
            
            self.set_voxel_recursive(
                &mut children[child_index],
                voxel_pos,
                child_min,
                half_size,
                material,
            );
            
            // After setting, check if all children are now uniform (can merge)
            self.try_merge_children(node);
        }
    }
    
    fn try_merge_children(&mut self, node: &mut OctreeNode) {
        if let OctreeNode::Branch { children, .. } = node {
            // Check if all children are same uniform material
            let first_material = match &children[0] {
                OctreeNode::Empty => Some(Material::AIR),
                OctreeNode::Solid(m) => Some(*m),
                OctreeNode::Branch { .. } => None,  // Mixed, can't merge
            };
            
            if let Some(material) = first_material {
                let all_same = children.iter().all(|child| {
                    match child {
                        OctreeNode::Empty => material == Material::AIR,
                        OctreeNode::Solid(m) => *m == material,
                        _ => false,
                    }
                });
                
                if all_same {
                    // Merge into single uniform node
                    *node = if material == Material::AIR {
                        OctreeNode::Empty
                    } else {
                        OctreeNode::Solid(material)
                    };
                }
            }
        }
    }
}
```

---

## MEMORY REQUIREMENTS

### Calculation for Different Scenarios

**Scenario 1: Entire Earth (uniform)**
- Deep underground: Solid(STONE)
- Atmosphere: Empty
- Estimated nodes: ~1000 (very coarse octree)
- Memory: ~64 KB

**Scenario 2: Surface layer (±200m detail)**
- Earth surface area: 510 million km²
- Volume ±200m: ~204 million km³
- At 1m voxels (if all stored): 2 × 10¹⁷ bytes = 200 petabytes ❌

**With octree compression:**
- Ocean (uniform water): ~71% of surface = 1 Solid node per km²
- Land terrain: ~29% of surface, varied
- Average compression: ~1000×
- Estimated: ~200 TB (still huge, need streaming)

**Scenario 3: 1km² detailed area (Kangaroo Point)**
- Volume: 1km × 1km × 400m (±200m detail)
- Voxels: 1000 × 1000 × 400 = 400 million voxels
- If all stored: 400 MB (material IDs only)

**With octree compression (~100× realistic):**
- Cliffs: detailed (little compression)
- River: uniform water (high compression)
- Underground: mostly uniform rock (high compression)
- Estimated: ~4 MB per km² (acceptable)

**Scenario 4: City (100 km²)**
- Buildings: detailed geometry
- Roads: thin layers
- Underground: utilities, parking
- Estimated: ~400 MB (with compression)

**Conclusion:** 
- ✅ Local areas (1-100 km²) fit in RAM
- ❌ Entire Earth surface can't fit in RAM
- ✅ Need streaming/chunking system (load nearby, unload distant)

---

## CHUNKING STRATEGY

**Problem:** Can't load entire Earth octree in RAM

**Solution:** Divide Earth into chunks (independent octrees)

### Chunk Size: 1km × 1km × 2km vertical

**Why 1km horizontal:**
- ✅ Fits in RAM (~4 MB compressed)
- ✅ Reasonable player interaction radius
- ✅ Aligns with SRTM tile boundaries
- ✅ Not too many chunks (512 million globally)

**Why 2km vertical:**
- ±1km from surface center
- Covers: -200m (tunnels) to +800m (tall buildings/mountains)
- Deep underground and atmosphere: separate sparse chunks

**Chunk ID from ECEF:**
```rust
const CHUNK_SIZE_HORIZONTAL: f64 = 1000.0;  // 1km
const CHUNK_SIZE_VERTICAL: f64 = 2000.0;    // 2km

fn ecef_to_chunk_id(ecef: Vec3<f64>) -> Vec3<i32> {
    Vec3::new(
        (ecef.x / CHUNK_SIZE_HORIZONTAL).floor() as i32,
        (ecef.y / CHUNK_SIZE_HORIZONTAL).floor() as i32,
        (ecef.z / CHUNK_SIZE_VERTICAL).floor() as i32,
    )
}
```

**Chunk loading strategy:**
- Load chunks within 5km of player (~79 chunks, ~320 MB)
- Unload chunks >10km from player
- Background thread: generate/load nearby chunks
- Cache generated chunks to disk

---

## GENERATION RULES

**From SRTM elevation to voxels:**

```rust
fn generate_chunk_voxels(chunk_id: Vec3<i32>, srtm: &SrtmData) -> Octree {
    let mut octree = Octree::new();
    
    for voxel_local in chunk_bounds(chunk_id) {
        // Convert to ECEF, then GPS
        let ecef = chunk_voxel_to_ecef(chunk_id, voxel_local);
        let gps = ecef_to_gps(ecef);
        
        // Query SRTM elevation at this lat/lon
        let ground_elevation = srtm.get_elevation(gps.lat, gps.lon)?;
        
        // Determine material based on altitude relative to ground
        let material = if gps.alt > ground_elevation {
            Material::AIR  // Above ground
        } else if gps.alt > ground_elevation - 1.0 {
            Material::SOIL  // Top layer (grass/dirt)
        } else if gps.alt > ground_elevation - 10.0 {
            Material::DIRT  // Subsoil
        } else if gps.alt > 0.0 {
            Material::STONE  // Below dirt, above sea level
        } else if gps.alt > -200.0 {
            Material::STONE  // Underground
        } else {
            Material::STONE  // Deep underground (uniform)
        };
        
        octree.set_voxel(voxel_local, material);
    }
    
    octree
}
```

**OSM feature integration:**
```rust
// After SRTM generation, apply OSM features
fn apply_osm_features(octree: &mut Octree, osm_features: &[OsmFeature]) {
    for feature in osm_features {
        match feature.kind {
            OsmKind::Building => {
                // Set voxels in building footprint to CONCRETE
                for (x, y, z) in building_voxels(feature) {
                    octree.set_voxel(Vec3::new(x, y, z), Material::CONCRETE);
                }
            }
            OsmKind::Road => {
                // Set thin layer to ASPHALT
                for (x, y, z) in road_voxels(feature) {
                    octree.set_voxel(Vec3::new(x, y, z), Material::ASPHALT);
                }
            }
            OsmKind::River => {
                // Carve out and fill with WATER
                for (x, y, z) in river_voxels(feature) {
                    octree.set_voxel(Vec3::new(x, y, z), Material::WATER);
                }
            }
            // ... etc
        }
    }
}
```

---

## PERFORMANCE CONSIDERATIONS

### Query Performance

**Goal:** 1 million voxel queries per second

**Octree depth 23:**
- Worst case: 23 node traversals
- Best case: 1 node (uniform region)
- Average: ~10-15 nodes

**Optimizations:**
- Cache frequently accessed nodes
- Batch queries (spatial locality)
- SIMD for ray marching
- GPU for large queries (compute shader)

**Benchmark target:**
```rust
#[bench]
fn bench_voxel_query(b: &mut Bencher) {
    let octree = create_test_octree();
    b.iter(|| {
        for _ in 0..1000 {
            let _ = octree.get_voxel(random_voxel_pos());
        }
    });
    // Target: <1ms for 1000 queries = 1 million queries/sec
}
```

---

### Modification Performance

**Goal:** 10,000 voxel modifications per second

**Set voxel cost:**
- Traverse to leaf: ~10-15 nodes
- Modify leaf: O(1)
- Propagate merge: up to 23 levels worst case
- Average: ~20 node operations

**Optimizations:**
- Batch modifications (set region)
- Defer merge checks (lazy consolidation)
- Multi-threaded (different chunks independent)

---

### Memory Access Patterns

**Cache-friendly:**
- Box<[OctreeNode; 8]>: children in contiguous memory
- Branch traversal: predictable (bit-based indexing)

**Cache-unfriendly:**
- Random voxel access (pointer chasing)
- Deep trees (many indirections)

**Mitigation:**
- Keep hot chunks in L3 cache (~8 MB)
- Prefetch child nodes
- Linearize octree for ray marching (SVO pointer format)

---

## SUMMARY

### Voxel Size: 1 meter base resolution
- Human-scale features visible
- Variable LOD (1m to 16m+ based on distance)

### Material: u8 (256 materials)
- 1 byte per voxel
- Properties in lookup table

### Octree: Depth 23, 3 node types
- Empty (1 byte) - atmosphere
- Solid (2 bytes) - uniform regions
- Branch (64 bytes) - detailed areas

### Chunking: 1km × 1km × 2km
- ~4 MB per chunk (compressed)
- Load nearby, unload distant
- ~79 chunks active (~320 MB)

### Coordinate Mapping: ECEF → Voxel
- i64 voxel coordinates
- 12.8M meter world bounds
- Floor division for conversion

### Generation: SRTM + OSM
- Fill from elevation data
- Apply building/road overlays
- Carve caves/tunnels

### Performance Targets:
- 1M queries/second
- 10K modifications/second
- 4 MB RAM per km²

**Question 6: ANSWERED ✅**

