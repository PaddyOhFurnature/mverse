# Question 7: Mesh Extraction Algorithm

**Last Updated:** 2026-02-17  
**Purpose:** Choose algorithm to convert voxels → smooth organic mesh  
**Target:** No Man's Sky level detail, not blocky Minecraft cubes

---

## THE PROBLEM

**Voxels are discrete cubes** (1m³ grid)
**Nature is smooth and organic** (rolling hills, curved cliffs)

**Can't just render cubes directly:**
- ❌ Blocky Minecraft aesthetic
- ❌ Stair-stepping on slopes
- ❌ Jagged edges on curves

**Need algorithm to:**
- Sample voxel grid
- Generate smooth interpolated surface
- Create triangle mesh for GPU rendering

---

## ALGORITHM OPTIONS

### Option 1: Marching Cubes (1987)

**How it works:**
1. Process each voxel cube (8 corners)
2. Check which corners are solid vs air
3. Create 8-bit index (2^8 = 256 cases)
4. Look up edge intersections from table
5. Interpolate vertex positions along edges
6. Generate triangles from table

**Visual:**
```
    7-------6
   /|      /|
  4-------5 |
  | |     | |    Each corner: 0 (air) or 1 (solid)
  | 3-----|-2    Binary: 01101001 = case 105
  |/      |/     Lookup table: which edges have vertices
  0-------1      Generate triangles from those vertices
```

**Lookup tables:**
- Edge table: 256 entries (which edges intersected)
- Triangle table: 256 × 15 entries (how to connect edges into triangles)

**Pros:**
- ✅ Well-documented (oldest algorithm, tons of resources)
- ✅ Simple implementation (~300 lines)
- ✅ Fast (lookup tables pre-computed)
- ✅ Smooth surfaces (interpolation between voxels)
- ✅ Proven at scale (medical imaging, games)

**Cons:**
- ❌ Thin triangles (some cases create slivers)
- ❌ Ambiguous cases (some configurations have multiple valid solutions)
- ❌ Poor sharp features (edges/corners get smoothed)
- ❌ More triangles than needed (not optimal topology)

**Best for:**
- Smooth organic terrain (hills, caves)
- First implementation (get something working)
- When you need it working TODAY

---

### Option 2: Dual Contouring (2002)

**How it works:**
1. Process each voxel cube (8 corners)
2. For each edge crossing: compute surface normal (gradient)
3. Place ONE vertex per cube (not on edges)
4. Position vertex to minimize error (QEF solver)
5. Connect vertices between adjacent cubes

**Visual:**
```
    7-------6        Instead of vertices on edges,
   /|      /|        place ONE vertex per cube
  4-+-----5 |        Position optimizes sharp features
  | |  *  | |        Connect vertices across cube faces
  | 3-----|-2
  |/      |/
  0-------1
```

**Pros:**
- ✅ Sharp features preserved (corners, edges look crisp)
- ✅ Better topology (one vertex per cube, cleaner mesh)
- ✅ Fewer triangles (more efficient)
- ✅ Adaptive resolution (can vary detail easily)

**Cons:**
- ❌ More complex (needs QEF solver, surface normals)
- ❌ Harder to implement (~800-1000 lines)
- ❌ Slower (optimization step per vertex)
- ❌ Need normal data (compute from voxel gradients)
- ❌ Can produce non-manifold geometry (holes, self-intersections)

**Best for:**
- Architectural geometry (buildings with sharp corners)
- Mixed organic/man-made (cities with terrain)
- When you need optimal quality

---

### Option 3: Surface Nets / Naive Surface Nets (2000)

**How it works:**
1. Process each voxel cube
2. Place vertex at average of edge crossings (simple, no QEF)
3. Connect vertices between adjacent cubes
4. Smooth vertex positions (relaxation pass)

**Similar to Dual Contouring but simpler:**
- No QEF solver (just average positions)
- No normal data needed
- Less sharp but still smooth

**Pros:**
- ✅ Simpler than Dual Contouring (~400 lines)
- ✅ Better topology than Marching Cubes
- ✅ Faster than Dual Contouring
- ✅ Smooth organic surfaces
- ✅ No lookup tables (procedural)

**Cons:**
- ⚠️ Not as sharp as Dual Contouring
- ⚠️ Not as smooth as Marching Cubes
- ❌ Less documented (fewer examples)

**Best for:**
- Balance between simplicity and quality
- Organic terrain that doesn't need perfect smoothness
- When Marching Cubes too rough, Dual Contouring too complex

---

### Option 4: Cubical Marching Squares (Transvoxel)

**Special case for terrain (heightmap-like voxels):**
- Optimized for mostly-horizontal surfaces
- LOD transitions without cracks
- Used in some terrain engines

**Pros:**
- ✅ Perfect for terrain LOD
- ✅ No cracks between detail levels

**Cons:**
- ❌ Not suitable for caves, overhangs, full 3D
- ❌ Complex LOD management

**Verdict:** Not suitable (we need full volumetric, not just terrain)

---

## COMPARISON TABLE

| Feature | Marching Cubes | Dual Contouring | Surface Nets |
|---------|----------------|-----------------|--------------|
| **Smoothness** | Excellent | Good | Good |
| **Sharp features** | Poor | Excellent | Fair |
| **Triangle count** | High | Low | Medium |
| **Implementation** | Simple (~300 lines) | Complex (~1000 lines) | Medium (~400 lines) |
| **Performance** | Fast (lookup) | Slow (QEF) | Fast |
| **Documentation** | Excellent | Good | Limited |
| **Caves/overhangs** | ✅ Yes | ✅ Yes | ✅ Yes |
| **Ambiguous cases** | ❌ Has them | ✅ None | ✅ None |
| **Topology quality** | Fair | Excellent | Good |

---

## DECISION CRITERIA

### For This Project:

**Requirements:**
1. ✅ Volumetric (caves, tunnels, overhangs) - ALL handle this
2. ✅ Smooth organic surfaces (not blocky) - ALL handle this
3. ✅ Earth-scale performance (millions of voxels) - Need FAST
4. ⚠️ Mixed terrain + buildings - Favor sharp features?
5. ✅ First implementation (get working soon) - Favor SIMPLE

**Priorities:**
1. **Get it working** (Phase 6 target: first render)
2. **Smooth enough** (No Man's Sky level acceptable)
3. **Fast enough** (generate 1km² terrain in <5 seconds)
4. **Iterate later** (can replace algorithm if needed)

---

## RECOMMENDATION: Marching Cubes

### Why Marching Cubes for Phase 1:

**✅ Proven and documented:**
- 37+ years of usage
- Thousands of implementations (can reference)
- Paul Bourke's reference page (canonical)
- Easy to find help when stuck

**✅ Fast implementation:**
- Lookup tables available (copy-paste)
- ~300 lines of code
- Can get working in 4-6 hours
- No complex math (just interpolation)

**✅ "Good enough" quality:**
- Smooth surfaces (better than voxel cubes)
- Organic terrain looks natural
- Caves work perfectly
- Sharp features not critical for Phase 1 (terrain is naturally smooth)

**✅ Fast execution:**
- Simple operations (lookups, lerp)
- Easy to parallelize (process cubes independently)
- Can optimize later (cache, SIMD)

**❌ Downsides acceptable for now:**
- Thin triangles: won't notice at 1m resolution
- Poor sharp corners: buildings will be added later (OSM geometry, not voxels)
- More triangles: GPUs handle millions easily

---

### Can Upgrade Later:

**Phase 1-6:** Marching Cubes (get it working)

**Future optimization (if needed):**
- Phase 10+: Dual Contouring for buildings (sharp corners)
- Hybrid: Marching Cubes for terrain, Dual Contouring for architecture
- Or: Keep Marching Cubes if it looks good enough

**"Perfect is the enemy of good"** - Start with Marching Cubes, iterate if needed

---

## MARCHING CUBES DETAILS

### Cube Indexing

```
Vertex numbering:
    7-------6
   /|      /|
  4-------5 |
  | |     | |
  | 3-----|-2
  |/      |/
  0-------1

Edge numbering:
     *---6---*
    /|      /|
   7 11    5 10
  /  |    /  |
 *---4---*   |
 |   *---+2--*
 8  /    9  /
 | 3     | 1
 |/      |/
 *---0---*
```

### Case Index Calculation

```rust
fn cube_index(cube_corners: [bool; 8]) -> u8 {
    let mut index = 0u8;
    for i in 0..8 {
        if cube_corners[i] {  // Solid
            index |= 1 << i;
        }
    }
    index
}

// Example:
// Corners: [air, solid, solid, air, solid, air, air, solid]
//          [0,   1,     1,     0,   1,     0,   0,   1   ]
// Binary:   00000000
//           |      |
//           76543210
//           10010110 = 150
```

### Edge Intersection

```rust
fn interpolate_vertex(
    pos1: Vec3<f64>,  // First corner position (air)
    pos2: Vec3<f64>,  // Second corner position (solid)
    val1: f32,        // Distance field value at pos1 (negative = air)
    val2: f32,        // Distance field value at pos2 (positive = solid)
) -> Vec3<f64> {
    // Linear interpolation to find zero-crossing
    let t = val1.abs() / (val1.abs() + val2.abs());
    pos1 + (pos2 - pos1) * t as f64
}

// For discrete voxels (no distance field):
// Just use 0.5 (midpoint of edge)
fn interpolate_vertex_simple(pos1: Vec3<f64>, pos2: Vec3<f64>) -> Vec3<f64> {
    (pos1 + pos2) * 0.5
}
```

### Lookup Tables

**Edge Table (256 entries):**
```rust
// Which edges have vertices for each case
const EDGE_TABLE: [u16; 256] = [
    0x000, 0x109, 0x203, 0x30a, 0x406, 0x50f, 0x605, 0x70c,
    0x80c, 0x905, 0xa0f, 0xb06, 0xc0a, 0xd03, 0xe09, 0xf00,
    // ... 240 more entries
];

// Example case 105 (binary 01101001):
// Corners: 0,3,5,6 solid
// EDGE_TABLE[105] = 0x0b2a
// Binary: 0000 1011 0010 1010
// Edges with vertices: 1,3,5,8,9,11
```

**Triangle Table (256 × 15 entries):**
```rust
// How to connect edges into triangles (max 5 triangles = 15 indices)
const TRIANGLE_TABLE: [[i8; 15]; 256] = [
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],  // Case 0: no triangles
    [0, 8, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],     // Case 1: one triangle
    // ... 254 more cases
];

// -1 terminates the list (< 15 vertices may be used)
```

**Tables available online:**
- Paul Bourke: http://paulbourke.net/geometry/polygonise/
- Copy-paste into Rust (public domain)

---

### Algorithm Implementation

```rust
pub struct MarchingCubes {
    edge_table: [u16; 256],
    triangle_table: [[i8; 15]; 256],
}

impl MarchingCubes {
    pub fn extract_mesh(&self, octree: &Octree, bounds: AABB) -> Mesh {
        let mut vertices = Vec::new();
        let mut triangles = Vec::new();
        
        // Iterate over all voxel cubes in bounds
        for x in bounds.min.x..bounds.max.x {
            for y in bounds.min.y..bounds.max.y {
                for z in bounds.min.z..bounds.max.z {
                    self.process_cube(
                        octree,
                        Vec3::new(x, y, z),
                        &mut vertices,
                        &mut triangles,
                    );
                }
            }
        }
        
        Mesh { vertices, triangles }
    }
    
    fn process_cube(
        &self,
        octree: &Octree,
        cube_min: Vec3<i64>,
        vertices: &mut Vec<Vertex>,
        triangles: &mut Vec<u32>,
    ) {
        // 1. Sample 8 cube corners
        let mut cube_corners = [false; 8];
        let mut corner_positions = [Vec3::ZERO; 8];
        
        for i in 0..8 {
            let offset = Vec3::new(
                (i & 1) as i64,
                ((i >> 1) & 1) as i64,
                ((i >> 2) & 1) as i64,
            );
            let pos = cube_min + offset;
            corner_positions[i] = voxel_to_ecef(pos);
            
            let material = octree.get_voxel(pos);
            cube_corners[i] = material != Material::AIR;
        }
        
        // 2. Calculate cube index
        let cube_index = self.cube_index(cube_corners);
        
        // 3. Check edge table
        let edges = self.edge_table[cube_index as usize];
        if edges == 0 {
            return;  // No surface in this cube
        }
        
        // 4. Generate vertices on intersected edges
        let mut edge_vertices = [Vec3::ZERO; 12];
        for edge in 0..12 {
            if (edges & (1 << edge)) != 0 {
                // This edge has a vertex
                let (v1, v2) = EDGE_CONNECTIONS[edge];
                edge_vertices[edge] = self.interpolate_vertex(
                    corner_positions[v1],
                    corner_positions[v2],
                );
            }
        }
        
        // 5. Generate triangles from triangle table
        let tri_table = self.triangle_table[cube_index as usize];
        let mut i = 0;
        while i < 15 && tri_table[i] != -1 {
            // Get 3 vertices for this triangle
            let v0 = edge_vertices[tri_table[i] as usize];
            let v1 = edge_vertices[tri_table[i + 1] as usize];
            let v2 = edge_vertices[tri_table[i + 2] as usize];
            
            // Calculate normal
            let normal = (v1 - v0).cross(v2 - v0).normalize();
            
            // Add vertices and triangle indices
            let idx = vertices.len() as u32;
            vertices.push(Vertex { position: v0, normal });
            vertices.push(Vertex { position: v1, normal });
            vertices.push(Vertex { position: v2, normal });
            
            triangles.push(idx);
            triangles.push(idx + 1);
            triangles.push(idx + 2);
            
            i += 3;
        }
    }
    
    fn cube_index(&self, corners: [bool; 8]) -> u8 {
        let mut index = 0u8;
        for i in 0..8 {
            if corners[i] {
                index |= 1 << i;
            }
        }
        index
    }
    
    fn interpolate_vertex(&self, pos1: Vec3<f64>, pos2: Vec3<f64>) -> Vec3<f64> {
        // Simple midpoint (can improve with distance field)
        (pos1 + pos2) * 0.5
    }
}

// Edge connections (which vertices each edge connects)
const EDGE_CONNECTIONS: [(usize, usize); 12] = [
    (0, 1), (1, 2), (2, 3), (3, 0),  // Bottom face
    (4, 5), (5, 6), (6, 7), (7, 4),  // Top face
    (0, 4), (1, 5), (2, 6), (3, 7),  // Vertical edges
];
```

---

## OPTIMIZATIONS (Future)

### 1. Vertex Sharing
**Problem:** Duplicate vertices at cube edges

**Solution:** Hash vertex positions, reuse indices
```rust
let mut vertex_map: HashMap<(i64, i64, i64), u32> = HashMap::new();

// Before adding vertex, check if already exists
let key = (pos.x as i64, pos.y as i64, pos.z as i64);
let index = *vertex_map.entry(key).or_insert_with(|| {
    let idx = vertices.len() as u32;
    vertices.push(Vertex { position: pos, normal });
    idx
});
```

**Benefit:** ~50% fewer vertices, smaller mesh

---

### 2. Normal Smoothing
**Problem:** Flat shading (one normal per triangle)

**Solution:** Average normals at shared vertices
```rust
// After generating all triangles, smooth normals
for tri in triangles.chunks(3) {
    let n = calculate_face_normal(vertices[tri[0]], vertices[tri[1]], vertices[tri[2]]);
    
    // Accumulate normals at each vertex
    vertex_normals[tri[0]] += n;
    vertex_normals[tri[1]] += n;
    vertex_normals[tri[2]] += n;
}

// Normalize accumulated normals
for normal in &mut vertex_normals {
    *normal = normal.normalize();
}
```

**Benefit:** Smoother lighting, more organic look

---

### 3. Parallel Processing
**Problem:** Processing millions of cubes is slow

**Solution:** Process chunks in parallel
```rust
use rayon::prelude::*;

let meshes: Vec<Mesh> = chunks.par_iter()
    .map(|chunk| marching_cubes.extract_mesh(chunk))
    .collect();

// Merge meshes
let final_mesh = merge_meshes(meshes);
```

**Benefit:** 4-8× speedup (one thread per CPU core)

---

### 4. LOD (Level of Detail)
**Problem:** Too many triangles at distance

**Solution:** Use larger voxels for distant terrain
```rust
// Distance from camera determines voxel size
let voxel_size = if distance < 100.0 {
    1.0  // 1m voxels (high detail)
} else if distance < 1000.0 {
    2.0  // 2m voxels (medium detail)
} else {
    4.0  // 4m voxels (low detail)
};
```

**Benefit:** 75% fewer triangles, better framerate

---

## VALIDATION TESTS

### Test 1: Single Solid Cube
```rust
#[test]
fn test_single_cube() {
    let mut octree = Octree::new();
    octree.set_voxel(Vec3::new(0, 0, 0), Material::STONE);
    
    let mesh = marching_cubes.extract_mesh(&octree);
    
    // Should generate cube-like mesh
    assert!(mesh.triangles.len() > 0);
    assert!(mesh.vertices.len() > 0);
}
```

### Test 2: Sphere of Voxels
```rust
#[test]
fn test_sphere() {
    let mut octree = Octree::new();
    
    // Fill sphere (r=5m)
    for x in -5..=5 {
        for y in -5..=5 {
            for z in -5..=5 {
                if x*x + y*y + z*z <= 25 {
                    octree.set_voxel(Vec3::new(x, y, z), Material::STONE);
                }
            }
        }
    }
    
    let mesh = marching_cubes.extract_mesh(&octree);
    
    // Should look spherical (smooth surface)
    // Visual inspection needed
    assert!(mesh.triangles.len() > 100);  // Many triangles for smooth sphere
}
```

### Test 3: Flat Terrain
```rust
#[test]
fn test_flat_terrain() {
    let mut octree = Octree::new();
    
    // Fill bottom half (flat ground at z=0)
    for x in 0..10 {
        for y in 0..10 {
            for z in -5..0 {
                octree.set_voxel(Vec3::new(x, y, z), Material::STONE);
            }
        }
    }
    
    let mesh = marching_cubes.extract_mesh(&octree);
    
    // Should generate flat top surface
    // All top vertices should have z ≈ 0
    for vertex in &mesh.vertices {
        if vertex.position.z > -0.5 && vertex.position.z < 0.5 {
            // This is a top surface vertex
            assert!(vertex.normal.z > 0.9);  // Normal points up
        }
    }
}
```

---

## IMPLEMENTATION TIMELINE

**Phase 5: Mesh Extraction (6-8 hours)**

**Step 1: Copy lookup tables (30 min)**
- Get EDGE_TABLE and TRIANGLE_TABLE from Paul Bourke
- Convert to Rust const arrays
- Add to `src/marching_cubes.rs`

**Step 2: Write core algorithm (2 hours)**
- Implement `process_cube()`
- Implement `cube_index()`
- Implement `interpolate_vertex()`
- Generate vertices and triangles

**Step 3: Write tests (2 hours)**
- Test single cube
- Test sphere
- Test flat terrain
- Test cliff (SRTM data)

**Step 4: Optimize (2 hours)**
- Vertex sharing (reduce duplicate vertices)
- Normal smoothing (better lighting)
- Profile performance

**Step 5: Validate (1 hour)**
- Visual inspection (screenshots)
- Compare to reference photos
- Check for holes, artifacts

**Step 6: Commit (30 min)**
- All tests pass ✅
- No warnings ✅
- Git commit

---

## SUMMARY

**Algorithm chosen: MARCHING CUBES**

**Why:**
- ✅ Simple (~300 lines)
- ✅ Fast (lookup tables)
- ✅ Well-documented (easy to implement)
- ✅ Good enough quality (No Man's Sky level)
- ✅ Can iterate later (upgrade to Dual Contouring if needed)

**How:**
- Process each voxel cube (8 corners)
- Lookup edge intersections (edge table)
- Generate triangles (triangle table)
- Interpolate vertex positions (linear)
- Output smooth triangle mesh

**Performance:**
- ~1ms per 1000 cubes
- 1km² terrain (~1M cubes) = ~1 second
- Acceptable for Phase 6 first render

**Future upgrades:**
- Vertex sharing (reduce mesh size)
- Normal smoothing (better lighting)
- Parallel processing (multi-core)
- LOD (distant terrain coarser)
- Dual Contouring (if buildings need sharp corners)

**Question 7: ANSWERED ✅**

