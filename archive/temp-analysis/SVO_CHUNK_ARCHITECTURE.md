# SVO Chunk Architecture - The Correct Approach

## The Problem I've Been Creating

I keep trying to voxelize the entire world (5km area) into ONE SVO. This causes:
- Out of memory (1024³ = too much even with sparse storage)
- Low resolution (256³ over 5km = 19m voxels = blocky)
- Wrong architecture (not streaming, not LOD-based)

## The Correct Architecture

### 1. World is Divided into Chunks
```
ChunkId = { face: 0-5, path: [quadtree] }
Depth 9 = ~60km chunks (recommended)
Depth 11 = ~15km chunks
```

### 2. Each Chunk Has an SVO
```
Chunk {
    id: ChunkId,
    svo: SparseVoxelOctree,  // Just this chunk's area
    bounds: (GpsPos, GpsPos), // Geographic bounds
    center: EcefPos,
}
```

### 3. SVO Generation Per Chunk
```rust
// Get chunk bounds
let (sw, ne) = chunk_bounds_gps(&chunk_id)?;
let area_size = distance(sw, ne); // e.g., 1km

// Create SVO for THIS CHUNK ONLY
let depth = 10; // 1024³
let svo_size = 1u32 << depth;
let voxel_size = area_size / svo_size as f64; // ~1m voxels

// Voxelize terrain + OSM within chunk bounds
let mut svo = SparseVoxelOctree::new(depth);
generate_terrain_from_elevation(&mut svo, |lat, lon| {
    if in_bounds(lat, lon, sw, ne) {
        srtm.get_elevation(lat, lon)
    } else {
        None
    }
}, coords_fn, voxel_size);

// Add OSM features within chunk
for building in osm_data.buildings {
    if building_in_chunk(building, sw, ne) {
        add_building(&mut svo, &chunk_center, building, voxel_size);
    }
}
```

### 4. LOD Based on Distance from Camera
```
Camera → ChunkId (player's chunk)
Load neighboring chunks within render distance

For each loaded chunk:
    distance = distance_to_camera(chunk_center, camera_pos)
    
    if distance < 50m:
        lod = 0  // Full detail, extract from depth 10 SVO
    elif distance < 200m:
        lod = 1  // High detail, extract from depth 9 SVO
    elif distance < 500m:
        lod = 2  // Medium detail, extract from depth 8 SVO
    elif distance < 1000m:
        lod = 3  // Low detail, extract from depth 7 SVO
    else:
        Don't render (culled)
```

### 5. Streaming System
```rust
struct WorldManager {
    chunks: HashMap<ChunkId, Chunk>,
    chunk_depth: usize,  // 9 for ~60km chunks
    render_distance: f64, // 1km
}

impl WorldManager {
    fn update(&mut self, camera_pos: &EcefPos) {
        // Get camera's chunk
        let camera_chunk = ecef_to_chunk_id(camera_pos, self.chunk_depth);
        
        // Find chunks in render distance
        let target_chunks = find_chunks_in_radius(camera_pos, self.render_distance);
        
        // Unload far chunks
        self.chunks.retain(|id, _| target_chunks.contains(id));
        
        // Load new chunks
        for chunk_id in target_chunks {
            if !self.chunks.contains_key(&chunk_id) {
                let chunk = generate_chunk(chunk_id, &srtm, &osm_data);
                self.chunks.insert(chunk_id, chunk);
            }
        }
    }
    
    fn extract_visible_meshes(&self, camera: &Camera) -> Vec<(Mesh, u32)> {
        let mut meshes = Vec::new();
        
        for (chunk_id, chunk) in &self.chunks {
            // Distance-based LOD
            let distance = distance_to_camera(&chunk.center, &camera.position);
            let lod = select_lod_level(distance);
            
            // Frustum culling
            if !camera.frustum_contains(&chunk.bounds) {
                continue;
            }
            
            // Extract mesh at appropriate LOD
            let mesh = generate_mesh(&chunk.svo, lod);
            meshes.push((mesh, lod));
        }
        
        meshes
    }
}
```

## Memory Budget

**Per chunk at LOD 0:**
- SVO depth 10 = 1024³ voxels
- Sparse storage = only occupied voxels
- Terrain = dense near surface, empty above/below
- Estimate: 10-50MB per chunk (mostly terrain)

**Active chunks:**
- 1km render distance, depth 9 chunks (~60km) = 1-4 active chunks
- Total memory: 40-200MB for voxel data
- Plus meshes: ~10MB per chunk = 10-40MB
- **Total: 50-240MB** - manageable

## What I Need to Implement

1. **`generate_chunk_svo(chunk_id, srtm, osm) -> SparseVoxelOctree`**
   - Get chunk bounds from ChunkId
   - Create appropriately sized SVO
   - Voxelize terrain + OSM within bounds only

2. **`WorldManager` struct**
   - Manages chunk streaming
   - Tracks loaded chunks
   - Handles LOD selection

3. **Update viewer to use WorldManager**
   - Instead of one giant SVO
   - Generate chunks on demand
   - Extract meshes at appropriate LOD

4. **Implement frustum culling**
   - Don't extract meshes outside camera view
   - Huge performance win

## Why This Works

- **Small area per chunk** = high voxel resolution (1m)
- **Sparse storage** = only stores occupied voxels
- **LOD system** = coarser detail at distance
- **Streaming** = only process nearby chunks
- **Build/destroy works** = modify voxels in relevant chunk, re-extract mesh

This is how Minecraft, GTA V, and flight simulators work.
This is the correct architecture.
