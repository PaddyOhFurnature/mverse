# Quick Test Setup - Ubuntu Laptop as Relay

## Files You Need
**On this dev machine** (`192.168.1.111`):
```
/home/main/metaverse/metaverse_core/target/release/examples/metaverse_relay
```
Just this one file (7.2MB).

## Step-by-Step Setup

### 1. Get laptop's IP address
On the Ubuntu laptop, open terminal:
```bash
hostname -I
# Note the first IP, like: 192.168.1.XXX
```

### 2. Copy relay to laptop
**From this dev machine** (192.168.1.111), copy file to laptop:
```bash
cd /home/main/metaverse/metaverse_core
scp target/release/examples/metaverse_relay username@LAPTOP_IP:~/
```
Replace `username` with laptop's username, `LAPTOP_IP` with laptop's IP.

Example:
```bash
scp target/release/examples/metaverse_relay john@192.168.1.50:~/
```

### 3. Run relay on laptop
SSH into laptop (or use laptop directly):
```bash
ssh username@LAPTOP_IP
```

Then run:
```bash
cd ~
chmod +x metaverse_relay
./metaverse_relay --port 4001
```

You should see:
```
🌐 Metaverse P2P Relay Server
================================
Port: 4001
...
🔑 Peer ID: 12D3KooW...
✅ Relay server started
👂 Listening on: /ip4/192.168.1.XXX/tcp/4001
```

**Copy that Peer ID** - you'll need it for clients!

### 4. Test from dev machine
From this machine, test connection:
```bash
nc -zv LAPTOP_IP 4001
# Should say: Connection succeeded
```

### 5. (Optional) Keep it running
To keep relay running after you close terminal:

**Option A - tmux:**
```bash
# On laptop:
tmux new -s relay
./metaverse_relay --port 4001
# Press: Ctrl+B then D (to detach)
# To reattach: tmux attach -t relay
```

**Option B - nohup:**
```bash
nohup ./metaverse_relay --port 4001 > relay.log 2>&1 &
# View logs: tail -f relay.log
```

## Firewall Setup (if needed)

If laptop has firewall enabled:
```bash
# Check firewall status:
sudo ufw status

# If active, allow port 4001:
sudo ufw allow 4001/tcp
sudo ufw allow 4001/udp
```

## Testing with Two Clients

### Scenario: Dev machine + laptop both run metaworld_alpha

**Current problem**: Clients don't connect to relay yet (relay client integration TODO).

**Once relay client is integrated**, both clients will:
1. Connect to relay at `/ip4/LAPTOP_IP/tcp/4001/p2p/PEER_ID`
2. Use DCUtR to hole-punch NAT
3. Establish direct P2P connection
4. Relay connection closes automatically

### For now (testing relay works):
Relay is running and listening. You can:
- Check logs show "Listening on" messages
- Test TCP connection with `nc -zv`
- Leave it running for when client integration is done

## Next Step: Relay Client Integration

Clients (metaworld_alpha) need to:
1. Add relay client behaviour to network stack
2. Connect to relay on startup
3. Use relay for peer discovery
4. Upgrade to direct P2P via DCUtR

This is the next TODO after relay server is deployed.

## Troubleshooting

**"Connection refused" when testing:**
- Check relay is running: `ps aux | grep metaverse_relay`
- Check firewall: `sudo ufw status`
- Try different port: `./metaverse_relay --port 4002`

**"Address already in use":**
- Port 4001 is taken
- Kill old process: `pkill metaverse_relay`
- Or use different port

**Can't SSH/SCP to laptop:**
- Install SSH server: `sudo apt install openssh-server`
- Start SSH: `sudo systemctl start ssh`
- Check laptop IP: `hostname -I`

**Relay crashes:**
- Check logs if running with nohup: `tail relay.log`
- Run in foreground to see errors: `./metaverse_relay --port 4001`
