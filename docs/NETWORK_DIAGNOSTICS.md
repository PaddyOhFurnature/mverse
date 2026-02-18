# Network Diagnostics for P2P Connection Issues

## Quick Checks

### 1. Check if Ports are Listening

**While both clients are running**, in a third terminal:

```bash
# See all TCP listening ports
ss -tlnp | grep phase1

# Or with netstat
netstat -tlnp 2>/dev/null | grep phase1

# Should show something like:
# tcp   0   0 0.0.0.0:33411   0.0.0.0:*   LISTEN   12345/phase1_mult
# tcp   0   0 0.0.0.0:42765   0.0.0.0:*   LISTEN   12346/phase1_mult
```

✅ If you see this: Ports ARE open and listening  
❌ If nothing shows: libp2p isn't binding ports (serious bug)

---

### 2. Check for mDNS Traffic

mDNS uses UDP port 5353. Check if packets are being sent:

```bash
# Listen for mDNS packets (requires root)
sudo tcpdump -i any port 5353 -n -v

# Should see periodic broadcasts like:
# 192.168.1.111.5353 > 224.0.0.251.5353: [udp sum ok] ...
```

✅ If you see traffic: mDNS is broadcasting  
❌ If silent: mDNS daemon might not be running

---

### 3. Check System mDNS Service

```bash
# Check if avahi (Linux mDNS) is running
systemctl status avahi-daemon

# Or check with:
ps aux | grep avahi

# List mDNS services being advertised
avahi-browse -a -t -r
```

Some systems disable mDNS by default. Ubuntu usually has it enabled.

---

### 4. Check Firewall

```bash
# Check if firewall is blocking
sudo ufw status

# If active, check rules for port 5353
sudo iptables -L -n | grep 5353

# Temporarily disable to test (DON'T leave disabled!)
sudo ufw disable
# <run test>
sudo ufw enable
```

---

### 5. Verbose Debug Output

**Rebuild with debug logging:**

```bash
cd /home/main/metaverse/metaverse_core
cargo build --release --example phase1_multiplayer
```

**Run and capture output:**

```bash
cargo run --release --example phase1_multiplayer 2>&1 | tee client1.log
# In second terminal:
cargo run --release --example phase1_multiplayer 2>&1 | tee client2.log
```

**Look for in the logs:**

```
🔧 [DEBUG] Swarm event: ...
```

This will show EVERY libp2p event. Look for:
- `Mdns(Discovered(...))` - mDNS found a peer
- `ConnectionEstablished` - TCP connection succeeded
- `NewListenAddr` - Port opened successfully
- `IncomingConnection` - Someone tried to connect

---

## Common Issues

### Issue 1: No Swarm Events at All

**Symptom:** Only see subscription messages, then silence  
**Cause:** Background thread crashed or select! not running  
**Fix:** Check for panic in background thread

### Issue 2: NewListenAddr but No Discovery

**Symptom:** See "Listening on" but no peer discovery  
**Cause:** mDNS not enabled or blocked by firewall  
**Fix:** Check avahi-daemon and firewall

### Issue 3: Discovery but No Connection

**Symptom:** See "Peer discovered" but not "Peer connected"  
**Cause:** Firewall blocking TCP connections  
**Fix:** Allow the specific ports or disable firewall temporarily

### Issue 4: Different Subnets

**Symptom:** Two machines but no discovery  
**Cause:** mDNS only works on same subnet (broadcast domain)  
**Check:** Both machines have 192.168.1.X addresses  
**Fix:** Ensure same network, or use manual dial

---

## Expected Working Output

```
📻 Subscribed to topic: player-state
📻 Subscribed to topic: voxel-ops
📻 Subscribed to topic: chat
🔍 Network thread started - polling for mDNS and connections...
🔧 [DEBUG] Swarm event: NewListenAddr { ... }
👂 Listening on: /ip4/127.0.0.1/tcp/33411
🔧 [DEBUG] Swarm event: NewListenAddr { ... }
👂 Listening on: /ip4/192.168.1.111/tcp/33411
🔧 [DEBUG] Swarm event: Behaviour(Mdns(Discovered([...])))
🔍 [Network Thread] mDNS discovered peer: 12D3KooWXXX...
🔧 [DEBUG] Swarm event: ConnectionEstablished { ... }
🔗 [Network Thread] Peer connected: 12D3KooWXXX...
FPS: 60 | Peers: 1 | ...
```

---

## Manual Connection (If mDNS Fails)

If mDNS doesn't work, we can add a command-line argument to dial manually:

```bash
# Client 1
cargo run --release --example phase1_multiplayer

# Note the port from "Listening on" message, e.g., 33411
# Client 2 with manual dial:
cargo run --release --example phase1_multiplayer -- /ip4/192.168.1.111/tcp/33411
```

(This requires code changes - let me know if needed)

---

## Next Steps

1. **Try the port check** - verify ports are open
2. **Run with verbose logging** - see what events happen  
3. **Share the debug output** - paste the first 30-50 lines of the log
4. **Check mDNS daemon** - make sure avahi is running

The verbose debug output will tell us exactly where it's failing!
