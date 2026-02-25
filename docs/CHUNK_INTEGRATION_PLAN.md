# Chunk Manager Integration Plan

## Status: Infrastructure Complete, Integration Pending

**What's Done:**
- ✅ ChunkManager struct (294 lines)
- ✅ generate_chunk() pure function
- ✅ GPS bounds calculation
- ✅ ChunkData per-chunk state
- ✅ Compiles successfully

**What's Next:**
- ⏳ Integrate ChunkManager into phase1_multiplayer.rs
- ⏳ Per-chunk rendering
- ⏳ Per-chunk collision

---

## Integration Changes Required

### 1. Replace Monolithic Terrain (lines 212-252)

**Current:**
```rust
let mut octree = Octree::new();
let mut generator = TerrainGenerator::new(elevation_pipeline);
generator.generate_region(&mut octree, &origin_gps, 100.0);
let mesh = extract_octree_mesh(&octree, &origin_voxel, 7);
let mut mesh_buffer = MeshBuffer::from_mesh(&context.device, &mesh);
```

**New:**
```rust
let generator = TerrainGenerator::new(elevation_pipeline);
let user_content = UserContentLayer::new();
let mut chunk_manager = ChunkManager::new(generator, user_content);
chunk_manager.update_visible_chunks(&spawn_chunk, 2, &world_dir);
```

### 2. Replace Collision Generation (lines 275-281)

**Current:**
```rust
let mut terrain_collider = metaverse_core::physics::update_region_collision(
    &mut physics,
    &octree,
    &origin_voxel,
    7,
    None,
);
```

**New:**
```rust
// Per-chunk collision (in separate loop over loaded chunks)
for chunk_data in chunk_manager.loaded_chunks_mut() {
    if chunk_data.dirty && chunk_data.collider.is_none() {
        let collider = update_chunk_collision(&mut physics, &chunk_data.octree);
        chunk_data.collider = Some(collider);
    }
}
```

### 3. Replace Dig/Place (lines 545-600)

**Current:**
```rust
if let Some(dug) = player.dig_voxel(&physics, &mut octree, 10.0) {
    // ... broadcast ...
    mesh_dirty = true;
}
```

**New:**
```rust
if let Some(dug) = player.dig_voxel(&physics, &mut chunk_manager, 10.0) {
    // ... broadcast ...
    // Chunk automatically marked dirty by chunk_manager.set_voxel()
}
```

### 4. Replace Mesh Regeneration (lines 730-743)

**Current:**
```rust
if mesh_dirty {
    let new_mesh = extract_octree_mesh(&octree, &origin_voxel, 7);
    mesh_buffer = MeshBuffer::from_mesh(&context.device, &new_mesh);
    terrain_collider = update_region_collision(...);
    mesh_dirty = false;
}
```

**New:**
```rust
// Regenerate only dirty chunks
for chunk_data in chunk_manager.loaded_chunks_mut() {
    if chunk_data.dirty {
        let min_voxel = chunk_data.chunk_id.min_voxel();
        let mesh = extract_octree_mesh(&chunk_data.octree, &min_voxel, 7);
        chunk_data.mesh_buffer = Some(MeshBuffer::from_mesh(&context.device, &mesh));
        
        // Update collision
        let collider = update_chunk_collision(&mut physics, &chunk_data.octree);
        if let Some(old_collider) = chunk_data.collider {
            physics.colliders.remove(old_collider, ...);
        }
        chunk_data.collider = Some(collider);
        
        chunk_data.dirty = false;
    }
}
```

### 5. Replace Rendering (line 463)

**Current:**
```rust
pipeline.render(&context, &camera_mat, &mesh_buffer);
```

**New:**
```rust
// Render all loaded chunks
for chunk_data in chunk_manager.loaded_chunks() {
    if let Some(mesh_buffer) = &chunk_data.mesh_buffer {
        pipeline.render(&context, &camera_mat, mesh_buffer);
    }
}
```

### 6. Replace Operation Replay (lines 377-380)

**Current:**
```rust
for op in user_content.op_log() {
    octree.set_voxel(op.coord, op.material.to_material_id());
}
```

**New:**
```rust
// Operations already loaded by chunk_manager.load_chunk()
// (handled internally by ChunkManager)
```

---

## Files to Modify

1. **examples/phase1_multiplayer.rs** (~150 lines changed)
   - Remove: `octree`, `mesh_buffer`, `terrain_collider`, `mesh_dirty`
   - Add: `chunk_manager`
   - Refactor: terrain gen, rendering, collision, dig/place

2. **src/physics.rs** (new function needed)
   - Add `update_chunk_collision()` helper
   - Takes single octree (chunk), returns collider

---

## Testing Plan

After integration:

1. **Compile test** - Ensure no errors
2. **Single chunk test** - Spawn, verify terrain exists
3. **Multi-chunk test** - Move around, verify chunks load/unload
4. **Dig test** - Dig in one chunk, verify only that chunk regenerates
5. **Performance test** - Verify no "1-second pause" (only ~100ms per chunk)
6. **Persistence test** - Dig, exit, restart, verify persists
7. **Multiplayer test** - 3 clients, dig in different chunks

---

## Estimated Effort

- **Integration:** 1-2 hours
- **Per-chunk rendering:** 30 minutes
- **Per-chunk collision:** 30 minutes  
- **Testing:** 1 hour
- **Total:** 3-4 hours

---

## Current Blockers

None - infrastructure is complete and compiles.

**Ready to proceed with integration.**
