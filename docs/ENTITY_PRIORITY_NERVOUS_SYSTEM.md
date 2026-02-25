# Entity Priority & Interaction System - "Nervous System" Design

**Created:** 2026-02-21  
**Context:** Discovery system clarification - priority beyond chunks  
**Key Insight:** "We need a priority tree, more like a nervous system than a tree"

**Core Principle:** **EFFICIENT > FAST**  
"Data must be pure, even if it means slower transmission. Humans adapt to consistent 100ms delay instantly. Think LoRa/packet radio."

---

## 🎯 The Real Problem: Entity State Priority

### What We Thought Was The Problem:
"How to sync chunks efficiently?"

### What The Problem Actually Is:
"How to sync entity interactions with the right priority?"

---

## 📊 Data Types & Their Characteristics

### 1. TERRAIN (Layer 1) - Low Priority, High Availability
```
SRTM terrain data:
  - ✅ Deterministic (generated from SRTM)
  - ✅ Cached everywhere (website, servers, clients)
  - ✅ Never changes (unless player edits)
  - ✅ Multiple sources available
  
Priority: LOW (can get from anywhere, anytime)
Sync: On-demand only
```

### 2. VOXEL EDITS (Layer 3) - Medium Priority, Selective
```
Player-edited chunks:
  - Only in specific areas (creative zones)
  - Mostly empty desert/ocean (very few edits)
  - Wipes regularly anyway
  - Small total data volume
  
Priority: MEDIUM (important but not critical)
Sync: Eventually consistent, cache snapshots
```

### 3. STATIC PROPS - Low Cycle Time
```
Lightpoles, bins, trees, buildings:
  - Placed once, rarely change
  - Low interaction frequency
  - Only activate when players nearby
  
Priority: LOW (until player interaction)
Sync: On player proximity only
```

### 4. DYNAMIC ENTITIES - Variable Priority
```
Players, vehicles, NPCs, animals:
  - Constantly moving
  - Position updates every frame
  - Interact with each other
  - Variable importance based on interaction
  
Priority: DYNAMIC (depends on what they're doing)
Sync: Real-time for interactions, degraded otherwise
```

### 5. CRITICAL INTERACTIONS - HIGHEST Priority
```
Player collisions, hitboxes, transactions:
  - Roulette bet on red ($1,000,000)
  - Player A punches Player B
  - Vehicle collision
  - Door open/close state
  
Priority: CRITICAL (must be accurate)
Sync: Immediate, authoritative, verified
```

---

## 🎰 The Casino Example (GTA V Online Style)

### What Can Be Faked Locally (Low Priority):
```
Visual effects:
  - Flashy lights blinking
  - Slot machine animations
  - Roulette wheel spinning
  - Background music
  - Ambient NPCs chatting
  
These can be:
  - Simulated locally (deterministic seed)
  - Out of sync between players
  - Doesn't affect gameplay
  
Example: Your slot machine shows cherry-cherry-lemon
         My view shows lemon-cherry-cherry
         Doesn't matter - it's visual only
```

### What MUST Be Accurate (Critical Priority):
```
Gameplay-affecting state:
  - Roulette ball lands on RED (money transaction)
  - Player A's position (hitbox for collision)
  - Player B swings fist (did it hit?)
  - Door state (open/closed affects pathfinding)
  - Vehicle position (collision detection)
  
These must be:
  - Synchronized precisely
  - Authority determined (who's right?)
  - Verified by all parties
  - Conflict resolution
  
Example: Player bets $1M on red
         Ball MUST land on same color for everyone
         Transaction must be atomic
         All players see same result
```

---

## 🧠 "Nervous System" Architecture

### Not a Tree - A Nervous System

**Tree model (wrong):**
```
Root → Branch → Leaf
Linear hierarchy
Top-down only
```

**Nervous System model (right):**
```
Sensory input → Processing → Motor output
Feedback loops
Priority routing
Reflex arcs (bypass slow processing)
```

### How a Nervous System Works:

```
SENSOR (Player A punches):
  ↓
SPINAL REFLEX (Critical interaction detected):
  ↓
FAST PATH (bypass normal processing):
  → Immediate sync to affected players
  → Authoritative resolution
  → Confirm to all parties
  
SLOW PATH (non-critical):
  → Queue for batch processing
  → Eventually consistent
  → Lower priority sync
```

---

## 🔥 Priority Levels (Nervous System Style)

### CRITICAL (Reflex Arc - Immediate):
```
Priority 0: Transaction-critical
  - Money transfers
  - Roulette result
  - Combat hit detection
  - Vehicle collision
  
Sync: Immediate (<16ms)
Authority: Deterministic or server arbitration
Conflicts: Must resolve, all parties agree
```

### HIGH (Conscious Action - Very Fast):
```
Priority 1: Direct player interaction
  - Player A position (when near Player B)
  - Player A hitbox
  - Player B seeing Player A
  - Vehicle position (when near players)
  
Sync: Very fast (~50ms)
Authority: Owner with validation
Conflicts: Rollback if detected
```

### MEDIUM (Awareness - Fast Enough):
```
Priority 2: Indirect interaction
  - Player position (far from others)
  - NPC position (no player interaction)
  - Door state changes
  - Object pickup
  
Sync: Fast (~200ms)
Authority: Owner
Conflicts: Last-write-wins
```

### LOW (Background - Eventually):
```
Priority 3: Non-interactive
  - Static props
  - Ambient NPCs
  - Visual effects (lights, particles)
  - Background audio
  
Sync: Eventually (~1-5s)
Authority: Local simulation
Conflicts: Don't care
```

---

## 🎯 Interaction Graph (Who Affects Who)

### Entity Interaction Zones:

```
Player A position:
  ↓
  ├─ CRITICAL zone (0-2m): Direct interaction
  │    • Player B hitbox overlap → CRITICAL PRIORITY
  │    • Vehicle collision → CRITICAL PRIORITY
  │    • Object pickup → HIGH PRIORITY
  │
  ├─ HIGH zone (2-10m): Nearby awareness
  │    • Player B sees Player A → HIGH PRIORITY
  │    • NPC reacts to player → MEDIUM PRIORITY
  │    • Door in view → MEDIUM PRIORITY
  │
  ├─ MEDIUM zone (10-50m): Peripheral awareness
  │    • Player visible at distance → MEDIUM PRIORITY
  │    • Sounds audible → LOW PRIORITY
  │
  └─ LOW zone (50m+): Out of interaction range
       • Eventually consistent → LOW PRIORITY
       • Can fake/extrapolate → LOCAL SIMULATION
```

### Dynamic Priority Escalation:

```
Slot machine:
  Normally: LOW priority (local animation)
  
  Player approaches:
    → MEDIUM priority (player might interact)
  
  Player inserts coin:
    → HIGH priority (transaction starting)
  
  Player pulls lever:
    → CRITICAL priority (money at stake)
    → All nearby players must see same result
    → Deterministic RNG with shared seed
  
  Result shown:
    → HIGH priority (confirm transaction)
  
  Player walks away:
    → Back to LOW priority (local animation)
```

---

## 🔄 Sync Strategy by Priority

### CRITICAL (Reflex Arc):
```rust
// Immediate sync, all affected parties
fn sync_critical_interaction(interaction: CriticalEvent) {
    // Determine authority (server or deterministic)
    let authority = determine_authority(&interaction);
    
    // Sync to ALL affected parties immediately
    let affected = get_affected_entities(&interaction);
    for entity in affected {
        send_immediate(entity, interaction, authority);
    }
    
    // Wait for confirmation
    let confirmations = collect_confirmations(affected, timeout: 16ms);
    
    // Resolve conflicts if any
    if has_conflicts(confirmations) {
        resolve_authoritative(authority, confirmations);
    }
}
```

### HIGH (Fast Path):
```rust
// Fast sync, nearby players
fn sync_high_priority(entity: Entity, state: State) {
    // Nearby players get immediate update
    let nearby = get_players_within(entity.position, 10m);
    for player in nearby {
        send_fast(player, entity, state);
    }
    
    // Distant players get batched update
    let distant = get_players_beyond(entity.position, 10m);
    batch_for_later(distant, entity, state);
}
```

### MEDIUM (Normal Path):
```rust
// Batched sync, aware players
fn sync_medium_priority(entity: Entity, state: State) {
    // Batch with other updates
    let aware = get_aware_players(entity);
    add_to_batch(aware, entity, state);
    
    // Send batch every 200ms
}
```

### LOW (Background):
```rust
// Eventually consistent, local simulation
fn sync_low_priority(entity: Entity, state: State) {
    // Local simulation + occasional sync
    simulate_locally(entity);
    
    // Sync checkpoint every 5 seconds
    if time_since_last_sync > 5s {
        send_snapshot(entity, state);
    }
}
```

---

## 🎮 GTA V Online Example: Bank Robbery

### Scenario: 4 players rob a bank

```
Player A (getaway driver):
  - Position: CRITICAL (near other players)
  - Vehicle: CRITICAL (collision with cops)
  - Actions: HIGH (driving affects others)

Player B (inside bank):
  - Position: CRITICAL (shooting at guards)
  - Weapon state: CRITICAL (hit detection)
  - Health: CRITICAL (taking damage)

NPC Guards:
  - Position: HIGH (combat with players)
  - AI state: MEDIUM (behavior matters)
  - Animations: LOW (visual only)

Cops outside:
  - Near players: HIGH (threat)
  - Distant: MEDIUM (approaching)
  - Far away: LOW (background)

Bank door:
  - Being accessed: CRITICAL (must agree on state)
  - Idle: MEDIUM (just state sync)

Money bags:
  - Being picked up: CRITICAL (transaction)
  - On ground: MEDIUM (just state)

Lights/alarms:
  - Blinking: LOW (local simulation)
  - State (on/off): MEDIUM (affects gameplay)
```

### Priority Flow:

```
1. Player B shoots guard:
   CRITICAL → Immediate hit detection
   → Confirm with all players in bank
   → Guard health update
   → Blood particles (LOW, local)

2. Player A crashes car:
   CRITICAL → Vehicle collision
   → Physics resolution
   → Damage calculation
   → Sync to all nearby

3. Alarm triggers:
   MEDIUM → State change
   → Notify all players
   → Spawn cops (eventually)
   
4. Alarm light blinks:
   LOW → Local simulation
   → Out of sync is fine
   → Just visual effect
```

---

## 🧬 Implementation: Interaction Graph + Priority Queue

### Data Structure:

```rust
struct InteractionGraph {
    /// All entities in the world
    entities: HashMap<EntityId, Entity>,
    
    /// Who is aware of who (spatial index)
    awareness: SpatialIndex<EntityId>,
    
    /// Critical interaction pairs (must be accurate)
    critical_links: Vec<(EntityId, EntityId, InteractionType)>,
    
    /// Priority queue for sync
    sync_queue: PriorityQueue<SyncEvent>,
}

struct Entity {
    id: EntityId,
    position: ECEF,
    entity_type: EntityType,
    
    /// Dynamic priority based on current interactions
    current_priority: Priority,
    
    /// Who is interacting with this entity
    interacting_with: Vec<EntityId>,
}

enum Priority {
    Critical,   // <16ms - transactions, combat, collisions
    High,       // <50ms - nearby players, active NPCs
    Medium,     // <200ms - distant players, object state
    Low,        // <5s - background, visual effects
}

enum InteractionType {
    Combat,        // Hitbox overlap
    Transaction,   // Money, items
    Collision,     // Physics
    Proximity,     // Just nearby
    Visual,        // Can see
}
```

### Priority Calculation:

```rust
fn calculate_priority(entity: &Entity, context: &InteractionGraph) -> Priority {
    // Check for critical interactions
    if has_critical_interaction(entity, context) {
        return Priority::Critical;
    }
    
    // Check for nearby players
    let nearby_players = context.awareness.get_nearby_players(entity.position, 10m);
    if !nearby_players.is_empty() {
        return Priority::High;
    }
    
    // Check if in player awareness range
    let aware_players = context.awareness.get_aware_players(entity.position, 50m);
    if !aware_players.is_empty() {
        return Priority::Medium;
    }
    
    // No players care
    Priority::Low
}

fn has_critical_interaction(entity: &Entity, context: &InteractionGraph) -> bool {
    // Check critical interaction graph
    context.critical_links.iter().any(|(e1, e2, interaction_type)| {
        (*e1 == entity.id || *e2 == entity.id) && 
        matches!(interaction_type, 
            InteractionType::Combat | 
            InteractionType::Transaction | 
            InteractionType::Collision
        )
    })
}
```

---

## 📊 Bandwidth Impact

### Traditional approach (everything synced):
```
1000 entities × 60 updates/sec × 100 bytes = 6 MB/s per player
```

### Nervous system approach:
```
Critical (10 entities): 10 × 60 × 100 = 60 KB/s
High (50 entities):     50 × 20 × 100 = 100 KB/s
Medium (200 entities):  200 × 5 × 100 = 100 KB/s
Low (740 entities):     740 × 0.2 × 100 = 15 KB/s

Total: ~275 KB/s (97% reduction!)
```

---

## 📡 Data Efficiency Principles

### The LoRa/Packet Radio Model

**Why This Matters:**
- LoRa sends rich data over 0.3-50 kbps
- Achieves this through **extreme efficiency**
- Sacrifices speed for reliability and encoding density

**Applied to Metaverse:**

#### 1. Tokenization Over Full State
```rust
// ❌ Traditional (bloated):
{
  "player_id": "abc123",
  "position": {"x": 123.45, "y": 67.89, "z": 234.56},
  "rotation": {"yaw": 45.2, "pitch": 0.0, "roll": 0.0},
  "velocity": {"x": 0.5, "y": 0.0, "z": 1.2},
  "animation": "walking"
}
// ~150 bytes JSON

// ✅ Tokenized (efficient):
0x4A 0x7B 0x12 0x45 0x03
// Token: move_north (1 byte)
// Delta XZ: compressed (2 bytes)
// Animation: enum (1 byte)
// Velocity: quantized (1 byte)
// ~5 bytes total = 97% reduction
```

#### 2. Priority-Based Encoding Precision
```
CRITICAL entities:
  - Full precision (f32)
  - Every change transmitted
  - Binary protocol, no compression latency
  
HIGH entities:
  - Medium precision (f16 or quantized)
  - Batched every 2-3 frames
  
MEDIUM entities:
  - Low precision (i16 quantized)
  - Batched every 5-10 frames
  
LOW entities:
  - Very low precision (i8 quantized)
  - Eventually consistent (seconds)
```

#### 3. The 100ms Constant Delay Advantage

**Human Perception:**
- Variable 10-500ms latency = **feels terrible forever**
- Constant 100ms latency = **invisible after 30 seconds**
- Brain adapts to predictable timing (like rhythm games)

**What This Enables:**
```
Traditional approach:
  - Send immediately = variable latency (10-500ms)
  - Jitter, packet loss, unpredictable
  
Efficient approach:
  - Buffer for 100ms (always)
  - Batch multiple updates
  - Compress batch
  - Send exactly at 100ms mark
  - Predictable latency = muscle memory adapts
```

**Real Example - Fighting Game:**
- 3 frames (50ms) consistent = tournament viable
- Variable 1-6 frames (16-100ms) = unplayable trash
- Players adapt to FIXED latency, not variable

#### 4. Delta Encoding + Prediction
```rust
// Don't send full state every frame
// Send CHANGES only

Frame 1: Full state (5 bytes)
Frame 2: Delta from F1 (1 byte) - "moved 0.5m north"
Frame 3: Delta from F2 (1 byte) - "same direction, same speed"
Frame 4: Delta from F3 (0 bytes) - "predicted correctly, no send"

// Client predicts, server corrects only on deviation
// Bandwidth: 5 bytes initial + ~0.5 bytes/frame average
```

#### 5. Efficient > Fast
```
Slow but efficient:
  ✅ Tokenized deltas
  ✅ Binary protocol
  ✅ Batch compression
  ✅ 100ms predictable latency
  ✅ Scales to thousands of entities
  
Fast but bloated:
  ❌ Full JSON states
  ❌ Send every frame
  ❌ Variable latency
  ❌ Doesn't scale past 100 entities
```

### Bandwidth Comparison with Efficiency

**Previous calculation (full states):**
- 275 KB/s with priority tiers

**With tokenization and deltas:**
```
Critical (10 entities): 10 × 5 bytes × 60 Hz = 3 KB/s
High (50 entities):     50 × 2 bytes × 20 Hz = 2 KB/s
Medium (200 entities):  200 × 1 byte × 5 Hz = 1 KB/s
Low (740 entities):     740 × 0.2 bytes × 1 Hz = 0.15 KB/s

Total: ~6 KB/s (99.9% reduction from naive!)
```

**Reality:** Will be ~20-30 KB/s with overhead, but still **200x better** than naive approach.

---

## 🎯 Summary

**Key Insights:**

1. **Terrain is not the problem** - SRTM data is deterministic and cached
2. **Chunks are not the problem** - Very few player edits, wipes regularly
3. **Entity interactions are the problem** - Need precision for gameplay
4. **Not all data is equal** - Priority must be dynamic
5. **Nervous system, not tree** - Reflex arcs for critical data
6. **Efficient > Fast** - Tokenization, batching, predictable latency
7. **Humans adapt to consistent delay** - 100ms constant = invisible

**What to build:**
- Interaction graph (who affects who)
- Dynamic priority calculation (changes based on context)
- Multi-tier sync (critical → high → medium → low)
- Authority resolution (who's right when conflicts occur)
- Reflex arcs (bypass slow paths for critical data)
- **Tokenization protocol** (binary, delta-encoded, priority-aware)
- **100ms batching system** (predictable latency buffer)

**This is not about chunks - it's about entity state synchronization with awareness-based priority and data efficiency.**
