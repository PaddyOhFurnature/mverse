# Async Architecture for P2P Networking

## The Problem

libp2p's mDNS and other networking components require a persistent tokio runtime.
However, the game loop uses winit which requires the main thread and is synchronous.

**Error when mixing:**
```
thread 'main' panicked at netlink-sys-0.8.8/src/tokio.rs:45:17:
there is no reactor running, must be called from the context of a Tokio 1.x runtime
```

## The Solution: Background Thread Architecture

NetworkNode runs in a dedicated background thread with its own tokio runtime.
The main game loop communicates with it via message-passing (channels).

```
┌─────────────────────────────────────────────────────────────────┐
│                         MAIN THREAD                              │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              MultiplayerSystem (proxy)                   │   │
│  │                                                           │   │
│  │  - Sends commands via tx channel                        │   │
│  │  - Receives events via rx channel                       │   │
│  │  - Non-blocking poll() for events                       │   │
│  │  - Manages PlayerStateManager, stats, etc               │   │
│  └─────────────────────────────────────────────────────────┘   │
│                          │           ▲                           │
│                          │ Commands  │ Events                    │
│                          ▼           │                           │
└──────────────────────────┼───────────┼──────────────────────────┘
                           │           │
                   ┌───────▼───────────┴────────┐
                   │   crossbeam channels       │
                   │  (bounded, non-blocking)   │
                   └───────┬───────────▲────────┘
                           │           │
┌──────────────────────────┼───────────┼──────────────────────────┐
│                BACKGROUND THREAD (tokio)                         │
│                          ▼           │                           │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              NetworkNode (actual)                        │   │
│  │                                                           │   │
│  │  - Runs in tokio runtime                                │   │
│  │  - Handles mDNS, swarm polling                          │   │
│  │  - Processes commands from channel                      │   │
│  │  - Sends events back to main thread                     │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  Runtime loop:                                                   │
│    loop {                                                        │
│      select! {                                                   │
│        cmd = cmd_rx.recv() => process_command(cmd),             │
│        _ = swarm.select_next_some() => poll_network(),          │
│      }                                                           │
│    }                                                             │
└─────────────────────────────────────────────────────────────────┘
```

## Message Types

### Commands (Main → Background)
```rust
enum NetworkCommand {
    Listen { multiaddr: String },
    Dial { peer: String },
    Subscribe { topic: String },
    Publish { topic: String, data: Vec<u8> },
    Shutdown,
}
```

### Events (Background → Main)
```rust
enum NetworkEvent {
    Connected { peer_id: PeerId },
    Disconnected { peer_id: PeerId },
    Message { topic: String, data: Vec<u8>, from: PeerId },
    ListenSuccess { addr: Multiaddr },
    Error { error: String },
}
```

## Implementation Details

### MultiplayerSystem (Main Thread)

```rust
pub struct MultiplayerSystem {
    // Channel to send commands to background thread
    cmd_tx: crossbeam::channel::Sender<NetworkCommand>,
    
    // Channel to receive events from background thread
    event_rx: crossbeam::channel::Receiver<NetworkEvent>,
    
    // Local state (stays on main thread)
    identity: Identity,
    remote_players: PlayerStateManager,
    clock: LamportClock,
    stats: MultiplayerStats,
    // ... other state
}

impl MultiplayerSystem {
    pub fn new_with_runtime(identity: Identity) -> Result<Self> {
        // Create bounded channels (back-pressure if main thread falls behind)
        let (cmd_tx, cmd_rx) = crossbeam::channel::bounded(1000);
        let (event_tx, event_rx) = crossbeam::channel::bounded(1000);
        
        // Spawn background thread
        std::thread::spawn(move || {
            run_network_thread(identity, cmd_rx, event_tx);
        });
        
        Ok(Self {
            cmd_tx,
            event_rx,
            // ... initialize local state
        })
    }
    
    pub fn listen_on(&self, addr: &str) -> Result<()> {
        self.cmd_tx.send(NetworkCommand::Listen { 
            multiaddr: addr.to_string() 
        })?;
        Ok(())
    }
    
    pub fn update(&mut self, dt: f32) {
        // Non-blocking drain of all pending events
        while let Ok(event) = self.event_rx.try_recv() {
            self.handle_event(event);
        }
        
        // Update interpolation, etc.
        self.remote_players.update_interpolation(dt);
    }
}
```

### Background Thread

```rust
fn run_network_thread(
    identity: Identity,
    cmd_rx: Receiver<NetworkCommand>,
    event_tx: Sender<NetworkEvent>,
) {
    // Create tokio runtime
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    
    rt.block_on(async {
        // Create network node (async version)
        let mut network = NetworkNode::new_async(identity).await.unwrap();
        
        loop {
            tokio::select! {
                // Process commands from main thread
                cmd = async { cmd_rx.recv() } => {
                    match cmd {
                        Ok(NetworkCommand::Listen { multiaddr }) => {
                            // Handle listen command
                        }
                        Ok(NetworkCommand::Shutdown) => break,
                        // ... handle other commands
                        Err(_) => break, // Channel closed
                    }
                }
                
                // Poll libp2p swarm
                event = network.swarm.select_next_some() => {
                    // Process swarm event
                    if let Some(net_event) = network.process_event(event) {
                        event_tx.send(net_event).ok();
                    }
                }
            }
        }
    });
}
```

## Benefits

1. **Separation of concerns:** Network runs async, game loop stays sync
2. **Non-blocking:** Main thread never waits on network I/O
3. **Back-pressure:** Bounded channels prevent unbounded memory growth
4. **Clean shutdown:** Send shutdown command, thread exits cleanly
5. **Testable:** Can mock the channel interface
6. **Platform-agnostic:** Works on any platform that supports threads

## Performance Considerations

- **Bounded channels (1000 capacity):** Prevents OOM if main thread falls behind
- **try_recv() in game loop:** Zero blocking, processes all available events each frame
- **Single background thread:** Sufficient for P2P with <100 peers
- **Future optimization:** Could use thread pool for multiple network nodes

## Migration Path

Phase 1 (Current): Implement channel-based architecture
Phase 2 (Future): Add connection pooling if needed
Phase 3 (Future): Optimize serialization (zero-copy where possible)

## Code Changes Required

1. **network.rs:**
   - Keep NetworkNode::new_async()
   - Add process_command() method
   - Convert methods to return NetworkCommand enum

2. **multiplayer.rs:**
   - Replace direct NetworkNode with channel proxy
   - Implement command sending methods
   - Implement event processing in update()

3. **examples/phase1_multiplayer.rs:**
   - No changes needed (API stays the same!)

## Estimated Work

- Refactor network.rs: 30 min
- Refactor multiplayer.rs: 45 min
- Testing: 15 min
- **Total: ~1.5 hours**

## Alternative Considered (Rejected)

**Disable mDNS:** Could remove mDNS and use manual peer connection.
- Pros: Simpler, works immediately
- Cons: No auto-discovery, harder LAN testing, removes key feature
- Decision: Rejected - auto-discovery is core to the vision
