# Phase 1 Multiplayer Testing Guide

**First Playable P2P Demo** - Complete localhost and LAN testing procedures

---

## 🎯 What This Tests

**Validates complete P2P architecture end-to-end:**
- Identity management (Ed25519)
- P2P transport (libp2p with 4 protocols)
- Message protocol (CRDT, signatures, Lamport clocks)
- State synchronization (interpolation, jitter buffer)
- Voxel operation broadcasting
- Remote player rendering
- Network statistics

---

## 📋 Localhost Testing (Phase 1)

**Goal:** Verify P2P works on same machine with loopback

### Setup
```bash
# Terminal 1
cd /home/main/metaverse/metaverse_core
cargo run --release --example phase1_multiplayer

# Terminal 2 (after Terminal 1 is running)
cargo run --release --example phase1_multiplayer
```

### Expected Behavior

**Connection (0-2 seconds):**
```
Terminal 1:
🌐 Starting P2P network node...
   Listening for connections...
   mDNS discovery active (auto-connect on LAN)
🔍 Peer discovered: 12Qk...
🔗 Peer connected: 12Qk... @ /ip4/127.0.0.1/tcp/...

Terminal 2:
🌐 Starting P2P network node...
   Listening for connections...
   mDNS discovery active (auto-connect on LAN)
🔍 Peer discovered: 12Qa...
🔗 Peer connected: 12Qa... @ /ip4/127.0.0.1/tcp/...
```

**FPS Output (every second):**
```
FPS: 60 | Peers: 1 | Local: (0.0, 53.0, 0.0) | Mode: Walk
```

**Network Stats (every 5 seconds when peers connected):**
```
📊 Network Statistics:
   Connected peers: 1
   Player states: sent=100, received=100
   Voxel ops: sent=0, received=0, applied=0, rejected=0
   Invalid signatures: 0
   Total messages: 100
```

### Test Cases

#### 1. Movement Synchronization
**Terminal 1:**
- Move forward with W key
- Observe local position changes in FPS output

**Terminal 2:**
- Should see blue wireframe capsule moving
- Position updates should be smooth (100ms jitter buffer)
- No stuttering or teleporting

**Success Criteria:**
✅ Remote player visible within 1 second of movement  
✅ Movement is smooth and continuous  
✅ Direction matches (forward/backward/left/right)  
✅ Player states received counter incrementing

#### 2. Rotation Synchronization
**Terminal 1:**
- Click to grab mouse
- Look around (yaw and pitch)

**Terminal 2:**
- Blue capsule should rotate
- Rotation should match camera movement

**Success Criteria:**
✅ Remote player rotates smoothly  
✅ No lag > 200ms  
✅ Rotation direction correct

#### 3. Voxel Dig Synchronization
**Terminal 1:**
- Press E to dig a voxel
- Should see message: `⛏️  Dug voxel at VoxelCoord { ... }`
- Mesh should regenerate

**Terminal 2:**
- Same voxel should disappear
- Mesh regeneration message should appear
- Voxel ops received counter should increment

**Success Criteria:**
✅ Voxel disappears in both clients  
✅ Sync happens within 500ms  
✅ Stats show: voxel_ops_sent=1, voxel_ops_received=1, voxel_ops_applied=1  
✅ No desync after 10 operations

#### 4. Voxel Place Synchronization
**Terminal 1:**
- Press Q to place a stone voxel
- Should see message: `🧱 Placed voxel at VoxelCoord { ... }`

**Terminal 2:**
- Stone block should appear
- Voxel ops counter increments

**Success Criteria:**
✅ Block appears in both clients  
✅ Material is correct (STONE)  
✅ Position matches exactly  
✅ No duplicate placements

#### 5. Chat Messaging
**Terminal 1:**
- Press T to send chat
- Should see: `💬 Sent chat message`

**Terminal 2:**
- Should see: `💬 12Qa...: Hello from P2P!`

**Success Criteria:**
✅ Message received within 100ms  
✅ Author PeerId shown correctly  
✅ Text content correct

#### 6. Mode Switching
**Terminal 1:**
- Press F to toggle to fly mode
- Movement should change to free flight

**Terminal 2:**
- Remote player should continue to track smoothly
- No desyncs when switching modes

**Success Criteria:**
✅ Mode changes don't cause desyncs  
✅ Player state continues updating  
✅ Velocity changes reflected

#### 7. Stress Test (Rapid Operations)
**Terminal 1:**
- Dig 50 voxels rapidly (hold E while moving)

**Terminal 2:**
- All voxels should eventually appear dug
- No crashes or hangs
- Stats show reasonable numbers

**Success Criteria:**
✅ All operations eventually applied  
✅ No crashes after 100+ operations  
✅ voxel_ops_applied ≈ voxel_ops_received  
✅ No invalid signatures

#### 8. Disconnection Handling
**Terminal 1:**
- Close window or Ctrl+C

**Terminal 2:**
- Should see: `💔 Peer disconnected: 12Qa...`
- Remote player should disappear
- Peer count should go to 0

**Success Criteria:**
✅ Disconnection detected within 5 seconds  
✅ No crashes  
✅ Can reconnect by restarting Terminal 1

### Expected Performance

**Localhost (ideal conditions):**
- FPS: 60 (stable)
- Connection latency: < 10ms
- Player state sync: < 50ms
- Voxel op sync: < 100ms
- Bandwidth: ~20 KB/s per peer
- Memory: ~100 MB per instance

---

## 🌐 LAN Testing (Phase 2)

**Goal:** Validate P2P over real network with actual latency

### Prerequisites
- 2 physical machines on same local network
- Both can ping each other
- No firewall blocking (or ports opened)
- Same codebase built on both machines

### Setup

**Machine A (192.168.1.10):**
```bash
cd /path/to/metaverse_core
cargo run --release --example phase1_multiplayer
```

**Machine B (192.168.1.20):**
```bash
cd /path/to/metaverse_core
cargo run --release --example phase1_multiplayer
```

### Discovery

**mDNS should work automatically on LAN.**

If mDNS fails (some networks block multicast):
- Check both machines are on same subnet
- Try disabling firewall temporarily
- Check router doesn't block mDNS (port 5353 UDP)

**Manual connection fallback** (not yet implemented):
```bash
# On Machine B, after Machine A shows its address:
# (Future feature: dial command)
```

### Test Cases (Same as Localhost)

Run all 8 test cases from localhost testing with these differences:

**Expected Performance (LAN):**
- FPS: 60 (stable)
- Connection latency: 1-50ms (depending on network)
- Player state sync: 50-150ms
- Voxel op sync: 100-300ms
- Bandwidth: ~20-30 KB/s per peer
- Jitter: 5-50ms

### Additional LAN Tests

#### 9. Latency Tolerance
**Machine A:**
- Move continuously in a circle

**Machine B:**
- Observe smoothness of remote player
- Should not stutter even with 50ms latency
- Jitter buffer (100ms) should absorb variance

**Success Criteria:**
✅ Movement is smooth despite latency  
✅ No visible stuttering  
✅ Interpolation works correctly

#### 10. Packet Loss Simulation
**Machine A or B:**
- Use `tc` (traffic control) to add 5% packet loss:
```bash
sudo tc qdisc add dev eth0 root netem loss 5%
```

**Both machines:**
- Movement should still work
- Some operations may be delayed
- No crashes

**Success Criteria:**
✅ Connection remains stable  
✅ Operations eventually sync  
✅ No crashes from missing packets

**Cleanup:**
```bash
sudo tc qdisc del dev eth0 root
```

#### 11. Bandwidth Measurement
**Use `iftop` or `nethogs` to measure bandwidth:**
```bash
sudo iftop -i eth0
```

**Expected:**
- 10-20 KB/s per peer (player state)
- Spikes to 30-50 KB/s during voxel operations

**Success Criteria:**
✅ Bandwidth < 100 KB/s per peer  
✅ No runaway bandwidth growth  
✅ Scales linearly with operations

---

## 🐛 Known Issues & Workarounds

### Issue: Peers don't discover each other
**Symptoms:** Both instances running, no connection after 30 seconds

**Causes:**
- Firewall blocking mDNS (port 5353 UDP)
- Router blocking multicast
- Different subnets

**Workaround:**
- Check `cargo run` output for "Listening on" address
- Manually connect (feature to be implemented)
- Try disabling firewall temporarily

### Issue: Voxel operations not syncing
**Symptoms:** Dig in one client, nothing happens in other

**Debugging:**
- Check stats: `voxel_ops_sent` should increment
- Check stats: `voxel_ops_received` should increment
- Check stats: `voxel_ops_rejected` - if > 0, signature issue

**Possible Causes:**
- Signature verification failing (should be Phase 1 trust mode)
- Network connectivity issue
- CRDT merge rejecting operation

### Issue: Remote player not visible
**Symptoms:** Connected, states syncing (stats show), but no blue capsule

**Debugging:**
- Check `peer_count` in FPS output
- Check `player_states_received` > 0
- Check terminal for rendering errors

**Possible Causes:**
- Remote player position out of view
- Transform matrix calculation error
- Bind group not created

### Issue: High latency (> 500ms)
**Symptoms:** Operations take > 1 second to sync

**Possible Causes:**
- Network congestion
- CPU throttling (try `--release` build)
- Other processes using bandwidth

**Fix:**
- Close bandwidth-heavy applications
- Use `--release` build (10x faster)
- Check network with `ping`

---

## 📊 Success Metrics

### Localhost (Must Pass All)
- ✅ Connection < 2 seconds
- ✅ FPS ≥ 55 (stable)
- ✅ Player state sync < 100ms
- ✅ Voxel op sync < 200ms
- ✅ No crashes after 100 operations
- ✅ Bandwidth < 50 KB/s per peer
- ✅ 0 invalid signatures
- ✅ Disconnection handled gracefully

### LAN (Must Pass 7/8)
- ✅ Connection < 10 seconds
- ✅ FPS ≥ 50 (allow some variance)
- ✅ Player state sync < 200ms
- ✅ Voxel op sync < 500ms
- ✅ No crashes after 50 operations
- ✅ Bandwidth < 100 KB/s per peer
- ✅ Survives 5% packet loss
- ⚠️ Jitter buffer smooths movement (subjective)

---

## 🎯 Next Steps After Testing

### If Localhost Passes
✅ **PROCEED TO LAN TESTING**

Document:
- Connection time
- Average latency
- Bandwidth usage
- Any issues observed

### If LAN Passes
✅ **P2P INFRASTRUCTURE VALIDATED**

Next phase:
1. Implement remaining optimizations (spatial-sharding, bandwidth-optimize)
2. Add NAT traversal for cross-internet
3. Implement vector clocks for better CRDT
4. Add operation log persistence
5. Begin Phase 2: Chunks and LOD

### If Tests Fail
❌ **DEBUG BEFORE PROCEEDING**

Priority debugging order:
1. Fix crashes (highest priority)
2. Fix connection issues (must connect)
3. Fix sync issues (operations must propagate)
4. Optimize performance (< 100 KB/s bandwidth)
5. Polish (jitter, latency compensation)

---

## 📝 Test Results Template

```markdown
# Phase 1 Multiplayer Test Results

**Date:** YYYY-MM-DD
**Tester:** [Your Name]
**Setup:** [Localhost | LAN]

## Connection
- Time to connect: [X] seconds
- mDNS discovery: [PASS | FAIL]
- Manual connection: [N/A | PASS | FAIL]

## Movement Sync
- Player state: [SMOOTH | LAGGY | BROKEN]
- Latency: ~[X]ms
- Jitter buffer effective: [YES | NO]

## Voxel Operations
- Dig sync: [PASS | FAIL]
- Place sync: [PASS | FAIL]
- Operations synced: [X]/[Y]
- Invalid signatures: [X]

## Performance
- FPS: [X] (average)
- Bandwidth: ~[X] KB/s
- Memory: ~[X] MB

## Stress Test
- Operations tested: [X]
- Crashes: [YES | NO]
- Desyncs: [YES | NO]

## Overall
- Status: [PASS | FAIL]
- Issues: [None | List issues]
- Ready for next phase: [YES | NO]
```

---

**Created:** 2026-02-18  
**Last Updated:** 2026-02-18  
**Status:** Ready for testing  
**Next Phase:** LAN validation → NAT traversal
