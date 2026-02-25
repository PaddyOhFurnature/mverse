# Metaverse Relay Server - Deployment Guide

## What Is It?
A lightweight P2P relay server for NAT traversal coordination. **NOT a game server** - just helps clients discover each other and punch through NAT firewalls to establish direct P2P connections.

## What You Need
**Just one file**: `metaverse_relay` (the compiled binary, currently 7.2MB)

## Platform Compatibility

### ✅ Easy Options (Recommended)

**1. Linux VPS (Best for public relay)**
- Oracle Cloud (always free tier: 1 ARM CPU, 1GB RAM)
- AWS EC2 t2.micro (free tier: 750 hours/month)
- DigitalOcean droplet ($4/month)
- Linode, Vultr, Hetzner (cheap)
- **Requirements**: Any Linux, 100MB RAM, port 4001 open
- **Pros**: Public IP, always-on, reliable
- **Cons**: Need account setup

**2. Home Linux Server / NAS**
- Any x86_64 or ARM64 Linux (Synology, QNAP, Raspberry Pi, old PC)
- **Requirements**: Always-on, port forwarding on router
- **Pros**: Free, full control
- **Cons**: Dynamic IP (use DDNS), need port forwarding

**3. Termux (Android) - Testing Only**
- Install Termux from F-Droid (NOT Google Play - outdated)
- Can run ARM64 Linux binaries
- **Pros**: Easy testing on phone over 4G
- **Cons**: Battery drain, unreliable (sleeps), not for production

### ❌ Won't Work

- **Standard web hosting** (shared hosting can't run custom binaries)
- **iOS** (locked down, can't run arbitrary executables)
- **Windows** (would need Windows build, not cross-compiled yet)

## Deployment Instructions

### Option 1: Oracle Cloud Free Tier (Public Relay)

**Setup (one-time):**
```bash
# 1. Create Oracle Cloud account (free): https://cloud.oracle.com/
# 2. Create ARM instance (Always Free: VM.Standard.A1.Flex)
# 3. SSH into instance
# 4. Open firewall:
sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 4001 -j ACCEPT
sudo iptables -I INPUT 6 -m state --state NEW -p udp --dport 4001 -j ACCEPT
sudo netfilter-persistent save
```

**Deploy:**
```bash
# Copy binary to server (from your dev machine):
scp target/release/examples/metaverse_relay ubuntu@<SERVER_IP>:~/

# SSH into server:
ssh ubuntu@<SERVER_IP>

# Make executable and run:
chmod +x metaverse_relay
./metaverse_relay --port 4001
```

**Run as service (stays running after logout):**
```bash
# Create systemd service:
sudo tee /etc/systemd/system/metaverse-relay.service > /dev/null << 'EOF'
[Unit]
Description=Metaverse P2P Relay Server
After=network.target

[Service]
Type=simple
User=ubuntu
ExecStart=/home/ubuntu/metaverse_relay --port 4001
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Enable and start:
sudo systemctl daemon-reload
sudo systemctl enable metaverse-relay
sudo systemctl start metaverse-relay

# Check status:
sudo systemctl status metaverse-relay

# View logs:
sudo journalctl -u metaverse-relay -f
```

### Option 2: Home NAS / Linux Server

**Requirements:**
- Linux machine that's always on
- Router with port forwarding capability
- (Optional) Dynamic DNS if your ISP changes your IP

**Deploy:**
```bash
# Copy binary to NAS/server:
scp target/release/examples/metaverse_relay user@<NAS_IP>:~/

# SSH into server:
ssh user@<NAS_IP>

# Make executable and run:
chmod +x metaverse_relay
./metaverse_relay --port 4001
```

**Port forwarding:**
1. Log into router admin panel (usually 192.168.1.1)
2. Find "Port Forwarding" or "Virtual Server" section
3. Forward external port 4001 → internal IP of NAS, port 4001
4. Allow both TCP and UDP

**Dynamic DNS (if your IP changes):**
- Use No-IP, DuckDNS, or your NAS's built-in DDNS
- Get a hostname like `myrelay.ddns.net`
- Clients connect to hostname instead of IP

### Option 3: Termux (Android Phone - Testing Only)

**Setup (one-time):**
```bash
# 1. Install Termux from F-Droid: https://f-droid.org/packages/com.termux/
# 2. In Termux, install dependencies:
pkg update && pkg upgrade
pkg install openssh

# 3. Get SSH access (optional, for easy file transfer):
sshd
# Note your phone's IP from Settings → About → Status
```

**Deploy:**
```bash
# Copy binary to phone:
# Option A: Use scp if sshd running:
scp target/release/examples/metaverse_relay <PHONE_IP>:~/

# Option B: Use Termux's shared storage:
# - Copy metaverse_relay to ~/storage/downloads on phone
# - In Termux: cp ~/storage/downloads/metaverse_relay ~/

# Make executable and run:
chmod +x metaverse_relay
./metaverse_relay --port 4001
```

**Keep phone awake:**
- In Termux, run: `termux-wake-lock`
- Keep Termux app in foreground or use "wake lock" setting
- Plug into charger (will drain battery)

**Limitations:**
- Android sleeps = server stops
- Mobile network changes = connections drop
- Good for short tests, not production

## Cross-Compiling for Different Architectures

**Current build**: x86_64 Linux (your dev machine)

**For ARM64 (Oracle Cloud, Raspberry Pi, Android):**
```bash
# Install ARM cross-compiler:
rustup target add aarch64-unknown-linux-gnu
sudo apt install gcc-aarch64-linux-gnu

# Build for ARM64:
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
cargo build --release --target aarch64-unknown-linux-gnu --example metaverse_relay

# Binary will be at:
# target/aarch64-unknown-linux-gnu/release/examples/metaverse_relay
```

**For ARMv7 (older Raspberry Pi, 32-bit Android):**
```bash
rustup target add armv7-unknown-linux-gnueabihf
sudo apt install gcc-arm-linux-gnueabihf

CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc \
cargo build --release --target armv7-unknown-linux-gnueabihf --example metaverse_relay
```

## Usage

```bash
# Default (port 4001):
./metaverse_relay

# Custom port:
./metaverse_relay --port 8080

# Custom limits:
./metaverse_relay \
  --port 4001 \
  --max-circuits 200 \
  --circuit-duration 180 \
  --circuit-data-limit 2097152  # 2MB in bytes

# View help:
./metaverse_relay --help
```

## Connecting Clients

**Once relay is running**, clients need to connect to it:
```
/ip4/<RELAY_IP>/tcp/4001/p2p/<RELAY_PEER_ID>
```

Example:
```
/ip4/123.45.67.89/tcp/4001/p2p/12D3KooWFkA2mvYzhSvZkz4bmZ2eGyH9ZqA8hzkwUKHyJXj7w2mE
```

The Peer ID is printed when relay starts:
```
🔑 Peer ID: 12D3KooWFkA2mvYzhSvZkz4bmZ2eGyH9ZqA8hzkwUKHyJXj7w2mE
```

## Monitoring

**Check if it's running:**
```bash
# Test TCP connection:
nc -zv <RELAY_IP> 4001

# Or with telnet:
telnet <RELAY_IP> 4001
```

**View logs (systemd):**
```bash
sudo journalctl -u metaverse-relay -f
```

**View logs (manual run):**
Server prints events to stdout:
- `👂 Listening on: /ip4/...` - Started successfully
- `📞 Reservation request from: ...` - Client asking for relay slot
- `🔄 Circuit established: ...` - NAT traversal in progress
- `✅ Circuit closed: ...` - Connection upgraded to direct P2P

## Troubleshooting

**"Address already in use":**
- Port 4001 is taken by another process
- Use different port: `--port 4002`

**"Permission denied" (port < 1024):**
- Ports below 1024 need root
- Use `sudo` or pick port > 1024

**Firewall blocking connections:**
```bash
# Oracle Cloud / Ubuntu:
sudo iptables -I INPUT -p tcp --dport 4001 -j ACCEPT
sudo iptables -I INPUT -p udp --dport 4001 -j ACCEPT
sudo netfilter-persistent save

# Check firewall:
sudo iptables -L -n | grep 4001
```

**Can't connect from outside:**
- Check VPS firewall (Oracle Cloud has network security rules)
- Check router port forwarding (home server)
- Check IP is correct: `curl ifconfig.me`

## Security Considerations

**What relay can do:**
- See client peer IDs
- See connection metadata (when, how many circuits)
- Route encrypted data temporarily during NAT traversal

**What relay CANNOT do:**
- Decrypt P2P traffic (end-to-end encrypted)
- Modify game data (just routes packets)
- Access player data or world state

**Production recommendations:**
- Run on isolated VPS (not same server as other services)
- Use firewall to only allow port 4001
- Monitor resource usage (limits prevent DoS)
- Rotate relay peer ID monthly (regenerate identity.key)

## Resource Usage

**Typical usage:**
- RAM: 50-100MB (mostly idle)
- CPU: < 1% (spikes during circuit setup)
- Network: Depends on circuits (fallback only, not primary data path)
- Disk: None (stateless server)

**Limits:**
- Max circuits: 100 (configurable)
- Circuit duration: 2 minutes (enough time to hole-punch)
- Data per circuit: 1MB (should upgrade to direct within seconds)

## Next Steps

**After deploying relay:**
1. Note the relay's public IP and Peer ID
2. Update metaworld_alpha clients to connect to relay (TODO: relay client integration)
3. Test NAT traversal with two clients on different networks
4. Monitor relay logs to confirm DCUtR hole-punching works

**Future enhancements** (not critical for NAT traversal):
- Multiple bootstrap relays (redundancy)
- Relay discovery via DHT (automatic relay finding)
- Heartbeat service (world discovery, player directory)
- World data cache (optional CDN for large worlds)
