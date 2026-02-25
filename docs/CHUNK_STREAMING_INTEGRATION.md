# Chunk Streaming Integration Guide

**Status:** Implementation guide for Phase 1 Multiplayer integration  
**Date:** 2026-02-19

## Overview

ChunkStreamer provides dynamic chunk loading/unloading based on player position.  
This replaces the current static `load_chunks_immediate()` approach with continuous streaming.

## Integration Points

### 1. Initialization (examples/phase1_multiplayer.rs:252)

**Current Code:**
```rust
let mut chunk_manager = ChunkManager::new(generator, user_content);
chunk_manager.load_chunks_immediate(&spawn_chunk, 2, &world_dir);
```

**With ChunkStreamer:**
```rust
use metaverse_core::chunk_streaming::{ChunkStreamer, ChunkStreamerConfig};
use metaverse_core::player_persistence::PlayerPersistence;

// Load or create player persistence
let persistence_path = world_dir.join("player.json");
let player_data = PlayerPersistence::load(&persistence_path)
    .unwrap_or_else(|_| PlayerPersistence::default_at_spawn());

// Use saved position as spawn
let spawn_pos = player_data.position.to_ecef();
let spawn_chunk = ChunkId::from_ecef(&spawn_pos);

// Create chunk streamer with config
let config = ChunkStreamerConfig {
    load_radius_m: 500.0,           // 500m radius
    unload_radius_m: 1000.0,        // Unload beyond 1km
    max_loaded_chunks: 100,         // Memory limit
    safe_zone_radius: 1,            // 3×3 chunks always loaded
    frame_budget_ms: 5.0,           // 5ms per frame
    placeholder_style: PlaceholderStyle::Wireframe,
};

let mut chunk_streamer = ChunkStreamer::new(config);

// Initial load (loads safe zone + priority chunks)
chunk_streamer.update(spawn_pos);
chunk_streamer.process_queues(100.0);  // 100ms budget for initial load

// Chunk manager still used for terrain generation
let mut chunk_manager = ChunkManager::new(generator, user_content);
```

### 2. Game Loop Update (main render loop)

**Add to main loop (every frame):**
```rust
// Update chunk streaming based on player position
let player_ecef = player_gps.to_ecef();

// Update desired chunks (calculates what should be loaded)
chunk_streamer.update(player_ecef);

// Process loading/unloading queues (respects time budget)
let more_work = chunk_streamer.process_queues(5.0);  // 5ms budget

if more_work {
    // Queue is not empty, will continue next frame
    // This is normal - chunks load gradually
}
```

### 3. Rendering

**Current approach** (renders all loaded chunks):
```rust
for (chunk_id, chunk) in chunk_manager.chunks.iter() {
    render_chunk(chunk);
}
```

**With ChunkStreamer:**
```rust
for (chunk_id, loaded_chunk) in chunk_streamer.loaded_chunks.iter() {
    // Render placeholder if still loading
    if loaded_chunk.load_state == ChunkLoadState::Loading {
        if let Some(ref placeholder_mesh) = loaded_chunk.placeholder_mesh {
            render_wireframe(placeholder_mesh, chunk_id);
        }
        continue;
    }
    
    // Render actual terrain if loaded
    if loaded_chunk.load_state == ChunkLoadState::Loaded {
        if let Some(ref mesh) = loaded_chunk.mesh {
            render_chunk_mesh(mesh, chunk_id);
        }
    }
}
```

### 4. Player Persistence (on exit)

**Add to shutdown:**
```rust
// Save player position before exit
let player_persistence = PlayerPersistence {
    position: player_gps,
    yaw: camera.yaw,
    pitch: camera.pitch,
    movement_mode: if is_flying { "fly" } else { "walk" }.to_string(),
    last_saved: SystemTime::now(),
};

player_persistence.save(&persistence_path)
    .expect("Failed to save player position");
```

### 5. Cross-Chunk Voxel Operations

**Current:** Operations may affect chunk not yet loaded  
**Solution:** Check if chunk is loaded before applying operation

```rust
fn apply_voxel_op(op: &VoxelOperation, chunk_streamer: &mut ChunkStreamer) -> Result<(), String> {
    // Find affected chunks
    let affected = ChunkId::chunks_affected_by_voxel(&op.voxel);
    
    for chunk_id in affected {
        // Only apply if chunk is loaded
        if let Some(loaded_chunk) = chunk_streamer.loaded_chunks.get_mut(&chunk_id) {
            // Apply operation to octree
            apply_to_octree(&mut loaded_chunk.octree, op);
            
            // Mark mesh as dirty (needs regeneration)
            loaded_chunk.mesh = None;
        } else {
            // Chunk not loaded - queue operation for later
            // Or: Load chunk immediately if in safe zone
            eprintln!("⚠️  Operation affects unloaded chunk: {:?}", chunk_id);
        }
    }
    
    Ok(())
}
```

## Testing Strategy

### Test 1: Walk 1km
**Goal:** Validate load/unload cycling

```bash
cargo run --release --example phase1_multiplayer
```

1. Spawn at Mount Everest
2. Walk north for 1km (W key held)
3. Monitor console output:
   - "📦 Loading chunk X" - new chunks load ahead
   - "🗑️  Unloading chunk Y" - old chunks unload behind
4. Verify FPS stays > 55 (60 FPS target, 5ms budget)
5. Verify memory stays < 2GB

**Success Criteria:**
- ✅ Chunks load ahead of player
- ✅ Chunks unload behind player
- ✅ No frame drops (FPS > 55)
- ✅ No memory leaks (use `htop`)

### Test 2: Fly 10km
**Goal:** Test emergency unload system

```bash
cargo run --release --example phase1_multiplayer
```

1. Toggle fly mode (F key)
2. Fly upward 500m (Space held)
3. Fly horizontal 10km at high speed (W + Shift)
4. Monitor statistics:
   - chunks_loaded should cap at ~100
   - emergency_unloads should trigger
5. Verify no crash, no OOM

**Success Criteria:**
- ✅ Max chunks caps at configured limit
- ✅ Emergency unload triggers (console message)
- ✅ Game continues without crash
- ✅ FPS degradation acceptable (> 30 FPS)

### Test 3: Position Persistence
**Goal:** Validate save/load

```bash
# Run 1
cargo run --release --example phase1_multiplayer
# Walk to new location
# Exit (Ctrl+C)

# Run 2
cargo run --release --example phase1_multiplayer
# Should spawn at last location (not Mount Everest)
```

**Success Criteria:**
- ✅ Player spawns at saved location
- ✅ Nearby chunks load immediately
- ✅ No artifacts or glitches

## Performance Metrics

**Expected Performance:**
- **FPS:** 60 (with 5ms chunk budget)
- **Memory:** ~1GB for 100 chunks
- **Load Time:** ~50ms per chunk (background thread)
- **Chunks Loaded:** 20-30 typical, 100 max

**Monitoring:**
```rust
println!("📊 Chunk Streaming Stats:");
println!("   Loaded: {}", chunk_streamer.stats.chunks_loaded);
println!("   Queued: {}", chunk_streamer.stats.chunks_queued);
println!("   Loading: {}", chunk_streamer.stats.chunks_loading);
println!("   Emergency Unloads: {}", chunk_streamer.stats.emergency_unloads);
println!("   Avg Load Time: {:.1}ms", chunk_streamer.chunk_loader.avg_load_time_ms());
```

## Migration Path

### Phase 1: Parallel Systems
- Keep existing ChunkManager for now
- Add ChunkStreamer alongside
- ChunkStreamer controls loading
- ChunkManager provides terrain generation

### Phase 2: Gradual Migration
- Move mesh generation into ChunkStreamer
- Move collision generation into ChunkStreamer
- ChunkManager becomes pure terrain generator

### Phase 3: Full Integration
- Remove ChunkManager
- ChunkStreamer handles everything
- Background terrain generation
- Background mesh generation
- Background collision generation

## Known Limitations

1. **Mesh Generation:** Currently synchronous, needs background thread
2. **Collision Generation:** Currently synchronous, blocks frame
3. **Terrain Generation:** Empty octrees for now, needs integration
4. **Network Sync:** Chunks may load/unload during state sync (rare race condition)

## Future Enhancements

1. **Priority Boost:** Load chunks in player's view frustum first
2. **Predictive Loading:** Predict movement direction, preload ahead
3. **LOD Integration:** Load distant chunks at lower detail
4. **Compression:** Store compressed octrees, decompress on load
5. **Streaming from Disk:** Save chunks to disk, load on demand

---

**Status:** Ready for integration  
**Estimated Integration Time:** 2-3 hours  
**Risk Level:** Medium (requires testing at scale)
