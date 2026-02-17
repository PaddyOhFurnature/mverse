# Adaptive Chunk System - Context-Aware Rendering

## The Real Architecture (What User Explained)

### NOT Traditional Game LOD
❌ Load all chunks in 2km radius
❌ Fixed LOD based on chunk distance
❌ Same view for all players
❌ Large chunks to reduce count

### ACTUALLY: Viewer-Dependent Streaming
✓ Load based on VISIBILITY + GAZE + SPEED + ACTIVITY
✓ Dynamic LOD per-viewer, per-chunk
✓ Each player sees different detail of same chunk
✓ Small chunks for fine-grained control

## Use Case Examples

### Sitting in Coffee Shop
**Context**: Stationary, indoor, focused attention
- **Visible radius**: ~20m (walls block view)
- **Chunks needed**: 10-20 chunks (10m size)
- **Detail level**: MAXIMUM nearby, fades at edges
- **Generation**: High detail for table, cup, window view
- **Network**: Tiny updates (someone walks by, car passes outside)

### Driving 100km/h on Highway
**Context**: High speed, forward focus
- **Gaze cone**: 25° forward (tunnel vision at speed)
- **Chunks needed**: 100-200 chunks (forward bias)
- **Detail level**: HIGH in front, LOW behind/sides
- **Generation**: Billboard trees, simplified buildings
- **Network**: Rapid chunk swapping as you move

### Flying in Commercial Plane
**Context**: High altitude, wide view, low detail
- **Visible area**: 50km radius
- **Chunks needed**: 500-1000 chunks (low LOD)
- **Detail level**: Terrain only, no buildings
- **Generation**: Height map, color texture
- **Network**: Large area, but minimal data per chunk

### Watching Singer on Stage
**Context**: Fixed view, high attention on one area
- **Focus**: Stage (5m × 5m area)
- **Chunks needed**: 5-10 chunks (stage + immediate area)
- **Detail level**: MAXIMUM on performer, audience blurred
- **Generation**: Facial animation, clothing detail
- **Network**: Only stage chunks updated frequently

### Hiking Mountain Trail
**Context**: Moderate speed, panoramic view
- **Visible area**: 2km radius (open terrain)
- **Chunks needed**: 200-500 chunks (terrain)
- **Detail level**: MEDIUM nearby, fade to horizon
- **Generation**: Rocks, trees, path detail close; silhouettes far
- **Network**: Terrain-only, static data

## Why Small Chunks (10-50m) Are Better

### 1. Fine-Grained Visibility
**Problem with large chunks (700m):**
- Sitting in coffee shop: You see 20m radius
- But chunk is 700m → loading 680m of invisible data
- 97% wasted!

**Solution with small chunks (10m):**
- Load only what's visible through windows/doors
- ~20 chunks = 4KB each = 80KB total
- 100% efficient

### 2. Gaze-Directed Loading
**Driving at 100km/h:**
- Large chunks: Load entire 3×3 grid ahead
- Small chunks: Load ONLY forward cone (25°)
- Save 85% of chunk loading

**Implementation:**
```rust
fn chunks_to_load(player: &Player) -> Vec<ChunkId> {
    let base_radius = match player.activity {
        Activity::Sitting => 20.0,
        Activity::Walking => 50.0,
        Activity::Driving => 200.0,
        Activity::Flying => 5000.0,
    };
    
    // Gaze direction bias
    let forward_bias = player.velocity.length() * 0.1;
    
    // Load chunks in view cone
    chunks_in_view_cone(
        player.position,
        player.gaze_direction,
        base_radius,
        forward_bias
    )
}
```

### 3. P2P Efficiency
**Large chunks (700m):**
- Someone paints wall: 700m × 700m × 100m chunk = 49M voxels affected
- Network update: 10MB delta
- All peers in chunk range: Download 10MB

**Small chunks (10m):**
- Someone paints wall: 10m × 10m × 10m chunk = 1000 voxels affected
- Network update: 10KB delta
- Only peers who can SEE that wall: Download 10KB

**P2P load distribution:**
- 1000 players painting walls: 1000 × 10KB = 10MB total
- Distributed across network
- Each peer only downloads chunks they can see

### 4. Delta Updates
**Build/destroy operations:**

Large chunk (700m):
- Dig well in backyard: Modify 1 chunk
- Update: Regenerate 238k vertices
- Network: Send 7.6MB mesh update
- All viewers: Re-render entire chunk

Small chunk (10m):
- Dig well: Modify 1 chunk  
- Update: Regenerate 50 vertices
- Network: Send 1.6KB delta
- Only viewers who can see yard: Update

### 5. LOD Per Viewer, Per Chunk
**Same chunk, different viewers:**

Chunk at 0,0,0:
- Player A (5m away): Render at LOD 0 (full detail)
- Player B (100m away): Render at LOD 2 (simplified)
- Player C (5km away): Don't render (culled)

**Implementation:**
```rust
fn render_chunk(chunk: &Chunk, player: &Player) -> Mesh {
    let distance = (chunk.center - player.position).length();
    let in_gaze_cone = player.is_looking_at(chunk.center);
    
    let lod = if distance < 50.0 && in_gaze_cone {
        0  // Full detail
    } else if distance < 200.0 {
        1  // Medium
    } else if distance < 1000.0 && in_gaze_cone {
        2  // Low (if looking at it)
    } else {
        return None;  // Cull
    };
    
    generate_mesh(&chunk.svo, lod)
}
```

## Chunk Size Recommendation: 25-50m

### Why This Range?

**25m chunks:**
- Human perception: ~25m is "near" distance
- Coffee shop: 1 chunk = one room
- Street: 1 chunk = one house or storefront
- Park: 1 chunk = picnic area
- Building: 1 chunk per floor/room

**Benefits:**
- Natural perceptual boundaries
- Building modifications isolated
- Indoor/outdoor transitions clean
- Perfect for P2P deltas

**At depth 18 (25m chunks):**
- Sphere distortion gap: ~0.3m (invisible!)
- Chunks for whole Earth: ~1 billion (manageable with P2P DHT)
- Memory for visible area: 10-100 chunks = 100MB-1GB
- Generation per chunk: <1 second
- Network delta: <5KB per modification

## Constraints Reconsidered

### "Too Many Chunks"
**Wrong assumption**: All chunks in radius loaded
**Reality**: Only VISIBLE chunks loaded (10-100 at a time)

### "Too Much Network Traffic"
**Wrong**: Large infrequent updates
**Right**: Small frequent updates = better P2P load sharing

### "Too Much Memory"
**Wrong**: Load entire 3×3 grid
**Right**: Load visibility set only

### "Too Many Draw Calls"
**Solution**: Batch visible chunks into single draw call
- Merge 50 chunks × 1k tris = 50k tris per batch
- Still only 10-20 draw calls total

## Implementation Strategy

### Phase 1: Increase Depth to 18 (25m chunks)
- Reduces gap to 0.3m (acceptable)
- Fine-grained streaming ready
- Small network deltas

### Phase 2: Visibility-Based Loading
- Replace radius-based loading
- Implement view frustum culling
- Add gaze direction bias

### Phase 3: Activity Context System
```rust
enum ActivityContext {
    Stationary,     // 20m radius
    Walking,        // 50m radius  
    Driving,        // 200m forward cone
    Flying,         // 5km low LOD
    Focused(Point), // High detail at focus point
}
```

### Phase 4: Per-Viewer LOD
- Same chunk, different LOD per viewer
- Server doesn't send mesh, sends SVO ops
- Each client generates mesh at appropriate LOD

### Phase 5: P2P Delta Sync
- Chunk modifications = op logs
- Only affected chunks sync
- Viewers subscribe to visible chunk set

## The Scale You're Describing

**I understand now:**

This isn't "World of Warcraft with more polygons."

This is **true 1:1 Earth simulation** where:
- 10 billion people could exist simultaneously
- Each sees DIFFERENT rendering of same space
- Detail emerges from viewer context, not world state
- Network scales because updates are local and small
- Storage scales because P2P distributes data
- Rendering scales because only visible chunks load

**Coffee shop example:**
- 4 people sitting at table
- Each sees ~20m radius (through windows)
- Overlap: 10 chunks shared
- Network: 4 × 10 chunks = 40 chunk subscriptions
- Server load: ZERO (P2P direct)
- Each client: ~100MB memory, 60 FPS

**Stadium example:**
- 50,000 people watching concert
- Most see stage (5m × 5m = 25 chunks)
- Network: 50k × 25 = 1.25M chunk subs
- But only ~100 unique chunks
- P2P mesh: Each peer serves 5-10 others
- Server load: Still manageable

## Recommendation

**Use depth 18 (25m chunks) or even depth 19 (12.5m chunks)**

The "too many chunks" concern is based on wrong mental model. With visibility-based loading and P2P distribution, chunk count doesn't matter - only VISIBLE chunk count matters, which is always 10-1000 depending on context.

Small chunks enable the adaptive system you're describing.
