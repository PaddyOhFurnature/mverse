# Terrain Optimization Roadmap

## Current State (Baseline)

**What works:**
- ✅ 250m × 250m in 10s (1.5M vertices, 60 FPS)
- ✅ 500m × 500m in 60s (6.1M vertices, 60 FPS)
- ✅ Rendering performance excellent
- ✅ Core algorithm correct

**Hard limits found:**
- ❌ GPU buffer: 256 MB maximum
- ❌ Monolithic mesh: 500m practical limit
- ❌ Mesh extraction: 60s too slow for gameplay
- ❌ No streaming: Must load entire area upfront

**Must solve before adding:**
- Buildings (OSM data)
- Roads/paths
- Vegetation
- Water
- Multiplayer

## The Streaming/Fade System Explained

### What You See in Modern Games

**Problem:** Loading 5km of terrain takes minutes, player would see black screen

**Solution:** Progressive refinement with smooth transitions

```
Camera moves → System detects → Background loads → Fades in
     ↓              ↓                  ↓              ↓
   WASD       "Need chunk 5,7"   Mesh generates   Alpha: 0→1
                   ↓                  ↓              ↓
            Check if loaded      Takes 2-10s    Invisible → Visible
                   ↓                               ↓
            Not loaded?                    Player never sees "pop"
                   ↓
            Queue for loading
```

### How It Works (Technical)

1. **Spatial Grid** - World divided into chunks (e.g., 256m × 256m)
2. **Active Set** - Chunks within view distance (e.g., 2km radius)
3. **Background Thread** - Generates meshes without blocking rendering
4. **Upload Queue** - New meshes sent to GPU between frames
5. **Alpha Blending** - New chunks fade in over 0.5-1.0 seconds
6. **Unload Old** - Chunks beyond distance deleted to free memory

### Example: Player Walking

```
Frame 1: Player at (0, 0)
  - Active chunks: 0,0 | 0,1 | 1,0 | 1,1 (loaded)
  - Rendering: 4 chunks, 60 FPS

Frame 5000: Player walks to (250, 0) - entered new chunk!
  - Detect: Chunk 2,0 now in range
  - Check: Not loaded yet
  - Action: Queue for background generation
  - Result: Keep rendering, no stutter

Frame 5120: Chunk 2,0 generation complete (2 seconds later)
  - Upload mesh to GPU (takes 1 frame, ~16ms)
  - Start fade-in: alpha = 0.0
  - Each frame: alpha += 0.02 (50 frames = 1 second)
  - Result: Chunk smoothly appears

Frame 5170: Fade complete
  - Chunk 2,0 fully visible (alpha = 1.0)
  - Active chunks: 0,0 | 0,1 | 1,0 | 1,1 | 2,0
  - Old chunk -1,0 beyond distance: unload, free memory
```

**Player never sees:**
- Black screen
- Loading bar
- Sudden "pop-in"
- Frame drops
- Memory exhaustion

**Player DOES see:**
- Smooth 60 FPS always
- Terrain gently fades in as they walk
- Infinite world feeling
- Distant terrain (lower detail) already there

## Optimization Priority Order

### Phase 1: Chunk System (CRITICAL - Enables Everything Else)

**Why first:** 
- Mandatory for 1km+ (GPU buffer limit)
- Foundation for all other optimizations
- Enables streaming, LOD, culling

**What to build:**
```rust
struct Chunk {
    id: ChunkId,           // (face, x, y)
    mesh: Option<Mesh>,    // None = not loaded
    state: ChunkState,     // Generating, Ready, Uploaded, Fading
    alpha: f32,            // For fade-in (0.0 to 1.0)
}

enum ChunkState {
    NotLoaded,       // Hasn't been requested
    Queued,          // Requested, waiting for thread
    Generating,      // Background thread working
    Ready,           // Mesh complete, needs GPU upload
    Uploaded,        // On GPU, visible
    FadingIn,        // Transitioning alpha 0→1
}

struct ChunkManager {
    chunks: HashMap<ChunkId, Chunk>,
    active_set: HashSet<ChunkId>,
    load_queue: VecDeque<ChunkId>,
    upload_queue: VecDeque<(ChunkId, Mesh)>,
}
```

**Implementation steps:**
1. Define chunk grid (256m × 256m recommended)
2. Split `generate_region()` into `generate_chunk()`
3. Create ChunkManager with HashMap storage
4. Implement active set calculation (chunks within distance)
5. Basic rendering (no streaming yet, just prove chunks work)

**Success criteria:**
- Load 4 chunks (512m × 512m total) as separate meshes
- Each chunk <50 MB (well under 256 MB limit)
- Same visual result as current monolithic mesh
- Can query/access individual chunks

**Time estimate:** 1-2 days

---

### Phase 2: Async Background Loading (CRITICAL - No More Waiting)

**Why second:**
- Eliminates startup delay
- Player can start moving immediately
- Hides the 60s generation time

**What to build:**
```rust
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;

struct AsyncChunkLoader {
    worker_threads: Vec<JoinHandle<()>>,
    request_tx: Sender<ChunkRequest>,
    result_rx: Receiver<ChunkResult>,
}

struct ChunkRequest {
    chunk_id: ChunkId,
    origin: GPS,
    lod: u8,
}

struct ChunkResult {
    chunk_id: ChunkId,
    mesh: Mesh,
    generation_time: Duration,
}
```

**Implementation steps:**
1. Create background worker threads (4-8 threads)
2. Request channel: Main thread → Workers
3. Result channel: Workers → Main thread
4. Workers call `generate_chunk()` independently
5. Main thread polls results each frame
6. Upload completed meshes to GPU

**Success criteria:**
- Viewer starts in <1 second (shows empty world)
- Chunks appear as they finish (2-10s each)
- 60 FPS maintained during generation
- Can queue unlimited chunks without blocking

**Time estimate:** 1-2 days

---

### Phase 3: Fade-In Transitions (POLISH - Looks Professional)

**Why third:**
- Makes streaming invisible to player
- AAA visual quality
- Simple to implement once chunks work

**What to build:**
```rust
// In vertex shader
struct VertexOutput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @builtin(position) clip_position: vec4<f32>,
};

// In fragment shader
@group(2) @binding(0)
var<uniform> chunk_alpha: f32;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = vec3<f32>(0.5, 0.5, 0.5);
    let lit = apply_lighting(color, in.normal);
    return vec4<f32>(lit, chunk_alpha);  // Apply fade
}

// In main loop
for chunk in chunks.iter_mut() {
    if chunk.state == ChunkState::FadingIn {
        chunk.alpha += 0.02;  // 50 frames = 1 second
        if chunk.alpha >= 1.0 {
            chunk.alpha = 1.0;
            chunk.state = ChunkState::Uploaded;
        }
    }
}
```

**Implementation steps:**
1. Add alpha uniform to shader
2. Enable alpha blending in render pipeline
3. Update chunk alpha each frame
4. Transition FadingIn → Uploaded when alpha = 1.0

**Success criteria:**
- New chunks fade in over 0.5-1.0 seconds
- No sudden pop-in
- Smooth transition

**Time estimate:** 0.5 days

---

### Phase 4: LOD System (PERFORMANCE - Smooth Distant Terrain)

**Why fourth:**
- Reduces vertex count 10-100×
- Smooths out distant terrain (no stepping)
- Enables 5-10km view distance

**What to build:**
```rust
struct LODLevel {
    distance: f32,      // Switch distance from camera
    voxel_size: f32,    // Voxel resolution (1m, 2m, 4m, etc.)
}

const LOD_LEVELS: [LODLevel; 5] = [
    LODLevel { distance: 0.0,    voxel_size: 1.0 },   // 0-500m: Full detail
    LODLevel { distance: 500.0,  voxel_size: 2.0 },   // 500m-1km: Half detail
    LODLevel { distance: 1000.0, voxel_size: 4.0 },   // 1-2km: Quarter detail
    LODLevel { distance: 2000.0, voxel_size: 8.0 },   // 2-4km: Eighth detail
    LODLevel { distance: 4000.0, voxel_size: 16.0 },  // 4km+: Sixteenth detail
];

fn calculate_chunk_lod(chunk_pos: Vec3, camera_pos: Vec3) -> u8 {
    let distance = (chunk_pos - camera_pos).length();
    for (i, level) in LOD_LEVELS.iter().enumerate() {
        if distance < level.distance {
            return i as u8;
        }
    }
    return (LOD_LEVELS.len() - 1) as u8;
}
```

**Implementation steps:**
1. Modify `generate_chunk()` to accept LOD parameter
2. LOD 0: 1m voxels (current)
3. LOD 1: 2m voxels (skip every other column)
4. LOD 2: 4m voxels (skip 3 of 4 columns)
5. Calculate LOD per chunk based on distance
6. Regenerate chunk when LOD changes (fade transition)

**Success criteria:**
- Near chunks: 1m detail (current quality)
- Far chunks: 16m detail (1/256th vertices)
- View distance: 5km+ with 60 FPS
- Smooth LOD transitions (fade old out, new in)

**Impact:**
- 5km × 5km at mixed LOD vs all LOD 0:
  - LOD 0 only: ~400M vertices (impossible)
  - Mixed LOD: ~20M vertices (manageable)

**Time estimate:** 2-3 days

---

### Phase 5: Frustum Culling (PERFORMANCE - Don't Render Off-Screen)

**Why fifth:**
- Skip rendering chunks player can't see
- 2-3× performance boost
- Simple AABB vs frustum test

**What to build:**
```rust
struct Frustum {
    planes: [Plane; 6],  // Left, right, top, bottom, near, far
}

impl Frustum {
    fn from_camera(camera: &Camera) -> Self {
        // Extract frustum planes from view-projection matrix
    }
    
    fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        // Test if AABB is visible in frustum
    }
}

// In render loop
let frustum = Frustum::from_camera(&camera);
for chunk in chunks {
    if frustum.intersects_aabb(chunk.min, chunk.max) {
        render_chunk(chunk);  // Visible
    }
    // else: skip rendering, save GPU time
}
```

**Implementation steps:**
1. Extract frustum planes from camera matrix
2. Calculate AABB for each chunk (min/max corners)
3. Test AABB vs frustum planes
4. Only render visible chunks

**Success criteria:**
- Looking at horizon: Render ~50% of chunks
- Looking at ground: Render ~30% of chunks
- Looking at sky: Render ~10% of chunks
- Measurable FPS increase

**Time estimate:** 1 day

---

### Phase 6: Mesh Extraction Optimization (PERFORMANCE - Faster Loading)

**Why sixth:**
- Reduce 60s → 10s for 500m area
- Allows more aggressive chunk generation
- Better player experience

**Options to explore:**

1. **Profile marching cubes** - Find hot spots
2. **Parallel mesh extraction** - Use rayon for multi-thread
3. **Dual contouring** - Better for sharp features, potentially faster
4. **GPU compute** - Ultra-fast but complex
5. **Caching** - Don't regenerate static terrain

**Time estimate:** 3-5 days (research + implementation)

---

## Recommended Implementation Order

```
Week 1: Chunks + Async Loading
  Day 1-2: Chunk system (split world into 256m pieces)
  Day 3-4: Async background loading (no more waiting)
  Day 5: Integration testing + bug fixes

Week 2: Streaming Experience + LOD
  Day 1: Fade-in transitions (polish)
  Day 2-4: LOD system (multi-resolution)
  Day 5: Test 5km × 5km with mixed LOD

Week 3: Performance + Polish
  Day 1: Frustum culling
  Day 2-3: Mesh extraction optimization
  Day 4-5: Profiling, tuning, stress testing
```

## Success Metrics (End of 3 Weeks)

- ✅ 10km × 10km world loads progressively
- ✅ Viewer starts in <1 second
- ✅ 60 FPS sustained
- ✅ Chunks fade in smoothly (no pop)
- ✅ Near terrain: 1m detail
- ✅ Far terrain: 16m detail (smooth, not stepped)
- ✅ Memory: <2 GB for active chunks
- ✅ Loading: Hidden from player

## After Optimization Complete

**Then ready for:**
- OSM building import
- Road/path rendering
- Vegetation placement
- Water bodies
- Lighting/shadows
- Multiplayer (P2P chunks)

All built on a solid, scalable foundation.
