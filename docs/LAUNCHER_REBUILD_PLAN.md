# Clean Launcher Rebuild Plan

## Goal
Build a production-quality launcher that matches what was working, but with clean architecture and room for expansion.

## What Was Working (Target State)
- ✅ Terrain generation from SRTM data
- ✅ Chunk streaming (load/unload based on player position)
- ✅ Real-time multiplayer (P2P, see other players)
- ✅ Voxel editing (dig/place blocks)
- ✅ Physics and collision
- ✅ FPS: 60 after terrain loads
- ✅ 3 instances running simultaneously

## What Needs To Be Added
- 🎨 Proper UI system (menus, HUD, settings)
- 📊 Loading screens with progress
- ⚙️ Configuration system (settings.json)
- 💾 Player position save/load
- 💾 Voxel edit persistence
- 🎮 Game options (graphics, controls, network)
- 🧹 Clean, maintainable code structure

## Architecture (Clean Room Design)

### Phase 1: Core Systems (Working Foundation)
```
examples/phase2_clean.rs
│
├── Initialization
│   ├── Config loading (settings.json)
│   ├── Graphics setup (wgpu)
│   ├── Terrain generator (SRTM)
│   └── Physics world (Rapier)
│
├── Loading Phase
│   ├── Show loading screen UI
│   ├── Load spawn area chunks (30 chunks)
│   ├── Progress bar updates
│   └── Wait for minimum playable area
│
├── Game Loop
│   ├── Input handling (WASD, mouse, etc.)
│   ├── Player movement + physics
│   ├── Chunk streaming (update, process queues)
│   ├── Voxel operations (dig/place)
│   ├── Rendering (terrain, crosshair, HUD)
│   └── Frame timing (maintain 60 FPS)
│
└── Multiplayer (P2P)
    ├── Network thread (tokio + libp2p)
    ├── Player state sync (position, rotation)
    ├── Remote player rendering
    └── Voxel operation broadcast
```

### Phase 2: Polish & Features
- UI system (egui or custom)
- Settings menu
- Save/load system
- Performance optimizations

## Implementation Order

### Step 1: Minimal Viable Launcher (Single Player)
**Goal:** Terrain loads, player can walk around, dig/place blocks
**Files:**
- `examples/phase2_clean.rs` - New clean launcher
- Use existing: `ChunkStreamer`, `TerrainGenerator`, `Physics`, `Renderer`
- Skip: Multiplayer, UI, persistence

**Success Criteria:**
- Compiles
- Loads 30 chunks in ~20 seconds
- FPS 60 after loading
- Player has collision
- Can dig/place blocks (no save yet)

### Step 2: Add Multiplayer
**Goal:** P2P networking, see other players
**Add:**
- Network thread (existing `NetworkNode`)
- Player state sync
- Remote player rendering
- Voxel operation broadcast

**Success Criteria:**
- 3 instances connect via mDNS
- Players see each other
- Voxel edits visible to all

### Step 3: Add Persistence
**Goal:** Save/load works
**Add:**
- Player position persistence (`PlayerPersistence`)
- Voxel edit persistence (`UserContent` integration)
- World directory management

**Success Criteria:**
- Player spawns at last position
- Voxel edits persist across restarts
- Each identity has separate world_data

### Step 4: Add UI & Polish
**Goal:** Professional game experience
**Add:**
- Loading screen with progress bar
- Settings menu
- Configuration system
- HUD improvements
- Performance metrics

## Code Structure (Clean)

```rust
// Phase 2 Clean Launcher Structure

struct GameConfig {
    graphics: GraphicsConfig,
    gameplay: GameplayConfig,
    network: NetworkConfig,
}

struct GameState {
    player: Player,
    camera: Camera,
    physics: PhysicsWorld,
    chunk_streamer: ChunkStreamer,
    multiplayer: Option<MultiplayerState>,
}

fn main() {
    // 1. Load config
    let config = GameConfig::load_or_default();
    
    // 2. Initialize systems
    let (window, event_loop) = init_window(&config);
    let renderer = init_renderer(&window);
    let terrain_gen = init_terrain_generator();
    let physics = init_physics();
    
    // 3. Loading phase
    show_loading_screen(&renderer);
    let chunk_streamer = load_spawn_area(terrain_gen, config.spawn_pos);
    
    // 4. Initialize game state
    let mut game = GameState::new(config, chunk_streamer, physics);
    
    // 5. Optional: Start multiplayer
    if config.network.enabled {
        game.multiplayer = Some(start_multiplayer(config.identity));
    }
    
    // 6. Game loop
    event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent { event, .. } => handle_input(&mut game, event),
            Event::AboutToWait => {
                update(&mut game);
                render(&renderer, &game);
            }
        }
    });
}

fn update(game: &mut GameState) {
    // Update in order:
    1. Process input -> player movement
    2. Update physics (collision, gravity)
    3. Update chunk streaming (load/unload)
    4. Process voxel operations
    5. Update multiplayer (if enabled)
    6. Update camera from player
}

fn render(renderer: &Renderer, game: &GameState) {
    // Render in order:
    1. Terrain chunks
    2. Remote players (if multiplayer)
    3. Crosshair
    4. HUD (FPS, position, mode)
}
```

## Key Differences from Old Launcher

### Old (phase1_multiplayer.rs)
- ❌ 1200+ lines, hard to follow
- ❌ Systems initialized in random order
- ❌ No clear separation of concerns
- ❌ Hacked-on features (sliding window broke it)
- ❌ No UI/loading screen
- ❌ Magic numbers everywhere

### New (phase2_clean.rs)
- ✅ Modular, clear structure
- ✅ Logical initialization order
- ✅ Separation: config, init, load, game loop, update, render
- ✅ Room for UI/loading screens
- ✅ Configurable settings
- ✅ Comments and documentation
- ✅ Easy to test and debug

## Testing Strategy

### Step 1 (Minimal):
1. Build and run
2. Wait for loading (should see progress)
3. Verify FPS 60
4. Walk around (WASD)
5. Dig/place blocks (E/Q)

### Step 2 (Multiplayer):
1. Launch 3 instances
2. Verify they connect via mDNS
3. Move in instance 1, see in instances 2/3
4. Dig in instance 1, see hole in instances 2/3

### Step 3 (Persistence):
1. Move player to new location
2. Close launcher
3. Relaunch - player should be at same location
4. Dig holes
5. Close launcher  
6. Relaunch - holes should still be there

### Step 4 (UI/Polish):
1. Loading screen shows progress
2. Settings menu works
3. Performance is smooth

## Next Action

**Ready to start?** I'll create `examples/phase2_clean.rs` with:
- Clean structure
- Comments explaining each section
- Proper error handling
- Room for expansion

Should take ~2-3 hours to get Step 1 (minimal viable) working.
